//! Careers and earned income.
//!
//! Responsibility: what each job pays each month on one path: the lifecycle wage
//! curve, persistent and transitory wage shocks, layoffs and re-employment (hazard
//! higher in recessions, re-employment at a wage scar), bonus, commission, equity
//! vesting and private-company liquidity events, career breaks, and self-employment
//! income with its own volatility.
//!
//! Contract:
//!
//! * One [`JobModel`] per job income is compiled once; one [`JobState`] per job lives
//!   in the path state. Stepping never allocates.
//! * Each person's jobs draw only from `Stream::Career(person)`; equity price paths
//!   and outcomes draw only from `Stream::EquityComp`. The number of draws per month
//!   depends only on the calendar (two extra normals every January), never on the
//!   state of the job: draw, then decide. A scenario change such as retiring earlier
//!   therefore does not shift later draws.
//! * `assumptions.career_risk == false` switches off every random element of pay
//!   (wage shocks, layoffs, bonus and commission noise). `risk.level == none` switches
//!   off wage shocks and layoffs but leaves the bonus and commission volatility the
//!   user entered. The same draws are consumed either way. Employer stock prices are
//!   market risk and are always simulated.
//! * All outputs are nominal dollars for the month. Wage curves are real; the caller
//!   supplies the price level.
//! * A layoff ends employer benefits (match, HSA, unvested equity is forfeited);
//!   re-employment restores the plan with the same terms.
//!
//! # The wage model (`data/actuarial/params.json`, section `earnings`)
//!
//! ```text
//! real salary(t) = anchor * curve(age_t) * exp(z - Var[z]/2 + e - sigma_eps^2/2)
//! z  <- rho * z + eta,  eta ~ N(0, sigma_eta^2)     every January
//! e  ~ N(0, sigma_eps^2)                            every January
//! z  <- z - L                                        on re-employment after a layoff
//! ```
//!
//! `anchor` is the salary the user entered, `curve` is 1 at the owner's age at
//! `profile.start` and follows either a flat real raise or the Cocco-Gomes-Maenhout
//! age polynomial. The `Var[z]/2` and `sigma_eps^2/2` terms make the Gaussian shocks
//! mean-preserving: with layoffs off, the expected salary is the deterministic curve.
//! Layoffs are deliberately *not* mean-preserving: months without pay and the scar `L`
//! pull the average below the curve. At the typical risk level, starting at 30 with
//! no recessions, the mean base salary is 2.5% below the curve after 5 years, 4%
//! after 10 and 6.5% after 20, and pay received over 20 years is 5.7% lower (see the
//! tests). That is the research's intent: the Gaussian part is kept small because
//! job loss is explicit. Planning code that uses [`JobModel::expected_real_salary`]
//! is therefore slightly optimistic for jobs with layoff risk.
//!
//! # Simplifications
//!
//! * The schema has no education field. `Typical` is the average of the CGM
//!   high-school and college profiles (pay at 55 is 1.66 times pay at 25, the median
//!   lifetime earner in Guvenen et al. 2021); `Steep` is the CGM college profile
//!   (2.03 times); `Flat` is no age-related growth. The layoff hazard uses the
//!   bachelor's-degree base rate, which matches the schema's "a few percent a year"
//!   for the typical level, scaled by the job-security multipliers.
//! * Unemployment spells are fractional months; a spell of 3.6 months is three months
//!   without pay and a fourth at 40% pay, so expected lost pay is exact.
//! * Bonus is paid in full in its month to whoever is employed then; no proration for
//!   a spell earlier in the year. Commission is CPI-indexed and does not follow the
//!   wage curve.
//! * All grants of one job share one employer stock: the `company` of the first grant.
//!   Options are exercised when they vest (or at the liquidity event) and are worth
//!   `max(price - strike, 0)`; underwater options lapse. Refresh grants vest monthly
//!   over four years with no cliff; refresh options are struck at the money.
//! * A private company's paper value is constant in real terms until its liquidity
//!   event. After a successful exit the stock follows the default public-company
//!   dynamics. `exit_volatility` is not used: the payout multiple comes from the
//!   research's outcome classes (modest, good, large), log-uniform inside the class.
//! * Vested private shares are kept after leaving the employer.
//! * The employer-stock-crash-raises-layoff-hazard link in the research is left out.

use serde_json::Value;

use crate::data::Data;
use crate::math;
use crate::model::{
    EquityCompany, EquityKind, Job, LifecycleProfile, RiskLevel, SelfEmployment, WageGrowth,
    YearMonth,
};
use crate::rng::{normal_from_bits, uniform_from_bits, Rng};

/// Maximum equity grants tracked per job. Refresh grants are pooled inside the slot
/// of the grant that spawns them, so this is the number of `job.equity` entries used;
/// entries beyond it are ignored (see [`JobModel::grants_dropped`]).
pub const MAX_GRANTS: usize = 8;

/// A refresh grant vests over this many months (the schema: "each refresh vests over
/// four years").
pub const REFRESH_VEST_MONTHS: u32 = 48;
/// Months between refresh grants (the schema: "each year while employed").
const REFRESH_INTERVAL_MONTHS: u32 = 12;
/// Refresh grants in flight per slot: `REFRESH_VEST_MONTHS / REFRESH_INTERVAL_MONTHS`.
const REFRESH_RING: usize = 4;

/// Age range over which the CGM polynomial was estimated (params.json,
/// `earnings.model.f`: "ages 20-65"). Outside it the curve is held flat.
const CURVE_MIN_AGE: f64 = 20.0;
const CURVE_MAX_AGE: f64 = 65.0;
/// Reference age of [`lifecycle_factor`].
const CURVE_REFERENCE_AGE: f64 = 40.0;
/// The wage-curve table covers ages up to this (beyond any `end_age`).
const CURVE_TABLE_MAX_AGE: f64 = 121.0;

/// Education row of `unemployment.separation_hazard` used as the base hazard. The
/// schema has no education field; the bachelor's row gives the "few percent chance of
/// job loss per year" that `RiskLevel::Typical` documents (5.8% a year at 35-44).
const BASE_HAZARD_EDUCATION: &str = "bachelors";
/// Spell length clip in months (params.json, `unemployment.duration.model`: "floor
/// 0.25 months, cap 36 months").
const SPELL_MIN_MONTHS: f64 = 0.25;
const SPELL_MAX_MONTHS: f64 = 36.0;
/// Recession durations and wage losses apply to spells that start in a recession or
/// within this many months after one (params.json,
/// `unemployment.recession_shift.duration_regime`).
const SLACK_MONTHS_AFTER_RECESSION: u32 = 12;
/// Scar add-on: +0.01 to the mean log wage loss per month of spell beyond 6, capped at
/// +0.10 (params.json, `unemployment.reemployment_wage_loss.long_spell_addon`).
const SCAR_LONG_SPELL_PER_MONTH: f64 = 0.01;
const SCAR_LONG_SPELL_FROM_MONTHS: f64 = 6.0;
const SCAR_LONG_SPELL_CAP: f64 = 0.10;
/// Age from which the scar is multiplied by `age_50_plus_multiplier`.
const SCAR_OLDER_AGE: f64 = 50.0;
/// Unemployment insurance waiting period (params.json,
/// `unemployment.unemployment_insurance.model`: "after a 1-week wait").
const UI_WAIT_WEEKS: f64 = 1.0;
const WEEKS_PER_YEAR: f64 = 52.0;
const WEEKS_PER_MONTH: f64 = WEEKS_PER_YEAR / 12.0;

