//! Household dynamics: divorce, the household's parents (their deaths, estates and
//! care), spousal caregiving and life insurance.
//!
//! Everything here is opt-in. [`FamilyPlan::active`] is false for a profile that
//! uses none of it, and the month loop then never calls into this module: the
//! only cost to such a profile is one predictable branch a month.
//!
//! Randomness comes only from this module's own streams (`Stream::Divorce`), drawn
//! a fixed number of times per path in [`State::begin_family`] whatever happens on
//! the path, so the streams of every other subsystem are untouched.

mod divorce;
#[cfg(test)]
mod tests;

pub(crate) use divorce::MarriageHazard;

use crate::data::Data;
use crate::model::{EventKind, Profile, YearMonth};
use crate::rng::{Rng, Stream};
use crate::MAX_PEOPLE;

use super::state::State;

/// Everything about household dynamics that can be resolved once per profile.
#[derive(Clone, Debug, Default)]
pub(crate) struct FamilyPlan {
    /// True when the month hook has anything to do.
    pub active: bool,
    /// Month index at which the marriage began (negative: before the start).
    pub married_month: i32,
    /// The divorce hazard, when `household.divorce_risk` is on.
    pub hazard: Option<MarriageHazard>,
}

impl FamilyPlan {
    /// Compiles the household-dynamics part of a validated profile.
    pub(crate) fn compile(
        profile: &Profile,
        _data: &Data,
        start: YearMonth,
        horizon_months: usize,
    ) -> FamilyPlan {
        let people = &profile.household.people;
        let joins = people
            .get(1)
            .and_then(|p| p.joins)
            .map_or(0, |j| (j - start).max(0));
        let married_month = match profile.household.married {
            Some(m) => m - start,
            None => joins,
        };
        let hazard = if people.len() == 2 {
            MarriageHazard::compile(profile, start, married_month, joins, horizon_months)
        } else {
            None
        };
        let has_divorce_event = profile
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Divorce(_)));
        FamilyPlan {
            active: hazard.is_some() || has_divorce_event,
            married_month,
            hazard,
        }
    }
}

/// Household-dynamics state of one path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FamilyState {
    /// Month the drawn divorce happens (`i32::MAX`: none on this path).
    pub divorce_month: i32,
    /// True once the couple has divorced.
    pub divorced: bool,
    /// True for a person who has left the household (alive, not present).
    pub left: [bool; MAX_PEOPLE],
    /// Multiplier on shared budget lines (1 unless divorced).
    pub shared_factor: f64,
    /// Support after a divorce, nominal dollars a month: positive is paid by the
    /// household, negative received.
    pub support_monthly: f64,
    /// First month without support.
    pub support_until: i32,
}

impl Default for FamilyState {
    fn default() -> Self {
        Self {
            divorce_month: i32::MAX,
            divorced: false,
            left: [false; MAX_PEOPLE],
            shared_factor: 1.0,
            support_monthly: 0.0,
            support_until: i32::MIN,
        }
    }
}

impl State {
    /// Resets the household-dynamics state for path `path` and makes its draws:
    /// one uniform on `Stream::Divorce` when the divorce hazard is on.
    pub(crate) fn begin_family(&mut self, plan: &Plan, seed: u64, path: u64) {
        self.fam = FamilyState::default();
        if let Some(h) = &plan.family.hazard {
            let mut rng = Rng::stream(seed, path, Stream::Divorce);
            self.fam.divorce_month = h.draw_month(rng.uniform());
        }
    }

    /// The month's household-dynamics step, at the start of month `t` after deaths
    /// and a partner joining. Returns true when the year must be re-planned.
    pub(crate) fn family_month(&mut self, plan: &Plan, price: f64, t: usize) -> bool {
        let ti = t as i32;
        let mut replan = false;
        if ti == self.fam.divorce_month {
            if let Some(h) = &plan.family.hazard {
                let terms = h.terms;
                replan |= self.divorce(plan, &terms, "Divorce", price, t);
            }
        }
        if self.fam.support_monthly != 0.0 && ti < self.fam.support_until {
            if self.alive[1] && self.alive[0] {
                let x = self.fam.support_monthly;
                if x > 0.0 {
                    self.spend(crate::stats::YearSeries::SpendEssential, x);
                } else {
                    self.recv(crate::stats::YearSeries::IncomeOther, -x);
                }
            } else {
                self.fam.support_until = ti;
            }
        }
        replan
    }

    /// Obligations of the household outside its budget lines that the year's plan
    /// should expect, nominal dollars a month (support paid).
    pub(crate) fn family_run_rate(&self, t: usize) -> f64 {
        if self.fam.support_monthly > 0.0 && (t as i32) < self.fam.support_until {
            self.fam.support_monthly
        } else {
            0.0
        }
    }
}

use super::plan::Plan;
