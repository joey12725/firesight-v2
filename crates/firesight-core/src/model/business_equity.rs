//! Opt-in risks for self-employment income and for stock from an employer: a
//! business that can close, profit that arrives in lumps, an employer whose shares
//! can collapse, and the timing and size of a private company's exit. Every default
//! here is the behaviour the engine had before these options existed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether a self-employment income can stop because the business closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BusinessFailure {
    /// The business never closes: profit varies from year to year but is always
    /// there until the income's planned end. The default.
    #[default]
    Never,
    /// Each year the business has a chance of closing, higher while it is young and
    /// in recessions. This is a risk on top of the entered profit, not a
    /// re-arrangement of it: average lifetime profit is lower than the amount
    /// entered. No unemployment insurance is paid.
    Hazard {
        /// Chance per year that a young business closes, in normal times, as a
        /// decimal from 0 to 0.5. Default 0.10.
        #[serde(default = "d_young_probability")]
        young_annual_probability: f64,
        /// Chance per year that an established business closes, in normal times,
        /// as a decimal from 0 to 0.5. Default 0.04.
        #[serde(default = "d_mature_probability")]
        mature_annual_probability: f64,
        /// How many years a business counts as young, from 0 to 30. Default 5.
        #[serde(default = "d_young_years")]
        young_years: f64,
        /// How long the business has been running, in years, when this income
        /// starts (today, if it is already being received). Leave empty to treat a
        /// business that is already running as established and one that starts
        /// later as new.
        #[serde(default)]
        years_in_business: Option<f64>,
        /// How many times more likely a closure is in a recession month, from 1 to
        /// 10. Default 2.
        #[serde(default = "d_recession_multiplier")]
        recession_multiplier: f64,
        /// What happens after a closure. Default: work starts again after 12 months
        /// at 70% of the old profit.
        #[serde(default)]
        then: AfterFailure,
    },
}

fn d_young_probability() -> f64 {
    BusinessFailure::DEFAULT_YOUNG_PROBABILITY
}
fn d_mature_probability() -> f64 {
    BusinessFailure::DEFAULT_MATURE_PROBABILITY
}
fn d_young_years() -> f64 {
    BusinessFailure::DEFAULT_YOUNG_YEARS
}
fn d_recession_multiplier() -> f64 {
    BusinessFailure::DEFAULT_RECESSION_MULTIPLIER
}

impl BusinessFailure {
    /// Default chance per year that a young business closes. A judgement
    /// (`business_and_equity.business_failure` in `data/actuarial/params.json`).
    pub const DEFAULT_YOUNG_PROBABILITY: f64 = 0.10;
    /// Default chance per year that an established business closes. A judgement.
    pub const DEFAULT_MATURE_PROBABILITY: f64 = 0.04;
    /// Default number of years a business counts as young. A judgement.
    pub const DEFAULT_YOUNG_YEARS: f64 = 5.0;
    /// Default multiplier on the chance of closing in a recession month. A
    /// judgement.
    pub const DEFAULT_RECESSION_MULTIPLIER: f64 = 2.0;

    /// The hazard at its default settings.
    pub fn hazard() -> BusinessFailure {
        BusinessFailure::Hazard {
            young_annual_probability: Self::DEFAULT_YOUNG_PROBABILITY,
            mature_annual_probability: Self::DEFAULT_MATURE_PROBABILITY,
            young_years: Self::DEFAULT_YOUNG_YEARS,
            years_in_business: None,
            recession_multiplier: Self::DEFAULT_RECESSION_MULTIPLIER,
            then: AfterFailure::default(),
        }
    }
}

/// What happens to a self-employment income after the business closes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AfterFailure {
    /// The income stops for good.
    Stops,
    /// After a gap with no income, work starts again at a share of the old profit.
    /// The new venture counts as a young business and can close in its turn.
    Restart {
        /// Months with no income before work starts again, from 0 to 120.
        /// Default 12.
        #[serde(default = "d_gap_months")]
        gap_months: u32,
        /// Profit after the restart as a share of what it would have been without
        /// the closure, as a decimal from 0 to 2 (0.7 = 70%). The cut is lasting.
        /// Default 0.7.
        #[serde(default = "d_income_fraction")]
        income_fraction: f64,
    },
}

