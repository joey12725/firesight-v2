//! Simulation-side hooks of long-term disability (`crate::career_realism::disability`).
//! Every hook is behind `plan.rz.any`; with the feature off none of this runs.

use crate::benefits::BenefitBasis;
use crate::career::{Employment, JobMonth};
use crate::career_realism::disability::{
    self, Change, DISABLED_SPELL_MONTHS, MIN_INSURED_YEARS, RETURN_SCAR, SSDI_WAIT_MONTHS,
};
use crate::economy::EconomyPath;
use crate::stats::{TraceEventKind, YearSeries};

use super::plan::Plan;
use super::state::State;

impl State {
    /// Start of the month, after the variates are drawn: onset and recovery.
    pub(super) fn rz_disability_step(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let now = plan.start + t as i32;
        let price = econ.price_level[t];
        for p in 0..self.n {
            let Some(model) = plan.rz.people.get(p).copied().flatten() else {
                continue;
            };
            let draw = self.rz.person_draw[p];
            let age = self.age(plan, p, t);
            let fra = f64::from(
                plan.ss
                    .full_retirement_age_months(plan.people[p].birth.year()),
            ) / 12.0;
            let working = self.alive[p] && self.present[p] && !self.person_retired[p];
            let at_risk = working && age < fra;
            let change = model.step(
                &mut self.rz.dis[p],
                age,
                at_risk,
                working,
                (draw.u_onset, draw.u_recover),
            );
            match change {
                Change::Onset => self.rz_disability_onset(plan, p, t, now, price, age, fra),
                Change::Recovery => self.rz_disability_recovery(plan, p, now),
                Change::None => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rz_disability_onset(
        &mut self,
        plan: &Plan,
        p: usize,
        _t: usize,
        now: crate::model::YearMonth,
        price: f64,
        age: f64,
        fra: f64,
    ) {
        let model = plan.rz.people[p].expect("model exists at onset");
        // Base pay at onset, for LTD: every job of the person that is paying.
        let mut salary_real = 0.0;
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            if jp.owner == p
                && matches!(
                    rt.st.employment,
                    Employment::Employed | Employment::OnBreak { .. }
                )
            {
                salary_real += rt.st.real_salary;
            }
        }
        self.rz_disable_jobs(plan, p);
        let st = &mut self.rz.dis[p];
        st.ltd_nominal = model.ltd_replacement * salary_real / 12.0 * price;
        // SSDI: insured people with a Social Security basis who have not claimed
        // retirement benefits.
        let insured = !self.ss_claimed[p]
            && match plan.people[p].basis {
                BenefitBasis::FromEarnings => self.records[p].years >= MIN_INSURED_YEARS,
                BenefitBasis::KnownPia(x) => x > 0.0,
                BenefitBasis::None => false,
            };
        st.ssdi_real = 0.0;
        st.ssdi_started = false;
        if insured {
            let pia = match plan.people[p].basis {
                BenefitBasis::KnownPia(x) => x,
                _ => disability::ssdi_pia_real(
                    &plan.ss,
                    &self.records[p],
                    plan.people[p].birth,
                    now,
                    price,
                ),
            };
            st.ssdi_real = pia;
            if let Some(c) = &mut self.claimants[p] {
                // Freeze the record at the disability PIA and convert to the
                // retirement benefit at full retirement age (see the module docs).
                st.saved_claim_age = c.claim_age;
                st.frozen = true;
                c.pia_real = Some(pia);
                c.claim_age = fra;
                c.annual_earnings = 0.0;
            }
        }
        let ssdi = st.ssdi_real;
        self.ss_dirty = true;
        self.req_dirty = true;
        self.event(now, TraceEventKind::LifeEvent, Some(ssdi * 12.0), || {
            format!("Person {} becomes disabled at {:.0}", p + 1, age.floor())
        });
    }

    fn rz_disability_recovery(&mut self, plan: &Plan, p: usize, now: crate::model::YearMonth) {
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            if jp.owner == p
                && matches!(rt.st.employment, Employment::Unemployed { .. })
                && rt.st.spell_months_left >= DISABLED_SPELL_MONTHS / 2.0
            {
                // The job model rehires at the next step, applying the scar.
                rt.st.spell_months_left = 0.0;
                rt.st.pending_scar = RETURN_SCAR;
            }
        }
        let st = &mut self.rz.dis[p];
        if st.frozen {
            if let Some(c) = &mut self.claimants[p] {
                c.claim_age = st.saved_claim_age;
            }
        }
        st.frozen = false;
        st.ssdi_real = 0.0;
        st.ltd_nominal = 0.0;
        self.ss_dirty = true;
        self.req_dirty = true;
        self.event(now, TraceEventKind::LifeEvent, None, || {
            format!("Person {} recovers and can work again", p + 1)
        });
    }

    /// Puts every paying job of person `p` into the open-ended idle state.
    fn rz_disable_jobs(&mut self, plan: &Plan, p: usize) {
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            if jp.owner != p {
                continue;
            }
            force_disabled(plan, jp, &mut rt.st);
        }
    }

