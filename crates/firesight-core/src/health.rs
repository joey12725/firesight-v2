//! Health costs and mortality.
//!
//! Responsibility: when each person dies on a path; health insurance premiums and
//! out-of-pocket costs by phase of life (employer plan while working, COBRA or ACA
//! between retirement and 65, Medicare with supplemental coverage after); random
//! medical cost shocks; a long-term care episode at the end of life.
//!
//! Contract:
//!
//! * Lifetimes are drawn once per path from `Stream::Longevity(person)`, before the
//!   monthly loop, by inverting the cohort survival curve (period life table from
//!   `data.life_table`, improved along the cohort diagonal with
//!   `data.mortality_improvement` when `improvement` is on, scaled by the
//!   `HealthStatus` multiplier). The draw uses exactly one uniform per person so
//!   longevity is paired across scenarios.
//! * Health draws come only from `Stream::Health(person)`:
//!   [`HealthModel::initial_state`] consumes exactly [`INITIAL_DRAWS`] 64-bit draws
//!   per person per path and [`HealthModel::step`] exactly [`STEP_DRAWS`] per
//!   person-month, whatever the settings, the phase or the state.
//! * Money returned by [`HealthModel::step`] is **nominal**: today's dollars times the
//!   caller's `price_level` times the excess medical inflation compounded over
//!   `months_since_start`. Pass `price_level = 1.0` to get today's dollars (still
//!   including excess medical growth). [`HealthModel::expected_annual_cost`] is in
//!   today's dollars.
//! * `spending.healthcare.mode == in_budget` disables premiums and routine
//!   out-of-pocket costs but not long-term care (when enabled).
//! * ACA premiums are returned gross with the benchmark premium alongside; the subsidy
//!   is the tax module's job because it depends on MAGI.
//!
//! Dollar amounts in `data/` carry their own year (2026 for employer and Medicare,
//! the latest published plan year for the ACA benchmark, 2025 for care costs). They
//! are used as today's dollars without rolling them to the profile's start month.
//! Children's health costs are not modelled here: the USDA-style child cost in the
//! spending module already includes them.

pub mod children;

use serde_json::Value;

use crate::data::Data;
use crate::math;
use crate::model::{
    EarlyRetirementCoverage, HealthStatus, Healthcare, HealthcareMode, Longevity, PlanTier, Sex,
    YearMonth,
};
use crate::rng::Rng;

/// Calendar year at which the Trustees' improvement rates switch from the transition
/// values to the ultimate values (the `2024_2050` and `2050_2100` columns of
/// `data/actuarial/ssa-mortality-improvement.csv`).
const IMPROVEMENT_SWITCH_YEAR: i32 = 2050;

/// Months of COBRA continuation after leaving a job: 18 (29 U.S.C. 1162(2)(A)(i)).
pub const COBRA_MONTHS: u32 = 18;

/// COBRA premium as a multiple of the full plan cost: 102% (29 U.S.C. 1162(3)).
const COBRA_PREMIUM_LOAD: f64 = 1.02;

/// Out-of-pocket maximum of a typical employer plan, today's dollars per year.
/// `docs/research/stochastic-models.md` section 4: "plan maximum, about $4-5k".
const EMPLOYER_OOP_MAX: f64 = 4_500.0;

/// Mean out-of-pocket multiplier for a bronze / high-deductible plan, from
/// `params.json` `healthcare.out_of_pocket_by_age_pre_65.note` ("Scale by 1.5 for a
/// high-deductible/bronze plan").
const BRONZE_OOP_FACTOR: f64 = 1.5;

/// Mean out-of-pocket multiplier for a gold plan, from the same note ("0.7 for a rich
/// employer plan"), applied to the gold tier as the rich-plan case.
const GOLD_OOP_FACTOR: f64 = 0.7;

/// The capped lognormal is solved so its mean after the cap equals the configured
/// mean. That is impossible when the mean approaches the cap, so the target is held
/// to at most this share of the cap (never binding with the embedded data).
const MAX_MEAN_SHARE_OF_CAP: f64 = 0.9;

/// Earliest age at which the end-of-life care episode can be paid care:
/// `params.json` `long_term_care.model.probability_of_episode` ("T must be >= 65").
const LTC_MIN_AGE_MONTHS: u32 = 65 * 12;

/// Lower edges in years of the duration bins of `long_term_care.model
/// .duration_years_distribution.bins` ("<=2", "2-4", ..., ">10"). Each bin is drawn
/// uniformly on `[edge, 2 * value - edge]` so its mean is the representative value
/// in the data (0.75, 3, 5, 7, 9, 11.5), which reproduces the implied mean duration.
const LTC_BIN_LOWER_EDGES: [f64; 6] = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0];

/// Share of the person's ordinary living expenses that stops while they live in
/// assisted living or a nursing home (`long_term_care.model
/// .offset_to_other_spending`). The spending module applies it when
/// [`HealthMonth::ltc_in_facility`] is set.
pub const LTC_FACILITY_SPENDING_OFFSET: f64 = 0.5;

/// 64-bit draws consumed by [`HealthModel::initial_state`].
pub const INITIAL_DRAWS: u32 = 5;

/// 64-bit draws consumed by every [`HealthModel::step`] call.
pub const STEP_DRAWS: u32 = 2;

/// Months of the excess-inflation table (120 years); later months are computed
/// directly.
const GROWTH_TABLE_MONTHS: usize = 120 * 12 + 1;

/// Mortality multiplier of `HealthStatus::Good`, the schema default: between the
/// population table (1.0) and the research's `healthy_high_income` preset (0.75,
/// which the research proposes as the default for this audience). A judgement: most
/// planners are healthier than average, not all are in the top income quartile.
pub const GOOD_HEALTH_MULTIPLIER: f64 = 0.85;

/// Highest age with its own entry in the per-age cost tables.
const MAX_TABLE_AGE: usize = 120;

fn num(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for key in path {
        cur = &cur[*key];
    }
    cur.as_f64()
        .unwrap_or_else(|| panic!("embedded data: missing number at {}", path.join(".")))
}

/// Standard normal quantile (Acklam's rational approximation, relative error below
/// 1.2e-9). Deterministic: only `ln`, `sqrt` and arithmetic. One uniform in, one
/// normal out, so the number of draws never depends on the values drawn (the polar
/// method in `rng` rejects a variable number of pairs).
fn inverse_normal_cdf(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const P_LOW: f64 = 0.024_25;
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < P_LOW {
        tail((-2.0 * math::ln(p)).sqrt())
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -tail((-2.0 * math::ln(1.0 - p)).sqrt())
    }
}

/// Standard normal distribution function (pure-Rust `erfc`, deterministic).
fn normal_cdf(x: f64) -> f64 {
    0.5 * math::erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

/// One uniform strictly inside `(0, 1)` from exactly one 64-bit draw.
#[inline]
fn open_uniform(rng: &mut Rng) -> f64 {
    const UNIT: f64 = 1.0 / (1u64 << 53) as f64;
    ((rng.next_u64() >> 11) as f64 + 0.5) * UNIT
}

// ---------------------------------------------------------------------------
// Mortality
// ---------------------------------------------------------------------------

/// Compiled mortality model: the survival curve of one person from the plan's start
/// month. Built in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct MortalityModel {
    /// `survival[k]` is the probability of being alive `k` months after `start`.
    /// Starts at 1 and ends at exactly 0.
    survival: Vec<f64>,
    /// Age in whole months at `start`.
    age_months_at_start: u32,
    /// Remaining life expectancy in years at `start`.
    life_expectancy: f64,
}

impl MortalityModel {
    /// Builds the cohort survival curve of a person from the life table.
    ///
    /// For month `k` after `start`, with `age` the person's age in whole years and
    /// `year` the calendar year of that month,
    ///
    /// ```text
    /// q = min(1, m * q_base(sex, age) * (1 - r_near)^(min(year, 2050) - base_year)
    ///                                 * (1 - r_ult)^max(0, year - 2050))
    /// ```
    ///
    /// and the month's survival probability is `(1 - q)^(1/12)` (constant force of
    /// mortality within the year of age). `m` is the health multiplier from
    /// `params.json` `mortality.health_multiplier.presets`: excellent 0.75
    /// (`healthy_high_income`), good 0.85 ([`GOOD_HEALTH_MULTIPLIER`]), average 1.0,
    /// poor 1.5. Improvement factors are 1 when
    /// `improvement` is off. Nobody outlives the last age of the table.
    ///
    /// `Longevity::FixedAge` gives a curve that is 1 until the month the person
    /// reaches that age and 0 from then on (at least one month from `start`).
    pub fn new(
        birth: YearMonth,
        sex: Sex,
        health: HealthStatus,
        start: YearMonth,
        longevity: &Longevity,
        data: &Data,
    ) -> MortalityModel {
        let age0 = birth.age_months_at(start).max(0) as u32;
        let survival = match *longevity {
            Longevity::FixedAge { age } => {
                let death_age = (age * 12.0).round().max(0.0) as u32;
                let months = death_age.saturating_sub(age0).max(1) as usize;
                let mut s = vec![1.0; months];
                s.push(0.0);
                s
            }
            Longevity::Stochastic { improvement } => {
                Self::cohort_curve(age0, sex, health, start, improvement, data)
            }
        };
        let mut sum = 0.0;
        for pair in survival.windows(2) {
            sum += 0.5 * (pair[0] + pair[1]);
        }
        MortalityModel {
            survival,
            age_months_at_start: age0,
            life_expectancy: sum / 12.0,
        }
    }

