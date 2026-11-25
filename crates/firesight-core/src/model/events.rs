//! Planned and possible life events.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    Assessment, Category, DivorceTerms, InsuranceGrowth, MaintenanceModel, MortgageKind, Property,
    PropertyUse, When,
};

/// Something that happens once at a point in the plan: a purchase, a move, a windfall,
/// a change in income or spending.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LifeEvent {
    /// Stable identifier used by patches, for example "buy_home".
    pub id: String,
    /// Display name, shown in timelines and in the event log of a traced path.
    pub name: String,
    /// When the event happens.
    pub when: When,
    /// Probability that it happens at all, from 0 to 1. 1.0 for a firm plan; 0.5 for
    /// "maybe we inherit something".
    pub probability: f64,
    /// Uncertainty about the timing: the actual date is drawn within plus or minus
    /// this many years of `when`. 0 for an exact date.
    pub timing_uncertainty_years: f64,
    /// What happens.
    pub kind: EventKind,
}

impl Default for LifeEvent {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            when: When::Start,
            probability: 1.0,
            timing_uncertainty_years: 0.0,
            kind: EventKind::default(),
        }
    }
}

/// What a life event does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    /// Buy a property with a mortgage.
    BuyHome(BuyHome),
    /// Sell a property the household owns.
    SellProperty(SellProperty),
    /// Move to another state or country.
    Relocate(Relocate),
    /// Receive a one-off sum: inheritance, gift, sale of a business.
    Windfall {
        /// Amount received in today's dollars.
        amount: f64,
        /// Whether it is taxed as ordinary income. Inheritances and gifts are not.
        #[serde(default)]
        taxable: bool,
    },
    /// A one-off cost: a wedding, a car, a renovation.
    Expense {
        /// Total cost in today's dollars.
        amount: f64,
        /// Loan terms if part of the cost is borrowed. Leave empty to pay in cash.
        #[serde(default)]
        financed: Option<Financing>,
    },
    /// Change an income source: a promotion, a pay cut, a job ending.
    IncomeChange {
        /// The `id` of the income affected.
        income: String,
        /// What changes.
        change: IncomeChangeKind,
    },
    /// Time out of work: parental leave, a sabbatical, caring for a relative.
    CareerBreak {
        /// Who takes the break: 0 for the first person, 1 for the second.
        #[serde(default)]
        person: usize,
        /// Length of the break in months.
        months: u32,
        /// Fraction of normal pay received during the break (0 unpaid, 0.6 for
        /// typical paid leave).
        #[serde(default)]
        income_fraction: f64,
    },
    /// Permanently change part of the budget.
    SpendingChange {
        /// Which spending is affected.
        target: SpendingTarget,
        /// What changes.
        change: SpendingChangeKind,
    },
    /// Change how much goes into an employer retirement plan.
    ContributionChange {
        /// The `id` of the job income whose plan is affected.
        income: String,
        /// New share of salary contributed (0.15 = 15%).
        employee_fraction: f64,
    },
    /// The couple divorces: the second person leaves the household with their share
    /// of the assets and their own income, and the plan continues for the first.
    Divorce(DivorceTerms),
}

impl Default for EventKind {
    fn default() -> Self {
        EventKind::Expense {
            amount: 0.0,
            financed: None,
        }
    }
}

/// Buying a property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BuyHome {
    /// The `id` the new property will have, so that later events can refer to it.
    /// Must not clash with an existing property.
    pub property_id: String,
    /// Purchase price in today's dollars. The engine scales it by simulated home-price
    /// growth between now and the purchase.
    pub price: f64,
    /// Down payment as a fraction of the price (0.2 = 20%). Below 20% the loan carries
    /// mortgage insurance until enough equity builds up.
    pub down_payment_fraction: f64,
    /// Mortgage interest rate as a decimal. Leave empty to use the simulated market
    /// rate at the time of purchase, which is the realistic choice for a future purchase.
    pub rate: Option<f64>,
    /// Mortgage term in years (30 or 15).
    pub term_years: u32,
    /// Whether the purchase replaces the household's rent: budget items in the "rent"
    /// category stop at the purchase date.
    pub replaces_rent: bool,
    /// Closing costs paid by the buyer as a fraction of the price. Typical 0.02 to 0.04.
    pub closing_cost_fraction: f64,
    /// How the property will be used.
    #[serde(rename = "use")]
    pub usage: PropertyUse,
    /// Annual property tax as a fraction of value. Leave empty to use the state average.
    pub property_tax_rate: Option<f64>,
    /// Annual homeowner's insurance as a fraction of value.
    pub insurance_rate: f64,
    /// Annual maintenance as a fraction of value.
    pub maintenance_rate: f64,
    /// Homeowner association fee per month, in today's dollars.
    pub hoa_monthly: f64,
    /// Expected appreciation relative to the national index, per year, as a decimal.
    pub appreciation_vs_national: f64,
    /// Annual volatility of this property's value around the national index.
    pub idiosyncratic_volatility: f64,
    /// Whether maintenance is a smooth charge or arrives as random major repairs
    /// with the same expected cost.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub maintenance: MaintenanceModel,
    /// How the property-tax assessed value follows the market value. The assessed
    /// value starts at the purchase price.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub assessment: Assessment,
    /// Whether the insurance premium drifts above inflation with occasional jumps.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub insurance_real_growth: InsuranceGrowth,
    /// Fixed-rate (the default) or adjustable-rate mortgage. For an adjustable rate
    /// without `rate`, the starting rate is the simulated market rate less 0.75 points.
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub mortgage_kind: MortgageKind,
}

