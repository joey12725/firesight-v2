//! The household's parents: their deaths, what their late-life care costs the
//! estate, and the inheritance.
//!
//! Each parent's month of death is drawn from the SSA life table for their age,
//! sex and health on `Stream::Parent(i)`, and their long-term care episode with
//! the household's own care model (probability by sex and age at death, lognormal
//! length, type) on the same stream: six draws per parent per path, always.
//!
//! At the death the household receives `share` of the estate less the care paid
//! out of it. The inherited-IRA part is deposited in the heir's traditional
//! account and paid out each December over the next ten years
//! ([`INHERITED_IRA_YEARS`]) in equal parts of what is left (the balance divided
//! by the years remaining), taxed as ordinary income and never penalised. The
//! rest arrives as cash: an inheritance is not income (IRC 102) and its basis is
//! stepped up to its value at death (IRC 1014), so it carries no gain.
//!
//! The inherited IRA is not a bucket of its own: it is a share of the heir's
//! traditional holdings, which therefore moves with their returns and with any
//! other withdrawal from them.

use crate::accounts::{Bucket, WithdrawContext};
use crate::data::Data;
use crate::health::{HealthModel, MortalityModel};
use crate::model::{
    CareSupport, Healthcare, HealthcareMode, Longevity, Profile, Sex, YearMonth,
    INHERITED_IRA_YEARS, MAX_PARENTS,
};
use crate::rng::{Rng, Stream};
use crate::stats::{TraceEventKind, YearSeries};

use super::super::plan::Plan;
use super::super::state::State;

/// One parent, compiled.
#[derive(Clone, Debug)]
pub(crate) struct ParentPlan {
    pub name: String,
    pub child: usize,
    pub sex: Sex,
    pub age0_months: u32,
    pub mortality: MortalityModel,
    /// Estate in today's dollars.
    pub estate: f64,
    pub share: f64,
    pub ira_fraction: f64,
    pub care_erodes_estate: bool,
    /// The household's part in the care, if any.
    pub support: Option<CareSupport>,
}

impl ParentPlan {
    /// Compiles the parents of a validated profile, with the care model they use.
    pub(crate) fn compile_all(
        profile: &Profile,
        data: &Data,
        start: YearMonth,
    ) -> (Vec<ParentPlan>, Option<HealthModel>) {
        let parents = &profile.household.parents;
        if parents.is_empty() {
            return (Vec::new(), None);
        }
        // A parent's life is always drawn from the table: a fixed lifespan set for
        // the household's own people says nothing about their parents, nor do the
        // household's couple linkage and income gradient.
        let longevity = match profile.assumptions.longevity {
            Longevity::Stochastic { improvement, .. } => Longevity::stochastic(improvement),
            Longevity::FixedAge { .. } => Longevity::stochastic(true),
        };
        let care = HealthModel::new(
            &Healthcare {
                mode: HealthcareMode::Modeled,
                long_term_care: true,
                ..profile.spending.healthcare
            },
            profile.household.location.state.as_deref(),
            profile.assumptions.health_risk,
            data,
        );
        let plans = parents
            .iter()
            .take(MAX_PARENTS)
            .map(|p| ParentPlan {
                name: if p.name.is_empty() {
                    p.id.clone()
                } else {
                    p.name.clone()
                },
                child: p.child,
                sex: p.sex,
                age0_months: p.birth.age_months_at(start).max(0) as u32,
                mortality: MortalityModel::new(p.birth, p.sex, p.health, start, &longevity, data),
                estate: p.estate.max(0.0),
                share: p.share.clamp(0.0, 1.0),
                ira_fraction: p.inherited_ira_fraction.clamp(0.0, 1.0),
                care_erodes_estate: p.care_erodes_estate,
                support: p.care_support.map(|s| CareSupport {
                    probability: s.probability.clamp(0.0, 1.0),
                    annual_cost: s.annual_cost.max(0.0),
                    hours: s.hours.clamp(0.0, 1.0),
                    caregiver: Some(s.caregiver.unwrap_or(p.child)),
                }),
            })
            .collect();
        (plans, Some(care))
    }
}

