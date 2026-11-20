//! When the household retires and how it draws down its savings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{HomeEquityBorrowing, YearMonth};

/// The retirement plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RetirementPlan {
    /// What decides the retirement date.
    pub trigger: RetirementTrigger,
    /// In a two-person household, how many years the second person keeps working after
    /// the first retires. Negative means they stop earlier. 0 means both stop together.
    pub partner_offset_years: f64,
    /// How much is spent each year in retirement and how it responds to markets.
    pub spending_rule: SpendingRule,
    /// Which accounts are drawn on first.
    pub withdrawal_order: WithdrawalOrder,
    /// How retirement accounts are reached before age 59 and a half.
    pub early_access: EarlyAccess,
    /// Amount the household wants to leave behind, in today's dollars. The plan counts
    /// as falling short if savings drop below it. 0 for no bequest goal.
    pub bequest_target: f64,
    /// What happens to property when savings run out.
    pub home_equity: HomeEquity,
    /// Borrowing against the home when savings run out, before property is sold or
    /// essential spending is cut: none (default), a home-equity line, or a reverse
    /// mortgage.
    #[serde(skip_serializing_if = "HomeEquityBorrowing::is_none")]
    pub home_equity_borrowing: HomeEquityBorrowing,
}

/// Whether property is sold when every account is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HomeEquity {
    /// Property is never sold unless a `sell_property` event says so. A household
    /// that runs out of savings cuts its spending to its income and is counted as
    /// depleted, whatever its home is worth.
    Never,
    /// When savings are gone and essential spending would have to be cut, property
    /// is sold: a vacation home first, then a rental, then the home itself (6%
    /// selling costs, the mortgage repaid, the gain taxed as usual). After selling
    /// its home the household rents, at 4% of the sale price a year. The plan is
    /// then not depleted; the sale is reported on its own (`home_sale_rate`).
    #[default]
    LastResort,
}

impl Default for RetirementPlan {
    fn default() -> Self {
        Self {
            trigger: RetirementTrigger::default(),
            partner_offset_years: 0.0,
            spending_rule: SpendingRule::ConstantReal,
            withdrawal_order: WithdrawalOrder::default(),
            early_access: EarlyAccess::Auto,
            bequest_target: 0.0,
            home_equity: HomeEquity::LastResort,
            home_equity_borrowing: HomeEquityBorrowing::None,
        }
    }
}

/// What decides when the household retires.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetirementTrigger {
    /// Retire when a person reaches an age.
    AtAge {
        /// Whose age: 0 for the first person, 1 for the second.
        #[serde(default)]
        person: usize,
        /// Retirement age in years.
        age: f64,
    },
    /// Retire in a fixed month.
    AtDate {
        /// The retirement month, as "YYYY-MM".
        at: YearMonth,
    },
    /// Retire as soon as savings can support retirement spending: the classic FIRE
    /// rule. The date is different on every simulated path.
    WhenFunded(WhenFunded),
    /// Retire when savings reach a fixed amount.
    FixedNumber {
        /// Target in today's dollars.
        amount: f64,
        /// What counts toward the target.
        #[serde(default)]
        basis: WealthBasis,
    },
    /// The household has already stopped working.
    AlreadyRetired,
}

impl Default for RetirementTrigger {
    fn default() -> Self {
        RetirementTrigger::WhenFunded(WhenFunded::default())
    }
}

/// Parameters of the "retire when funded" rule.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WhenFunded {
    /// The share of savings assumed safe to spend each year, as a decimal. The target
    /// is annual retirement spending divided by this rate: 0.04 means 25 times
    /// spending. Lower is more cautious; 0.033 to 0.035 is common for retirements
    /// longer than 30 years.
    pub withdrawal_rate: f64,
    /// What counts toward the target. Home equity is not spendable, so "investable"
    /// is the honest default.
    pub basis: WealthBasis,
    /// Reduce the target by the present value of income that continues in retirement
    /// (Social Security, pensions, rental income).
    pub count_future_income: bool,
    /// Do not retire before this age of the first person, even if funded. Leave empty
    /// for no minimum.
    pub min_age: Option<f64>,
    /// Retire at this age of the first person at the latest, funded or not. Default 70.
    pub max_age: f64,
}

impl Default for WhenFunded {
    fn default() -> Self {
        Self {
            withdrawal_rate: 0.04,
            basis: WealthBasis::Investable,
            count_future_income: true,
            min_age: None,
            max_age: 70.0,
        }
    }
}

/// Which assets count toward a retirement target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WealthBasis {
    /// Cash, taxable, retirement and HSA accounts, minus non-mortgage debt.
    #[default]
    Investable,
    /// Everything, including home equity.
    NetWorth,
}