    fn cohort_curve(
        age0: u32,
        sex: Sex,
        health: HealthStatus,
        start: YearMonth,
        improvement: bool,
        data: &Data,
    ) -> Vec<f64> {
        let mortality = &data.actuarial["mortality"];
        let presets = &mortality["health_multiplier"]["presets"];
        let multiplier = match health {
            HealthStatus::Excellent => num(presets, &["healthy_high_income"]),
            HealthStatus::Good => GOOD_HEALTH_MULTIPLIER,
            HealthStatus::Average => num(presets, &["average"]),
            HealthStatus::Poor => num(presets, &["poor_health"]),
        };
        let base_year = num(mortality, &["base_table", "base_year"]) as i32;
        let end_age_months = (data.life_table.max_age() as u32 + 1) * 12;
        let months = end_age_months.saturating_sub(age0).max(1) as usize;

        let mut survival = Vec::with_capacity(months + 1);
        survival.push(1.0);
        let mut alive = 1.0;
        let mut cached = (u32::MAX, i32::MIN);
        let mut monthly_survival = 0.0;
        for k in 0..months - 1 {
            let age = (age0 + k as u32) / 12;
            let year = (start + k as i32).year();
            if cached != (age, year) {
                cached = (age, year);
                let mut q = data.life_table.qx(sex, age as usize) * multiplier;
                if improvement {
                    if let Some(band) = data
                        .mortality_improvement
                        .iter()
                        .find(|b| b.sex == sex && age >= b.age_from && age <= b.age_to)
                    {
                        let near = (year.min(IMPROVEMENT_SWITCH_YEAR) - base_year).max(0);
                        let ultimate = (year - IMPROVEMENT_SWITCH_YEAR).max(0);
                        q *= math::powi(1.0 - band.reduction_to_2050, near)
                            * math::powi(1.0 - band.reduction_after_2050, ultimate);
                    }
                }
                monthly_survival = if q >= 1.0 {
                    0.0
                } else {
                    math::exp(math::ln_1p(-q.max(0.0)) / 12.0)
                };
            }
            alive *= monthly_survival;
            survival.push(alive);
        }
        survival.push(0.0);
        survival
    }

    /// Number of months of life from `start`, drawn with exactly one uniform: the
    /// person is alive in months `0..n` of the simulation and dead from month `n`
    /// (`n >= 1`). `P(n > k) = survival(k)`. With `Longevity::FixedAge` the draw is
    /// consumed and ignored. A larger uniform means an earlier death, on every
    /// scenario alike.
    pub fn draw_months_to_death(&self, rng: &mut Rng) -> u32 {
        let u = rng.uniform();
        // The curve is non-increasing and ends at 0 <= u, so the index exists.
        self.survival.partition_point(|&s| s > u) as u32
    }

    /// Probability of being alive `months` after `start` (0 beyond the table).
    pub fn survival(&self, months: u32) -> f64 {
        self.survival.get(months as usize).copied().unwrap_or(0.0)
    }

    /// The whole curve: entry `k` is [`MortalityModel::survival`]`(k)`. Slice it from
    /// the current month to feed `SocialSecurityModel::present_value`.
    pub fn survival_table(&self) -> &[f64] {
        &self.survival
    }

    /// Remaining life expectancy in years at `start`.
    pub fn life_expectancy(&self) -> f64 {
        self.life_expectancy
    }

    /// Age in whole months at `start`.
    pub fn age_months_at_start(&self) -> u32 {
        self.age_months_at_start
    }

    /// The largest value [`MortalityModel::draw_months_to_death`] can return.
    pub fn max_months(&self) -> u32 {
        (self.survival.len() - 1) as u32
    }
}

// ---------------------------------------------------------------------------
// Health costs
// ---------------------------------------------------------------------------

/// Where a person's health insurance comes from this month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CoveragePhase {
    /// An employer plan (their own or their partner's).
    #[default]
    Employer,
    /// COBRA continuation after leaving a job.
    Cobra,
    /// An ACA marketplace plan.
    Aca,
    /// Medicare with Part B, Part D and supplemental coverage.
    Medicare,
    /// A fixed user-specified cost.
    Custom,
    /// Outside the modelled system (healthcare is in the budget).
    None,
}

/// Kind of paid long-term care in an episode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CareType {
    /// No episode on this path.
    #[default]
    None,
    /// Paid care at home.
    Home,
    /// Assisted living facility.
    AssistedLiving,
    /// Nursing home.
    NursingHome,
    /// `health_risk` is off: the probability-weighted average cost over the average
    /// duration, not a real episode.
    Expected,
}

/// One person's health costs for one month, **nominal** dollars (see the module
/// documentation).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HealthMonth {
    /// Phase the costs belong to.
    pub phase: CoveragePhase,
    /// Premium before any subsidy, for this person for the month.
    pub premium: f64,
    /// ACA benchmark (second-lowest silver) premium for this person for the month,
    /// 0 off-marketplace. Sum over the covered people and months of the year to get
    /// `TaxInputs::aca_benchmark_premium`.
    pub aca_benchmark: f64,
    /// Out-of-pocket medical spending.
    pub out_of_pocket: f64,
    /// Long-term care cost.
    pub long_term_care: f64,
    /// True in the month a drawn long-term care episode starts (for the event log).
    pub ltc_started: bool,
    /// True while the person lives in assisted living or a nursing home; the
    /// spending module then cuts their share of ordinary living expenses by
    /// [`LTC_FACILITY_SPENDING_OFFSET`].
    pub ltc_in_facility: bool,
}

impl HealthMonth {
    /// Premium plus out-of-pocket plus long-term care.
    pub fn total(&self) -> f64 {
        self.premium + self.out_of_pocket + self.long_term_care
    }
}

/// Per-path health state of one person. Create it with
/// [`HealthModel::initial_state`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HealthState {
    /// First simulation month (months since start) of the long-term care episode;
    /// `u32::MAX` when there is none.
    pub ltc_start_month: u32,
    /// First month after the episode (the month of death).
    pub ltc_end_month: u32,
    /// Cost of care in today's dollars per month, before excess inflation.
    pub ltc_monthly_cost: f64,
    /// Kind of care.
    pub ltc_care: CareType,
    /// Persistent component of log out-of-pocket spending in units of the phase's
    /// sigma (people with a bad year tend to have another).
    pub oop_persistent: f64,
    /// This year's total standardised shock (persistent plus transitory), mean 0 and
    /// variance 1.
    pub oop_shock: f64,
    /// The year's out-of-pocket amount for the plan and age it was last priced for
    /// (`oop_cache_key`), so the lognormal is evaluated once a year, not monthly.
    pub oop_cache: f64,
    /// Identifies what `oop_cache` was priced for; `u32::MAX` when empty.
    pub oop_cache_key: u32,
}

impl Default for HealthState {
    fn default() -> Self {
        Self {
            ltc_start_month: u32::MAX,
            ltc_end_month: 0,
            ltc_monthly_cost: 0.0,
            ltc_care: CareType::None,
            oop_persistent: 0.0,
            oop_shock: 0.0,
            oop_cache: 0.0,
            oop_cache_key: u32::MAX,
        }
    }
}

impl HealthState {
    /// Months of paid care on this path (0 when there is no episode).
    pub fn ltc_months(&self) -> u32 {
        if self.ltc_start_month == u32::MAX {
            0
        } else {
            self.ltc_end_month.saturating_sub(self.ltc_start_month)
        }
    }
}

/// Facts about a person this month that decide their coverage and price their costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HealthContext {
    /// Age in years.
    pub age: f64,
    /// True if this person or their partner has an employer plan this month.
    pub employer_coverage: bool,
    /// Months since employer coverage ended (for COBRA's 18-month limit). Use
    /// `u32::MAX` for someone who never had employer coverage in the simulation.
    pub months_since_coverage_ended: u32,
    /// True if the household lives in the US.
    pub in_us: bool,
    /// Months since the plan's start month (0 in the first simulated month). Drives
    /// excess medical inflation, the annual out-of-pocket draw (refreshed whenever
    /// this is a multiple of 12) and the long-term care schedule.
    pub months_since_start: u32,
    /// CPI level relative to the start month (1.0 at start).
    pub price_level: f64,
}

impl Default for HealthContext {
    fn default() -> Self {
        Self {
            age: 0.0,
            employer_coverage: false,
            months_since_coverage_ended: u32::MAX,
            in_us: true,
            months_since_start: 0,
            price_level: 1.0,
        }
    }
}

/// A capped lognormal for one year of out-of-pocket spending.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct OopParams {
    /// Mean after the cap, today's dollars per year.
    mean: f64,
    /// Location of the underlying lognormal, chosen so the capped mean is `mean`.
    mu: f64,
    /// Log standard deviation.
    sigma: f64,
    /// Out-of-pocket maximum (`f64::INFINITY` for none).
    cap: f64,
}

