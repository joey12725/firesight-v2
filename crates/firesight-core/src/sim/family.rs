//! Family extensions to the simulated life: children on the health plan,
//! uncertain births, college cost uncertainty, adult children who need support and
//! parental leave. Each is opt-in through new schema fields; with all of them off the
//! hot loop meets only the `Plan::family` flags (predictable branches).
//!
//! The hooks in `month.rs`, `year.rs`, `state.rs` and `plan.rs` are one-line calls
//! into this file.

use crate::data::Data;
use crate::health::children::{ACA_CHILD_RATING_AGE, ACA_MAX_CHILDREN_CHARGED};
use crate::health::CoveragePhase;
use crate::model::{ChildCosts, HealthcareMode, Profile};

use super::plan::{ChildPlan, Plan};
use super::state::State;

/// Compiled family settings. Everything is `false`/empty when the profile uses none
/// of the family features.
#[derive(Clone, Debug, Default)]
pub(crate) struct FamilyPlan {
    /// Children are charged health premiums and out-of-pocket costs.
    pub cover_children: bool,
    /// Age at which a child leaves the parents' plan.
    pub cover_until_age: f64,
}

/// Per-path state of the family features.
#[derive(Clone, Debug, Default)]
pub(crate) struct FamilyState {
    /// Children are on the health plan (for the trace event).
    pub kids_on_plan: bool,
}

impl FamilyState {
    pub(crate) fn new(_plan: &Plan) -> Self {
        Self::default()
    }

    /// Resets for path `path`. Allocation-free.
    pub(crate) fn begin(&mut self, _plan: &Plan, _seed: u64, _path: u64) {
        self.kids_on_plan = false;
    }
}

/// Reads the family settings of a profile.
pub(crate) fn compile(profile: &Profile) -> FamilyPlan {
    let hc = &profile.spending.healthcare;
    FamilyPlan {
        cover_children: hc.cover_children && hc.mode == HealthcareMode::Modeled,
        cover_until_age: hc.children_coverage_end_age,
    }
}

/// With children on the health plan the engine charges their premiums and
/// out-of-pocket costs itself, so the health-care column of the USDA estimate (which
/// holds the same things) is taken out of the modelled core cost. `tier` is the
/// USDA income tier key chosen for the household and `usda_factor` the CPI factor
/// from 2015 dollars.
pub(crate) fn drop_usda_health_share(
    children: &mut [ChildPlan],
    profile: &Profile,
    data: &Data,
    tier: &str,
    usda_factor: f64,
) {
    if !compile(profile).cover_children {
        return;
    }
    const BANDS: [&str; 6] = ["0-2", "3-5", "6-8", "9-11", "12-14", "15-17"];
    /// Column of `health_care` in `usda_2015.columns`.
    const HEALTH_COLUMN: usize = 5;
    let table = &data.actuarial["children"]["usda_2015"]
        ["married_couple_overall_us_annual_per_child"][tier];
    for (c, kid) in children.iter_mut().zip(&profile.household.children) {
        if kid.costs != ChildCosts::Modeled {
            continue;
        }
        for age in 0..18 {
            let share = table[BANDS[age / 3]][HEALTH_COLUMN]
                .as_f64()
                .expect("embedded data: USDA health care column")
                * usda_factor;
            c.core[age] = (c.core[age] - share).max(0.0);
            c.lump_year[age] = c.core[age] + c.care[age];
        }
    }
}

/// Which plans the adults are on this month.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AdultPhases {
    pub adults: u32,
    pub employer: bool,
    pub aca: u32,
    pub cobra: u32,
}

impl AdultPhases {
    pub(crate) fn note(&mut self, phase: CoveragePhase) {
        self.adults += 1;
        match phase {
            CoveragePhase::Employer => self.employer = true,
            CoveragePhase::Aca => self.aca += 1,
            CoveragePhase::Cobra => self.cobra += 1,
            _ => {}
        }
    }
}