/// One parent on one path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParentPath {
    /// Month of death (months since start).
    pub death: u32,
    /// First month of paid care (`u32::MAX`: none).
    pub care_start: u32,
    /// First month after the care (the death month).
    pub care_end: u32,
    /// Cost of the care, today's dollars a month before excess medical inflation.
    pub care_monthly: f64,
    /// Total cost of the care in today's dollars (with excess medical inflation)
    /// that the estate pays.
    pub care_total: f64,
    /// What the household pays toward the care, today's dollars a month before
    /// excess medical inflation (0: it does not take part on this path).
    pub household_monthly: f64,
    /// True when the household takes part in the care on this path.
    pub helps: bool,
    /// True once the death has been handled.
    pub done: bool,
}

impl Default for ParentPath {
    fn default() -> Self {
        Self {
            death: u32::MAX,
            care_start: u32::MAX,
            care_end: 0,
            care_monthly: 0.0,
            care_total: 0.0,
            household_monthly: 0.0,
            helps: false,
            done: false,
        }
    }
}

/// An inherited IRA being paid out.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InheritedIra {
    /// Whose traditional account holds it.
    pub heir: usize,
    /// Share of the heir's traditional holdings that is this inherited IRA.
    pub share: f64,
    /// Distributions still to make (one each December).
    pub years_left: u32,
    /// First calendar year with a distribution.
    pub first_year: i32,
    /// Its value at the start of the last month it was seen, nominal (to hand it
    /// over if the heir dies).
    pub value: f64,
}

impl State {
    /// Draws the parents' lifetimes and care for this path.
    pub(crate) fn begin_parents(&mut self, plan: &Plan, seed: u64, path: u64) {
        let Some(care) = &plan.dynamics.parent_care else {
            return;
        };
        for (i, p) in plan.dynamics.parents.iter().enumerate() {
            let mut rng = Rng::stream(seed, path, Stream::Parent(i));
            let death = p.mortality.draw_months_to_death(&mut rng);
            let hs = care.initial_state(p.sex, p.age0_months, death, &mut rng);
            // Whether the household takes part: drawn on every path, used or not.
            let u_helps = rng.uniform();
            let mut path = ParentPath {
                death,
                ..ParentPath::default()
            };
            if hs.ltc_months() > 0 {
                path.care_start = hs.ltc_start_month;
                path.care_end = hs.ltc_end_month;
                path.care_monthly = hs.ltc_monthly_cost;
                if let Some(s) = &p.support {
                    if u_helps < s.probability {
                        path.helps = true;
                        path.household_monthly = (s.annual_cost / 12.0).min(hs.ltc_monthly_cost);
                    }
                }
                let estate_monthly = hs.ltc_monthly_cost - path.household_monthly;
                for m in hs.ltc_start_month..hs.ltc_end_month {
                    path.care_total += estate_monthly * care.excess_growth(m);
                }
            }
            self.dynamics.parents[i] = path;
        }
    }

    /// The parents' part of the month: deaths and inheritances, and the December
    /// distributions of inherited IRAs. Returns true when the year must be
    /// re-planned.
    pub(crate) fn parents_month(&mut self, plan: &Plan, price: f64, t: usize) -> bool {
        let mut replan = false;
        let tu = t as u32;
        for i in 0..plan.dynamics.parents.len() {
            let pp = self.dynamics.parents[i];
            if pp.helps && !pp.done && tu >= pp.care_start && tu < pp.care_end {
                replan |= self.support_parent(plan, i, price, t);
            }
            if !pp.done && tu >= pp.death {
                self.dynamics.parents[i].done = true;
                replan |= self.inherit_from_parent(plan, i, price, t);
            }
        }
        self.inherited_ira_month(plan, price, t);
        replan
    }

