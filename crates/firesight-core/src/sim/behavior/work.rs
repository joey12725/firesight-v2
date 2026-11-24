//! Going back to work after retirement (`retirement.return_to_work`).
//!
//! # What is modelled
//!
//! Each January a retired household runs the funded test of the retirement trigger
//! on itself: savings (on the rule's basis) against the requirement for the
//! retirement budget as it now stands. Its *funded ratio* is the quotient. Then, for
//! each adult who has stopped working, is present and is younger than `until_age`:
//!
//! * ratio below `funded_ratio_below` and not yet looking: a **search** starts. Its
//!   length is drawn from the engine's unemployment-duration model (lognormal,
//!   median 2.2 months in an expansion and 4 in a recession or the 12 months after
//!   one, sigma 1; BLS-calibrated, `unemployment.duration` in
//!   `data/actuarial/params.json`), times its 1.4 age
//!   multiplier from 55 and a further [`OLDER_SEARCH_MULTIPLIER`] from 65, clamped to
//!   0.25 to 36 months; or a fixed number of months.
//! * When the search ends the person **works**: real pay `pay_fraction` times their
//!   former real pay (the real salary of the jobs that ended at retirement, or
//!   `former_annual_pay`), paid monthly as wages, with employee FICA, counted toward
//!   the Social Security earnings record and the earnings test, and taxed on the
//!   year's return.
//! * Work (or the search) **stops** in a January when the ratio is 1 or more again,
//!   and in any month the person reaches `until_age` or leaves the household.
//!
//! # Random numbers
//!
//! One 64-bit word per person per month from `Stream::ReturnToWork(person)`, drawn
//! whatever the household does; a search uses the word of the month it starts.
//!
//! # Not modelled
//!
//! Re-employment pay has no wage shocks, layoffs or growth; the job brings no
//! employer health cover, workplace plan or HSA; the search does not depend on how
//! long the person has been out of work; the regime is judged at the start of the
//! search only.

use crate::career::SLACK_MONTHS_AFTER_RECESSION;
use crate::economy::EconomyPath;
use crate::math;
use crate::rng::{normal_from_bits, Rng};
use crate::stats::{TraceEventKind, YearSeries};
use crate::MAX_PEOPLE;

use super::super::plan::Plan;
use super::super::state::{State, TAX_PAYROLL};

/// Extra multiplier on the median search length from 65, on top of the 55-plus
/// multiplier of the unemployment model: the ratio of mean unemployment duration at
/// 65 and over (28.9 weeks) to 55 to 64 (26.2 weeks), BLS Current Population Survey
/// annual averages 2024, table 31. A judgement as much as a measurement: the median
/// durations go the other way (10.9 against 12.0 weeks), and a retiree re-entering
/// the labour force is not the same as an unemployed worker.
pub const OLDER_SEARCH_MULTIPLIER: f64 = 1.10;

/// Age from which [`OLDER_SEARCH_MULTIPLIER`] applies.
const OLDER_SEARCH_AGE: f64 = 65.0;

/// Shortest and longest search, months (the bounds of the unemployment model).
const SEARCH_MIN_MONTHS: f64 = 0.25;
const SEARCH_MAX_MONTHS: f64 = 36.0;

/// Funded ratio at which a household that went back to work stops again: the plan
/// is fully funded.
pub const RESUME_RETIREMENT_RATIO: f64 = 1.0;

/// Compiled `retirement.return_to_work`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RtwPlan {
    pub ratio_below: f64,
    /// Month index at which each person reaches `until_age`.
    pub until_month: [i32; MAX_PEOPLE],
    pub pay_fraction: f64,
    /// `former_annual_pay`, today's dollars a year.
    pub former_pay: Option<f64>,
    /// `search: fixed`.
    pub fixed_months: Option<f64>,
    /// Unemployment model: median months `[expansion, recession]`.
    pub spell_median: [f64; 2],
    pub spell_sigma: [f64; 2],
    /// Median multipliers for ages under 35, 35 to 54, 55 and over.
    pub spell_age_multiplier: [f64; 3],
}

impl RtwPlan {
    /// Length in months of a search that starts at `age` (years), in a slack labour
    /// market (a recession or the year after) or not, from the standard normal `z`.
    pub(crate) fn search_months(&self, age: f64, slack: bool, z: f64) -> f64 {
        if let Some(m) = self.fixed_months {
            return m;
        }
        let r = usize::from(slack);
        let band = if age < 35.0 {
            0
        } else if age < 55.0 {
            1
        } else {
            2
        };
        let mut median = self.spell_median[r] * self.spell_age_multiplier[band];
        if age >= OLDER_SEARCH_AGE {
            median *= OLDER_SEARCH_MULTIPLIER;
        }
        (median * math::exp(self.spell_sigma[r] * z)).clamp(SEARCH_MIN_MONTHS, SEARCH_MAX_MONTHS)
    }
}