    /// After a job step: a disabled person's job pays nothing (it also catches a job
    /// that starts during the disability).
    pub(super) fn rz_disability_job(&mut self, plan: &Plan, j: usize, jm: &mut JobMonth) {
        let jp = &plan.jobs[j];
        let p = jp.owner;
        if !self.rz.dis.get(p).is_some_and(|d| d.disabled) {
            return;
        }
        let st = &mut self.jobs[j].st;
        if matches!(
            st.employment,
            Employment::Employed | Employment::OnBreak { .. }
        ) {
            *jm = JobMonth::default();
            force_disabled(plan, jp, st);
        }
        if matches!(st.employment, Employment::Unemployed { .. }) {
            // The idle month is not an unemployment spell.
            st.months_unemployed_total = (st.months_unemployed_total - 1.0).max(0.0);
        }
    }

    /// Whether person `p`'s Social Security record is frozen at the disability PIA.
    #[inline]
    pub(super) fn rz_record_frozen(&self, p: usize) -> bool {
        self.rz.dis.get(p).is_some_and(|d| d.frozen)
    }

    /// Disability benefits for the month: SSDI and LTD, into the existing ledger
    /// series (Social Security and other income) and the tax year.
    pub(super) fn rz_disability_income(&mut self, plan: &Plan, t: usize, price: f64) {
        let now = plan.start + t as i32;
        for p in 0..self.n {
            let Some(model) = plan.rz.people.get(p).copied().flatten() else {
                continue;
            };
            let st = self.rz.dis[p];
            if !st.disabled || !self.present[p] {
                continue;
            }
            let age_months = self.age_months(plan, p, t);
            let age = f64::from(age_months) / 12.0;
            let fra_months = plan
                .ss
                .full_retirement_age_months(plan.people[p].birth.year());
            let mut ssdi = 0.0;
            if st.ssdi_real > 0.0 && st.months >= SSDI_WAIT_MONTHS && age_months < fra_months as i32
            {
                ssdi = st.ssdi_real * price * plan.ss.payable_fraction(now.year());
                self.recv(YearSeries::IncomeSocialSecurity, ssdi);
                self.ty.social_security[p] += ssdi;
                self.wage_like += ssdi;
                if !st.ssdi_started {
                    self.rz.dis[p].ssdi_started = true;
                    self.event(
                        now,
                        TraceEventKind::LifeEvent,
                        Some(ssdi * 12.0 / price),
                        || format!("Person {} starts disability insurance (SSDI)", p + 1),
                    );
                }
            }
            let ltd = model.ltd_benefit(&st, age, ssdi);
            if ltd > 0.0 {
                self.recv(YearSeries::IncomeOther, ltd);
                self.ty.other_ordinary += ltd;
                self.wage_like += ltd;
            }
        }
    }
}

/// Idle state of a disabled person's job: no pay, no benefits, no layoff, unvested
/// equity forfeited. A job that has not started or has ended is left as it is.
fn force_disabled(plan: &Plan, jp: &super::plan::JobPlan, st: &mut crate::career::JobState) {
    let _ = plan;
    if matches!(
        st.employment,
        Employment::Employed | Employment::OnBreak { .. }
    ) {
        jp.model.forfeit_unvested(st);
        st.employment = Employment::Unemployed { months: 0 };
        st.spell_months_left = DISABLED_SPELL_MONTHS;
        st.pending_scar = RETURN_SCAR;
        st.ui_weeks_left = 0.0;
        st.ui_wait_weeks_left = 0.0;
        st.ui_weekly_real = 0.0;
    }
}
