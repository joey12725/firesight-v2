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
use crate::math;
use crate::model::{Arrival, ChildCosts, HealthcareMode, Profile, YearMonth};
use crate::rng::{Rng, Stream};

use super::plan::{ChildPlan, Plan};
use super::state::State;
use crate::stats::TraceEventKind;

/// Month index standing for "never": a child who does not arrive.
pub(crate) const NEVER: i32 = i32::MAX / 2;

/// Compiled uncertain arrival of one planned child.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArrivalPlan {
    pub probability: f64,
    pub delay_mean: f64,
    pub twins: f64,
}

/// Compiled family settings. Everything is `false`/empty when the profile uses none
/// of the family features.
#[derive(Clone, Debug, Default)]
pub(crate) struct FamilyPlan {
    /// Children are charged health premiums and out-of-pocket costs.
    pub cover_children: bool,
    /// Age at which a child leaves the parents' plan.
    pub cover_until_age: f64,
    /// Per child: uncertain arrival, for planned children only.
    pub arrival: Vec<Option<ArrivalPlan>>,
    /// Something needs the monthly family calendar (births to announce, leave to
    /// start, support spells).
    pub dynamic: bool,
}

/// Per-path state of the family features.
#[derive(Clone, Debug, Default)]
pub(crate) struct FamilyState {
    /// Children are on the health plan (for the trace event).
    pub kids_on_plan: bool,
    /// Month index of each child's birth on this path (`NEVER` if it does not happen).
    pub birth: Vec<i32>,
    /// Whether the birth is twins.
    pub twin: Vec<bool>,
    /// Bit `k` set when some realised birth falls in a month with `t % 12 == k`.
    pub birthday_extra: u32,
}

impl FamilyState {
    pub(crate) fn new(plan: &Plan) -> Self {
        let n = plan.children.len();
        FamilyState {
            birth: vec![0; n],
            twin: vec![false; n],
            ..Self::default()
        }
    }

    /// Resets for path `path`, drawing what is random about the family. Returns the
    /// month by which the last child's costs end (`kids_until`), at least `until`.
    /// Allocation-free.
    pub(crate) fn begin(&mut self, plan: &Plan, seed: u64, path: u64, until: i32) -> i32 {
        self.kids_on_plan = false;
        self.birthday_extra = 0;
        let mut until = until;
        for (k, c) in plan.children.iter().enumerate() {
            self.birth[k] = c.birth;
            self.twin[k] = false;
            if let Some(a) = plan.family.arrival[k] {
                // Always three draws, whatever the values.
                let mut rng = Rng::stream(seed, path, Stream::ChildArrival(k));
                let (u_arrive, u_delay, u_twin) = (rng.uniform(), rng.uniform(), rng.uniform());
                let (birth, twin) = realise_arrival(&a, c.birth, u_arrive, u_delay, u_twin);
                self.birth[k] = birth;
                self.twin[k] = twin;
                if birth < NEVER {
                    self.birthday_extra |= 1 << birth.rem_euclid(12);
                    until = until.max(birth + 22 * 12);
                }
            }
        }
        until
    }
}

/// Turns three uniforms into a realised birth: `(month index, twins)`. The child
/// arrives when `u_arrive < probability`; the delay is exponential with the given
/// mean, rounded to whole months; the birth is twins when `u_twin < twins`.
pub(crate) fn realise_arrival(
    a: &ArrivalPlan,
    planned: i32,
    u_arrive: f64,
    u_delay: f64,
    u_twin: f64,
) -> (i32, bool) {
    if u_arrive >= a.probability {
        return (NEVER, false);
    }
    let delay = if a.delay_mean > 0.0 {
        (-a.delay_mean * math::ln(1.0 - u_delay) + 0.5).floor() as i32
    } else {
        0
    };
    (planned + delay, u_twin < a.twins)
}

