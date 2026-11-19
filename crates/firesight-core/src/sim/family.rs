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
use crate::model::{Arrival, ChildCosts, College, CollegeCost, HealthcareMode, Profile, YearMonth};
use crate::rng::{Rng, Stream};

use super::plan::{ChildPlan, Plan};
use super::state::State;
use crate::stats::TraceEventKind;

/// Month index standing for "never": a child who does not arrive.
pub(crate) const NEVER: i32 = i32::MAX / 2;

/// Years of the tuition path drawn per path (the index is flat after this).
pub(crate) const TUITION_YEARS: usize = 60;

/// Persistence of the yearly deviation of real tuition growth from its mean (AR(1)
/// coefficient). A judgement: price growth ran fast for three decades and slowly for
/// one, which needs a persistent component; 0.8 gives deviations a half-life of about
/// three years.
pub(crate) const TUITION_AR1: f64 = 0.8;

/// Share of students who take 4, 5 and 6 years among those who attend: cumulative
/// 0.55, 0.83, 1. A judgement anchored to NCES: 64% of first-time bachelor's students
/// finish at their first institution within six years, fewer in four.
pub(crate) const DURATION_CDF: [f64; 2] = [0.55, 0.83];

/// Points `(household income in today's dollars, mean aid discount on the published
/// price)` of the aid schedule, interpolated linearly and flat outside. The first two
/// are read from Brookings' NPSAS 2019-20 net prices (about $18,000 under $50,000 of
/// income and $29,000 at $125,000 against a published cost near $45,000); the last
/// (little aid above $250,000, mostly merit aid) is a judgement.
pub(crate) const AID_SCHEDULE: [(f64, f64); 3] =
    [(50_000.0, 0.60), (125_000.0, 0.36), (250_000.0, 0.10)];

/// Standard deviation of the discount around the schedule for a child (a judgement).
pub(crate) const AID_SD: f64 = 0.10;

/// Largest discount: aid does not cover more than 90% of the published price.
pub(crate) const AID_MAX: f64 = 0.9;

/// Mean aid discount at a household income (today's dollars a year).
pub(crate) fn aid_mean_discount(income: f64) -> f64 {
    let pts = &AID_SCHEDULE;
    if income <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        if income < w[1].0 {
            let f = (income - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + f * (w[1].1 - w[0].1);
        }
    }
    pts[pts.len() - 1].1
}

/// Compiled stochastic college cost of one child.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollegePlan {
    pub attend: f64,
    /// `ln(1 + mean real growth)`.
    pub log_growth: f64,
    pub sd: f64,
    pub aid_scale: f64,
}

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
    /// Per child: stochastic college cost, for children with a college plan.
    pub college: Vec<Option<CollegePlan>>,
    /// Some child has stochastic college costs.
    pub any_college: bool,
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
    /// Years each child spends in college on this path (4 unless stochastic).
    pub college_years: Vec<u8>,
    /// Standardised aid shock of each child.
    pub aid_z: Vec<f64>,
    /// Tuition index relative to CPI by year since the start, per child
    /// (`TUITION_YEARS + 1` entries each).
    pub tuition: Vec<f64>,
    /// Standard normals of the shared tuition path.
    z_path: Vec<f64>,
    /// Price level of the current month (set by the month loop).
    pub price: f64,
    /// Household income (MAGI) of the last two tax years in today's dollars.
    pub income_real: [f64; 2],
}