    /// Month `t` of parent `i`'s care spell, in which the household takes part.
    /// Returns true in the first month (the year is re-planned).
    pub(super) fn support_parent(&mut self, plan: &Plan, i: usize, price: f64, t: usize) -> bool {
        let p = &plan.dynamics.parents[i];
        let pp = self.dynamics.parents[i];
        let child = p.child.min(self.n.saturating_sub(1));
        // Only while the parent's child is in this household: not before a partner
        // joins, not after a divorce and not after the child's death.
        if !self.present[child] {
            return false;
        }
        let growth = plan
            .dynamics
            .parent_care
            .as_ref()
            .map_or(1.0, |c| c.excess_growth(t as u32));
        let paid = pp.household_monthly * growth * price;
        if paid > 0.0 {
            self.spend(YearSeries::SpendLongTermCare, paid);
        }
        if t as u32 != pp.care_start {
            return false;
        }
        let support = p.support.unwrap_or_default();
        let who = support
            .caregiver
            .unwrap_or(child)
            .min(self.n.saturating_sub(1));
        let mut cut = false;
        if support.hours > 0.0 && self.present[who] {
            let months = pp.care_end - pp.care_start;
            for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
                if jp.owner == who && rt.st.employment == crate::career::Employment::Employed {
                    jp.model
                        .start_break(&mut rt.st, months, 1.0 - support.hours);
                    cut = true;
                }
            }
        }
        self.req_dirty = true;
        let now = plan.start + t as i32;
        let years = f64::from(pp.care_end - pp.care_start) / 12.0;
        self.event(
            now,
            TraceEventKind::LongTermCare,
            Some(paid * 12.0 / price),
            || {
                let mut s = format!(
                    "{} needs care for {years:.1} years; the household pays ${:.0}k a year",
                    p.name,
                    paid * 12.0 / price / 1000.0
                );
                if cut {
                    s.push_str(&format!(
                        "; person {} works {:.0}% less",
                        who + 1,
                        support.hours * 100.0
                    ));
                }
                s
            },
        );
        true
    }

    /// Parent `i` dies at the start of month `t`.
    pub(super) fn inherit_from_parent(
        &mut self,
        plan: &Plan,
        i: usize,
        price: f64,
        t: usize,
    ) -> bool {
        let p = &plan.dynamics.parents[i];
        let pp = self.dynamics.parents[i];
        let now = plan.start + t as i32;
        let child = p.child.min(self.n.saturating_sub(1));
        // The child's household, if it is still this one: the child, or after the
        // child's death the partner who stays. Nothing comes from the parent of a
        // partner who has left or has not yet joined.
        let heir = if self.present[child] {
            Some(child)
        } else if self.dynamics.left[child] || self.alive[child] {
            None
        } else {
            (0..self.n).find(|&o| self.present[o])
        };
        let care = if p.care_erodes_estate {
            pp.care_total
        } else {
            0.0
        };
        let left = (p.estate - care).max(0.0);
        let real = p.share * left;
        let Some(heir) = heir else {
            self.event(now, TraceEventKind::LifeEvent, None, || {
                format!("{} dies; the estate does not come to the household", p.name)
            });
            return false;
        };
        let nominal = real * price;
        let ira = nominal * p.ira_fraction;
        self.recv(YearSeries::IncomeOther, nominal - ira);
        if ira > 0.0 {
            self.led[YearSeries::IncomeOther.index()] += ira;
            self.led[YearSeries::SaveTraditional.index()] += ira;
            let before = self.traditional_of(heir);
            let mix = self.acc.buy_mix(ira, &self.target);
            self.acc.contribute(Bucket::Traditional(heir), ira, &mix);
            let after = self.traditional_of(heir);
            self.rescale_inherited(heir, before, after, usize::MAX, 0.0);
            if let Some(slot) = self.dynamics.ira.iter_mut().find(|s| s.years_left == 0) {
                *slot = InheritedIra {
                    heir,
                    share: if after > 0.0 { ira / after } else { 0.0 },
                    years_left: INHERITED_IRA_YEARS,
                    first_year: now.year() + 1,
                    value: ira,
                };
            }
        }
        // Nothing to re-plan for when nothing arrived.
        let arrived = nominal > 0.0;
        if arrived {
            self.req_dirty = true;
        }
        if !self.detail {
            return arrived;
        }
        let care_note = if care > 0.0 {
            format!(", after ${:.0}k of care", care / 1000.0)
        } else {
            String::new()
        };
        let ira_note = if ira > 0.0 {
            format!(" (${:.0}k in an inherited IRA)", ira / price / 1000.0)
        } else {
            String::new()
        };
        self.event(now, TraceEventKind::Windfall, Some(real), || {
            format!(
                "Inheritance from {}: ${:.0}k{ira_note}{care_note}",
                p.name,
                real / 1000.0
            )
        });
        arrived
    }

    /// Hands over inherited IRAs whose heir has left and makes the December
    /// distributions.
    pub(super) fn inherited_ira_month(&mut self, plan: &Plan, price: f64, t: usize) {
        let now = plan.start + t as i32;
        // The heir died (their accounts passed to the partner): the partner now
        // holds it. Their traditional holdings grew by the heir's, so every
        // inherited IRA they hold is re-based on its value of a month ago, the last
        // one seen, before any is valued this month.
        for k in 0..self.dynamics.ira.len() {
            let slot = self.dynamics.ira[k];
            if slot.years_left == 0 || self.present[slot.heir] {
                continue;
            }
            let Some(o) = (0..self.n).find(|&o| self.present[o]) else {
                self.dynamics.ira[k].years_left = 0;
                continue;
            };
            let trad = self.traditional_of(o);
            for s in self.dynamics.ira.iter_mut() {
                if s.years_left == 0 || (s.heir != o && s.heir != slot.heir) {
                    continue;
                }
                s.heir = o;
                s.share = if trad > 0.0 {
                    (s.value / trad).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
        }
        for k in 0..self.dynamics.ira.len() {
            if self.dynamics.ira[k].years_left == 0 {
                continue;
            }
            let slot = self.dynamics.ira[k];
            let trad = self.traditional_of(slot.heir);
            self.dynamics.ira[k].value = slot.share * trad;
            if now.month() != 12 || now.year() < slot.first_year {
                continue;
            }
            let want = slot.share * trad / f64::from(slot.years_left);
            let ctx = WithdrawContext {
                owner_age: self.age(plan, slot.heir, t),
                penalty_exempt: true,
                ..WithdrawContext::default()
            };
            let w = self
                .acc
                .withdraw(Bucket::Traditional(slot.heir), want, &ctx);
            self.acc.cash += w.amount;
            self.led[YearSeries::WithdrawTraditional.index()] += w.amount;
            self.ty.traditional_distributions += w.ordinary;
            let after = self.traditional_of(slot.heir);
            self.rescale_inherited(slot.heir, trad, after, k, w.amount);
            self.dynamics.ira[k].years_left -= 1;
            if self.dynamics.ira[k].years_left == 0 {
                self.dynamics.ira[k].share = 0.0;
            }
            self.dynamics.ira[k].value = self.dynamics.ira[k].share * after;
            self.req_dirty = true;
            let left = self.dynamics.ira[k].years_left;
            self.event(
                now,
                TraceEventKind::LifeEvent,
                Some(w.amount / price),
                || format!("Inherited IRA distribution ({left} more to come)"),
            );
        }
    }

    /// After person `heir`'s traditional holdings went from `before` to `after`,
    /// keeps each inherited IRA's dollar value: slot `taken` lost `amount`, the
    /// others nothing.
    fn rescale_inherited(
        &mut self,
        heir: usize,
        before: f64,
        after: f64,
        taken: usize,
        amount: f64,
    ) {
        for (k, s) in self.dynamics.ira.iter_mut().enumerate() {
            if s.years_left == 0 || s.heir != heir {
                continue;
            }
            let mut value = s.share * before;
            if k == taken {
                value -= amount;
            }
            s.share = if after > 0.0 {
                (value / after).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }

    /// Moves person `from`'s retirement accounts into person `to`'s (a divorce:
    /// the rollover of what is left after the split) and keeps the dollar value of
    /// every inherited IRA either of them holds.
    pub(super) fn merge_person_keeping_inherited(&mut self, from: usize, to: usize) {
        let mut values = [0.0; MAX_PARENTS];
        for (k, s) in self.dynamics.ira.iter().enumerate() {
            if s.years_left > 0 {
                values[k] = s.share * self.traditional_of(s.heir);
            }
        }
        self.acc.merge_person(from, to);
        let trad = self.traditional_of(to);
        for (k, s) in self.dynamics.ira.iter_mut().enumerate() {
            if s.years_left == 0 || (s.heir != from && s.heir != to) {
                continue;
            }
            s.heir = to;
            s.value = values[k];
            s.share = if trad > 0.0 {
                (values[k] / trad).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }

    /// Person `p`'s traditional holdings, nominal.
    fn traditional_of(&self, p: usize) -> f64 {
        self.acc.person[p].traditional.iter().sum()
    }
}

impl State {
    /// What the household is paying toward its parents' care this month, nominal
    /// dollars before excess medical inflation (for the year's plan).
    pub(super) fn parent_support_rate(&self, plan: &Plan, t: usize) -> f64 {
        let tu = t as u32;
        let mut r = 0.0;
        for (i, _) in plan.dynamics.parents.iter().enumerate() {
            let pp = &self.dynamics.parents[i];
            if pp.helps && !pp.done && tu >= pp.care_start && tu < pp.care_end {
                r += pp.household_monthly;
            }
        }
        r
    }
}
