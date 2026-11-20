//! Choices about the home beyond owning it: borrowing against it (a home-equity line
//! or a reverse mortgage), moving somewhere cheaper, refinancing for cash, paying the
//! mortgage down early, and how a rental's tenants and rent behave.
//!
//! Every type here is opt-in: its default leaves the engine exactly as it was, and a
//! default value is left out when a profile is written back to JSON.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Borrowing against the home when savings cannot pay the month, before any property
/// is sold (`home_equity: last_resort`) or essential spending is cut.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HomeEquityBorrowing {
    /// The household never borrows against its home.
    #[default]
    None,
    /// A home-equity line of credit on the main home, at any age: opened the first
    /// time it is needed, drawn on when savings are gone, repaid from later surpluses
    /// or when the home is sold.
    Heloc(Heloc),
    /// A reverse mortgage (FHA Home Equity Conversion Mortgage) on the main home, once
    /// the youngest adult is 62: a line of credit sized by age and interest rates that
    /// grows while unused, nothing to repay while living in the home (property tax
    /// and insurance must still be paid), the balance repaid when the home is sold or
    /// the last borrower dies, never more than the home fetches.
    ReverseMortgage(ReverseMortgage),
}

impl HomeEquityBorrowing {
    /// True for the default (no borrowing); used to leave it out of saved profiles.
    pub fn is_none(&self) -> bool {
        matches!(self, HomeEquityBorrowing::None)
    }
}

/// Terms of a home-equity line of credit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Heloc {
    /// Largest combined loan-to-value the lender allows: the mortgage and the line
    /// together, as a share of the home's value. Typical 0.80 to 0.85.
    pub max_combined_ltv: f64,
    /// The line's variable rate above the Treasury bill rate, as a decimal. The prime
    /// rate runs 3 points above the Federal Reserve's rate and lenders add a margin,
    /// so 0.035 is typical.
    pub spread: f64,
    /// Years after opening during which the line can be drawn and only interest is
    /// due. Typically 10.
    pub draw_years: f64,
    /// Years over which the balance is repaid after the draw period ends. Typically 20.
    pub repay_years: f64,
    /// Chance that lenders freeze lines (the unused part of an open line, and new
    /// lines) when a recession or a fall in home prices of 10% or more begins. Lines
    /// open again when it ends. 0.3 by default; 0 to ignore freezes.
    pub freeze_probability: f64,
}

impl Default for Heloc {
    fn default() -> Self {
        Self {
            max_combined_ltv: 0.80,
            spread: 0.035,
            draw_years: 10.0,
            repay_years: 20.0,
            freeze_probability: 0.3,
        }
    }
}

/// Terms of a reverse mortgage (FHA Home Equity Conversion Mortgage).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReverseMortgage {
    /// When the line is opened. It is drawn on only when savings cannot pay the month.
    pub open_when: ReverseMortgageOpen,
    /// The lender's margin above the Treasury rate, as a decimal: the loan's rate is
    /// the bill rate plus this margin (plus 0.5% a year of mortgage insurance), and the
    /// rate that sizes the line is the 10-year Treasury yield plus this margin.
    pub lender_margin: f64,
    /// Closing costs other than the insurance premium and the origination fee
    /// (appraisal, title, counselling, recording), today's dollars. Financed into the
    /// loan.
    pub other_closing_costs: f64,
}

impl Default for ReverseMortgage {
    fn default() -> Self {
        Self {
            open_when: ReverseMortgageOpen::LastResort,
            lender_margin: 0.02,
            other_closing_costs: 3_000.0,
        }
    }
}

/// When a reverse mortgage line is opened.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReverseMortgageOpen {
    /// Only when savings are gone and the month cannot be paid.
    #[default]
    LastResort,
    /// As soon as the youngest adult is 62 and liquid savings fall below this many
    /// years of the household's planned spending, so that the unused line grows
    /// before it is needed (the "standby line" strategy).
    SavingsBelow {
        /// Years of planned spending, for example 5.
        years: f64,
    },
}