fn d_gap_months() -> u32 {
    AfterFailure::DEFAULT_GAP_MONTHS
}
fn d_income_fraction() -> f64 {
    AfterFailure::DEFAULT_INCOME_FRACTION
}

impl AfterFailure {
    /// Default months without income after a closure. A judgement.
    pub const DEFAULT_GAP_MONTHS: u32 = 12;
    /// Default share of the old profit after a restart. A judgement.
    pub const DEFAULT_INCOME_FRACTION: f64 = 0.7;
}

impl Default for AfterFailure {
    fn default() -> Self {
        AfterFailure::Restart {
            gap_months: Self::DEFAULT_GAP_MONTHS,
            income_fraction: Self::DEFAULT_INCOME_FRACTION,
        }
    }
}

/// Whether a listed employer's shares can become worthless for good.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StockRuin {
    /// The shares rise and fall but never go to zero in one step. The default.
    #[default]
    Never,
    /// Each year the shares have a chance of becoming worthless for good, set by
    /// how volatile they are: about 0.3% a year for a large established company,
    /// 1% for a typical growth company and 3% for a small or recently listed one.
    /// In the other years they earn a little more, so the average return is the
    /// same as without this option.
    ByVolatility,
    /// As `by_volatility` with the chance stated directly.
    Stated {
        /// Chance per year that the shares become worthless, as a decimal from 0
        /// to 0.5 (0.01 = 1%). Default 0.01.
        #[serde(default = "d_ruin_probability")]
        annual_probability: f64,
    },
}

fn d_ruin_probability() -> f64 {
    StockRuin::DEFAULT_ANNUAL_PROBABILITY
}

impl StockRuin {
    /// Default stated chance per year of a total loss: the typical growth company
    /// of `equity_compensation.single_public_stock.defaults` in params.json.
    pub const DEFAULT_ANNUAL_PROBABILITY: f64 = 0.01;
}

/// How the date of a private company's outcome (an IPO, a sale or a shutdown) is
/// decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExitTiming {
    /// The same chance every year (`liquidity_hazard`), so the single most likely
    /// year for the outcome is the first. The default.
    #[default]
    ConstantHazard,
    /// The outcome is very unlikely in the first year or two and becomes likely
    /// around a typical wait that you state: half of such companies have had
    /// their outcome by then. `liquidity_hazard` is not used. Recession months
    /// still postpone the outcome.
    Lognormal {
        /// Years until half of such companies have had their outcome, from 0.25
        /// to 40. About 7 for a company at seed or series A, 5 at series B or C
        /// and 3 for a late-stage company. Default 5.
        #[serde(default = "d_median_years")]
        median_years: f64,
        /// How spread out the wait is, from 0.05 to 2. With the default 0.5, one
        /// company in ten has its outcome before half the typical wait and one in
        /// ten after about twice it. Default 0.5.
        #[serde(default = "d_exit_sigma")]
        sigma: f64,
    },
}

fn d_median_years() -> f64 {
    ExitTiming::DEFAULT_MEDIAN_YEARS
}
fn d_exit_sigma() -> f64 {
    ExitTiming::DEFAULT_SIGMA
}

impl ExitTiming {
    /// Default typical wait in years: the series B to C stage of
    /// `equity_compensation.private_company.model.stage_table` in params.json.
    pub const DEFAULT_MEDIAN_YEARS: f64 = 5.0;
    /// Default spread of the wait: the 0.5 of the same section's equation.
    pub const DEFAULT_SIGMA: f64 = 0.5;

    /// The lognormal wait at its default settings.
    pub const fn lognormal() -> ExitTiming {
        ExitTiming::Lognormal {
            median_years: Self::DEFAULT_MEDIAN_YEARS,
            sigma: Self::DEFAULT_SIGMA,
        }
    }
}

/// How large the best outcomes of a private company can be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExitSize {
    /// A large outcome pays between 4 and 20 times the paper value, and never
    /// more. The default.
    #[default]
    Classes,
    /// A large outcome pays from 4 times the paper value up to 200 times, with
    /// most near the bottom of that range and a few far above it. The chance of
    /// each kind of outcome and the average payout are the same as under
    /// `classes`; about one large outcome in twelve pays more than 20 times.
    ParetoTail,
}
