//! Schema of the opt-in long-term-care insurance (`person.ltc_insurance`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A long-term-care insurance policy owned by one person. Premiums are paid each
/// month until a care episode begins (they are then waived); once care has lasted
/// the elimination period the policy pays the cost of care up to the daily benefit
/// until the benefit pool (daily benefit x 365 x benefit years) is used up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LtcInsurance {
    /// Premium per year in dollars of the plan's start. Premiums of these policies
    /// are level in dollars (they do not rise with inflation) unless the insurer
    /// raises them for the whole class (`premium_increase_risk`). A typical policy
    /// bought at 55: $1,700 to $3,800 a year.
    pub annual_premium: f64,
    /// Most the policy pays per day of care, in dollars of the plan's start. Typical:
    /// $150 to $250.
    pub daily_benefit: f64,
    /// Size of the benefit pool in years of the full daily benefit. Typical: 2 to 5.
    pub benefit_years: f64,
    /// Days of paid care before benefits begin (the deductible in time). Typical: 90.
    pub elimination_days: f64,
    /// Growth of the daily benefit and of what is left of the pool each year.
    pub inflation_rider: InflationRider,
    /// Whether the insurer may raise the premium for the whole policy class (as most
    /// have done since 2001; on by default). Off: the premium stays as stated.
    pub premium_increase_risk: bool,
}

impl Default for LtcInsurance {
    fn default() -> Self {
        LtcInsurance {
            annual_premium: 2_500.0,
            daily_benefit: 200.0,
            benefit_years: 3.0,
            elimination_days: 90.0,
            inflation_rider: InflationRider::None,
            premium_increase_risk: true,
        }
    }
}

/// How the policy's benefit grows.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InflationRider {
    /// The benefit stays at its starting dollar amount.
    #[default]
    None,
    /// The daily benefit and the remaining pool grow by `rate` a year, compounded
    /// (3% is the common choice).
    Compound {
        /// Annual growth, for example 0.03.
        rate: f64,
    },
}