/// Correlation of variable pay with the equity return of the same year (params.json,
/// `earnings.shock_process.bonus_or_variable_pay`: 0.5, a judgement). Used for the
/// bonus factor and for self-employment profit.
const VARIABLE_PAY_MARKET_CORRELATION: f64 = 0.5;
/// The standardised market signal is clipped to this many standard deviations so a
/// crash year cannot produce an absurd factor (numerical guard, not a calibration).
const MARKET_SIGNAL_CLIP: f64 = 4.0;
/// Year-to-year persistence of self-employment profit shocks. Judgement: business
/// income is far less persistent than a salary (rho 0.97) but not independent from
/// year to year; half of a shock carries into the next year.
const SELF_EMPLOYMENT_PERSISTENCE: f64 = 0.5;

/// Typed view of the `earnings`, `unemployment` and `equity_compensation` sections of
/// `data/actuarial/params.json`, plus the equity-market statistics the bonus factor
/// needs. Build it once per plan, never in a hot path.
#[derive(Clone, Debug, PartialEq)]
pub struct CareerParams {
    /// CGM age-polynomial coefficients `[age, age^2/10, age^3/100]`, high school.
    pub cgm_high_school: [f64; 3],
    /// CGM age-polynomial coefficients, college.
    pub cgm_college: [f64; 3],
    /// AR(1) coefficient of the persistent wage shock.
    pub rho: f64,
    /// `[sigma_eta, sigma_eps]` for the low, medium and high volatility settings.
    pub shock_sigma: [[f64; 2]; 3],
    /// Monthly layoff probability at 35-44 in an expansion before the security knob.
    pub base_hazard: f64,
    /// Hazard multipliers for ages 20-24, 25-34, 35-44, 45-54, 55-64, 65+.
    pub hazard_age_multiplier: [f64; 6],
    /// Hazard multipliers for very secure, typical and volatile jobs.
    pub security_multiplier: [f64; 3],
    /// Hazard multiplier in a recession month.
    pub recession_hazard_multiplier: f64,
    /// Median spell length in months, expansion and recession regime.
    pub spell_median: [f64; 2],
    /// Log standard deviation of spell length, expansion and recession regime.
    pub spell_sigma: [f64; 2],
    /// Median multipliers for ages under 35, 35-54 and 55+.
    pub spell_age_multiplier: [f64; 3],
    /// Mean log wage loss on re-employment, expansion and recession regime.
    pub scar_mean: [f64; 2],
    /// Standard deviation of the log wage loss.
    pub scar_sd: f64,
    /// Multiplier on the mean loss from age 50.
    pub scar_older_multiplier: f64,
    /// Unemployment insurance replacement rate.
    pub ui_replacement_rate: f64,
    /// Weekly benefit cap in today's dollars.
    pub ui_weekly_cap: f64,
    /// Weeks of benefit in normal times.
    pub ui_max_weeks: f64,
    /// Extra weeks for spells that start in the recession regime.
    pub ui_recession_extension_weeks: f64,
    /// Private-company stages sorted by `p_zero`: `[p_zero, p_modest, p_good, p_large]`.
    pub private_stages: [[f64; 4]; 3],
    /// Payout multiple ranges `[low, high]` for the modest, good and large classes.
    pub private_multiple_ranges: [[f64; 2]; 3],
    /// Mean of the 12-month log return of US stocks in the embedded history.
    pub equity_log_mean_12m: f64,
    /// Standard deviation of the 12-month log return (monthly sd times sqrt 12).
    pub equity_log_sd_12m: f64,
}

fn num(root: &Value, path: &[&str]) -> f64 {
    let mut v = root;
    for key in path {
        v = &v[*key];
    }
    v.as_f64().unwrap_or_else(|| {
        panic!(
            "data/actuarial/params.json: expected a number at {}",
            path.join(".")
        )
    })
}

impl CareerParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks one of the expected numbers (a build error: the file is
    /// embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> CareerParams {
        let a = &data.actuarial;
        let cgm = |edu: &str| {
            let p = ["earnings", "age_profile", "cgm_coefficients", edu];
            [
                num(a, &[p[0], p[1], p[2], p[3], "age"]),
                num(a, &[p[0], p[1], p[2], p[3], "age2_over_10"]),
                num(a, &[p[0], p[1], p[2], p[3], "age3_over_100"]),
            ]
        };
        let sigma = |level: &str| {
            let p = ["earnings", "shock_process", "career_volatility", level];
            [
                num(a, &[p[0], p[1], p[2], p[3], "sigma_eta"]),
                num(a, &[p[0], p[1], p[2], p[3], "sigma_eps"]),
            ]
        };
        let sep = "separation_hazard";
        let age_mult = |band: &str| num(a, &["unemployment", sep, "age_multiplier", band]);
        let knob = |k: &str| num(a, &["unemployment", sep, "job_security_knob", k]);
        let dur = |regime: &str, k: &str| num(a, &["unemployment", "duration", regime, k]);
        let dur_age = |band: &str| {
            num(
                a,
                &["unemployment", "duration", "age_multiplier_on_median", band],
            )
        };
        let scar = |k: &str| num(a, &["unemployment", "reemployment_wage_loss", k]);
        let ui = |k: &str| num(a, &["unemployment", "unemployment_insurance", k]);
        let stage = |name: &str| {
            let p = [
                "equity_compensation",
                "private_company",
                "model",
                "stage_table",
                name,
            ];
            [
                num(a, &[p[0], p[1], p[2], p[3], p[4], "p_zero"]),
                num(a, &[p[0], p[1], p[2], p[3], p[4], "p_modest"]),
                num(a, &[p[0], p[1], p[2], p[3], p[4], "p_good"]),
                num(a, &[p[0], p[1], p[2], p[3], p[4], "p_large"]),
            ]
        };
        let range = |class: &str| {
            let v = &a["equity_compensation"]["private_company"]["model"]
                ["multiple_ranges_on_paper_value"][class];
            match (v[0].as_f64(), v[1].as_f64()) {
                (Some(lo), Some(hi)) => [lo, hi],
                _ => panic!("data/actuarial/params.json: bad multiple range for {class}"),
            }
        };
        let mut private_stages = [
            stage("late_stage_pre_ipo"),
            stage("series_b_to_c"),
            stage("seed_to_series_a"),
        ];
        private_stages.sort_by(|x, y| x[0].total_cmp(&y[0]));

        // Equity statistics from the embedded monthly history.
        let returns = &data.history.equity_us;
        let n = returns.len().max(1) as f64;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for r in returns {
            let l = math::ln_1p(*r);
            sum += l;
            sum_sq += l * l;
        }
        let mean = sum / n;
        let var = (sum_sq / n - mean * mean).max(0.0);

        CareerParams {
            cgm_high_school: cgm("high_school"),
            cgm_college: cgm("college"),
            rho: num(a, &["earnings", "shock_process", "default_rho"]),
            shock_sigma: [sigma("low"), sigma("medium"), sigma("high")],
            base_hazard: num(
                a,
                &[
                    "unemployment",
                    sep,
                    "base_monthly_prob_by_education_at_age_35_44",
                    BASE_HAZARD_EDUCATION,
                ],
            ),
            hazard_age_multiplier: [
                age_mult("20-24"),
                age_mult("25-34"),
                age_mult("35-44"),
                age_mult("45-54"),
                age_mult("55-64"),
                age_mult("65+"),
            ],
            security_multiplier: [
                knob("very_secure"),
                knob("typical"),
                knob("volatile_industry"),
            ],
            recession_hazard_multiplier: num(
                a,
                &[
                    "unemployment",
                    "recession_shift",
                    "separation_multiplier_in_recession",
                ],
            ),
            spell_median: [
                dur("expansion", "median_months"),
                dur("recession", "median_months"),
            ],
            spell_sigma: [dur("expansion", "sigma"), dur("recession", "sigma")],
            spell_age_multiplier: [dur_age("under_35"), dur_age("35-54"), dur_age("55_plus")],
            scar_mean: [scar("mu_L_expansion"), scar("mu_L_recession")],
            scar_sd: scar("sd_L"),
            scar_older_multiplier: scar("age_50_plus_multiplier"),
            ui_replacement_rate: ui("replacement_rate"),
            ui_weekly_cap: ui("weekly_cap_default_2026"),
            ui_max_weeks: ui("max_weeks_default"),
            ui_recession_extension_weeks: ui("recession_extension_weeks"),
            private_stages,
            private_multiple_ranges: [range("modest"), range("good"), range("large")],
            equity_log_mean_12m: 12.0 * mean,
            equity_log_sd_12m: (12.0 * var).sqrt(),
        }
    }

    /// Polynomial coefficients of a lifecycle profile (zero for `Flat`).
    fn curve_coefficients(&self, profile: LifecycleProfile) -> [f64; 3] {
        match profile {
            LifecycleProfile::Flat => [0.0; 3],
            LifecycleProfile::Typical => {
                let (h, c) = (&self.cgm_high_school, &self.cgm_college);
                [
                    0.5 * (h[0] + c[0]),
                    0.5 * (h[1] + c[1]),
                    0.5 * (h[2] + c[2]),
                ]
            }
            LifecycleProfile::Steep => self.cgm_college,
        }
    }

    /// Standardised 12-month equity return: about N(0, 1) across years.
    fn market_signal(&self, equity_return_12m: f64) -> f64 {
        if self.equity_log_sd_12m <= 0.0 {
            return 0.0;
        }
        let log_return = math::ln((1.0 + equity_return_12m).max(1e-9));
        ((log_return - self.equity_log_mean_12m) / self.equity_log_sd_12m)
            .clamp(-MARKET_SIGNAL_CLIP, MARKET_SIGNAL_CLIP)
    }
}

