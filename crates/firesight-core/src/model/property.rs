//! Real estate the household owns directly.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Assessment, InsuranceGrowth, MaintenanceModel, MortgageKind, RentalDynamics};

/// A home, rental or vacation property owned today. Future purchases are events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Property {
    /// Stable identifier used by patches and by sale events, for example "home".
    pub id: String,
    /// Display name.
    pub name: String,
    /// How the property is used: lived in, rented out, or a second home.
    #[serde(rename = "use")]
    pub usage: PropertyUse,
    /// Current market value in dollars.
    pub value: f64,
    /// Purchase price plus improvements, used to compute the taxable gain on sale.
    /// Leave empty to assume it equals the current value.
    pub basis: Option<f64>,
    /// The mortgage on the property, if any.
    pub mortgage: Option<Mortgage>,
    /// Annual property tax as a fraction of value (0.011 = 1.1%). Leave empty to use
    /// the average for the household's state.
    pub property_tax_rate: Option<f64>,
    /// Annual homeowner's insurance as a fraction of value. Typical 0.004 to 0.01;
    /// higher in disaster-prone areas.
    pub insurance_rate: f64,
    /// Annual maintenance and repairs as a fraction of value. The usual rule of thumb
    /// is 0.01 to 0.02.
    pub maintenance_rate: f64,
    /// Homeowner association or condo fee per month, in today's dollars.
    pub hoa_monthly: f64,
    /// How much faster (or slower) this property is expected to appreciate than the
    /// national home price index, per year, as a decimal. 0 follows the national market.
    pub appreciation_vs_national: f64,
    /// Annual volatility of this particular property's value around the national
    /// index, as a decimal. Individual homes are far more volatile than the index;
    /// 0.08 is typical.
    pub idiosyncratic_volatility: f64,
    /// Whether maintenance is a smooth charge or arrives as random major repairs
    /// with the same expected cost.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub maintenance: MaintenanceModel,
    /// How the property-tax assessed value follows the market value.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub assessment: Assessment,
    /// Whether the insurance premium drifts above inflation with occasional jumps.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub insurance_real_growth: InsuranceGrowth,
}

impl Default for Property {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            usage: PropertyUse::Primary,
            value: 0.0,
            basis: None,
            mortgage: None,
            property_tax_rate: None,
            insurance_rate: Self::DEFAULT_INSURANCE_RATE,
            maintenance_rate: Self::DEFAULT_MAINTENANCE_RATE,
            hoa_monthly: 0.0,
            appreciation_vs_national: 0.0,
            idiosyncratic_volatility: Self::DEFAULT_IDIOSYNCRATIC_VOLATILITY,
            maintenance: MaintenanceModel::default(),
            assessment: Assessment::default(),
            insurance_real_growth: InsuranceGrowth::default(),
        }
    }
}

impl Property {
    /// Default annual insurance cost as a fraction of value.
    pub const DEFAULT_INSURANCE_RATE: f64 = 0.005;
    /// Default annual maintenance cost as a fraction of value.
    pub const DEFAULT_MAINTENANCE_RATE: f64 = 0.01;
    /// Default idiosyncratic volatility of a single property.
    pub const DEFAULT_IDIOSYNCRATIC_VOLATILITY: f64 = 0.08;
}

/// How a property is used.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyUse {
    /// The household's main home. Up to $250,000 of gain per owner is tax-free on sale.
    #[default]
    Primary,
    /// Rented to tenants. Rent, expenses and depreciation flow through the tax return.
    Rental(RentalTerms),
    /// A second home that earns no rent.
    Vacation,
}

/// The economics of a rental property.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RentalTerms {
    /// Total rent per month across all units when fully let, in today's dollars.
    pub monthly_rent: f64,
    /// Fraction of the year the property stands empty (0.05 = about 18 days).
    pub vacancy: f64,
    /// Property manager's fee as a fraction of collected rent (0.08 to 0.10 is typical;
    /// 0 if self-managed).
    pub management_fee: f64,
    /// Expected annual rent growth above inflation, as a decimal.
    pub rent_real_growth: f64,
    /// Share of the property's cost basis attributed to land, which cannot be
    /// depreciated. Typical 0.2 to 0.3.
    pub land_fraction: f64,
    /// Years the property has already been a rental, which decides how much
    /// depreciation is left (residential rentals depreciate over 27.5 years).
    pub years_in_service: f64,
    /// How tenants and rents behave over time: "simple" (default: short random
    /// vacancies at the entered rate, rent on its trend) or "realistic" (tenants move
    /// out at random and the unit takes months to re-let, longer in recessions;
    /// rents follow a market index; occasional evictions and large repairs; the same
    /// long-run vacancy).
    #[serde(skip_serializing_if = "RentalDynamics::is_simple")]
    pub dynamics: RentalDynamics,
}

impl Default for RentalTerms {
    fn default() -> Self {
        Self {
            monthly_rent: 0.0,
            vacancy: 0.05,
            management_fee: 0.0,
            rent_real_growth: 0.0,
            land_fraction: 0.2,
            years_in_service: 0.0,
            dynamics: RentalDynamics::Simple,
        }
    }
}

/// A mortgage, fixed-rate unless `kind` says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Mortgage {
    /// Principal still owed, in dollars.
    pub balance: f64,
    /// Annual interest rate as a decimal (0.065 = 6.5%).
    pub rate: f64,
    /// Months of payments left (360 for a new 30-year loan).
    pub remaining_months: u32,
    /// Monthly principal-and-interest payment in dollars, excluding tax and insurance.
    /// Leave empty to compute it from the balance, rate and remaining term.
    pub payment: Option<f64>,
    /// Fixed rate (the default) or adjustable rate.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub kind: MortgageKind,
}

impl Default for Mortgage {
    fn default() -> Self {
        Self {
            balance: 0.0,
            rate: 0.065,
            remaining_months: 360,
            payment: None,
            kind: MortgageKind::Fixed,
        }
    }
}