impl FamilyState {
    pub(crate) fn new(plan: &Plan) -> Self {
        let n = plan.children.len();
        FamilyState {
            birth: vec![0; n],
            twin: vec![false; n],
            college_years: vec![4; n],
            aid_z: vec![0.0; n],
            tuition: vec![1.0; n * (TUITION_YEARS + 1)],
            z_path: vec![0.0; TUITION_YEARS],
            price: 1.0,
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
        self.price = 1.0;
        self.income_real = [0.0; 2];
        if plan.family.any_college {
            let mut rng = Rng::stream(seed, path, Stream::CollegePrices);
            for z in self.z_path.iter_mut() {
                *z = rng.normal_one_draw();
            }
        }
        for (k, c) in plan.children.iter().enumerate() {
            self.birth[k] = c.birth;
            self.twin[k] = false;
            self.college_years[k] = 4;
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
            if let Some(cp) = plan.family.college[k] {
                let mut rng = Rng::stream(seed, path, Stream::ChildCollege(k));
                let (u_attend, u_dur, z_aid) =
                    (rng.uniform(), rng.uniform(), rng.normal_one_draw());
                self.college_years[k] = realise_duration(cp.attend, u_attend, u_dur);
                self.aid_z[k] = z_aid;
                let idx = &mut self.tuition[k * (TUITION_YEARS + 1)..(k + 1) * (TUITION_YEARS + 1)];
                build_tuition_index(idx, &cp, &self.z_path);
                let b = self.birth[k];
                if b < NEVER {
                    until = until.max(b + (18 + i32::from(self.college_years[k])) * 12);
                }
            }
        }
        until
    }
}

/// The tuition index relative to CPI by year since the start: `idx[0] = 1` and
/// `idx[y + 1] = idx[y] * exp(ln(1 + mean growth) + x_y)` where `x` is a stationary
/// AR(1) with coefficient [`TUITION_AR1`] and standard deviation `sd`, driven by the
/// standard normals `z`.
pub(crate) fn build_tuition_index(idx: &mut [f64], cp: &CollegePlan, z: &[f64]) {
    idx[0] = 1.0;
    let innovation = cp.sd * (1.0 - TUITION_AR1 * TUITION_AR1).sqrt();
    let mut x = cp.sd * z[0];
    for y in 0..TUITION_YEARS {
        if y > 0 {
            x = TUITION_AR1 * x + innovation * z[y];
        }
        idx[y + 1] = idx[y] * math::exp(cp.log_growth + x);
    }
}

/// Years in college from two uniforms: 0 if the child does not attend (`u_attend` at
/// or above `attend`), else 4, 5 or 6 by [`DURATION_CDF`].
pub(crate) fn realise_duration(attend: f64, u_attend: f64, u_dur: f64) -> u8 {
    if u_attend >= attend {
        0
    } else if u_dur < DURATION_CDF[0] {
        4
    } else if u_dur < DURATION_CDF[1] {
        5
    } else {
        6
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
    let college: Vec<Option<CollegePlan>> = profile
        .household
        .children
        .iter()
        .map(|c| match c.college_cost {
            CollegeCost::Stochastic {
                attend_probability,
                real_tuition_growth,
                tuition_volatility,
                aid_scale,
            } if c.college != College::None => Some(CollegePlan {
                attend: attend_probability.clamp(0.0, 1.0),
                log_growth: math::ln_1p(real_tuition_growth.max(-0.5)),
                sd: tuition_volatility.max(0.0),
                aid_scale: aid_scale.max(0.0),
            }),
            _ => None,
        })
        .collect();
    let any_college = college.iter().any(Option::is_some);
    FamilyPlan {
        cover_children: hc.cover_children && hc.mode == HealthcareMode::Modeled,
        cover_until_age: hc.children_coverage_end_age,
        dynamic: any_arrival || any_college,
        arrival,
        college,
        any_college,
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

    /// Years child `k` spends in college on this path.
    #[inline]
    pub(crate) fn college_years(&self, k: usize) -> usize {
        usize::from(self.fam.college_years[k])
    }

    /// Household income for aid, today's dollars a year: the income of two tax years
    /// ago once known (the FAFSA's prior-prior year), else the latest, else the
    /// starting income.
    fn aid_income(&self, plan: &Plan) -> f64 {
        match self.magi_known {
            2.. => self.fam.income_real[0],
            1 => self.fam.income_real[1],
            _ => plan.income_rate0,
        }
    }

    /// Records the household income of the tax year just closed (nominal MAGI).
    pub(crate) fn note_aid_income(&mut self, magi_nominal: f64) {
        self.fam.income_real = [self.fam.income_real[1], magi_nominal / self.fam.price];
    }

    /// The price of a year of college relative to the plan's today's-dollar cost, in
    /// month `t`: the fixed growth of childcare and college prices by default, or the
    /// path's tuition index after the aid discount for stochastic children.
    pub(crate) fn college_factor(&self, plan: &Plan, k: usize, t: usize) -> f64 {
        match plan.family.college[k] {
            None => self.service_growth,
            Some(c) => {
                let year = (t / 12).min(TUITION_YEARS);
                self.fam.tuition[k * (TUITION_YEARS + 1) + year]
                    * (1.0 - self.aid_discount(plan, k, &c))
            }
        }
    }

    /// The aid discount child `k` receives now.
    fn aid_discount(&self, plan: &Plan, k: usize, c: &CollegePlan) -> f64 {
        let mean = aid_mean_discount(self.aid_income(plan));
        (c.aid_scale * (mean + AID_SD * self.fam.aid_z[k])).clamp(0.0, AID_MAX)
    }

    /// The monthly family calendar: announces realised births in the trace.
    pub(crate) fn family_calendar(&mut self, plan: &Plan, t: usize, now: YearMonth) {
        if !self.detail {
            return;
        }
        let ti = t as i32;
        for k in 0..plan.children.len() {
            let birth = self.fam.birth[k];
            let name = &plan.profile.household.children[k].name;
            if let Some(c) = plan.family.college[k] {
                if birth < NEVER && birth + 18 * 12 == ti {
                    let years = self.college_years(k);
                    let discount = self.aid_discount(plan, k, &c);
                    self.event(now, TraceEventKind::Child, None, || {
                        if years == 0 {
                            format!("{name} does not attend college")
                        } else {
                            format!(
                                "{name} starts college: {years} years, aid {:.0}% off the published price",
                                discount * 100.0
                            )
                        }
                    });
                }
            }
            let arrives = plan.family.arrival[k].is_some();
            if arrives && birth == ti {
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
            } else if arrives && birth >= NEVER && plan.children[k].birth == ti {
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

    #[test]
    fn aid_schedule_is_interpolated() {
        assert_eq!(aid_mean_discount(20_000.0), 0.60);
        assert_eq!(aid_mean_discount(50_000.0), 0.60);
        // Halfway between (50k, 0.60) and (125k, 0.36).
        assert!((aid_mean_discount(87_500.0) - 0.48).abs() < 1e-12);
        assert!((aid_mean_discount(125_000.0) - 0.36).abs() < 1e-12);
        // Halfway between (125k, 0.36) and (250k, 0.10).
        assert!((aid_mean_discount(187_500.0) - 0.23).abs() < 1e-12);
        assert_eq!(aid_mean_discount(250_000.0), 0.10);
        assert_eq!(aid_mean_discount(900_000.0), 0.10);
    }

    #[test]
    fn duration_follows_the_cdf() {
        assert_eq!(realise_duration(0.95, 0.95, 0.0), 0, "does not attend");
        assert_eq!(realise_duration(0.95, 0.94, 0.54), 4);
        assert_eq!(realise_duration(0.95, 0.94, 0.55), 5);
        assert_eq!(realise_duration(0.95, 0.0, 0.82), 5);
        assert_eq!(realise_duration(0.95, 0.0, 0.83), 6);
        assert_eq!(realise_duration(0.0, 0.0, 0.0), 0);
    }

    #[test]
    fn tuition_index_without_volatility_is_the_fixed_growth_path() {
        let cp = CollegePlan {
            attend: 1.0,
            log_growth: math::ln_1p(0.02),
            sd: 0.0,
            aid_scale: 1.0,
        };
        let mut idx = vec![0.0; TUITION_YEARS + 1];
        build_tuition_index(&mut idx, &cp, &vec![1.7; TUITION_YEARS]);
        assert_eq!(idx[0], 1.0);
        // 1.02^10 = 1.21899...
        assert!((idx[10] - 1.02f64.powi(10)).abs() < 1e-12, "{}", idx[10]);
    }

    #[test]
    fn tuition_index_is_persistent_and_has_the_stated_deviation() {
        let cp = CollegePlan {
            attend: 1.0,
            log_growth: 0.0,
            sd: 0.012,
            aid_scale: 1.0,
        };
        // Pseudo-normals from the crate's own generator.
        let mut rng = Rng::from_seed(77);
        let (n, mut sum_sq, mut cov, mut var) = (4000, 0.0, 0.0, 0.0);
        let mut idx = vec![0.0; TUITION_YEARS + 1];
        for _ in 0..n {
            let z: Vec<f64> = (0..TUITION_YEARS).map(|_| rng.normal_one_draw()).collect();
            build_tuition_index(&mut idx, &cp, &z);
            // Yearly log growth deviations are idx ratios.
            let g: Vec<f64> = (0..TUITION_YEARS)
                .map(|y| math::ln(idx[y + 1] / idx[y]))
                .collect();
            for y in 10..TUITION_YEARS - 1 {
                sum_sq += g[y] * g[y];
                cov += g[y] * g[y + 1];
                var += g[y] * g[y];
            }
        }
        let count = f64::from(n) * (TUITION_YEARS as f64 - 11.0);
        let sd = (sum_sq / count).sqrt();
        let rho = cov / var;
        assert!((sd - 0.012).abs() < 0.0006, "sd {sd}");
        assert!((rho - TUITION_AR1).abs() < 0.03, "autocorrelation {rho}");
    }
}