/// Selling the home and moving somewhere cheaper, once, when a condition is met.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Downsize {
    /// What triggers the move.
    pub when: DownsizeWhen,
    /// Price of the new home as a share of the old home's market value at the time
    /// (0.6 = a home worth 60% as much). It is bought for cash out of the sale
    /// proceeds; with `rent_instead`, or if the proceeds do not cover it, the
    /// household rents such a home instead.
    pub to_value_fraction: f64,
    /// Rent instead of buying: the household pays 4% of the new home's value a year
    /// in rent, growing like other rents.
    pub rent_instead: bool,
    /// Selling costs as a share of the sale price (agent, transfer tax, concessions).
    pub selling_cost_fraction: f64,
    /// Buying costs as a share of the new home's price (title, recording, inspection).
    pub buying_cost_fraction: f64,
}

impl Default for Downsize {
    fn default() -> Self {
        Self {
            when: DownsizeWhen::WhenChildrenLeave,
            to_value_fraction: 0.6,
            rent_instead: false,
            selling_cost_fraction: 0.06,
            buying_cost_fraction: 0.03,
        }
    }
}

/// What triggers a planned downsizing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DownsizeWhen {
    /// When a person reaches an age.
    AtAge {
        /// Whose age: 0 for the first person, 1 for the second.
        #[serde(default)]
        person: usize,
        /// Age in years.
        age: f64,
    },
    /// When the youngest child leaves home: at 18, or at 22 if college is planned.
    WhenChildrenLeave,
    /// A year after the first death in a couple.
    WhenWidowed,
    /// In a January of retirement when investable savings fall below this share of
    /// what the retirement test requires (1.0 = exactly funded).
    WhenFundedRatioBelow {
        /// The funded ratio below which the household moves, for example 0.8.
        ratio: f64,
    },
}

/// Refinancing for cash when savings would otherwise be withdrawn with a penalty.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CashOutRefinance {
    /// The new loan is this share of the home's value (lenders allow up to 0.80 on a
    /// main home).
    pub max_ltv: f64,
    /// How much more a cash-out loan costs than the market's mortgage rate, as a
    /// decimal (lenders price cash-out loans higher).
    pub rate_premium: f64,
    /// Term of the new loan in years.
    pub term_years: u32,
}

impl Default for CashOutRefinance {
    fn default() -> Self {
        Self {
            max_ltv: 0.80,
            rate_premium: 0.0025,
            term_years: 30,
        }
    }
}

/// Whether spare cash pays the mortgage down early.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MortgagePrepayment {
    /// Spare cash is invested; mortgages are paid on schedule.
    #[default]
    Never,
    /// Spare cash that would go to the taxable account pays down the mortgage with
    /// the highest rate instead, while that rate after expected inflation is above
    /// `real_rate`.
    WhenRateAbove {
        /// Real rate above which paying the mortgage beats investing, as a decimal.
        /// Leave empty to use the after-tax real yield of 10-year Treasuries at the
        /// time (the safe return the money would otherwise earn).
        #[serde(default)]
        real_rate: Option<f64>,
    },
}

impl MortgagePrepayment {
    /// True for the default (never); used to leave it out of saved profiles.
    pub fn is_never(&self) -> bool {
        matches!(self, MortgagePrepayment::Never)
    }
}

/// How a rental's occupancy and rent behave over time.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RentalDynamics {
    /// Short random vacancies at the entered rate, rent growing smoothly.
    #[default]
    Simple,
    /// Tenants move out at random and the unit takes months to re-let (longer in a
    /// recession); rents follow a market index that wanders around its trend and a
    /// new lease starts at the market rent; occasional damage, evictions and large
    /// repairs. The long-run vacancy is still the entered rate.
    Realistic {
        /// Share of tenancies that end each year. Single-family landlords report
        /// about 0.25.
        #[serde(default = "d_turnover")]
        annual_turnover: f64,
    },
}

fn d_turnover() -> f64 {
    0.25
}

impl RentalDynamics {
    /// True for the default (simple); used to leave it out of saved profiles.
    pub fn is_simple(&self) -> bool {
        matches!(self, RentalDynamics::Simple)
    }
}
