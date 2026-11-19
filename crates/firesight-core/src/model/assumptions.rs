//! Modelling assumptions and simulation settings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::When;

/// Assumptions about markets, taxes and lifespans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Assumptions {
    /// How future market returns, interest rates and inflation are generated.
    pub market: MarketModel,
    /// Which tax rules apply.
    pub taxes: TaxModel,
    /// How long people live in the simulation.
    pub longevity: Longevity,
    /// Simulate job loss and pay shocks. Turning this off makes every job follow its
    /// growth curve exactly, whatever its own risk level says.
    pub career_risk: bool,
    /// Simulate random medical costs and long-term care. Turning this off uses average
    /// costs every year.
    pub health_risk: bool,
}

impl Default for Assumptions {
    fn default() -> Self {
        Self {
            market: MarketModel::default(),
            taxes: TaxModel::default(),
            longevity: Longevity::default(),
            career_risk: true,
            health_risk: true,
        }
    }
}

/// The market scenario generator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MarketModel {
    /// The method used to generate returns and inflation.
    pub kind: MarketKind,
    /// Added to the annual return of stocks, as a decimal. The market history is
    /// that of the United States from 1926, the best-performing large market of
    /// the century (7.2% a year after inflation; the world outside the US returned
    /// about 4.5%, and estimates based on today's valuations are 3 to 5%). The
    /// default of -0.015 plans for stocks returning one and a half points a year
    /// less than that record. Set 0 to use the record as it was, or a more negative
    /// value to be more cautious.
    pub equity_premium_adjustment: f64,
    /// Added to annual inflation, as a decimal. Use a positive value to plan for
    /// higher inflation than history.
    pub inflation_adjustment: f64,
    /// Stress scenarios forced onto every path, on top of the random draws.
    pub stress: Vec<Stress>,
}

/// Default of [`MarketModel::equity_premium_adjustment`]: a judgement between the
/// US record (0) and the long-run international and valuation-based views (-0.02
/// to -0.03).
pub const DEFAULT_EQUITY_PREMIUM_ADJUSTMENT: f64 = -0.015;

impl Default for MarketModel {
    fn default() -> Self {
        Self {
            kind: MarketKind::default(),
            equity_premium_adjustment: DEFAULT_EQUITY_PREMIUM_ADJUSTMENT,
            inflation_adjustment: 0.0,
            stress: Vec::new(),
        }
    }
}

/// Method for generating market paths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarketKind {
    /// Resample blocks of real history (stocks, bonds, inflation and rates together),
    /// which keeps crashes, recoveries and inflation regimes realistic. The default.
    Bootstrap {
        /// Average length in months of each resampled block. Longer blocks preserve
        /// more of history's multi-year patterns. Default 60.
        #[serde(default = "d_block")]
        mean_block_months: u32,
        /// Only resample history from this year onwards. Leave empty to use all
        /// available history.
        #[serde(default)]
        from_year: Option<i32>,
    },
    /// A statistical model that switches between calm and crisis regimes.
    Regime,
    /// Independent lognormal returns each month: the simple textbook model, with no
    /// crashes clustering and no mean reversion.
    Lognormal,
    /// Replay history exactly: path 1 starts in the first month of the record, path 2
    /// in the next, and so on. Answers "would this plan have survived 1966?". Set
    /// `equity_premium_adjustment` to 0 with this model, or the replay is history
    /// less that adjustment.
    Historical,
}

fn d_block() -> u32 {
    60
}

impl Default for MarketKind {
    fn default() -> Self {
        MarketKind::Bootstrap {
            mean_block_months: d_block(),
            from_year: None,
        }
    }
}

/// A stress scenario applied to every path.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Stress {
    /// Stocks crash over the six months after the household retires, then recover:
    /// the worst case for sequence risk.
    CrashAtRetirement {
        /// Peak-to-trough fall in stocks as a positive decimal (0.4 = a 40% fall).
        #[serde(default = "d_drawdown")]
        drawdown: f64,
        /// Years for stocks to climb back to the pre-crash level (in nominal terms).
        /// What they would have earned over the crash and the recovery is lost for
        /// good.
        #[serde(default = "d_recovery")]
        recovery_years: f64,
    },
    /// Ten years in which stocks return nothing after inflation.
    LostDecade {
        /// When the decade starts.
        start: When,
    },
    /// A period of high inflation with weak stock and bond returns.
    Stagflation {
        /// When it starts.
        start: When,
        /// How long it lasts, in years.
        #[serde(default = "d_stag_years")]
        years: f64,
        /// Annual inflation during the period, as a decimal (0.08 = 8%).
        #[serde(default = "d_stag_inflation")]
        inflation: f64,
    },
}

fn d_drawdown() -> f64 {
    0.4
}
fn d_recovery() -> f64 {
    5.0
}
fn d_stag_years() -> f64 {
    8.0
}
fn d_stag_inflation() -> f64 {
    0.08
}

