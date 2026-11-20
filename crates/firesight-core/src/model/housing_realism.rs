//! Opt-in housing realism: lumpy maintenance, property-tax assessment rules and
//! insurance drift, market rents, slow home sales and adjustable-rate mortgages.
//!
//! Every type here defaults to the behaviour the engine had before the option
//! existed, so a profile that does not mention them simulates bit-identically.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a property's maintenance and repair cost arrives through time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceModel {
    /// A smooth charge every month: `maintenance_rate` times the value, divided by 12.
    #[default]
    Smooth,
    /// Half of `maintenance_rate` is routine upkeep, charged smoothly. The other half
    /// arrives as random major repairs (roof, heating and cooling, water heater,
    /// exterior, appliances) whose chance rises with the years since each part was last
    /// replaced. The expected cost over a long time equals the smooth rate; the path
    /// is lumpy.
    Lumpy,
}

/// How the value on which property tax is charged follows the market value.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Assessment {
    /// Reassessed to market value every year. The default.
    #[default]
    Market,
    /// The assessed value may rise by at most `annual_cap` a year (California's
    /// Proposition 13 is 2%) and never exceeds market value. It is reset to the
    /// purchase price when the household buys a home.
    Capped {
        /// Largest yearly rise in assessed value, as a decimal (0.02 = 2%).
        annual_cap: f64,
        /// Assessed value today, in dollars. Leave empty to start from the basis
        /// (the purchase price plus improvements), or the value if there is no basis.
        assessed_value: Option<f64>,
    },
}

/// How homeowner's insurance premiums change relative to inflation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InsuranceGrowth {
    /// The premium is `insurance_rate` of the value and has no other drift. The default.
    #[default]
    Fixed,
    /// On top of the value-based premium, a persistent real increase that varies
    /// year to year, with occasional jumps of 10 to 20% when insurers reprice
    /// (as in 2021 to 2025). The same index applies to every property of the path.
    Stochastic,
}

/// How a mortgage's rate is set.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MortgageKind {
    /// The rate never changes. The default.
    #[default]
    Fixed,
    /// Adjustable rate: fixed for `fixed_months`, then reset every
    /// `adjust_every_months` to the simulated short-term index (the Treasury bill
    /// yield) plus `margin`, limited by the caps. At each reset the payment is
    /// recomputed to repay the balance over the months left.
    Arm {
        /// Months at the starting rate before the first reset (60 for a 5/1 ARM). For a
        /// mortgage that already exists, months from the start of the plan.
        fixed_months: u32,
        /// Months between resets after the fixed period (12 for a 5/1 ARM).
        adjust_every_months: u32,
        /// Added to the index to get the fully indexed rate, as a decimal (0.0275).
        /// The rate never falls below this margin.
        margin: f64,
        /// Largest change at one reset, up or down, as a decimal (0.02). Also applies
        /// to the first reset.
        periodic_cap: f64,
        /// Largest rise above the starting rate over the life of the loan (0.05).
        lifetime_cap: f64,
    },
}

/// How a budget line in the rent category grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RentGrowth {
    /// Inflation plus the line's `real_growth` (default 0.5% a year). The default.
    #[default]
    Fixed,
    /// Rent follows the path's local rent market: it responds, with a lag, to home
    /// prices, carries persistent local noise, and is reset to market at each yearly
    /// lease renewal, sometimes sharply (a forced move). Only for the rent category;
    /// `real_growth` is then ignored.
    Market,
}

/// How long selling a home takes and what it fetches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HousingLiquidity {
    /// A sale closes the month it is decided, at the market value. The default.
    #[default]
    Instant,
    /// A sale (planned or forced by running out of money) takes a random number of
    /// months to close, longer in recessions and when prices are falling, and closes
    /// below market value in those conditions. Carrying costs continue until closing.
    Realistic,
}

/// Serde helper: omit a field from the JSON when it holds its default, so profiles
/// that do not use the option serialise exactly as before.
pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