/// What the children's health cover costs this month, nominal dollars.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ChildHealth {
    /// Everything charged: family-tier increment, premiums, out-of-pocket.
    pub total: f64,
    /// The part that is a marketplace premium eligible for the credit (non-zero
    /// only when an adult is on the marketplace too).
    pub aca_premium: f64,
    /// Benchmark premium of those children, for the credit.
    pub aca_benchmark: f64,
}

impl State {
    /// Children's health cost in month `t` given the adults' plans. The child's
    /// coverage phase is the household's: the employer plan when anyone has one,
    /// else the marketplace when an adult is on it, else COBRA, else (adults on
    /// Medicare) an unsubsidised marketplace plan. Households with a custom
    /// early-retirement amount, outside the US, or with nobody present are not
    /// charged.
    pub(crate) fn child_health(
        &self,
        plan: &Plan,
        t: usize,
        price: f64,
        adults: AdultPhases,
    ) -> ChildHealth {
        let mut out = ChildHealth::default();
        if adults.adults == 0 {
            return out;
        }
        let phase = if adults.employer {
            CoveragePhase::Employer
        } else if adults.aca > 0 || (adults.cobra == 0 && self.in_us) {
            CoveragePhase::Aca
        } else if adults.cobra > 0 {
            CoveragePhase::Cobra
        } else {
            return out;
        };
        // Adults on Medicare or without a marketplace plan: the children's premium
        // is not part of anyone's credit.
        let subsidised = adults.aca > 0;
        let growth = plan.health.excess_growth(t as u32) * price;
        let ti = t as i32;
        let n = plan.children.len();
        let mut covered_any = false;
        for k in 0..n {
            let months = ti - self.child_birth(plan, k);
            if months < 0 {
                continue;
            }
            let age = f64::from(months) / 12.0;
            if age >= plan.family.cover_until_age {
                continue;
            }
            for copy in 0..self.child_count(k) {
                covered_any = true;
                let m = plan.health.child_month(age, phase, growth);
                out.total += m.out_of_pocket;
                if phase == CoveragePhase::Aca
                    && (age >= ACA_CHILD_RATING_AGE
                        || self.rating_rank(plan, t, k, copy) < ACA_MAX_CHILDREN_CHARGED)
                {
                    out.total += m.aca_premium;
                    if subsidised {
                        out.aca_premium += m.aca_premium;
                        out.aca_benchmark += m.aca_benchmark;
                    }
                }
            }
        }
        if covered_any {
            out.total += plan.health.child_family_increment(phase, growth);
        }
        out
    }

    /// Number of children under 21 who are older than entry `(k, copy)` (ties broken
    /// by index): the ACA charges the oldest three. A child 21 or over is not
    /// ranked and is always charged by the caller.
    fn rating_rank(&self, plan: &Plan, t: usize, k: usize, copy: u32) -> usize {
        let ti = t as i32;
        let age_of = |j: usize| {
            let m = ti - self.child_birth(plan, j);
            (m >= 0).then(|| f64::from(m) / 12.0)
        };
        let Some(mine) = age_of(k) else {
            return usize::MAX;
        };
        if mine >= ACA_CHILD_RATING_AGE {
            return usize::MAX;
        }
        let mut older = 0;
        for j in 0..plan.children.len() {
            let Some(a) = age_of(j) else { continue };
            if a >= ACA_CHILD_RATING_AGE || a >= plan.family.cover_until_age {
                continue;
            }
            for c in 0..self.child_count(j) {
                if a > mine || (a == mine && (j, c) < (k, copy)) {
                    older += 1;
                }
            }
        }
        older
    }

    /// Month index of child `k`'s birth on this path.
    #[inline]
    pub(crate) fn child_birth(&self, plan: &Plan, k: usize) -> i32 {
        plan.children[k].birth
    }

    /// Children born at child `k`'s birth: 1, or 2 for twins.
    #[inline]
    pub(crate) fn child_count(&self, _k: usize) -> u32 {
        1
    }
}