impl Default for BuyHome {
    fn default() -> Self {
        Self {
            property_id: "home".into(),
            price: 0.0,
            down_payment_fraction: 0.2,
            rate: None,
            term_years: 30,
            replaces_rent: true,
            closing_cost_fraction: 0.03,
            usage: PropertyUse::Primary,
            property_tax_rate: None,
            insurance_rate: Property::DEFAULT_INSURANCE_RATE,
            maintenance_rate: Property::DEFAULT_MAINTENANCE_RATE,
            hoa_monthly: 0.0,
            appreciation_vs_national: 0.0,
            idiosyncratic_volatility: Property::DEFAULT_IDIOSYNCRATIC_VOLATILITY,
            maintenance: MaintenanceModel::default(),
            assessment: Assessment::default(),
            insurance_real_growth: InsuranceGrowth::default(),
            mortgage_kind: MortgageKind::default(),
        }
    }
}

/// Selling a property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SellProperty {
    /// The `id` of the property to sell: an existing property or the `property_id` of
    /// an earlier purchase event.
    pub property: String,
    /// Agent commission and other selling costs as a fraction of the sale price.
    /// Typical 0.05 to 0.07.
    pub selling_cost_fraction: f64,
    /// If the household rents afterwards, the monthly rent in today's dollars. Leave
    /// empty if it moves into another property it owns.
    pub then_rent_monthly: Option<f64>,
}

impl Default for SellProperty {
    fn default() -> Self {
        Self {
            property: String::new(),
            selling_cost_fraction: 0.06,
            then_rent_monthly: None,
        }
    }
}

/// Moving somewhere else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Relocate {
    /// Destination country as an ISO two-letter code ("US", "PT", "MX").
    pub country: String,
    /// Destination US state as a two-letter code. Leave empty when moving abroad.
    pub state: Option<String>,
    /// Destination city. Optional.
    pub city: Option<String>,
    /// Flat effective income tax rate at the destination when it is outside the US, as
    /// a decimal. Leave empty for a US destination.
    pub foreign_tax_rate: Option<f64>,
    /// One-off cost of the move in today's dollars.
    pub moving_cost: f64,
    /// How the cost of living changes with the move.
    pub cost_of_living: CostOfLivingChange,
}

impl Default for Relocate {
    fn default() -> Self {
        Self {
            country: "US".into(),
            state: None,
            city: None,
            foreign_tax_rate: None,
            moving_cost: 0.0,
            cost_of_living: CostOfLivingChange::Unchanged,
        }
    }
}

/// How a move changes the budget.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CostOfLivingChange {
    /// Spending stays the same.
    #[default]
    Unchanged,
    /// Every budget item is multiplied by one factor.
    Factor {
        /// Multiplier on all spending (0.85 = 15% cheaper).
        overall: f64,
    },
    /// Different multipliers for different categories.
    ByCategory {
        /// Multiplier per spending category; categories not listed are unchanged.
        factors: BTreeMap<Category, f64>,
    },
}

/// Loan terms for a financed expense.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Financing {
    /// Fraction of the cost paid up front (0.2 = 20% down).
    pub down_fraction: f64,
    /// Annual interest rate of the loan as a decimal.
    pub rate: f64,
    /// Loan term in months.
    pub term_months: u32,
}

impl Default for Financing {
    fn default() -> Self {
        Self {
            down_fraction: 0.2,
            rate: 0.07,
            term_months: 60,
        }
    }
}

/// A change to an income source.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IncomeChangeKind {
    /// Set the annual amount (salary, net profit or annual payment) to a new value.
    SetSalary {
        /// New annual amount in today's dollars.
        amount: f64,
    },
    /// Multiply the annual amount.
    Scale {
        /// Multiplier (1.15 = a 15% raise, 0.5 = going half-time).
        factor: f64,
    },
    /// The income stops.
    End,
}

/// Which spending an event changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpendingTarget {
    /// One budget item.
    Item {
        /// The `id` of the budget item.
        id: String,
    },
    /// Every item in a category.
    Category {
        /// The spending category.
        category: Category,
    },
    /// The whole budget.
    All,
}

/// A change to spending.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpendingChangeKind {
    /// Multiply the amount.
    Scale {
        /// Multiplier (0.9 = cut by 10%).
        factor: f64,
    },
    /// Replace the amount. For a category or the whole budget, items are scaled so
    /// that their total equals this value.
    Set {
        /// New amount per year in today's dollars.
        annual: f64,
    },
}