/// Reads the family settings of a profile. `start` is the plan's first month.
pub(crate) fn compile(profile: &Profile, start: YearMonth) -> FamilyPlan {
    let hc = &profile.spending.healthcare;
    let arrival: Vec<Option<ArrivalPlan>> = profile
        .household
        .children
        .iter()
        .map(|c| match c.arrival {
            // Only a child still to be born can fail to arrive.
            Arrival::Uncertain {
                probability,
                delay_mean_months,
                twins_probability,
            } if c.birth - start > 0 => Some(ArrivalPlan {
                probability: probability.clamp(0.0, 1.0),
                delay_mean: delay_mean_months.max(0.0),
                twins: twins_probability.clamp(0.0, 1.0),
            }),
            _ => None,
        })
        .collect();
    let any_arrival = arrival.iter().any(Option::is_some);
    FamilyPlan {
        cover_children: hc.cover_children && hc.mode == HealthcareMode::Modeled,
        cover_until_age: hc.children_coverage_end_age,
        dynamic: any_arrival,
        arrival,
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
    if !(profile.spending.healthcare.cover_children
        && profile.spending.healthcare.mode == HealthcareMode::Modeled)
    {
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
    pub(crate) fn child_birth(&self, _plan: &Plan, k: usize) -> i32 {
        self.fam.birth[k]
    }

    /// Children born at child `k`'s birth: 1, or 2 for twins (0 if it never happens).
    #[inline]
    pub(crate) fn child_count(&self, k: usize) -> u32 {
        1 + u32::from(self.fam.twin[k])
    }

    /// The monthly family calendar: announces realised births in the trace.
    pub(crate) fn family_calendar(&mut self, plan: &Plan, t: usize, now: YearMonth) {
        if !self.detail {
            return;
        }
        let ti = t as i32;
        for k in 0..plan.children.len() {
            if plan.family.arrival[k].is_none() {
                continue;
            }
            let birth = self.fam.birth[k];
            let name = &plan.profile.household.children[k].name;
            if birth == ti {
                let twins = self.fam.twin[k];
                let late = birth - plan.children[k].birth;
                self.event(now, TraceEventKind::Child, None, || {
                    format!(
                        "{} born{}{}",
                        name,
                        if twins { " (twins)" } else { "" },
                        if late > 0 {
                            format!(", {late} months later than planned")
                        } else {
                            String::new()
                        }
                    )
                });
            } else if birth >= NEVER && plan.children[k].birth == ti {
                self.event(now, TraceEventKind::Child, None, || {
                    format!("Planned child {name} does not arrive")
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ArrivalPlan = ArrivalPlan {
        probability: 0.85,
        delay_mean: 4.0,
        twins: 0.03,
    };

    #[test]
    fn arrival_is_hand_computed() {
        // No arrival when the first uniform is at or above the probability.
        assert_eq!(realise_arrival(&A, 14, 0.85, 0.5, 0.0), (NEVER, false));
        assert_eq!(realise_arrival(&A, 14, 0.99, 0.5, 0.0), (NEVER, false));
        // Median of an exponential with mean 4 is 4 ln 2 = 2.77; plus a half and
        // floored gives 3 months late.
        assert_eq!(realise_arrival(&A, 14, 0.1, 0.5, 0.5), (17, false));
        // u_delay = 0 is no delay; u_delay = 0.99: -4 ln 0.01 = 18.4 -> 18.
        assert_eq!(realise_arrival(&A, 14, 0.1, 0.0, 0.5), (14, false));
        assert_eq!(realise_arrival(&A, 14, 0.1, 0.99, 0.5), (32, false));
        // Twins when the third uniform is below 0.03.
        assert_eq!(realise_arrival(&A, 14, 0.1, 0.0, 0.029), (14, true));
        assert_eq!(realise_arrival(&A, 14, 0.1, 0.0, 0.03), (14, false));
        // Mean delay of zero: exactly on time.
        let on_time = ArrivalPlan {
            delay_mean: 0.0,
            ..A
        };
        assert_eq!(realise_arrival(&on_time, 14, 0.1, 0.9, 0.5), (14, false));
    }

    #[test]
    fn delay_distribution_has_the_stated_mean() {
        // Average of the rounded exponential over a grid of the uniform: mean 4.
        let n = 20_000;
        let total: f64 = (0..n)
            .map(|i| {
                let u = (f64::from(i) + 0.5) / f64::from(n);
                f64::from(realise_arrival(&A, 0, 0.0, u, 0.5).0)
            })
            .sum();
        let mean = total / f64::from(n);
        assert!((mean - 4.0).abs() < 0.1, "{mean}");
    }
}