/// Where a retired person stands.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum WorkStatus {
    /// Retired and not looking.
    #[default]
    Idle,
    /// Looking for work, with this many months to go.
    Searching { months_left: f64 },
    /// Working again.
    Working,
}

/// One person's return-to-work state on a path.
#[derive(Clone, Debug)]
pub(crate) struct RtwState {
    pub status: WorkStatus,
    /// Former real pay, today's dollars a year; NaN until it is known.
    pub former_real: f64,
    pub rng: Rng,
}

impl Default for RtwState {
    fn default() -> Self {
        Self {
            status: WorkStatus::Idle,
            former_real: f64::NAN,
            rng: Rng::from_seed(0),
        }
    }
}

impl State {
    /// The funded ratio of a retired household: wealth over the requirement of the
    /// funded test, as things stand in month `t` (infinite when nothing is required).
    /// Never negative: a household in debt has a ratio of zero.
    pub(crate) fn retired_funded_ratio(&self, plan: &Plan, t: usize, price: f64) -> f64 {
        let rule = self.funding_rule(plan);
        let req = self.requirement(plan, t, price, &rule);
        let wealth = self.wealth(rule.basis, price).max(0.0);
        if req.total > 0.0 {
            wealth / req.total
        } else {
            f64::INFINITY
        }
    }