/// Tax assumptions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TaxModel {
    /// Which tax system is applied.
    pub kind: TaxKind,
    /// Raise tax brackets, deductions and contribution limits with inflation each year,
    /// as current law does. Turning this off freezes them in dollars, which slowly
    /// raises taxes.
    pub index_brackets: bool,
    /// Plan for a cut in Social Security benefits when the trust fund runs short.
    /// Leave empty to assume benefits are paid in full.
    pub social_security_haircut: Option<SocialSecurityHaircut>,
    /// Index the thresholds that current law ties to the chained CPI (income tax
    /// brackets, the standard deduction, capital-gain thresholds, QBI thresholds and
    /// the child credit amounts) at about a quarter of a point a year less than the
    /// CPI-U that drives everything else. Off by default, which indexes them with the
    /// CPI-U. Brackets creep up on real income by about 2.5% per decade when on.
    /// Contribution limits, IRMAA tiers and state schedules stay on the CPI-U.
    #[serde(skip_serializing_if = "is_false")]
    pub chained_cpi: bool,
    /// How much of the state rules to apply. `basic` (the default) is the schedule,
    /// deductions, exemptions and the main retirement and Social Security rules.
    /// `full` adds three rules the basic tables leave out: California's 1.3% State
    /// Disability Insurance payroll tax on wages, New York's recapture of the lower
    /// brackets above $107,650 of income, and Utah's Social Security benefits credit.
    /// It changes nothing for households in other states.
    #[serde(skip_serializing_if = "is_basic")]
    pub state_detail: StateDetail,
}

impl Default for TaxModel {
    fn default() -> Self {
        Self {
            kind: TaxKind::UsCurrentLaw,
            index_brackets: true,
            social_security_haircut: None,
            chained_cpi: false,
            state_detail: StateDetail::Basic,
        }
    }
}

/// Serde helper: options that are off are left out of the JSON, so profiles written
/// before an option existed serialise exactly as they did.
fn is_false(b: &bool) -> bool {
    !*b
}

fn is_basic(d: &StateDetail) -> bool {
    *d == StateDetail::Basic
}

/// How completely state tax rules are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateDetail {
    /// State schedule, deductions, exemptions, credits and retirement-income rules as
    /// in the standard tables.
    #[default]
    Basic,
    /// Also California SDI on wages, New York's tax-benefit recapture for high
    /// earners and Utah's Social Security benefits credit.
    Full,
}

/// Tax system.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaxKind {
    /// US federal and state tax as currently legislated, including payroll tax,
    /// capital gains rates, credits and health-insurance subsidies.
    #[default]
    UsCurrentLaw,
    /// One flat rate on all income and realised gains.
    Flat {
        /// Tax rate as a decimal (0.2 = 20%).
        rate: f64,
    },
    /// No tax at all. Useful only for comparisons.
    None,
}

/// A future reduction in Social Security benefits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SocialSecurityHaircut {
    /// Calendar year from which benefits are reduced. The trustees currently project
    /// the retirement trust fund to run short around 2033.
    pub from_year: i32,
    /// Fraction of scheduled benefits that is paid from then on (0.79 = a 21% cut,
    /// roughly what payroll taxes alone could fund).
    pub factor: f64,
}

impl Default for SocialSecurityHaircut {
    fn default() -> Self {
        Self {
            from_year: 2033,
            factor: 0.79,
        }
    }
}

/// How lifespans are simulated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Longevity {
    /// Each person's age at death is drawn at random from actuarial life tables,
    /// adjusted for sex and health, on every path.
    Stochastic {
        /// Assume mortality keeps improving as it has historically, which adds a year
        /// or two of life expectancy for younger people.
        #[serde(default = "d_true")]
        improvement: bool,
    },
    /// Everyone lives to exactly this age. Simple, but hides the risk of living longer.
    FixedAge {
        /// Age at death in years.
        age: f64,
    },
}

fn d_true() -> bool {
    true
}

impl Default for Longevity {
    fn default() -> Self {
        Longevity::Stochastic { improvement: true }
    }
}

/// How the simulation is run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SimulationSettings {
    /// Number of simulated lifetimes. More paths give smoother results; 10,000 is
    /// plenty for most questions.
    pub paths: u32,
    /// Random seed. The same profile and seed always produce exactly the same results.
    /// Keep it below 9,007,199,254,740,992 so JavaScript can represent it exactly.
    pub seed: u64,
    /// The simulation stops when the youngest adult would reach this age, even if they
    /// are still alive on a path. Default 100.
    pub end_age: u32,
}

impl Default for SimulationSettings {
    fn default() -> Self {
        Self {
            paths: 10_000,
            seed: 1,
            end_age: 100,
        }
    }
}