impl OopParams {
    fn new(mean: f64, sigma: f64, cap: f64) -> OopParams {
        if mean <= 0.0 {
            return OopParams {
                mean: 0.0,
                mu: f64::NEG_INFINITY,
                sigma,
                cap,
            };
        }
        if !cap.is_finite() || sigma <= 0.0 {
            return OopParams {
                mean,
                mu: math::ln(mean) - 0.5 * sigma * sigma,
                sigma,
                cap,
            };
        }
        let mean = mean.min(MAX_MEAN_SHARE_OF_CAP * cap);
        let ln_cap = math::ln(cap);
        // E[min(exp(mu + sigma Z), cap)], increasing in mu.
        let capped_mean = |mu: f64| {
            math::exp(mu + 0.5 * sigma * sigma) * normal_cdf((ln_cap - mu) / sigma - sigma)
                + cap * (1.0 - normal_cdf((ln_cap - mu) / sigma))
        };
        let mut lo = math::ln(mean) - 0.5 * sigma * sigma;
        let mut hi = ln_cap + 8.0 * sigma;
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if capped_mean(mid) < mean {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        OopParams {
            mean,
            mu: 0.5 * (lo + hi),
            sigma,
            cap,
        }
    }

    /// The year's spending for a standardised shock `z`.
    #[inline]
    fn draw(&self, z: f64) -> f64 {
        if self.mean <= 0.0 {
            0.0
        } else {
            math::exp(self.mu + self.sigma * z).min(self.cap)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EarlyCoverage {
    Aca,
    Cobra,
    /// Total cost per month, today's dollars.
    Custom(f64),
}

/// Long-term care parameters for one sex.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct LtcSex {
    /// Added to the logit of the episode probability.
    shift: f64,
    /// Cumulative probability of the duration bins.
    duration_cdf: [f64; 6],
    /// Mean duration in years.
    mean_duration: f64,
}

/// Compiled health cost model for the household.
#[derive(Clone, Debug)]
pub struct HealthModel {
    modeled: bool,
    risk: bool,
    ltc_enabled: bool,
    early: EarlyCoverage,
    medicare_age: f64,
    /// Employee share of an employer plan, per month.
    employer_premium: f64,
    /// Full cost of the employer plan plus the COBRA load, per month.
    cobra_premium: f64,
    /// Benchmark silver premium per month by age (index = age, last entry for older).
    aca_benchmark: Vec<f64>,
    /// Premium of the chosen metal tier relative to the benchmark.
    tier_premium_factor: f64,
    /// Part B + Part D + Medigap per month at 65.
    medicare_premium: f64,
    /// Multiplier on every Medicare-phase cost by age (index = age; 1.0 up to 65).
    medicare_age_factor: Vec<f64>,
    oop_employer: Vec<OopParams>,
    oop_aca: Vec<OopParams>,
    oop_medicare: OopParams,
    /// AR(1) coefficient of the persistent shock, per year.
    rho: f64,
    /// Innovation scale of the persistent shock (sigma units).
    persistent_innovation_sd: f64,
    /// Stationary standard deviation of the persistent shock (sigma units).
    persistent_sd: f64,
    /// Standard deviation of the transitory shock (sigma units).
    transitory_sd: f64,
    /// `(1 + excess)^(k / 12)` for month `k`.
    excess_growth: Vec<f64>,
    /// `ln(1 + excess) / 12`.
    excess_log_monthly: f64,
    ltc_intercept: f64,
    ltc_slope: f64,
    ltc_reference_age: f64,
    ltc_max_probability: f64,
    /// Indexed female, male, unspecified.
    ltc_sex: [LtcSex; 3],
    /// Representative duration in years of each bin.
    ltc_bin_values: [f64; 6],
    /// Cumulative probability of home care, then assisted living (the rest is a
    /// nursing home).
    ltc_type_cdf: [f64; 2],
    /// Cost per month by type: home, assisted living, nursing home.
    ltc_monthly_cost: [f64; 3],
    /// Mix-weighted cost per month.
    ltc_blended_monthly: f64,
    /// Monthly increments for putting children on the household's plan.
    child: children::ChildCoverage,
}

fn sex_index(sex: Sex) -> usize {
    match sex {
        Sex::Female => 0,
        Sex::Male => 1,
        Sex::Unspecified => 2,
    }
}

impl HealthModel {
    /// Compiles premiums and cost distributions for the household's settings.
    /// `risk_enabled` is `assumptions.health_risk`.
    ///
    /// `state` is accepted for the day the data carries state premium factors or
    /// state age curves; the embedded data has national averages only, so it is not
    /// used yet.
    pub fn new(
        settings: &Healthcare,
        state: Option<&str>,
        risk_enabled: bool,
        data: &Data,
    ) -> HealthModel {
        // National figures only: see the doc comment.
        let _ = state;
        let hc = &data.actuarial["healthcare"];
        let aca = &data.tax_federal["aca"];
        let medicare_age = num(&data.tax_federal["medicare"], &["eligibility_age"]);

        // Employer plan (KFF survey).
        let employer = &hc["employer_phase"]["premiums_2026"];
        let employer_premium = num(employer, &["worker_contribution_single"]) / 12.0;
        let cobra_premium =
            num(employer, &["average_total_premium_single"]) * COBRA_PREMIUM_LOAD / 12.0;

        // ACA benchmark: latest published plan year, scaled by the CMS age curve.
        let by_year = aca["benchmark_premium"]["monthly_age_40"]
            .as_object()
            .expect("embedded data: aca.benchmark_premium.monthly_age_40");
        let latest_year = by_year
            .keys()
            .filter_map(|k| k.parse::<i32>().ok())
            .max()
            .expect("embedded data: no ACA benchmark year");
        let benchmark_40 = by_year[&latest_year.to_string()]
            .as_f64()
            .expect("embedded data: ACA benchmark premium");
        let curve = &aca["age_rating_curve"]["factor_by_age"];
        let factor = |age: usize| num(curve, &[&age.to_string()]);
        let last_curve_age = curve
            .as_object()
            .expect("embedded data: aca.age_rating_curve.factor_by_age")
            .keys()
            .filter_map(|k| k.parse::<usize>().ok())
            .max()
            .expect("embedded data: empty ACA age curve");
        let factor_40 = factor(40);
        let aca_benchmark: Vec<f64> = (0..=last_curve_age)
            .map(|age| benchmark_40 * factor(age) / factor_40)
            .collect();
        let av = &aca["cost_sharing"]["actuarial_value"];
        // Premiums are taken as proportional to actuarial value (the share of
        // covered costs the plan pays); silver is the benchmark.
        let (tier_premium_factor, tier_oop_factor) = match settings.quality {
            PlanTier::Bronze => (
                num(av, &["bronze"]) / num(av, &["silver"]),
                BRONZE_OOP_FACTOR,
            ),
            PlanTier::Silver => (1.0, 1.0),
            PlanTier::Gold => (num(av, &["gold"]) / num(av, &["silver"]), GOLD_OOP_FACTOR),
        };
        let aca_oop_max = {
            let caps = &aca["cost_sharing"]["out_of_pocket_maximum"];
            let key = latest_year.to_string();
            if caps[&key].is_object() {
                num(caps, &[&key, "self_only"])
            } else {
                num(
                    &hc["aca_phase"]["out_of_pocket_maximum_2026"],
                    &["self_only"],
                )
            }
        };

        // Out-of-pocket means by age band before 65.
        let variance = &hc["variance_model"];
        let sigma_pre = num(variance, &["sigma_pre_65"]);
        let means = &hc["out_of_pocket_by_age_pre_65"]["mean_annual_oop_per_person_2026_dollars"];
        let bands: Vec<(usize, usize, f64)> = means
            .as_object()
            .expect("embedded data: out-of-pocket means by age")
            .iter()
            .map(|(band, v)| {
                let (lo, hi) = band
                    .split_once('-')
                    .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                    .unwrap_or_else(|| panic!("embedded data: bad age band {band:?}"));
                (
                    lo,
                    hi,
                    v.as_f64().expect("embedded data: out-of-pocket mean"),
                )
            })
            .collect();
        let pre65_mean = |age: usize| {
            bands
                .iter()
                .find(|(lo, hi, _)| age >= *lo && age <= *hi)
                .or_else(|| bands.iter().max_by_key(|(_, hi, _)| *hi))
                .map_or(0.0, |(_, _, m)| *m)
        };
        let table = |scale: f64, cap: f64| -> Vec<OopParams> {
            let mut out: Vec<OopParams> = Vec::with_capacity(MAX_TABLE_AGE + 1);
            for age in 0..=MAX_TABLE_AGE {
                let mean = pre65_mean(age) * scale;
                let params = match out.last() {
                    Some(prev) if pre65_mean(age - 1) == pre65_mean(age) => *prev,
                    _ => OopParams::new(mean, sigma_pre, cap),
                };
                out.push(params);
            }
            out
        };
        let oop_employer = table(1.0, EMPLOYER_OOP_MAX);
        let oop_aca = table(tier_oop_factor, aca_oop_max);

        // Medicare with Medigap.
        let parts = &hc["medicare_phase"]["components_2026_per_person_annual"];
        let medicare_premium = (num(parts, &["part_b_premium"])
            + num(parts, &["part_d_premium"])
            + num(parts, &["medigap_plan_g_premium"]))
            / 12.0;
        let medicare_oop = num(parts, &["part_b_deductible"])
            + num(parts, &["drugs_out_of_pocket"])
            + num(parts, &["dental_vision_hearing_other"]);
        let oop_medicare = OopParams::new(
            medicare_oop,
            num(variance, &["sigma_medicare_with_medigap"]),
            f64::INFINITY,
        );
        // "Real cost rises about 1.5%/yr with age after 65" (medicare_phase
        // .age_gradient, a sentence in the data): the same number as the excess
        // medical inflation default, read from there.
        let excess = num(hc, &["excess_medical_inflation", "default_pct_over_cpi"]) / 100.0;
        let age_gradient = excess;
        let medicare_age_factor: Vec<f64> = (0..=MAX_TABLE_AGE)
            .map(|age| {
                math::powi(
                    1.0 + age_gradient,
                    (age as i32 - medicare_age as i32).max(0),
                )
            })
            .collect();

        let rho = num(variance, &["rho_persistent"]);
        let share = num(variance, &["share_persistent"]);
        let excess_log_monthly = math::ln_1p(excess) / 12.0;
        let excess_growth = (0..GROWTH_TABLE_MONTHS)
            .map(|k| math::exp(excess_log_monthly * k as f64))
            .collect();

        // Long-term care.
        let ltc = &data.actuarial["long_term_care"]["model"];
        let (intercept, slope, reference_age, max_probability) =
            parse_ltc_probability(ltc["probability_of_episode"].as_str().unwrap_or(""));
        let durations = &ltc["duration_years_distribution"];
        let array6 = |v: &Value, what: &str| -> [f64; 6] {
            let a = v
                .as_array()
                .unwrap_or_else(|| panic!("embedded data: {what}"));
            assert_eq!(a.len(), 6, "embedded data: {what} must have 6 bins");
            let mut out = [0.0; 6];
            for (slot, x) in out.iter_mut().zip(a) {
                *slot = x
                    .as_f64()
                    .unwrap_or_else(|| panic!("embedded data: {what}"));
            }
            out
        };
        let ltc_bin_values = array6(&durations["values"], "long-term care duration values");
        let male_p = array6(&durations["male"], "long-term care male durations");
        let female_p = array6(&durations["female"], "long-term care female durations");
        let per_sex = |shift: f64, p: [f64; 6]| {
            let total: f64 = p.iter().sum();
            let mut cdf = [0.0; 6];
            let mut acc = 0.0;
            let mut mean = 0.0;
            for i in 0..6 {
                acc += p[i] / total;
                cdf[i] = acc;
                mean += p[i] / total * ltc_bin_values[i];
            }
            cdf[5] = 1.0;
            LtcSex {
                shift,
                duration_cdf: cdf,
                mean_duration: mean,
            }
        };
        let shift_m = num(ltc, &["shift_sex", "male"]);
        let shift_f = num(ltc, &["shift_sex", "female"]);
        let mut blend = [0.0; 6];
        for i in 0..6 {
            blend[i] = 0.5 * (male_p[i] + female_p[i]);
        }
        let ltc_sex = [
            per_sex(shift_f, female_p),
            per_sex(shift_m, male_p),
            per_sex(0.5 * (shift_f + shift_m), blend),
        ];
        let mix = &ltc["care_type_mix"];
        let cost = &ltc["annual_cost_by_type_2025_dollars"];
        let shares = [
            num(mix, &["home_care_only"]),
            num(mix, &["assisted_living"]),
            num(mix, &["nursing_home"]),
        ];
        let share_total: f64 = shares.iter().sum();
        let ltc_monthly_cost = [
            num(cost, &["home_care_only"]) / 12.0,
            num(cost, &["assisted_living"]) / 12.0,
            num(cost, &["nursing_home"]) / 12.0,
        ];
        let ltc_blended_monthly = shares
            .iter()
            .zip(&ltc_monthly_cost)
            .map(|(s, c)| s / share_total * c)
            .sum();

        HealthModel {
            modeled: settings.mode == HealthcareMode::Modeled,
            risk: risk_enabled,
            ltc_enabled: settings.long_term_care,
            early: match settings.early_retirement {
                EarlyRetirementCoverage::Aca => EarlyCoverage::Aca,
                EarlyRetirementCoverage::Cobra => EarlyCoverage::Cobra,
                EarlyRetirementCoverage::Custom { annual } => {
                    EarlyCoverage::Custom(annual.max(0.0) / 12.0)
                }
            },
            medicare_age,
            employer_premium,
            cobra_premium,
            aca_benchmark,
            tier_premium_factor,
            medicare_premium,
            medicare_age_factor,
            oop_employer,
            oop_aca,
            oop_medicare,
            rho,
            persistent_innovation_sd: (share * (1.0 - rho * rho)).sqrt(),
            persistent_sd: share.sqrt(),
            transitory_sd: (1.0 - share).sqrt(),
            excess_growth,
            excess_log_monthly,
            ltc_intercept: intercept,
            ltc_slope: slope,
            ltc_reference_age: reference_age,
            ltc_max_probability: max_probability,
            ltc_sex,
            ltc_bin_values,
            ltc_type_cdf: [
                shares[0] / share_total,
                (shares[0] + shares[1]) / share_total,
            ],
            ltc_monthly_cost,
            ltc_blended_monthly,
            child: children::ChildCoverage::new(employer),
        }
    }

    /// The coverage phase implied by a person's situation.
    ///
    /// * Healthcare `in_budget`: always `None`.
    /// * Employer coverage (own or partner's) wins at any age.
    /// * In the US from 65: `Medicare`.
    /// * In the US before 65 without employer coverage: `Custom` if the profile
    ///   gives a custom cost; `Cobra` for the first [`COBRA_MONTHS`] months after
    ///   coverage ended when the profile chose COBRA; otherwise `Aca`.
    /// * Outside the US without employer coverage: `Custom` at every age if the
    ///   profile gives a custom cost, otherwise `None` (the budget must carry it).
    pub fn phase(&self, ctx: &HealthContext) -> CoveragePhase {
        if !self.modeled {
            return CoveragePhase::None;
        }
        if ctx.employer_coverage {
            return CoveragePhase::Employer;
        }
        if !ctx.in_us {
            return match self.early {
                EarlyCoverage::Custom(_) => CoveragePhase::Custom,
                _ => CoveragePhase::None,
            };
        }
        if ctx.age >= self.medicare_age {
            return CoveragePhase::Medicare;
        }
        match self.early {
            EarlyCoverage::Custom(_) => CoveragePhase::Custom,
            EarlyCoverage::Cobra if ctx.months_since_coverage_ended < COBRA_MONTHS => {
                CoveragePhase::Cobra
            }
            _ => CoveragePhase::Aca,
        }
    }

    /// `(1 + excess medical inflation)^(months / 12)`: growth of health and care
    /// prices over CPI since the start month.
    #[inline]
    pub fn excess_growth(&self, months_since_start: u32) -> f64 {
        match self.excess_growth.get(months_since_start as usize) {
            Some(g) => *g,
            None => math::exp(self.excess_log_monthly * f64::from(months_since_start)),
        }
    }

    /// Unsubsidised ACA benchmark (second-lowest-cost silver) premium for one person
    /// of this age, today's dollars per month, before excess medical inflation.
    #[inline]
    pub fn aca_benchmark_monthly(&self, age: f64) -> f64 {
        let i = (age.max(0.0) as usize).min(self.aca_benchmark.len() - 1);
        self.aca_benchmark[i]
    }

    /// Premium and the out-of-pocket distribution for a phase at an age, today's
    /// dollars (premium per month, distribution per year), with the Medicare age
    /// gradient applied as `scale`.
    #[inline]
    fn price(&self, age: f64, phase: CoveragePhase) -> (f64, OopParams, f64) {
        let i = (age.max(0.0) as usize).min(MAX_TABLE_AGE);
        match phase {
            CoveragePhase::Employer => (self.employer_premium, self.oop_employer[i], 1.0),
            CoveragePhase::Cobra => (self.cobra_premium, self.oop_employer[i], 1.0),
            CoveragePhase::Aca => (
                self.aca_benchmark_monthly(age) * self.tier_premium_factor,
                self.oop_aca[i],
                1.0,
            ),
            CoveragePhase::Medicare => (
                self.medicare_premium,
                self.oop_medicare,
                self.medicare_age_factor[i],
            ),
            CoveragePhase::Custom => match self.early {
                EarlyCoverage::Custom(monthly) => (monthly, OopParams::default(), 1.0),
                _ => (0.0, OopParams::default(), 1.0),
            },
            CoveragePhase::None => (0.0, OopParams::default(), 1.0),
        }
    }

    /// Probability that someone of this sex who dies at `age_at_death` (years)
    /// spends the end of their life in paid long-term care.
    pub fn ltc_probability(&self, sex: Sex, age_at_death: f64) -> f64 {
        if age_at_death * 12.0 < f64::from(LTC_MIN_AGE_MONTHS) {
            return 0.0;
        }
        let x = self.ltc_intercept
            + self.ltc_slope * (age_at_death - self.ltc_reference_age)
            + self.ltc_sex[sex_index(sex)].shift;
        (1.0 / (1.0 + math::exp(-x))).min(self.ltc_max_probability)
    }

    /// Mix-weighted cost of a year of paid care, today's dollars (care-survey year
    /// dollars used as today's), before excess inflation.
    pub fn ltc_blended_annual_cost(&self) -> f64 {
        self.ltc_blended_monthly * 12.0
    }

    /// The person's health state at the start of a path: the long-term care episode
    /// (scheduled backwards from the death month, which the simulation knows from
    /// `MortalityModel::draw_months_to_death`) and the starting level of the
    /// persistent medical-cost shock.
    ///
    /// Consumes exactly [`INITIAL_DRAWS`] draws from `rng` (`Stream::Health(person)`),
    /// always. `age_months_at_start` is the age in whole months in the start month.
    ///
    /// Episode: with probability [`HealthModel::ltc_probability`] at the age at
    /// death, paid care lasts a duration drawn from the data's distribution and ends
    /// at death; it cannot begin before age 65 or before the start month. One care
    /// type per episode. With `health_risk` off the episode is replaced by its
    /// expected value: the mean duration at probability times the blended cost.
    pub fn initial_state(
        &self,
        sex: Sex,
        age_months_at_start: u32,
        months_to_death: u32,
        rng: &mut Rng,
    ) -> HealthState {
        let u_occurs = open_uniform(rng);
        let u_duration = open_uniform(rng);
        let u_type = open_uniform(rng);
        let u_persistent = open_uniform(rng);
        let u_transitory = open_uniform(rng);

        let mut state = HealthState::default();
        if self.risk {
            state.oop_persistent = self.persistent_sd * inverse_normal_cdf(u_persistent);
            state.oop_shock =
                state.oop_persistent + self.transitory_sd * inverse_normal_cdf(u_transitory);
        }
        if !self.ltc_enabled {
            return state;
        }
        let death_age_months = age_months_at_start.saturating_add(months_to_death);
        if death_age_months <= LTC_MIN_AGE_MONTHS {
            return state;
        }
        let p = self.ltc_probability(sex, f64::from(death_age_months) / 12.0);
        let per_sex = &self.ltc_sex[sex_index(sex)];
        let (years, monthly_cost, care) = if self.risk {
            if u_occurs >= p {
                return state;
            }
            let bin = per_sex
                .duration_cdf
                .iter()
                .position(|&c| u_duration < c)
                .unwrap_or(5);
            let below = if bin == 0 {
                0.0
            } else {
                per_sex.duration_cdf[bin - 1]
            };
            let within = (u_duration - below) / (per_sex.duration_cdf[bin] - below);
            let lo = LTC_BIN_LOWER_EDGES[bin];
            let hi = 2.0 * self.ltc_bin_values[bin] - lo;
            let (cost, care) = if u_type < self.ltc_type_cdf[0] {
                (self.ltc_monthly_cost[0], CareType::Home)
            } else if u_type < self.ltc_type_cdf[1] {
                (self.ltc_monthly_cost[1], CareType::AssistedLiving)
            } else {
                (self.ltc_monthly_cost[2], CareType::NursingHome)
            };
            (lo + (hi - lo) * within, cost, care)
        } else {
            (
                per_sex.mean_duration,
                p * self.ltc_blended_monthly,
                CareType::Expected,
            )
        };
        let wanted = ((years * 12.0).round() as u32).max(1);
        let since_65 = death_age_months - LTC_MIN_AGE_MONTHS;
        let months = wanted.min(since_65).min(months_to_death);
        if months == 0 {
            return state;
        }
        state.ltc_start_month = months_to_death - months;
        state.ltc_end_month = months_to_death;
        state.ltc_monthly_cost = monthly_cost;
        state.ltc_care = care;
        state
    }

    /// One person's costs for the month in nominal dollars, advancing their state.
    ///
    /// Consumes exactly [`STEP_DRAWS`] draws from `rng`, always. The out-of-pocket
    /// shock is annual: it is redrawn when `ctx.months_since_start` is a multiple of
    /// 12 and the year's amount is spread evenly over the months, repriced each
    /// month for the current phase and age:
    ///
    /// ```text
    /// p' = rho * p + w,  z = p' + u            (sigma units, Var[z] = 1)
    /// year's out-of-pocket = min(exp(mu + sigma * z), plan maximum)
    /// ```
    ///
    /// with `mu` solved at compile time so that the mean after the cap equals the
    /// configured mean for the age and plan. With `health_risk` off the configured
    /// mean is charged every year.
    pub fn step(&self, state: &mut HealthState, ctx: &HealthContext, rng: &mut Rng) -> HealthMonth {
        let u_persistent = open_uniform(rng);
        let u_transitory = open_uniform(rng);
        let t = ctx.months_since_start;
        if self.risk && t % 12 == 0 {
            state.oop_persistent = self.rho * state.oop_persistent
                + self.persistent_innovation_sd * inverse_normal_cdf(u_persistent);
            state.oop_shock =
                state.oop_persistent + self.transitory_sd * inverse_normal_cdf(u_transitory);
        }

        let phase = self.phase(ctx);
        let growth = self.excess_growth(t) * ctx.price_level;
        let (premium, oop, scale) = self.price(ctx.age, phase);
        let annual_oop = if self.risk {
            // The draw changes with the year's shock, the plan and the age band.
            let key = ((t / 12) << 12) | ((phase as u32) << 8) | (ctx.age.max(0.0) as u32 & 0xff);
            if state.oop_cache_key != key {
                state.oop_cache = oop.draw(state.oop_shock);
                state.oop_cache_key = key;
            }
            state.oop_cache
        } else {
            oop.mean
        };
        let mut out = HealthMonth {
            phase,
            premium: premium * scale * growth,
            aca_benchmark: if phase == CoveragePhase::Aca {
                self.aca_benchmark_monthly(ctx.age) * growth
            } else {
                0.0
            },
            out_of_pocket: annual_oop / 12.0 * scale * growth,
            ..HealthMonth::default()
        };
        if t >= state.ltc_start_month && t < state.ltc_end_month {
            out.long_term_care = state.ltc_monthly_cost * growth;
            out.ltc_started = t == state.ltc_start_month && state.ltc_care != CareType::Expected;
            out.ltc_in_facility = matches!(
                state.ltc_care,
                CareType::AssistedLiving | CareType::NursingHome
            );
        }
        out
    }

    /// Expected annual cost (premium before subsidy plus mean out-of-pocket, no
    /// long-term care) in today's dollars at an age in a phase, with no excess
    /// medical inflation: the cost someone of that age would face today.
    pub fn expected_annual_cost(&self, age: f64, phase: CoveragePhase) -> f64 {
        if !self.modeled {
            return 0.0;
        }
        let (premium, oop, scale) = self.price(age, phase);
        (premium * 12.0 + oop.mean) * scale
    }

    /// The two parts of [`HealthModel::expected_annual_cost_at`]: `(premiums, mean
    /// out-of-pocket)` per year.
    pub fn expected_annual_parts_at(
        &self,
        age: f64,
        phase: CoveragePhase,
        months_since_start: u32,
    ) -> (f64, f64) {
        if !self.modeled {
            return (0.0, 0.0);
        }
        let (premium, oop, scale) = self.price(age, phase);
        let growth = scale * self.excess_growth(months_since_start);
        (premium * 12.0 * growth, oop.mean * growth)
    }

    /// [`HealthModel::expected_annual_cost`] at a future month: today's dollars
    /// (deflated by CPI) including the excess medical inflation accumulated by then.
    /// This is the figure for the FI test's projection of retirement spending.
    pub fn expected_annual_cost_at(
        &self,
        age: f64,
        phase: CoveragePhase,
        months_since_start: u32,
    ) -> f64 {
        self.expected_annual_cost(age, phase) * self.excess_growth(months_since_start)
    }
}

/// Reads the coefficients of the episode probability from the formula string in
/// `params.json` (`long_term_care.model.probability_of_episode`):
/// `"p(sex, T) = min(0.90, logistic(-1.208 + 0.065 * (T - 70) + shift_sex) * ...)"`.
/// Returns `(intercept, slope, reference age, maximum probability)`.
fn parse_ltc_probability(formula: &str) -> (f64, f64, f64, f64) {
    fn number_after<'a>(text: &'a str, marker: &str) -> Option<(f64, &'a str)> {
        let rest = text[text.find(marker)? + marker.len()..].trim_start();
        let end = rest
            .char_indices()
            .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && c == '-')))
            .map_or(rest.len(), |(i, _)| i);
        Some((rest[..end].parse().ok()?, &rest[end..]))
    }
    let parsed = (|| {
        let (max_p, rest) = number_after(formula, "min(")?;
        let (intercept, rest) = number_after(rest, "logistic(")?;
        let (slope, rest) = number_after(rest, "+")?;
        let (reference, _) = number_after(rest, "(T -")?;
        Some((intercept, slope, reference, max_p))
    })();
    parsed.unwrap_or_else(|| {
        panic!("embedded data: cannot read long_term_care.model.probability_of_episode")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Stream;

    fn data() -> &'static Data {
        Data::embedded()
    }

    fn mortality(
        age: i32,
        sex: Sex,
        health: HealthStatus,
        improvement: bool,
        start_year: i32,
    ) -> MortalityModel {
        let start = YearMonth::ym(start_year, 1);
        MortalityModel::new(
            YearMonth::ym(start_year - age, 1),
            sex,
            health,
            start,
            &Longevity::Stochastic { improvement },
            data(),
        )
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn inverse_normal_matches_known_quantiles() {
        assert!(close(inverse_normal_cdf(0.5), 0.0, 1e-12));
        assert!(close(inverse_normal_cdf(0.975), 1.959_963_985, 1e-7));
        assert!(close(inverse_normal_cdf(0.001), -3.090_232_306, 1e-7));
        assert!(close(inverse_normal_cdf(0.84), 0.994_457_883, 1e-7));
        for &p in &[1e-9, 1e-4, 0.01, 0.2, 0.5, 0.77, 0.99, 1.0 - 1e-7] {
            assert!(close(
                normal_cdf(inverse_normal_cdf(p)),
                p,
                1e-8 * p.max(1e-3)
            ));
        }
    }

    #[test]
    fn period_life_expectancy_matches_ssa_table() {
        // SSA 2023 period table: e65 18.12 (male), 20.66 (female); e0 75.79 / 81.06.
        let check = &data().actuarial["mortality"]["base_table"]["check"];
        for (sex, key65, key0) in [
            (Sex::Male, "male_e65", "male_e0"),
            (Sex::Female, "female_e65", "female_e0"),
        ] {
            let e65 = mortality(65, sex, HealthStatus::Average, false, 2026).life_expectancy();
            assert!(close(e65, num(check, &[key65]), 0.1), "{sex:?} e65 {e65}");
            let e0 = mortality(0, sex, HealthStatus::Average, false, 2026).life_expectancy();
            assert!(close(e0, num(check, &[key0]), 0.1), "{sex:?} e0 {e0}");
        }
        let male = mortality(65, Sex::Male, HealthStatus::Average, false, 2026).life_expectancy();
        let female =
            mortality(65, Sex::Female, HealthStatus::Average, false, 2026).life_expectancy();
        let blend =
            mortality(65, Sex::Unspecified, HealthStatus::Average, false, 2026).life_expectancy();
        assert!(blend > male && blend < female);
    }

    #[test]
    fn cohort_life_expectancy_matches_the_research_table() {
        // params.json mortality.derived_life_expectancy_for_person_alive_in_2026:
        // (sex, age, health, le_cohort, le_period, p90, p95, p100).
        let cases = [
            (
                Sex::Male,
                65,
                HealthStatus::Average,
                19.2,
                18.1,
                0.295,
                0.122,
                0.030,
            ),
            (
                Sex::Male,
                65,
                HealthStatus::Excellent,
                21.7,
                20.5,
                0.404,
                0.212,
                0.077,
            ),
            (
                Sex::Male,
                65,
                HealthStatus::Poor,
                15.9,
                15.0,
                0.155,
                0.039,
                0.004,
            ),
            (
                Sex::Female,
                65,
                HealthStatus::Average,
                21.7,
                20.7,
                0.402,
                0.195,
                0.059,
            ),
            (
                Sex::Female,
                65,
                HealthStatus::Excellent,
                24.1,
                23.0,
                0.507,
                0.298,
                0.125,
            ),
            (
                Sex::Male,
                40,
                HealthStatus::Average,
                41.3,
                38.6,
                0.310,
                0.146,
                0.045,
            ),
            (
                Sex::Male,
                40,
                HealthStatus::Excellent,
                44.7,
                41.7,
                0.418,
                0.240,
                0.101,
            ),
            (
                Sex::Female,
                40,
                HealthStatus::Excellent,
                48.2,
                45.5,
                0.524,
                0.330,
                0.156,
            ),
            (
                Sex::Female,
                30,
                HealthStatus::Poor,
                51.0,
                47.9,
                0.287,
                0.115,
                0.025,
            ),
        ];
        for (sex, age, health, le_cohort, le_period, p90, p95, p100) in cases {
            let cohort = mortality(age, sex, health, true, 2026);
            let period = mortality(age, sex, health, false, 2026);
            let label = format!("{sex:?} {age} {health:?}");
            assert!(
                close(cohort.life_expectancy(), le_cohort, 0.1),
                "{label}: cohort {}",
                cohort.life_expectancy()
            );
            assert!(
                close(period.life_expectancy(), le_period, 0.1),
                "{label}: period {}",
                period.life_expectancy()
            );
            let alive_at = |a: i32| cohort.survival(((a - age) * 12) as u32);
            assert!(
                close(alive_at(90), p90, 0.005),
                "{label}: p90 {}",
                alive_at(90)
            );
            assert!(
                close(alive_at(95), p95, 0.005),
                "{label}: p95 {}",
                alive_at(95)
            );
            assert!(
                close(alive_at(100), p100, 0.005),
                "{label}: p100 {}",
                alive_at(100)
            );
        }
        // docs/research/stochastic-models.md: a man of 65 lives to 84.2 on average
        // (86.7 with the 0.75 multiplier); a woman of 40 to 85.2 (88.2).
        let m = mortality(65, Sex::Male, HealthStatus::Excellent, true, 2026);
        assert!(close(65.0 + m.life_expectancy(), 86.7, 0.1));
        let w = mortality(40, Sex::Female, HealthStatus::Average, true, 2026);
        assert!(close(40.0 + w.life_expectancy(), 85.2, 0.1));
    }

    #[test]
    fn survival_curve_is_a_proper_distribution() {
        let m = mortality(37, Sex::Female, HealthStatus::Average, true, 2027);
        let table = m.survival_table();
        assert_eq!(table[0], 1.0);
        assert_eq!(*table.last().unwrap(), 0.0);
        assert!(table.windows(2).all(|w| w[1] <= w[0]));
        assert_eq!(m.age_months_at_start(), 37 * 12);
        assert_eq!(m.max_months(), (120 - 37) * 12);
        assert_eq!(m.survival(m.max_months()), 0.0);
        assert_eq!(m.survival(u32::MAX), 0.0);
        // Improvement only lowers mortality.
        let frozen = mortality(37, Sex::Female, HealthStatus::Average, false, 2027);
        assert!(m.survival(600) > frozen.survival(600));
        // A mid-year birthday is handled in months.
        let odd = MortalityModel::new(
            YearMonth::ym(1961, 7),
            Sex::Male,
            HealthStatus::Average,
            YearMonth::ym(2026, 11),
            &Longevity::Stochastic { improvement: true },
            data(),
        );
        assert_eq!(odd.age_months_at_start(), 65 * 12 + 4);
        assert!(close(odd.life_expectancy(), 19.0, 0.3));
        // Someone not yet born is treated as a newborn.
        let unborn = MortalityModel::new(
            YearMonth::ym(2030, 1),
            Sex::Male,
            HealthStatus::Average,
            YearMonth::ym(2026, 11),
            &Longevity::default(),
            data(),
        );
        assert_eq!(unborn.age_months_at_start(), 0);
    }

    #[test]
    fn drawn_lifetimes_follow_the_survival_curve() {
        let m = mortality(55, Sex::Unspecified, HealthStatus::Excellent, true, 2026);
        let n = 200_000u32;
        let mut counts = vec![0u32; m.max_months() as usize + 2];
        let mut sum = 0.0;
        for path in 0..n {
            let mut rng = Rng::stream(11, u64::from(path), Stream::Longevity(0));
            let d = m.draw_months_to_death(&mut rng);
            assert!(d >= 1 && d <= m.max_months());
            counts[d as usize] += 1;
            sum += f64::from(d);
        }
        // Kolmogorov-Smirnov distance between the empirical and model survival.
        let mut alive = n;
        let mut worst: f64 = 0.0;
        for k in 0..=m.max_months() {
            alive -= counts[k as usize];
            worst = worst.max((f64::from(alive) / f64::from(n) - m.survival(k)).abs());
        }
        // 1% critical value is 1.63 / sqrt(n) = 0.0036.
        assert!(worst < 1.63 / f64::from(n).sqrt(), "KS distance {worst}");
        // Mean whole months lived is the life expectancy plus half a month.
        let mean_years = sum / f64::from(n) / 12.0;
        assert!(close(mean_years, m.life_expectancy() + 1.0 / 24.0, 0.08));
    }

    #[test]
    fn one_draw_per_lifetime_and_fixed_age() {
        let m = mortality(40, Sex::Male, HealthStatus::Average, true, 2026);
        let mut a = Rng::stream(5, 9, Stream::Longevity(1));
        let mut b = a.clone();
        let first = m.draw_months_to_death(&mut a);
        b.next_u64();
        assert_eq!(a.next_u64(), b.next_u64());
        let mut again = Rng::stream(5, 9, Stream::Longevity(1));
        assert_eq!(m.draw_months_to_death(&mut again), first);

        let fixed = MortalityModel::new(
            YearMonth::ym(1986, 5),
            Sex::Female,
            HealthStatus::Poor,
            YearMonth::ym(2026, 11),
            &Longevity::FixedAge { age: 95.0 },
            data(),
        );
        // 95 years after May 1986 is May 2081: 654 months after November 2026.
        let mut c = Rng::stream(5, 9, Stream::Longevity(1));
        let mut d = c.clone();
        assert_eq!(fixed.draw_months_to_death(&mut c), 654);
        d.next_u64();
        assert_eq!(c.next_u64(), d.next_u64());
        assert_eq!(fixed.survival(653), 1.0);
        assert_eq!(fixed.survival(654), 0.0);
        assert!(close(
            fixed.life_expectancy(),
            654.0 / 12.0 - 1.0 / 24.0,
            1e-9
        ));
        // Already past the fixed age: dies after the first month.
        let old = MortalityModel::new(
            YearMonth::ym(1920, 1),
            Sex::Male,
            HealthStatus::Average,
            YearMonth::ym(2026, 11),
            &Longevity::FixedAge { age: 90.0 },
            data(),
        );
        assert_eq!(old.draw_months_to_death(&mut c), 1);
    }

    fn model(settings: &Healthcare, risk: bool) -> HealthModel {
        HealthModel::new(settings, Some("CO"), risk, data())
    }

    fn ctx(age: f64, employer: bool) -> HealthContext {
        HealthContext {
            age,
            employer_coverage: employer,
            ..HealthContext::default()
        }
    }

    #[test]
    fn coverage_phases() {
        let aca = model(&Healthcare::default(), true);
        assert_eq!(aca.phase(&ctx(40.0, true)), CoveragePhase::Employer);
        assert_eq!(aca.phase(&ctx(40.0, false)), CoveragePhase::Aca);
        assert_eq!(aca.phase(&ctx(64.99, false)), CoveragePhase::Aca);
        assert_eq!(aca.phase(&ctx(65.0, false)), CoveragePhase::Medicare);
        assert_eq!(aca.phase(&ctx(67.0, true)), CoveragePhase::Employer);
        let abroad = HealthContext {
            in_us: false,
            ..ctx(70.0, false)
        };
        assert_eq!(aca.phase(&abroad), CoveragePhase::None);

        let cobra = model(
            &Healthcare {
                early_retirement: EarlyRetirementCoverage::Cobra,
                ..Healthcare::default()
            },
            true,
        );
        let since = |m: u32| HealthContext {
            months_since_coverage_ended: m,
            ..ctx(50.0, false)
        };
        assert_eq!(cobra.phase(&since(0)), CoveragePhase::Cobra);
        assert_eq!(cobra.phase(&since(17)), CoveragePhase::Cobra);
        assert_eq!(cobra.phase(&since(18)), CoveragePhase::Aca);
        assert_eq!(cobra.phase(&since(u32::MAX)), CoveragePhase::Aca);
        assert_eq!(
            cobra.phase(&HealthContext {
                months_since_coverage_ended: 3,
                ..ctx(66.0, false)
            }),
            CoveragePhase::Medicare
        );

        let custom = model(
            &Healthcare {
                early_retirement: EarlyRetirementCoverage::Custom { annual: 6_000.0 },
                ..Healthcare::default()
            },
            true,
        );
        assert_eq!(custom.phase(&ctx(50.0, false)), CoveragePhase::Custom);
        assert_eq!(custom.phase(&ctx(70.0, false)), CoveragePhase::Medicare);
        assert_eq!(custom.phase(&abroad), CoveragePhase::Custom);
        assert!(close(
            custom.expected_annual_cost(50.0, CoveragePhase::Custom),
            6_000.0,
            1e-9
        ));

        let in_budget = model(
            &Healthcare {
                mode: HealthcareMode::InBudget,
                ..Healthcare::default()
            },
            true,
        );
        assert_eq!(in_budget.phase(&ctx(40.0, true)), CoveragePhase::None);
        assert_eq!(in_budget.phase(&ctx(80.0, false)), CoveragePhase::None);
        assert_eq!(
            in_budget.expected_annual_cost(80.0, CoveragePhase::Medicare),
            0.0
        );
    }

    #[test]
    fn aca_premiums_follow_the_three_to_one_age_curve() {
        let m = model(&Healthcare::default(), false);
        // KFF national benchmark for a 40-year-old, latest plan year in the data.
        assert_eq!(m.aca_benchmark_monthly(40.0), 703.0);
        // CMS default curve: 21 = 1.000, 30 = 1.135, 40 = 1.278, 45 = 1.444,
        // 60 = 2.714, 64 = 3.000.
        let at21 = 703.0 / 1.278;
        assert!(close(m.aca_benchmark_monthly(21.0), at21, 1e-9));
        assert!(close(m.aca_benchmark_monthly(30.0), at21 * 1.135, 1e-9));
        assert!(close(m.aca_benchmark_monthly(45.9), at21 * 1.444, 1e-9));
        assert!(close(m.aca_benchmark_monthly(60.0), at21 * 2.714, 1e-9));
        assert!(close(m.aca_benchmark_monthly(64.0), at21 * 3.0, 1e-9));
        assert!(close(
            m.aca_benchmark_monthly(64.0) / m.aca_benchmark_monthly(21.0),
            3.0,
            1e-12
        ));
        // Research: unsubsidised 2027 benchmark about $11,800 a year at 50, $14,700
        // at 55, $17,900 at 60 and $19,800 at 64.
        for (age, annual) in [
            (50.0, 11_806.0),
            (55.0, 14_741.0),
            (60.0, 17_940.0),
            (64.0, 19_831.0),
        ] {
            assert!(
                close(m.aca_benchmark_monthly(age) * 12.0, annual, 0.002 * annual),
                "age {age}: {}",
                m.aca_benchmark_monthly(age) * 12.0
            );
        }

        // The step reports the gross premium and the benchmark, nominal.
        let mut state = HealthState::default();
        let mut rng = Rng::from_seed(1);
        let month = m.step(&mut state, &ctx(60.0, false), &mut rng);
        assert_eq!(month.phase, CoveragePhase::Aca);
        assert!(close(month.premium, at21 * 2.714, 1e-9));
        assert_eq!(month.aca_benchmark, month.premium);
        assert!(close(month.out_of_pocket, 1_700.0 / 12.0, 1e-6));
        assert_eq!(month.long_term_care, 0.0);
        assert!(close(
            month.total(),
            month.premium + month.out_of_pocket,
            1e-12
        ));

        // Metal tiers move the premium with actuarial value (60 / 70 / 80%) and the
        // out-of-pocket mean the other way; the benchmark does not move.
        for (tier, premium_factor, oop_factor) in [
            (PlanTier::Bronze, 0.6 / 0.7, 1.5),
            (PlanTier::Gold, 0.8 / 0.7, 0.7),
        ] {
            let t = model(
                &Healthcare {
                    quality: tier,
                    ..Healthcare::default()
                },
                false,
            );
            let month = t.step(&mut HealthState::default(), &ctx(60.0, false), &mut rng);
            assert!(close(month.premium, at21 * 2.714 * premium_factor, 1e-9));
            assert!(close(month.aca_benchmark, at21 * 2.714, 1e-9));
            assert!(close(
                month.out_of_pocket,
                1_700.0 * oop_factor / 12.0,
                1e-6
            ));
        }
    }

    #[test]
    fn employer_cobra_and_medicare_costs() {
        let m = model(&Healthcare::default(), false);
        // KFF 2026: worker pays $1,765 of a $9,804 single premium; mean out-of-pocket
        // $800 at 18-44, $1,200 at 45-54, $1,700 at 55-64.
        assert!(close(
            m.expected_annual_cost(30.0, CoveragePhase::Employer),
            1_765.0 + 800.0,
            1e-6
        ));
        assert!(close(
            m.expected_annual_cost(50.0, CoveragePhase::Employer),
            1_765.0 + 1_200.0,
            1e-6
        ));
        assert!(close(
            m.expected_annual_cost(60.0, CoveragePhase::Cobra),
            9_804.0 * 1.02 + 1_700.0,
            1e-6
        ));
        // Research: Medicare with Medigap costs $7,300 a person at 65, of which
        // $5,135 is premiums (Part B $2,435, Part D $500, Medigap $2,200).
        assert!(close(
            m.expected_annual_cost(65.0, CoveragePhase::Medicare),
            7_300.0,
            20.0
        ));
        let month = m.step(
            &mut HealthState::default(),
            &ctx(65.0, false),
            &mut Rng::from_seed(2),
        );
        assert_eq!(month.phase, CoveragePhase::Medicare);
        assert!(close(month.premium * 12.0, 5_135.0, 1e-6));
        assert!(close(month.out_of_pocket * 12.0, 2_183.0, 1e-6));
        assert_eq!(month.aca_benchmark, 0.0);
        // The Part B component is the statutory premium in the tax data.
        let part_b = num(
            &data().tax_federal["medicare"],
            &["part_b", "standard_premium_monthly"],
        );
        let parts =
            &data().actuarial["healthcare"]["medicare_phase"]["components_2026_per_person_annual"];
        assert!(close(part_b * 12.0, num(parts, &["part_b_premium"]), 1.0));
        // Research cross-check: $7,300 growing 1.5% with age and 1.5% excess
        // inflation for 21 years sums to about $210k in today's dollars.
        let lifetime: f64 = (0..21)
            .map(|k| {
                m.expected_annual_cost_at(65.0 + f64::from(k), CoveragePhase::Medicare, k * 12)
            })
            .sum();
        assert!(close(lifetime, 210_000.0, 2_000.0), "{lifetime}");
        // Age gradient: 1.5% a year after 65, none before.
        assert!(close(
            m.expected_annual_cost(75.0, CoveragePhase::Medicare)
                / m.expected_annual_cost(65.0, CoveragePhase::Medicare),
            math::powi(1.015, 10),
            1e-12
        ));
        assert!(m
            .expected_annual_cost(500.0, CoveragePhase::Medicare)
            .is_finite());
        assert!(m.expected_annual_cost(-3.0, CoveragePhase::Aca) > 0.0);
    }

    #[test]
    fn costs_are_nominal_with_excess_medical_inflation() {
        let m = model(&Healthcare::default(), false);
        assert_eq!(m.excess_growth(0), 1.0);
        assert!(close(m.excess_growth(12), 1.015, 1e-12));
        assert!(close(m.excess_growth(240), math::powi(1.015, 20), 1e-9));
        // Beyond the table the value is computed directly.
        assert!(close(
            m.excess_growth(1_800),
            math::powi(1.015, 150),
            1e-9 * math::powi(1.015, 150)
        ));
        let base = m.step(
            &mut HealthState::default(),
            &ctx(50.0, true),
            &mut Rng::from_seed(3),
        );
        let later = m.step(
            &mut HealthState::default(),
            &HealthContext {
                months_since_start: 120,
                price_level: 1.3,
                ..ctx(50.0, true)
            },
            &mut Rng::from_seed(3),
        );
        let factor = 1.3 * math::powi(1.015, 10);
        assert!(close(later.premium, base.premium * factor, 1e-9));
        assert!(close(
            later.out_of_pocket,
            base.out_of_pocket * factor,
            1e-9
        ));
    }

    #[test]
    fn simulated_out_of_pocket_mean_equals_the_configured_mean() {
        // (settings tier, employer coverage, age, configured mean, cap)
        let cases = [
            (PlanTier::Silver, true, 30.0, 800.0, 4_500.0),
            (PlanTier::Silver, true, 60.0, 1_700.0, 4_500.0),
            (PlanTier::Silver, false, 50.0, 1_200.0, 12_000.0),
            (PlanTier::Bronze, false, 60.0, 2_550.0, 12_000.0),
            (
                PlanTier::Silver,
                false,
                70.0,
                2_183.0 * 1.015f64 * 1.015 * 1.015 * 1.015 * 1.015,
                f64::INFINITY,
            ),
        ];
        for (tier, employer, age, mean, cap) in cases {
            let m = model(
                &Healthcare {
                    quality: tier,
                    long_term_care: false,
                    ..Healthcare::default()
                },
                true,
            );
            let context = ctx(age, employer);
            let mut sum = 0.0;
            let mut sum_sq = 0.0;
            let mut worst: f64 = 0.0;
            let paths = 4_000u64;
            let years = 30u32;
            for path in 0..paths {
                let mut rng = Rng::stream(3, path, Stream::Health(0));
                let mut state = m.initial_state(Sex::Female, 30 * 12, 900, &mut rng);
                for t in 0..years * 12 {
                    // Age is held fixed so every year has the same distribution;
                    // price level and excess growth are divided back out.
                    let c = HealthContext {
                        months_since_start: t,
                        ..context
                    };
                    let month = m.step(&mut state, &c, &mut rng);
                    if t % 12 == 0 {
                        let annual = month.out_of_pocket * 12.0 / m.excess_growth(t);
                        sum += annual;
                        sum_sq += annual * annual;
                        worst = worst.max(annual);
                    }
                }
            }
            let n = (paths * u64::from(years)) as f64;
            let simulated = sum / n;
            let sd = (sum_sq / n - simulated * simulated).sqrt();
            assert!(
                close(simulated, mean, 0.03 * mean),
                "{tier:?} employer={employer} age {age}: mean {simulated} vs {mean}"
            );
            assert!(worst <= cap * (1.0 + 1e-12), "cap {cap} exceeded: {worst}");
            assert!(sd > 0.3 * mean, "no dispersion: sd {sd}");
        }
    }

    #[test]
    fn medical_cost_shocks_persist_from_year_to_year() {
        let m = model(
            &Healthcare {
                long_term_care: false,
                ..Healthcare::default()
            },
            true,
        );
        // Correlation of the standardised shock one year apart: rho * share
        // = 0.92 * 0.33 = 0.30; variance 1.
        let (mut sxy, mut sxx, mut n) = (0.0, 0.0, 0.0);
        for path in 0..20_000u64 {
            let mut rng = Rng::stream(8, path, Stream::Health(1));
            let mut state = m.initial_state(Sex::Male, 40 * 12, 600, &mut rng);
            let mut previous = None;
            for t in 0..(6 * 12) {
                let c = HealthContext {
                    months_since_start: t,
                    ..ctx(40.0, true)
                };
                m.step(&mut state, &c, &mut rng);
                if t % 12 == 0 {
                    if let Some(p) = previous {
                        sxy += p * state.oop_shock;
                        sxx += state.oop_shock * state.oop_shock;
                        n += 1.0;
                    }
                    previous = Some(state.oop_shock);
                }
            }
        }
        assert!(close(sxx / n, 1.0, 0.03), "variance {}", sxx / n);
        assert!(
            close(sxy / n, 0.92 * 0.33, 0.03),
            "autocovariance {}",
            sxy / n
        );
    }

    /// Simulates 200k people aged 65 under the 2023 period table and returns
    /// (share with an episode, mean lifetime cost in today's dollars without excess
    /// inflation, mean years of care among users).
    fn ltc_experience(sex: Sex) -> (f64, f64, f64) {
        let mortality = mortality(65, sex, HealthStatus::Average, false, 2026);
        let m = model(&Healthcare::default(), true);
        let n = 200_000u64;
        let (mut users, mut cost, mut months) = (0u64, 0.0, 0u64);
        for path in 0..n {
            let mut longevity = Rng::stream(21, path, Stream::Longevity(0));
            let mut health = Rng::stream(21, path, Stream::Health(0));
            let death = mortality.draw_months_to_death(&mut longevity);
            let state = m.initial_state(sex, 65 * 12, death, &mut health);
            if state.ltc_months() > 0 {
                users += 1;
                months += u64::from(state.ltc_months());
                cost += f64::from(state.ltc_months()) * state.ltc_monthly_cost;
                assert_eq!(state.ltc_end_month, death);
                assert!(state.ltc_start_month < death);
                assert_ne!(state.ltc_care, CareType::None);
            }
        }
        (
            users as f64 / n as f64,
            cost / n as f64,
            months as f64 / 12.0 / users.max(1) as f64,
        )
    }

    #[test]
    fn long_term_care_matches_the_aspe_calibration() {
        // Johnson (2019): 38% of men and 55% of women who reach 65 use paid care,
        // for 2.4 and 3.4 years on average in the model's duration table; blended
        // cost $81,900 a year. Research: expected lifetime cost roughly
        // 0.38 x 2.4 x $82k = $75k (men), 0.55 x 3.4 x $82k = $153k (women).
        let (share_m, cost_m, years_m) = ltc_experience(Sex::Male);
        let (share_f, cost_f, years_f) = ltc_experience(Sex::Female);
        assert!(close(share_m, 0.38, 0.01), "male incidence {share_m}");
        assert!(close(share_f, 0.55, 0.01), "female incidence {share_f}");
        // Episodes cannot start before 65, which trims the longest ones slightly.
        assert!(years_m > 2.2 && years_m <= 2.43, "male years {years_m}");
        assert!(years_f > 3.1 && years_f <= 3.41, "female years {years_f}");
        assert!(
            close(cost_m, 75_000.0, 0.08 * 75_000.0),
            "male cost {cost_m}"
        );
        assert!(
            close(cost_f, 153_000.0, 0.08 * 153_000.0),
            "female cost {cost_f}"
        );
        let m = model(&Healthcare::default(), true);
        assert!(close(m.ltc_blended_annual_cost(), 81_900.0, 1.0));
        // By age at death (Johnson: 23% / 36% / 54% for 65-74 / 75-84 / 85+).
        assert_eq!(m.ltc_probability(Sex::Male, 64.9), 0.0);
        assert!(m.ltc_probability(Sex::Female, 70.0) > m.ltc_probability(Sex::Male, 70.0));
        // Hand-computed: logistic(-1.208 + 0.065 * 10 - 0.173) = logistic(-0.731)
        // = 0.3250 for a man dying at 80; logistic(-1.208 + 0.402) = 0.3087 for a
        // woman dying at 70; unspecified uses the mean shift, logistic(-1.0935)
        // = 0.2510 at 70.
        assert!(close(m.ltc_probability(Sex::Male, 80.0), 0.3250, 5e-4));
        assert!(close(m.ltc_probability(Sex::Female, 70.0), 0.3087, 5e-4));
        assert!(close(
            m.ltc_probability(Sex::Unspecified, 70.0),
            0.2510,
            5e-4
        ));
        assert_eq!(m.ltc_probability(Sex::Female, 119.0), 0.90);
    }

    #[test]
    fn long_term_care_episode_is_charged_until_death() {
        let m = model(&Healthcare::default(), true);
        // Find a path with a facility episode.
        let (mut state, mut rng) = (0..10_000u64)
            .find_map(|path| {
                let mut rng = Rng::stream(4, path, Stream::Health(0));
                let s = m.initial_state(Sex::Female, 70 * 12, 240, &mut rng);
                (s.ltc_care == CareType::NursingHome && s.ltc_months() >= 3).then_some((s, rng))
            })
            .expect("a nursing-home episode in 10,000 paths");
        assert_eq!(state.ltc_end_month, 240);
        assert!(close(state.ltc_monthly_cost, 114_975.0 / 12.0, 1e-9));
        let at = |t: u32| HealthContext {
            months_since_start: t,
            price_level: 2.0,
            ..ctx(70.0 + f64::from(t) / 12.0, false)
        };
        let start = state.ltc_start_month;
        if start > 0 {
            let before = m.step(&mut state, &at(start - 1), &mut rng);
            assert_eq!(before.long_term_care, 0.0);
            assert!(!before.ltc_in_facility && !before.ltc_started);
        }
        let first = m.step(&mut state, &at(start), &mut rng);
        assert!(first.ltc_started && first.ltc_in_facility);
        assert!(close(
            first.long_term_care,
            114_975.0 / 12.0 * 2.0 * m.excess_growth(start),
            1e-6
        ));
        let last = m.step(&mut state, &at(239), &mut rng);
        assert!(last.long_term_care > 0.0 && !last.ltc_started);
        let after = m.step(&mut state, &at(240), &mut rng);
        assert_eq!(after.long_term_care, 0.0);

        // Deaths before 65 and disabled care never schedule an episode.
        for path in 0..2_000u64 {
            let mut rng = Rng::stream(4, path, Stream::Health(0));
            assert_eq!(
                m.initial_state(Sex::Female, 30 * 12, 420, &mut rng)
                    .ltc_months(),
                0
            );
        }
        let off = model(
            &Healthcare {
                long_term_care: false,
                ..Healthcare::default()
            },
            true,
        );
        for path in 0..2_000u64 {
            let mut rng = Rng::stream(4, path, Stream::Health(0));
            assert_eq!(
                off.initial_state(Sex::Female, 70 * 12, 300, &mut rng)
                    .ltc_months(),
                0
            );
        }
        // An episode never starts before the simulation or before 65.
        for path in 0..5_000u64 {
            let mut rng = Rng::stream(6, path, Stream::Health(0));
            let s = m.initial_state(Sex::Male, 64 * 12, 30, &mut rng);
            assert!(s.ltc_months() <= 18);
            let s = m.initial_state(Sex::Male, 95 * 12, 5, &mut rng);
            assert!(s.ltc_months() <= 5);
        }
    }

    #[test]
    fn risk_off_gives_expected_values() {
        let m = model(&Healthcare::default(), false);
        let mut rng = Rng::stream(1, 1, Stream::Health(0));
        // Dies at 90: probability-weighted blended cost over the mean duration.
        let state = m.initial_state(Sex::Female, 60 * 12, 360, &mut rng);
        assert_eq!(state.ltc_care, CareType::Expected);
        assert_eq!(state.oop_shock, 0.0);
        // Female mean duration 3.4 years = 41 months.
        assert_eq!(state.ltc_months(), 41);
        let p = m.ltc_probability(Sex::Female, 90.0);
        assert!(close(state.ltc_monthly_cost, p * 81_900.0 / 12.0, 1e-6));
        let mut s = state;
        let c = HealthContext {
            months_since_start: 359,
            ..ctx(89.9, false)
        };
        let month = m.step(&mut s, &c, &mut rng);
        assert!(month.long_term_care > 0.0);
        assert!(!month.ltc_started && !month.ltc_in_facility);
        // Out-of-pocket is the mean every month, whatever the draws.
        let a = m.step(&mut s, &ctx(50.0, true), &mut Rng::from_seed(1));
        let b = m.step(&mut s, &ctx(50.0, true), &mut Rng::from_seed(2));
        assert_eq!(a, b);
        assert!(close(a.out_of_pocket, 100.0, 1e-9));
    }

    #[test]
    fn in_budget_mode_keeps_only_long_term_care() {
        let m = model(
            &Healthcare {
                mode: HealthcareMode::InBudget,
                ..Healthcare::default()
            },
            false,
        );
        let mut rng = Rng::from_seed(9);
        let mut state = m.initial_state(Sex::Male, 80 * 12, 60, &mut rng);
        let month = m.step(
            &mut state,
            &HealthContext {
                months_since_start: 59,
                ..ctx(84.9, false)
            },
            &mut rng,
        );
        assert_eq!(month.phase, CoveragePhase::None);
        assert_eq!(month.premium, 0.0);
        assert_eq!(month.out_of_pocket, 0.0);
        assert_eq!(month.aca_benchmark, 0.0);
        assert!(month.long_term_care > 0.0);
    }

    #[test]
    fn draws_are_fixed_and_results_deterministic() {
        let settings = [
            (Healthcare::default(), true),
            (Healthcare::default(), false),
            (
                Healthcare {
                    mode: HealthcareMode::InBudget,
                    long_term_care: false,
                    ..Healthcare::default()
                },
                true,
            ),
        ];
        for (setting, risk) in settings {
            let m = model(&setting, risk);
            for (age, death) in [(30u32, 700u32), (64, 3), (99, 12)] {
                let mut rng = Rng::stream(77, 5, Stream::Health(0));
                let mut reference = rng.clone();
                let mut state = m.initial_state(Sex::Unspecified, age * 12, death, &mut rng);
                for _ in 0..INITIAL_DRAWS {
                    reference.next_u64();
                }
                assert_eq!(rng.clone().next_u64(), reference.clone().next_u64());
                for t in 0..30u32 {
                    let c = HealthContext {
                        months_since_start: t,
                        employer_coverage: t < 10,
                        months_since_coverage_ended: t.saturating_sub(10),
                        ..ctx(f64::from(age) + f64::from(t) / 12.0, false)
                    };
                    m.step(&mut state, &c, &mut rng);
                    for _ in 0..STEP_DRAWS {
                        reference.next_u64();
                    }
                }
                assert_eq!(rng.next_u64(), reference.next_u64());
            }
        }
        // Same seed, same costs.
        let m = model(&Healthcare::default(), true);
        let run = || {
            let mut rng = Rng::stream(123, 45, Stream::Health(1));
            let mut state = m.initial_state(Sex::Female, 55 * 12, 500, &mut rng);
            let mut total = 0.0;
            for t in 0..500u32 {
                let c = HealthContext {
                    months_since_start: t,
                    price_level: 1.0 + f64::from(t) * 0.002,
                    ..ctx(55.0 + f64::from(t) / 12.0, t < 60)
                };
                total += m.step(&mut state, &c, &mut rng).total();
            }
            total
        };
        let first = run();
        assert!(first.is_finite() && first > 0.0);
        assert_eq!(first.to_bits(), run().to_bits());
    }

    #[test]
    fn capped_lognormal_solver() {
        let p = OopParams::new(1_700.0, 1.0, 4_500.0);
        // Numerical integral of min(exp(mu + sigma z), cap) over the normal density.
        let mut integral = 0.0;
        let steps = 200_000;
        for i in 0..steps {
            let z = -10.0 + 20.0 * (f64::from(i) + 0.5) / f64::from(steps);
            let density = math::exp(-0.5 * z * z) / (2.0 * std::f64::consts::PI).sqrt();
            integral += p.draw(z) * density * 20.0 / f64::from(steps);
        }
        assert!(close(integral, 1_700.0, 0.01), "{integral}");
        assert!(p.mu > math::ln(1_700.0) - 0.5);
        // No cap: the plain lognormal location.
        let q = OopParams::new(2_000.0, 0.5, f64::INFINITY);
        assert!(close(q.mu, math::ln(2_000.0) - 0.125, 1e-12));
        // Degenerate inputs.
        assert_eq!(OopParams::new(0.0, 1.0, 100.0).draw(3.0), 0.0);
        assert_eq!(OopParams::new(-5.0, 1.0, 100.0).draw(3.0), 0.0);
        let huge = OopParams::new(1e9, 1.0, 1_000.0);
        assert!(close(huge.mean, 900.0, 1e-9));
        assert!(huge.draw(0.0) <= 1_000.0);
    }

    #[test]
    fn ltc_formula_is_read_from_the_data() {
        let (intercept, slope, reference, max_p) = parse_ltc_probability(
            data().actuarial["long_term_care"]["model"]["probability_of_episode"]
                .as_str()
                .unwrap(),
        );
        assert_eq!(
            (intercept, slope, reference, max_p),
            (-1.208, 0.065, 70.0, 0.90)
        );
    }
}