/// How much is spent each year in retirement.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpendingRule {
    /// Spend the retirement budget every year, adjusted for inflation, whatever markets
    /// do. Predictable income, highest risk of running out.
    #[default]
    ConstantReal,
    /// Spend a fixed percentage of the current portfolio each year. Never runs out,
    /// but income swings with the market.
    PercentOfPortfolio {
        /// Share of the portfolio spent each year, as a decimal (0.04 = 4%).
        rate: f64,
    },
    /// Guyton-Klinger style guardrails: spending follows the budget, but is cut when
    /// the withdrawal rate drifts too high and raised when it falls too low.
    Guardrails {
        /// Cut spending when the current withdrawal rate exceeds the initial rate by
        /// this fraction (0.2 = 20% above).
        #[serde(default = "d_guard_band")]
        upper: f64,
        /// Raise spending when the current withdrawal rate falls below the initial rate
        /// by this fraction (0.2 = 20% below).
        #[serde(default = "d_guard_band")]
        lower: f64,
        /// Size of each cut or raise, as a fraction of spending (0.1 = 10%).
        #[serde(default = "d_guard_adjust")]
        adjust: f64,
    },
    /// Vanguard's dynamic spending: target a percentage of the portfolio but limit the
    /// change in real spending from one year to the next.
    VanguardDynamic {
        /// Largest real increase in one year, as a decimal (0.05 = +5%).
        #[serde(default = "d_vg_ceiling")]
        ceiling: f64,
        /// Largest real decrease in one year, as a positive decimal (0.025 = -2.5%).
        #[serde(default = "d_vg_floor")]
        floor: f64,
        /// Share of the portfolio targeted each year, as a decimal (0.04 = 4%). Leave
        /// empty to use the household's own withdrawal rate in its first year of
        /// retirement.
        #[serde(default)]
        rate: Option<f64>,
    },
    /// Variable percentage withdrawal: each year spend the amount that would exhaust
    /// the portfolio exactly at the end of the plan given expected returns. Spends
    /// more late in life, never runs out, varies with markets.
    Vpw,
    /// Spend a percentage of the portfolio, but never less than a floor nor more than
    /// a ceiling relative to the retirement budget.
    FloorCeiling {
        /// Lowest spending as a fraction of the retirement budget (0.85 = never cut
        /// more than 15%).
        #[serde(default = "d_fc_floor")]
        floor: f64,
        /// Highest spending as a fraction of the retirement budget (1.2 = never more
        /// than 20% above).
        #[serde(default = "d_fc_ceiling")]
        ceiling: f64,
        /// Share of the portfolio targeted each year, as a decimal (0.045 = 4.5%).
        /// Leave empty to use the household's own withdrawal rate in its first year
        /// of retirement.
        #[serde(default)]
        rate: Option<f64>,
    },
}

fn d_guard_band() -> f64 {
    0.2
}
fn d_guard_adjust() -> f64 {
    0.1
}
fn d_vg_ceiling() -> f64 {
    0.05
}
fn d_vg_floor() -> f64 {
    0.025
}
fn d_fc_floor() -> f64 {
    0.85
}
fn d_fc_ceiling() -> f64 {
    1.2
}

/// Which accounts retirement spending is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WithdrawalOrder {
    /// The textbook order: taxable accounts first, then traditional, then Roth.
    Conventional,
    /// Draw from every account in proportion to its size.
    Proportional,
    /// Plan withdrawals each year to keep lifetime tax low.
    TaxAware {
        /// Fill ordinary income up to the top of this federal bracket each year with
        /// traditional withdrawals or Roth conversions, as the bracket's rate
        /// (0.12 or 0.22 are the usual choices).
        #[serde(default = "d_fill_bracket")]
        fill_to_bracket: f64,
        /// Convert traditional money to Roth in low-income years.
        #[serde(default = "d_true")]
        roth_conversions: bool,
        /// Keep income under the ACA premium-credit cliff while on a marketplace plan.
        #[serde(default = "d_true")]
        mind_aca: bool,
    },
}

fn d_fill_bracket() -> f64 {
    0.12
}
fn d_true() -> bool {
    true
}

impl Default for WithdrawalOrder {
    fn default() -> Self {
        WithdrawalOrder::TaxAware {
            fill_to_bracket: d_fill_bracket(),
            roth_conversions: true,
            mind_aca: true,
        }
    }
}

/// How retirement money is reached before age 59 and a half.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EarlyAccess {
    /// Use every penalty-free route in order: taxable accounts, Roth contributions,
    /// Roth conversions that are five years old, then 72(t) equal payments. Pay the
    /// 10% penalty only as a last resort.
    #[default]
    Auto,
    /// Withdraw from retirement accounts directly and pay the 10% penalty.
    PayPenalty,
}