/// Log of the CGM age profile up to a constant: `b1*a + b2*a^2/10 + b3*a^3/100`,
/// with the age held inside the estimation range.
fn log_profile(coef: &[f64; 3], age: f64) -> f64 {
    let a = age.clamp(CURVE_MIN_AGE, CURVE_MAX_AGE);
    coef[0] * a + coef[1] * a * a / 10.0 + coef[2] * a * a * a / 100.0
}

/// Real wage level by age on a job's growth setting, in arbitrary units: the curve
/// [`JobModel`] follows going forward, for rebuilding the earnings of the years
/// before the plan starts on the same curve (so the last rebuilt year and the first
/// simulated one join without a kink, and a flat raise or `extra` is honoured).
pub fn wage_curve_level(growth: &WageGrowth, age: f64, params: &CareerParams) -> f64 {
    match *growth {
        WageGrowth::Flat { real_raise } => math::exp(math::ln_1p(real_raise.max(-0.99)) * age),
        WageGrowth::Lifecycle { profile, extra } => math::exp(
            log_profile(&params.curve_coefficients(profile), age)
                + math::ln_1p(extra.max(-0.99)) * age,
        ),
    }
}

fn hazard_age_band(age: f64) -> usize {
    if age < 25.0 {
        0
    } else if age < 35.0 {
        1
    } else if age < 45.0 {
        2
    } else if age < 55.0 {
        3
    } else if age < 65.0 {
        4
    } else {
        5
    }
}

fn spell_age_band(age: f64) -> usize {
    if age < 35.0 {
        0
    } else if age < 55.0 {
        1
    } else {
        2
    }
}

/// Index into the low / medium / high tables; `None` has no row.
fn risk_index(level: RiskLevel) -> Option<usize> {
    match level {
        RiskLevel::None => None,
        RiskLevel::Low => Some(0),
        RiskLevel::Typical => Some(1),
        RiskLevel::High => Some(2),
    }
}

/// Log standard deviation of a unit-mean lognormal whose standard deviation relative
/// to its mean is `cv`.
fn log_sd_from_cv(cv: f64) -> f64 {
    math::ln_1p(cv.max(0.0) * cv.max(0.0)).sqrt()
}

/// Deterministic real wage growth.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Curve {
    /// `(1 + raise)^(age - age0)`; the field is `ln(1 + raise)`.
    Flat { log_raise: f64 },
    /// CGM polynomial ratio times `(1 + extra)^(age - age0)`.
    Lifecycle { coef: [f64; 3], log_extra: f64 },
}

