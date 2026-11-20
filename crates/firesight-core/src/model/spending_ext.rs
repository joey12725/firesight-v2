//! Opt-in spending realism: vehicles that wear out, seasonal spending, personal
//! inflation by category. Every type here defaults to "off", so a profile that does
//! not mention them behaves exactly as before.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::When;

/// A vehicle the household replaces on a cycle. Spending on cars is lumpy: a
/// $40,000 purchase every ten years, not $4,000 every year.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Vehicle {
    /// Stable identifier used by patches, for example "family_suv".
    pub id: String,
    /// Display name.
    pub name: String,
    /// Price of the replacement in today's dollars. It rises with general prices.
    pub price: f64,
    /// Years between replacements (a car kept 10 years: 10). The first replacement
    /// falls `replace_every_years - age_years` from now; each later one falls one
    /// cycle after the last, give or take a random jitter of about six months so that
    /// paths do not all buy a car in the same month.
    pub replace_every_years: f64,
    /// How old the vehicle is today, in years. A vehicle already older than one
    /// cycle is replaced in the first months of the plan.
    pub age_years: f64,
    /// Share of the replacement price covered by the trade-in or sale of the old
    /// vehicle, from 0 to 1. 0.25 means the household pays 75% of the price.
    pub trade_in_fraction: f64,
    /// Loan terms. Leave empty to pay cash. A financed replacement pays the down
    /// payment out of cash and borrows the rest as an auto debt that is repaid by the
    /// debt payments.
    pub financing: Option<VehicleFinancing>,
    /// If the vehicle belongs to one person (0 or 1) it is no longer replaced after
    /// that person dies. Leave empty for a shared vehicle.
    pub owner: Option<usize>,
    /// Stop replacing this vehicle from this date (for example when driving stops).
    /// Leave empty to keep replacing it for the whole plan.
    pub end: Option<When>,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            price: 0.0,
            replace_every_years: 10.0,
            age_years: 0.0,
            trade_in_fraction: 0.2,
            financing: None,
            owner: None,
            end: None,
        }
    }
}

/// Loan terms for a financed vehicle purchase.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleFinancing {
    /// Fraction of the net price (after trade-in) paid up front (0.2 = 20% down).
    pub down_fraction: f64,
    /// Annual interest rate of the loan as a decimal. The rate is fixed in nominal terms.
    pub rate: f64,
    /// Loan term in months.
    pub term_months: u32,
}

impl Default for VehicleFinancing {
    fn default() -> Self {
        Self {
            down_fraction: 0.1,
            rate: 0.07,
            term_months: 60,
        }
    }
}

/// How a budget line's annual amount is spread over the months of the year.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Seasonality {
    /// Every line is spent in equal monthly amounts.
    #[default]
    Off,
    /// Each category follows a typical US seasonal pattern: holiday-heavy December
    /// for gifts and shopping, summer travel, winter heating and summer cooling in
    /// utilities. Annual totals are unchanged; only their timing moves. It changes
    /// the month a cash shortfall appears, not the year's spending.
    Typical,
}

/// Whether each spending category has its own price level that wanders around CPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategoryInflation {
    /// Every budget line grows with the simulated CPI (plus its real growth).
    #[default]
    Off,
    /// Budget lines in food, utilities, transport, housing (rent), healthcare and
    /// education/childcare categories also carry a persistent random price wedge
    /// against CPI, so the household's own inflation differs from the headline
    /// number for years at a time. The wedge averages zero in expectation (it adds
    /// risk, not a drift), starts at zero today and is fitted to the BLS CPI
    /// sub-indexes. Other categories track CPI.
    Stochastic,
}

/// Serde helper: leave a field at its default out of the serialised profile, so a profile
/// that does not use the feature serialises exactly as it did before the feature existed.
pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// Serde helper: leave a zero out of the serialised profile (see [`is_default`]).
pub(crate) fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