    /// Return to work for month `t`, after careers and before other income.
    pub(crate) fn return_to_work_month(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let Some(rp) = plan.beh.rtw else { return };
        let mut bits = [0u64; MAX_PEOPLE];
        for (b, s) in bits.iter_mut().zip(self.beh.rtw.iter_mut()) {
            *b = s.rng.next_u64();
        }
        if !self.retired {
            return;
        }
        let now = plan.start + t as i32;
        let ti = t as i32;
        let price = econ.price_level[t];
        // Former pay is fixed the first month a person is seen retired.
        for p in 0..self.n {
            if self.person_retired[p] && self.beh.rtw[p].former_real.is_nan() {
                self.beh.rtw[p].former_real = rp.former_pay.unwrap_or_else(|| {
                    let mut pay = 0.0;
                    for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
                        if jp.owner == p && jp.ends_at_retirement && rt.begun {
                            pay += rt.st.real_salary;
                        }
                    }
                    pay
                });
            }
        }
        if now.month() == 1 {
            let eligible = (0..self.n).any(|p| {
                self.present[p]
                    && self.person_retired[p]
                    && ti < rp.until_month[p]
                    && self.beh.rtw[p].former_real > 0.0
            });
            if eligible {
                let ratio = self.retired_funded_ratio(plan, t, price);
                // Slack: a recession now or in the last 12 months (the unemployment
                // model's duration regime).
                let from = t.saturating_sub(SLACK_MONTHS_AFTER_RECESSION as usize);
                let slack = econ.recession[from..=t].iter().any(|r| *r != 0);
                for p in 0..self.n {
                    self.january_work_decision(plan, &rp, t, ratio, bits[p], p, slack);
                }
            }
        }
        for p in 0..self.n {
            self.work_step(plan, &rp, t, price, p);
        }
    }

    /// The January decision for person `p` at funded ratio `ratio`.
    #[allow(clippy::too_many_arguments)]
    fn january_work_decision(
        &mut self,
        plan: &Plan,
        rp: &RtwPlan,
        t: usize,
        ratio: f64,
        bits: u64,
        p: usize,
        slack: bool,
    ) {
        let now = plan.start + t as i32;
        let can = self.present[p]
            && self.person_retired[p]
            && (t as i32) < rp.until_month[p]
            && self.beh.rtw[p].former_real > 0.0;
        match self.beh.rtw[p].status {
            WorkStatus::Idle if can && ratio < rp.ratio_below => {
                let age = self.age(plan, p, t);
                let months = rp.search_months(age, slack, normal_from_bits(bits));
                self.beh.rtw[p].status = WorkStatus::Searching {
                    months_left: months,
                };
                self.event(now, TraceEventKind::LifeEvent, Some(ratio), || {
                    format!(
                        "Person {} looks for work again: savings cover {:.0}% of the plan",
                        p + 1,
                        ratio * 100.0
                    )
                });
            }
            WorkStatus::Searching { .. } | WorkStatus::Working
                if ratio >= RESUME_RETIREMENT_RATIO =>
            {
                self.stop_working(plan, t, p, "the plan is funded again");
            }
            _ => {}
        }
    }

    fn stop_working(&mut self, plan: &Plan, t: usize, p: usize, why: &'static str) {
        let was = self.beh.rtw[p].status;
        self.beh.rtw[p].status = WorkStatus::Idle;
        if was == WorkStatus::Working {
            self.req_dirty = true;
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::LifeEvent, None, || {
                format!("Person {} stops working again: {why}", p + 1)
            });
        }
    }

    /// One month of searching or working for person `p`.
    fn work_step(&mut self, plan: &Plan, rp: &RtwPlan, t: usize, price: f64, p: usize) {
        if self.beh.rtw[p].status == WorkStatus::Idle {
            return;
        }
        if !self.present[p] || t as i32 >= rp.until_month[p] {
            self.stop_working(plan, t, p, "age limit reached");
            return;
        }
        let mut paid = 1.0;
        if let WorkStatus::Searching { months_left } = self.beh.rtw[p].status {
            // Whole months of searching are unpaid; the job starts in the month
            // the search has less than one month to run, paid for the rest of it.
            if months_left >= 1.0 {
                self.beh.rtw[p].status = WorkStatus::Searching {
                    months_left: months_left - 1.0,
                };
                return;
            }
            paid = 1.0 - months_left;
            self.beh.rtw[p].status = WorkStatus::Working;
            let salary = self.beh.rtw[p].former_real * rp.pay_fraction;
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::Rehired, Some(salary), || {
                format!("Person {} back at work after retirement", p + 1)
            });
        }
        let annual_real = self.beh.rtw[p].former_real * rp.pay_fraction;
        let gross = annual_real / 12.0 * price * paid;
        if gross <= 0.0 {
            return;
        }
        self.recv(YearSeries::IncomeWages, gross);
        self.ty.wages[p] += gross;
        self.year_earned[p] += gross;
        self.wage_like += gross;
        if self.fica_on {
            let f = &self.params.federal;
            let before = self.ytd_fica_wages[p];
            let after = before + gross;
            let fica = f.oasdi_rate
                * (after.min(f.oasdi_wage_base) - before.min(f.oasdi_wage_base))
                + f.medicare_rate * gross;
            self.ytd_fica_wages[p] = after;
            self.pay(YearSeries::TaxPayroll, fica);
            self.paid[TAX_PAYROLL] += fica;
        }
        // The earnings test sees the job (the January plan recomputes it from the
        // household's jobs, which do not include this one).
        let annual = annual_real * price;
        if let Some(c) = &mut self.claimants[p] {
            if c.annual_earnings < annual {
                c.annual_earnings = annual;
                self.ss_dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> RtwPlan {
        RtwPlan {
            ratio_below: 0.8,
            until_month: [120; MAX_PEOPLE],
            pay_fraction: 0.5,
            former_pay: None,
            fixed_months: None,
            spell_median: [2.2, 4.0],
            spell_sigma: [1.0, 1.0],
            spell_age_multiplier: [0.85, 1.0, 1.4],
        }
    }

    #[test]
    fn search_length_follows_the_unemployment_model_with_age_and_slack() {
        let p = plan();
        // At the median draw: 2.2 months x 1.4 at 60, x 1.4 x 1.10 at 66.
        assert!((p.search_months(60.0, false, 0.0) - 3.08).abs() < 1e-12);
        assert!((p.search_months(66.0, false, 0.0) - 3.388).abs() < 1e-12);
        // Slack market: 4 months median, x 1.4 = 5.6.
        assert!((p.search_months(60.0, true, 0.0) - 5.6).abs() < 1e-12);
        // One standard deviation longer: x e.
        let e = std::f64::consts::E;
        assert!((p.search_months(60.0, false, 1.0) - 3.08 * e).abs() < 1e-9);
        // Clamped to 0.25 and 36 months.
        assert_eq!(p.search_months(60.0, false, -10.0), 0.25);
        assert_eq!(p.search_months(60.0, true, 10.0), 36.0);
    }

    #[test]
    fn a_fixed_search_ignores_age_and_the_draw() {
        let p = RtwPlan {
            fixed_months: Some(3.0),
            ..plan()
        };
        assert_eq!(p.search_months(68.0, true, 2.5), 3.0);
    }

    #[test]
    fn constants_match_the_data_file() {
        let d = &crate::data::Data::embedded().actuarial["household_behavior"]["return_to_work"];
        assert_eq!(
            d["older_search_multiplier"]["value"].as_f64(),
            Some(OLDER_SEARCH_MULTIPLIER)
        );
        assert_eq!(
            d["resume_retirement_ratio"]["value"].as_f64(),
            Some(RESUME_RETIREMENT_RATIO)
        );
    }
}