impl Curve {
    fn at(&self, age0: f64, age: f64) -> f64 {
        let years = (age - age0).max(0.0);
        match self {
            Curve::Flat { log_raise } => math::exp(log_raise * years),
            Curve::Lifecycle { coef, log_extra } => math::exp(
                log_profile(coef, age.max(age0)) - log_profile(coef, age0) + log_extra * years,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BonusModel {
    target_fraction: f64,
    log_sd: f64,
    month: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CommissionModel {
    monthly_real: f64,
    /// Log sd of the monthly factor, sized so that the annual total has the entered
    /// coefficient of variation: monthly cv = annual cv * sqrt(12).
    log_sd: f64,
}

/// Employer stock dynamics.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Company {
    /// The job has no equity.
    None,
    Public(StockModel),
    Private {
        /// Monthly probability of the liquidity event outside recessions.
        monthly_hazard: f64,
        fail_prob: f64,
        /// Cumulative probability of the modest and good classes given success.
        class_cumulative: [f64; 2],
        /// `[ln(low), ln(high / low)]` of the modest, good and large multiples.
        class_log_range: [[f64; 2]; 3],
        /// Dynamics after a successful exit.
        after_exit: StockModel,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StockModel {
    beta: f64,
    /// Monthly log sd of the idiosyncratic factor.
    idio_log_sd: f64,
}

impl StockModel {
    /// `volatility` is the stock's total annual volatility; the idiosyncratic part is
    /// what is left after the market component `beta * sigma_market`.
    fn new(volatility: f64, beta: f64, params: &CareerParams) -> StockModel {
        let market = beta * params.equity_log_sd_12m;
        let idio_var = (volatility * volatility - market * market).max(0.0);
        StockModel {
            beta,
            idio_log_sd: (idio_var / 12.0).sqrt(),
        }
    }

    #[inline]
    fn gross_return(&self, equity_return: f64, z: f64) -> f64 {
        let market = (1.0 + self.beta * equity_return).max(0.0);
        if self.idio_log_sd > 0.0 {
            let s = self.idio_log_sd;
            market * math::exp(s * z - 0.5 * s * s)
        } else {
            market
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct GrantModel {
    /// Shares in the original grant, in units worth one today's dollar at start.
    shares: f64,
    /// Shares scheduled to vest per month.
    per_month: f64,
    vest_months: u32,
    cliff_months: u32,
    /// `Some(strike)` for options; the strike is a fraction of the start price and is
    /// fixed in nominal terms.
    strike: Option<f64>,
    /// Real value of each annual refresh grant.
    refresh_real: f64,
    sell_on_vest: bool,
}

/// Compiled parameters of one job. Built in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct JobModel {
    salary: f64,
    age0: f64,
    curve: Curve,
    /// `curve.at(age0 + m / 12)` for month `m` since the start.
    curve_table: Vec<f64>,
    /// Wage shocks and layoffs are simulated.
    wage_risk: bool,
    /// Bonus and commission noise is simulated.
    pay_noise: bool,
    rho: f64,
    sigma_eta: f64,
    sigma_eps: f64,
    /// Monthly layoff probability `[recession][age band]`.
    hazard: [[f64; 6]; 2],
    params: CareerParams,
    bonus: Option<BonusModel>,
    commission: Option<CommissionModel>,
    grants: [GrantModel; MAX_GRANTS],
    n_grants: usize,
    grants_dropped: usize,
    company: Company,
}

/// Months of real growth of a self-employment income tabulated at compile time.
const SELF_EMPLOYMENT_GROWTH_MONTHS: u32 = 1_200;

/// Compiled parameters of one self-employment income.
#[derive(Clone, Debug)]
pub struct SelfEmploymentModel {
    monthly_real: f64,
    log_growth: f64,
    /// `exp(log_growth * m / 12)` for `m` whole months since the start, the real
    /// growth factor `step` would otherwise evaluate every month.
    growth: Box<[f64]>,
    log_sd: f64,
    risk: bool,
    params: CareerParams,
}

/// Per-path state of one self-employment income.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelfEmploymentState {
    /// Log deviation of profit from its expected path.
    pub log_shock: f64,
    /// Variance of `log_shock` so far (for the mean-preserving correction).
    pub shock_variance: f64,
    /// Current multiplicative factor on expected profit (mean 1).
    pub factor: f64,
    /// Steps taken.
    pub steps: u32,
}

impl Default for SelfEmploymentState {
    fn default() -> Self {
        Self {
            log_shock: 0.0,
            shock_variance: 0.0,
            factor: 1.0,
            steps: 0,
        }
    }
}

/// Employment status of a job on a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Employment {
    /// Working and being paid.
    #[default]
    Employed,
    /// Laid off and searching; `months` whole months so far.
    Unemployed { months: u32 },
    /// On a planned career break with `months_left` to go.
    OnBreak { months_left: u32 },
    /// The job has ended for good (retirement, `end` date or an `income_change` event).
    Ended,
    /// The income's `start` is in the future. Pays nothing and cannot be lost; call
    /// [`JobModel::begin`] in the start month.
    NotStarted,
}

/// What has happened to a private employer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Liquidity {
    /// Listed company, or no equity.
    #[default]
    Liquid,
    /// Private, no liquidity event yet: vested shares cannot be sold.
    Pending,
    /// Private company that has had a successful exit; trades like a listed stock.
    Exited,
    /// Private company that failed: all equity is worthless.
    Failed,
}

/// Per-path state of one job.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct JobState {
    /// Current status.
    pub employment: Employment,
    /// Current real annual base salary (today's dollars), including the persistent
    /// and transitory shocks and any scar from past layoffs.
    pub real_salary: f64,
    /// Real salary at the start age with no shocks; the deterministic path is
    /// `anchor * curve(age)`.
    pub anchor: f64,
    /// Wage curve factor at the last step (1 at the start age).
    pub curve: f64,
    /// Log of the persistent wage shock component, including scars.
    pub persistent_shock: f64,
    /// Variance of the Gaussian part of the persistent shock so far.
    pub persistent_variance: f64,
    /// Log of this year's transitory shock, mean-corrected (`e - sigma_eps^2 / 2`);
    /// zero until the first January.
    pub transitory_shock: f64,
    /// `exp` of the mean-corrected shocks: real salary = anchor * curve * this.
    pub shock_factor: f64,
    /// Fraction of pay received while on a break.
    pub break_income_fraction: f64,
    /// Months of the current unemployment spell still to run (fractional).
    pub spell_months_left: f64,
    /// Log wage loss that will be applied on re-employment.
    pub pending_scar: f64,
    /// Weekly unemployment benefit of the current spell, today's dollars.
    pub ui_weekly_real: f64,
    /// Weeks of benefit left in the current spell.
    pub ui_weeks_left: f64,
    /// Waiting weeks left before benefits start.
    pub ui_wait_weeks_left: f64,
    /// Months since the last recession month (large if none yet).
    pub months_since_recession: u32,
    /// Steps taken (months since `profile.start`).
    pub steps: u32,
    /// Unvested shares of each original grant. One share was worth one today's
    /// dollar at `profile.start`.
    pub unvested: [f64; MAX_GRANTS],
    /// Months of vesting left per original grant.
    pub vest_months_left: [u32; MAX_GRANTS],
    /// Months until the cliff per original grant.
    pub cliff_months_left: [u32; MAX_GRANTS],
    /// Shares vesting per month from each refresh grant in flight.
    pub refresh_rate: [[f64; REFRESH_RING]; MAX_GRANTS],
    /// Strike of each refresh grant in flight (options only), nominal.
    pub refresh_strike: [[f64; REFRESH_RING]; MAX_GRANTS],
    /// Months worked since the start, which times the refresh grants.
    pub equity_clock: u32,
    /// Nominal stock price relative to the price at `profile.start` (1.0 at start).
    pub stock_price: f64,
    /// Status of a private employer.
    pub liquidity: Liquidity,
    /// Vested private-company shares waiting for a liquidity event, per grant.
    pub illiquid_vested: [f64; MAX_GRANTS],
    /// Total strike owed on the vested private options, per grant, nominal.
    pub illiquid_strike_cost: [f64; MAX_GRANTS],
    /// Months spent unemployed so far on this path, fractional (a path scalar).
    pub months_unemployed_total: f64,
}

/// What one job paid in one month, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct JobMonth {
    /// Base pay.
    pub wages: f64,
    /// Bonus paid this month (0 outside the bonus month).
    pub bonus: f64,
    /// Commission paid this month.
    pub commission: f64,
    /// Value of RSUs vesting, options exercised and private-company payouts this
    /// month: taxed as wages.
    pub equity_income: f64,
    /// Portion of `equity_income` (before tax) from grants with `sell_on_vest` false.
    /// The simulation withholds tax on it and holds the rest as single stock.
    pub equity_retained: f64,
    /// Unemployment insurance received (taxable, not wages).
    pub unemployment_benefit: f64,
    /// True in the month a layoff happens (for the event log).
    pub laid_off: bool,
    /// True in the month a new job starts after unemployment.
    pub rehired: bool,
}

impl JobMonth {
    /// Total taxable compensation for the month.
    pub fn gross(&self) -> f64 {
        self.wages + self.bonus + self.commission + self.equity_income
    }
}

/// The economy as careers see it this month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CareerClimate {
    /// Calendar month.
    pub now: YearMonth,
    /// The owner's age in years.
    pub age: f64,
    /// Price level relative to `profile.start`.
    pub price_level: f64,
    /// 1 in a recession month.
    pub recession: u8,
    /// US stock return this month (drives employer stock through its beta).
    pub equity_return: f64,
    /// US stock total return over the 12 months up to and including this one, as a
    /// simple fraction (0.10 = +10%). Drives the bonus factor and self-employment
    /// profit. Before 12 months of the path exist, compound what there is.
    pub equity_return_12m: f64,
}

impl JobModel {
    /// Compiles a job. `owner_age_at_start` is the owner's age at `profile.start`;
    /// the entered salary is the pay at that age. `risk_enabled` is
    /// `assumptions.career_risk`.
    pub fn new(job: &Job, owner_age_at_start: f64, risk_enabled: bool, data: &Data) -> JobModel {
        JobModel::with_params(
            job,
            owner_age_at_start,
            risk_enabled,
            CareerParams::from_data(data),
        )
    }

    /// As [`JobModel::new`] with parameters already read (reuse one [`CareerParams`]
    /// across the jobs of a plan).
    pub fn with_params(
        job: &Job,
        owner_age_at_start: f64,
        risk_enabled: bool,
        params: CareerParams,
    ) -> JobModel {
        let age0 = owner_age_at_start;
        let curve = match job.growth {
            WageGrowth::Flat { real_raise } => Curve::Flat {
                log_raise: math::ln_1p(real_raise.max(-0.99)),
            },
            WageGrowth::Lifecycle { profile, extra } => Curve::Lifecycle {
                coef: params.curve_coefficients(profile),
                log_extra: math::ln_1p(extra.max(-0.99)),
            },
        };
        let table_months = ((CURVE_TABLE_MAX_AGE - age0).max(0.0) * 12.0).ceil() as usize + 1;
        let curve_table = (0..table_months)
            .map(|m| curve.at(age0, age0 + m as f64 / 12.0))
            .collect();

        let level = if risk_enabled {
            risk_index(job.risk.level)
        } else {
            None
        };
        let (sigma_eta, sigma_eps) = level.map_or((0.0, 0.0), |i| {
            (params.shock_sigma[i][0], params.shock_sigma[i][1])
        });
        let mut hazard = [[0.0; 6]; 2];
        if let Some(i) = level {
            for (rec, row) in hazard.iter_mut().enumerate() {
                for (band, h) in row.iter_mut().enumerate() {
                    let cycle = if rec == 1 {
                        params.recession_hazard_multiplier
                    } else {
                        1.0
                    };
                    *h = (params.base_hazard
                        * params.hazard_age_multiplier[band]
                        * params.security_multiplier[i]
                        * cycle)
                        .clamp(0.0, 1.0);
                }
            }
        }

        let bonus = job
            .bonus
            .filter(|b| b.target_fraction > 0.0)
            .map(|b| BonusModel {
                target_fraction: b.target_fraction,
                log_sd: log_sd_from_cv(b.volatility),
                month: b.month,
            });
        let commission =
            job.commission
                .filter(|c| c.expected_annual > 0.0)
                .map(|c| CommissionModel {
                    monthly_real: c.expected_annual / 12.0,
                    log_sd: log_sd_from_cv(c.volatility * 12f64.sqrt()),
                });

        let mut grants = [GrantModel::default(); MAX_GRANTS];
        let n_grants = job.equity.len().min(MAX_GRANTS);
        for (slot, g) in grants.iter_mut().zip(&job.equity) {
            let shares = g.unvested_value.max(0.0);
            let vest_months = if shares > 0.0 {
                g.vest_months_remaining.max(1)
            } else {
                0
            };
            *slot = GrantModel {
                shares,
                per_month: if vest_months > 0 {
                    shares / f64::from(vest_months)
                } else {
                    0.0
                },
                vest_months,
                cliff_months: g.cliff_months.min(vest_months),
                strike: match g.kind {
                    EquityKind::Rsu => None,
                    EquityKind::Option { strike_fraction } => Some(strike_fraction.max(0.0)),
                },
                refresh_real: g.refresh_annual_value.max(0.0),
                sell_on_vest: g.sell_on_vest,
            };
        }
        let company = match job.equity.first().map(|g| g.company) {
            None => Company::None,
            Some(EquityCompany::Public { volatility, beta }) => {
                Company::Public(StockModel::new(volatility, beta, &params))
            }
            Some(EquityCompany::Private {
                liquidity_hazard,
                fail_prob,
                exit_volatility: _,
            }) => {
                let (class_cumulative, class_log_range) = private_classes(&params, fail_prob);
                let after_exit = match EquityCompany::default() {
                    EquityCompany::Public { volatility, beta } => {
                        StockModel::new(volatility, beta, &params)
                    }
                    EquityCompany::Private { .. } => StockModel {
                        beta: 1.0,
                        idio_log_sd: 0.0,
                    },
                };
                Company::Private {
                    monthly_hazard: 1.0
                        - math::pow(1.0 - liquidity_hazard.clamp(0.0, 1.0), 1.0 / 12.0),
                    fail_prob: fail_prob.clamp(0.0, 1.0),
                    class_cumulative,
                    class_log_range,
                    after_exit,
                }
            }
        };

        JobModel {
            salary: job.salary,
            age0,
            curve,
            curve_table,
            wage_risk: level.is_some(),
            pay_noise: risk_enabled,
            rho: params.rho,
            sigma_eta,
            sigma_eps,
            hazard,
            params,
            bonus,
            commission,
            grants,
            n_grants,
            grants_dropped: job.equity.len() - n_grants,
            company,
        }
    }

    /// Number of `job.equity` entries beyond [`MAX_GRANTS`] that are not simulated.
    pub fn grants_dropped(&self) -> usize {
        self.grants_dropped
    }

    /// State at `profile.start` for a job that is already held. For a job whose
    /// `start` is in the future use [`JobModel::initial_state_not_started`].
    pub fn initial_state(&self) -> JobState {
        let mut s = JobState {
            employment: Employment::Employed,
            real_salary: self.salary,
            anchor: self.salary,
            curve: 1.0,
            shock_factor: 1.0,
            months_since_recession: u32::MAX / 2,
            stock_price: 1.0,
            liquidity: match self.company {
                Company::Private { .. } => Liquidity::Pending,
                _ => Liquidity::Liquid,
            },
            ..JobState::default()
        };
        for (g, grant) in self.grants.iter().enumerate().take(self.n_grants) {
            s.unvested[g] = grant.shares;
            s.vest_months_left[g] = grant.vest_months;
            s.cliff_months_left[g] = grant.cliff_months;
        }
        s
    }

    /// State at `profile.start` for a job that starts later.
    pub fn initial_state_not_started(&self) -> JobState {
        JobState {
            employment: Employment::NotStarted,
            ..self.initial_state()
        }
    }

    /// Starts a job that was [`Employment::NotStarted`], at the owner's age `age`.
    /// No effect otherwise.
    ///
    /// The salary entered for a job is its pay in today's dollars when it starts:
    /// the wage curve is anchored at the starting age, so the first pay is the
    /// entered amount and growth runs from there (not from `profile.start`).
    pub fn begin(&self, state: &mut JobState, age: f64) {
        if state.employment == Employment::NotStarted {
            state.employment = Employment::Employed;
            let curve = self.curve_lookup(age);
            if curve > 0.0 {
                state.anchor = self.salary / curve;
            }
            state.curve = curve;
            state.real_salary = self.salary;
            // Wage shocks drawn while the job did not exist are not its history.
            state.persistent_shock = 0.0;
            state.persistent_variance = 0.0;
            state.transitory_shock = 0.0;
            state.shock_factor = 1.0;
            state.steps = 0;
        }
    }

    /// Monthly layoff probability of this job at an age.
    pub fn layoff_probability(&self, age: f64, recession: bool) -> f64 {
        self.hazard[usize::from(recession)][hazard_age_band(age)]
    }

    #[inline]
    fn curve_lookup(&self, age: f64) -> f64 {
        let m = ((age - self.age0) * 12.0).round().max(0.0) as usize;
        self.curve_table[m.min(self.curve_table.len() - 1)]
    }

    #[inline]
    fn refresh_shock_factor(&self, s: &mut JobState) {
        s.shock_factor =
            math::exp(s.persistent_shock - 0.5 * s.persistent_variance + s.transitory_shock);
    }

    fn forfeit_unvested(&self, s: &mut JobState) {
        for g in 0..self.n_grants {
            s.unvested[g] = 0.0;
            s.vest_months_left[g] = 0;
            s.cliff_months_left[g] = 0;
            s.refresh_rate[g] = [0.0; REFRESH_RING];
            s.refresh_strike[g] = [0.0; REFRESH_RING];
        }
    }

    /// Advances one month and returns what was paid. Called for every job every
    /// month, whatever its status, so the draw count per stream is constant.
    ///
    /// Draws, in order, one 64-bit word each. From `career`: layoff, spell length,
    /// scar, commission, bonus, and in January two more (persistent and transitory
    /// wage shock). From `equity`: stock, liquidity event, outcome class, multiple.
    pub fn step(
        &self,
        state: &mut JobState,
        climate: &CareerClimate,
        career: &mut Rng,
        equity: &mut Rng,
    ) -> JobMonth {
        let s = state;
        let c = climate;
        // Every variate is one 64-bit word, drawn whether or not it is used; the
        // words are turned into normals only where a branch needs them.
        let b_layoff = career.next_u64();
        let r_spell = career.next_u64();
        let r_scar = career.next_u64();
        let r_commission = career.next_u64();
        let r_bonus = career.next_u64();
        let january = c.now.month() == 1;
        let (r_eta, r_eps) = if january {
            (career.next_u64(), career.next_u64())
        } else {
            (0, 0)
        };
        let r_stock = equity.next_u64();
        let b_equity = [equity.next_u64(), equity.next_u64(), equity.next_u64()];

        let mut out = JobMonth::default();

        // Annual wage shocks. The first step keeps the entered salary.
        if january && s.steps > 0 && self.wage_risk {
            let z_eta = normal_from_bits(r_eta);
            let z_eps = normal_from_bits(r_eps);
            s.persistent_shock = self.rho * s.persistent_shock + self.sigma_eta * z_eta;
            s.persistent_variance =
                self.rho * self.rho * s.persistent_variance + self.sigma_eta * self.sigma_eta;
            s.transitory_shock = self.sigma_eps * z_eps - 0.5 * self.sigma_eps * self.sigma_eps;
            self.refresh_shock_factor(s);
        }
        s.steps = s.steps.saturating_add(1);
        s.curve = self.curve_lookup(c.age);
        s.real_salary = s.anchor * s.curve * s.shock_factor;

        if c.recession != 0 {
            s.months_since_recession = 0;
        } else {
            s.months_since_recession = s.months_since_recession.saturating_add(1);
        }
        let slack = usize::from(s.months_since_recession <= SLACK_MONTHS_AFTER_RECESSION);

        // Layoff.
        if s.employment == Employment::Employed
            && uniform_from_bits(b_layoff)
                < self.hazard[usize::from(c.recession != 0)][hazard_age_band(c.age)]
        {
            let p = &self.params;
            let z_spell = normal_from_bits(r_spell);
            let z_scar = normal_from_bits(r_scar);
            let median = p.spell_median[slack] * p.spell_age_multiplier[spell_age_band(c.age)];
            let spell = (median * math::exp(p.spell_sigma[slack] * z_spell))
                .clamp(SPELL_MIN_MONTHS, SPELL_MAX_MONTHS);
            let mut scar_mean = p.scar_mean[slack];
            if c.age >= SCAR_OLDER_AGE {
                scar_mean *= p.scar_older_multiplier;
            }
            scar_mean += (SCAR_LONG_SPELL_PER_MONTH
                * (spell - SCAR_LONG_SPELL_FROM_MONTHS).max(0.0))
            .min(SCAR_LONG_SPELL_CAP);
            s.spell_months_left = spell;
            s.pending_scar = scar_mean + p.scar_sd * z_scar;
            s.ui_weekly_real =
                (p.ui_replacement_rate * s.real_salary / WEEKS_PER_YEAR).min(p.ui_weekly_cap);
            s.ui_weeks_left = p.ui_max_weeks
                + if slack == 1 {
                    p.ui_recession_extension_weeks
                } else {
                    0.0
                };
            s.ui_wait_weeks_left = UI_WAIT_WEEKS;
            s.employment = Employment::Unemployed { months: 0 };
            self.forfeit_unvested(s);
            out.laid_off = true;
        }

        // Share of a full month's pay earned this month.
        let mut paid = 0.0;
        match s.employment {
            Employment::Employed => paid = 1.0,
            Employment::OnBreak { months_left } => {
                paid = s.break_income_fraction;
                s.employment = if months_left <= 1 {
                    Employment::Employed
                } else {
                    Employment::OnBreak {
                        months_left: months_left - 1,
                    }
                };
            }
            Employment::Unemployed { months } => {
                let idle = s.spell_months_left.min(1.0);
                s.spell_months_left -= idle;
                s.months_unemployed_total += idle;
                let weeks = idle * WEEKS_PER_MONTH;
                let waited = weeks.min(s.ui_wait_weeks_left);
                s.ui_wait_weeks_left -= waited;
                let payable = (weeks - waited).min(s.ui_weeks_left);
                s.ui_weeks_left -= payable;
                out.unemployment_benefit = s.ui_weekly_real * payable * c.price_level;
                if s.spell_months_left <= 0.0 {
                    s.persistent_shock -= s.pending_scar;
                    s.pending_scar = 0.0;
                    self.refresh_shock_factor(s);
                    s.real_salary = s.anchor * s.curve * s.shock_factor;
                    s.employment = Employment::Employed;
                    out.rehired = true;
                    paid = 1.0 - idle;
                } else {
                    s.employment = Employment::Unemployed { months: months + 1 };
                }
            }
            Employment::Ended | Employment::NotStarted => {}
        }

        if paid > 0.0 {
            out.wages = s.real_salary / 12.0 * c.price_level * paid;
            if let Some(b) = &self.bonus {
                if c.now.month() == b.month {
                    let factor = if self.pay_noise && b.log_sd > 0.0 {
                        let rho = VARIABLE_PAY_MARKET_CORRELATION;
                        let mix = rho * self.params.market_signal(c.equity_return_12m)
                            + (1.0 - rho * rho).sqrt() * normal_from_bits(r_bonus);
                        math::exp(b.log_sd * mix - 0.5 * b.log_sd * b.log_sd)
                    } else {
                        1.0
                    };
                    out.bonus = b.target_fraction * s.real_salary * c.price_level * factor * paid;
                }
            }
            if let Some(k) = &self.commission {
                let factor = if self.pay_noise && k.log_sd > 0.0 {
                    math::exp(k.log_sd * normal_from_bits(r_commission) - 0.5 * k.log_sd * k.log_sd)
                } else {
                    1.0
                };
                out.commission = k.monthly_real * c.price_level * factor * paid;
            }
        }

        if self.n_grants > 0 {
            self.step_equity(s, c, r_stock, b_equity, &mut out);
        }
        out
    }

    /// Employer stock price, liquidity event, vesting and refresh grants.
    #[allow(clippy::too_many_arguments)]
    fn step_equity(
        &self,
        s: &mut JobState,
        c: &CareerClimate,
        r_stock: u64,
        b_equity: [u64; 3],
        out: &mut JobMonth,
    ) {
        match &self.company {
            Company::None => {}
            Company::Public(stock) => {
                s.stock_price *= stock.gross_return(c.equity_return, normal_from_bits(r_stock));
            }
            Company::Private {
                monthly_hazard,
                fail_prob,
                class_cumulative,
                class_log_range,
                after_exit,
            } => match s.liquidity {
                Liquidity::Pending => {
                    // Paper value is constant in real terms until the event.
                    s.stock_price = c.price_level;
                    // The words become uniforms only here, where they are used.
                    let [u_liquidity, u_class, u_multiple] = b_equity.map(uniform_from_bits);
                    if c.recession == 0 && u_liquidity < *monthly_hazard {
                        if u_class < *fail_prob {
                            s.liquidity = Liquidity::Failed;
                            s.stock_price = 0.0;
                            self.forfeit_unvested(s);
                            s.illiquid_vested = [0.0; MAX_GRANTS];
                            s.illiquid_strike_cost = [0.0; MAX_GRANTS];
                        } else {
                            let v = (u_class - fail_prob) / (1.0 - fail_prob);
                            let class = if v < class_cumulative[0] {
                                0
                            } else if v < class_cumulative[1] {
                                1
                            } else {
                                2
                            };
                            let [log_low, log_width] = class_log_range[class];
                            s.liquidity = Liquidity::Exited;
                            s.stock_price =
                                c.price_level * math::exp(log_low + log_width * u_multiple);
                            for g in 0..self.n_grants {
                                let grant = &self.grants[g];
                                let gross = s.illiquid_vested[g] * s.stock_price;
                                let value = if grant.strike.is_some() {
                                    (gross - s.illiquid_strike_cost[g]).max(0.0)
                                } else {
                                    gross
                                };
                                out.equity_income += value;
                                if !grant.sell_on_vest {
                                    out.equity_retained += value;
                                }
                                s.illiquid_vested[g] = 0.0;
                                s.illiquid_strike_cost[g] = 0.0;
                            }
                        }
                    }
                }
                Liquidity::Exited => {
                    s.stock_price *=
                        after_exit.gross_return(c.equity_return, normal_from_bits(r_stock));
                }
                Liquidity::Liquid | Liquidity::Failed => {}
            },
        }

        if !matches!(
            s.employment,
            Employment::Employed | Employment::OnBreak { .. }
        ) || out.rehired
        {
            return;
        }
        s.equity_clock += 1;
        let refresh_due = s.equity_clock % REFRESH_INTERVAL_MONTHS == 0;
        let ring_slot = (s.equity_clock / REFRESH_INTERVAL_MONTHS) as usize % REFRESH_RING;
        let price = s.stock_price;
        let pending = s.liquidity == Liquidity::Pending;
        for g in 0..self.n_grants {
            let grant = &self.grants[g];
            // (shares, strike) vesting this month: the original grant, then refreshes.
            let mut vests = [(0.0, 0.0); REFRESH_RING + 1];
            if s.vest_months_left[g] > 0 {
                s.vest_months_left[g] -= 1;
                s.cliff_months_left[g] = s.cliff_months_left[g].saturating_sub(1);
                if s.cliff_months_left[g] == 0 {
                    let still_scheduled = grant.per_month * f64::from(s.vest_months_left[g]);
                    let shares = (s.unvested[g] - still_scheduled).max(0.0);
                    s.unvested[g] -= shares;
                    vests[0] = (shares, grant.strike.unwrap_or(0.0));
                }
            }
            for j in 0..REFRESH_RING {
                vests[j + 1] = (s.refresh_rate[g][j], s.refresh_strike[g][j]);
            }
            for (shares, strike) in vests {
                if shares <= 0.0 {
                    continue;
                }
                if pending {
                    s.illiquid_vested[g] += shares;
                    if grant.strike.is_some() {
                        s.illiquid_strike_cost[g] += shares * strike;
                    }
                } else {
                    let value = if grant.strike.is_some() {
                        shares * (price - strike).max(0.0)
                    } else {
                        shares * price
                    };
                    out.equity_income += value;
                    if !grant.sell_on_vest {
                        out.equity_retained += value;
                    }
                }
            }
            if refresh_due {
                // The grant made four years ago finished vesting above; replace it.
                let rate = if grant.refresh_real > 0.0 && price > 0.0 {
                    grant.refresh_real * c.price_level / price / f64::from(REFRESH_VEST_MONTHS)
                } else {
                    0.0
                };
                s.refresh_rate[g][ring_slot] = rate;
                s.refresh_strike[g][ring_slot] = if grant.strike.is_some() { price } else { 0.0 };
            }
        }
    }

    /// Consumes exactly the draws [`JobModel::step`] would, without doing anything.
    /// For a job that has ended for good and has no private-company payout pending,
    /// this keeps both streams aligned at a fraction of the cost.
    #[inline]
    pub fn skip(&self, january: bool, career: &mut Rng, equity: &mut Rng) {
        self.skip_career(january, career);
        self.skip_equity(equity);
    }

    /// The career-stream half of [`JobModel::skip`].
    #[inline]
    pub fn skip_career(&self, january: bool, career: &mut Rng) {
        for _ in 0..if january { 7 } else { 5 } {
            career.next_u64();
        }
    }

    /// The equity-stream half of [`JobModel::skip`].
    #[inline]
    pub fn skip_equity(&self, equity: &mut Rng) {
        for _ in 0..4 {
            equity.next_u64();
        }
    }

    /// True when stepping the job can no longer produce anything: it has ended and
    /// no private-company liquidity event is pending.
    #[inline]
    pub fn is_inert(&self, state: &JobState) -> bool {
        state.employment == Employment::Ended && state.liquidity != Liquidity::Pending
    }

    /// Ends the job (retirement or event). Unvested equity is forfeited; vested
    /// private shares are kept and still pay out at the liquidity event, so keep
    /// calling [`JobModel::step`].
    pub fn end(&self, state: &mut JobState) {
        state.employment = Employment::Ended;
        state.spell_months_left = 0.0;
        self.forfeit_unvested(state);
    }

    /// Starts a career break of `months` at `income_fraction` of pay. Only an
    /// employed person can take a break; otherwise nothing happens. Equity keeps
    /// vesting during a break and the job cannot be lost during it.
    pub fn start_break(&self, state: &mut JobState, months: u32, income_fraction: f64) {
        if state.employment == Employment::Employed && months > 0 {
            state.employment = Employment::OnBreak {
                months_left: months,
            };
            state.break_income_fraction = income_fraction.clamp(0.0, 1.0);
        }
    }

    /// Applies an `income_change` "set salary" event: the real salary becomes exactly
    /// `real_salary` now and follows the wage curve from here. Past shocks and scars
    /// are wiped (the new figure already reflects them).
    pub fn set_real_salary(&self, state: &mut JobState, real_salary: f64) {
        let curve = if state.curve > 0.0 { state.curve } else { 1.0 };
        state.anchor = real_salary / curve;
        state.persistent_shock = 0.0;
        state.persistent_variance = 0.0;
        state.transitory_shock = 0.0;
        state.shock_factor = 1.0;
        state.real_salary = real_salary;
    }

    /// Applies an `income_change` "scale" event: multiplies pay by `factor`, keeping
    /// the shocks.
    pub fn scale_real_salary(&self, state: &mut JobState, factor: f64) {
        state.anchor *= factor;
        state.real_salary *= factor;
    }

    /// Expected real annual base salary at an age with no shocks and no job loss: the
    /// deterministic curve from the entered salary, used for Social Security backfill
    /// and forward projection. Ages before the start age return the entered salary.
    pub fn expected_real_salary(&self, age: f64) -> f64 {
        self.salary * self.curve.at(self.age0, age)
    }

    /// Wage curve factor at an age (1 at the owner's age at `profile.start`).
    pub fn curve_factor(&self, age: f64) -> f64 {
        self.curve.at(self.age0, age)
    }

    /// Real annual base salary projected to `age` from the path's current salary
    /// along the deterministic curve (for the FI projection on a path).
    pub fn projected_real_salary(&self, state: &JobState, age: f64) -> f64 {
        let now = if state.curve > 0.0 { state.curve } else { 1.0 };
        state.real_salary * self.curve.at(self.age0, age) / now
    }

    /// Nominal value of all unvested equity at the current price (RSUs at the share
    /// price, options at their spread), for net-worth reporting. Not spendable.
    pub fn unvested_value(&self, state: &JobState) -> f64 {
        let price = state.stock_price;
        let mut total = 0.0;
        for g in 0..self.n_grants {
            let grant = &self.grants[g];
            let per_share = |strike: f64| {
                if grant.strike.is_some() {
                    (price - strike).max(0.0)
                } else {
                    price
                }
            };
            total += state.unvested[g] * per_share(grant.strike.unwrap_or(0.0));
            // The refresh made k intervals ago has 48 - 12k - (clock mod 12) months left.
            let into = state.equity_clock % REFRESH_INTERVAL_MONTHS;
            let newest = (state.equity_clock / REFRESH_INTERVAL_MONTHS) as usize % REFRESH_RING;
            for k in 0..REFRESH_RING {
                let slot = (newest + REFRESH_RING - k) % REFRESH_RING;
                let elapsed = k as u32 * REFRESH_INTERVAL_MONTHS + into;
                let left = REFRESH_VEST_MONTHS.saturating_sub(elapsed);
                total += state.refresh_rate[g][slot]
                    * f64::from(left)
                    * per_share(state.refresh_strike[g][slot]);
            }
        }
        total
    }
}

/// Outcome classes of a private company given that it does not fail: cumulative
/// probabilities of the modest and good classes, and the log ranges of the multiples.
/// The research gives three stages keyed by their failure probability; the class mix
/// is interpolated between them on `fail_prob`.
fn private_classes(params: &CareerParams, fail_prob: f64) -> ([f64; 2], [[f64; 2]; 3]) {
    let stages = &params.private_stages;
    let conditional = |row: &[f64; 4]| {
        let success = (row[1] + row[2] + row[3]).max(1e-12);
        [row[1] / success, row[2] / success, row[3] / success]
    };
    let f = fail_prob.clamp(stages[0][0], stages[2][0]);
    let (lo, hi) = if f <= stages[1][0] { (0, 1) } else { (1, 2) };
    let span = stages[hi][0] - stages[lo][0];
    let w = if span > 0.0 {
        (f - stages[lo][0]) / span
    } else {
        0.0
    };
    let (a, b) = (conditional(&stages[lo]), conditional(&stages[hi]));
    let mix = [
        a[0] + w * (b[0] - a[0]),
        a[1] + w * (b[1] - a[1]),
        a[2] + w * (b[2] - a[2]),
    ];
    let mut ranges = [[0.0; 2]; 3];
    for (out, r) in ranges.iter_mut().zip(&params.private_multiple_ranges) {
        *out = [math::ln(r[0]), math::ln(r[1] / r[0])];
    }
    ([mix[0], mix[0] + mix[1]], ranges)
}

impl SelfEmploymentModel {
    /// Compiles a self-employment income. `risk_enabled` is `assumptions.career_risk`.
    ///
    /// Profit is `net_annual / 12 * (1 + real_growth)^years * price level * F`, where
    /// `F` is a unit-mean lognormal factor redrawn each January:
    /// `x <- 0.5 x + s sqrt(1 - 0.25) (0.5 m + sqrt(0.75) e)`, `F = exp(x - Var[x]/2)`.
    /// `s` is the log sd matching the entered volatility, `m` the standardised equity
    /// return of the last 12 months (so profit falls in bad years for the economy) and
    /// `e` an independent normal.
    pub fn new(se: &SelfEmployment, risk_enabled: bool, data: &Data) -> SelfEmploymentModel {
        let log_growth = math::ln_1p(se.real_growth.max(-0.99));
        SelfEmploymentModel {
            monthly_real: se.net_annual / 12.0,
            log_growth,
            growth: (0..SELF_EMPLOYMENT_GROWTH_MONTHS)
                .map(|m| math::exp(log_growth * (f64::from(m) / 12.0).max(0.0)))
                .collect(),
            log_sd: log_sd_from_cv(se.volatility),
            risk: risk_enabled && se.volatility > 0.0,
            params: CareerParams::from_data(data),
        }
    }

    /// State at `profile.start`.
    pub fn initial_state(&self) -> SelfEmploymentState {
        SelfEmploymentState::default()
    }

    /// Net profit for the month, nominal. `years_since_start` (years since
    /// `profile.start`) drives real growth. Consumes exactly one normal from `career`
    /// every month; it is used in January. Call it every month even when the income
    /// is not active and ignore the result.
    pub fn step(
        &self,
        state: &mut SelfEmploymentState,
        years_since_start: f64,
        climate: &CareerClimate,
        career: &mut Rng,
    ) -> f64 {
        let bits = career.next_u64();
        if self.risk && climate.now.month() == 1 && state.steps > 0 {
            let z = normal_from_bits(bits);
            let phi = SELF_EMPLOYMENT_PERSISTENCE;
            let rho = VARIABLE_PAY_MARKET_CORRELATION;
            let innovation_var = self.log_sd * self.log_sd * (1.0 - phi * phi);
            let mix = rho * self.params.market_signal(climate.equity_return_12m)
                + (1.0 - rho * rho).sqrt() * z;
            state.log_shock = phi * state.log_shock + innovation_var.sqrt() * mix;
            state.shock_variance = phi * phi * state.shock_variance + innovation_var;
            state.factor = math::exp(state.log_shock - 0.5 * state.shock_variance);
        }
        state.steps = state.steps.saturating_add(1);
        self.monthly_real * self.growth_at(years_since_start) * climate.price_level * state.factor
    }

    /// `exp(log_growth * max(years, 0))`, from the table when `years` is a whole
    /// number of months (as the simulation passes it): the table entry was computed
    /// from the same value, so it is the same number.
    #[inline]
    fn growth_at(&self, years: f64) -> f64 {
        let m = (years * 12.0).round();
        if m >= 0.0
            && m < SELF_EMPLOYMENT_GROWTH_MONTHS as f64
            && f64::from(m as u32) / 12.0 == years
        {
            self.growth[m as usize]
        } else {
            math::exp(self.log_growth * years.max(0.0))
        }
    }

    /// Expected real annual profit `years_since_start` years after `profile.start`.
    pub fn expected_real_annual(&self, years_since_start: f64) -> f64 {
        self.monthly_real * 12.0 * math::exp(self.log_growth * years_since_start.max(0.0))
    }
}

/// Real wage relative to age 40 on a lifecycle curve (1.0 at 40). Reads the data on
/// every call: for display and tests, not for the monthly loop.
pub fn lifecycle_factor(profile: LifecycleProfile, age: f64, data: &Data) -> f64 {
    let coef = CareerParams::from_data(data).curve_coefficients(profile);
    math::exp(log_profile(&coef, age) - log_profile(&coef, CURVE_REFERENCE_AGE))
}

/// Monthly layoff probability for a risk level at an age, in and out of recession.
/// Reads the data on every call: for display and tests, not for the monthly loop.
pub fn layoff_hazard(level: RiskLevel, age: f64, recession: bool, data: &Data) -> f64 {
    let p = CareerParams::from_data(data);
    let Some(i) = risk_index(level) else {
        return 0.0;
    };
    let cycle = if recession {
        p.recession_hazard_multiplier
    } else {
        1.0
    };
    (p.base_hazard
        * p.hazard_age_multiplier[hazard_age_band(age)]
        * p.security_multiplier[i]
        * cycle)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
