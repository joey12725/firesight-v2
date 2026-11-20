//! What the household spends.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{RentGrowth, When};

/// The household budget and the rules that shape spending over time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Spending {
    /// Budget lines. Do not include mortgage payments, property tax or debt payments
    /// for properties and debts listed elsewhere in the profile; those are computed.
    pub items: Vec<BudgetItem>,
    /// Share of each real raise that turns into higher spending, from 0 to 1. With 0.3,
    /// a 10% real pay rise lifts non-essential spending by 3%. It only acts while
    /// income is growing; it is not a perpetual growth rate.
    pub lifestyle_creep: f64,
    /// Apply the observed "retirement spending smile": real spending drifts down by
    /// about 1% a year through the 70s and levels off, while healthcare is modelled
    /// separately and rises.
    pub retirement_smile: bool,
    /// Irregular unplanned expenses (car repairs, a new roof, helping family) drawn at
    /// random. "typical" averages roughly 3% of annual spending.
    pub shocks: ShockLevel,
    /// How health insurance and medical costs are handled.
    pub healthcare: Healthcare,
    /// After one partner dies, household spending is multiplied by this factor.
    /// 0.7 means a survivor spends 70% of what the couple did.
    pub survivor_factor: f64,
}

impl Default for Spending {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            lifestyle_creep: 0.25,
            retirement_smile: true,
            shocks: ShockLevel::Typical,
            healthcare: Healthcare::default(),
            survivor_factor: 0.7,
        }
    }
}

/// Size of random unplanned expenses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShockLevel {
    /// No unplanned expenses.
    Off,
    /// About one surprise a year, averaging around 3% of annual spending in total.
    #[default]
    Typical,
    /// Larger and more frequent surprises, averaging around 6% of annual spending.
    High,
}

/// Health insurance and medical costs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Healthcare {
    /// "modeled": the engine adds premiums and out-of-pocket costs for each phase of
    /// life (employer plan, early retirement, Medicare). "in_budget": the budget items
    /// already cover healthcare and the engine adds nothing.
    pub mode: HealthcareMode,
    /// Where insurance comes from between retiring and Medicare at 65.
    pub early_retirement: EarlyRetirementCoverage,
    /// Include the risk of needing long-term care late in life (about half of people
    /// need some; a small share need several years costing over $100,000 a year).
    pub long_term_care: bool,
    /// Plan generosity, which trades premiums against out-of-pocket costs.
    pub quality: PlanTier,
}

impl Default for Healthcare {
    fn default() -> Self {
        Self {
            mode: HealthcareMode::Modeled,
            early_retirement: EarlyRetirementCoverage::Aca,
            long_term_care: true,
            quality: PlanTier::Silver,
        }
    }
}

/// Whether healthcare is modelled or already in the budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthcareMode {
    /// The engine computes premiums and out-of-pocket costs. Do not also budget for them.
    #[default]
    Modeled,
    /// Healthcare is covered by budget items in the "healthcare" category.
    InBudget,
}

/// Health coverage between retirement and Medicare eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EarlyRetirementCoverage {
    /// An ACA marketplace plan, with premium tax credits that depend on income.
    #[default]
    Aca,
    /// COBRA continuation of the employer plan for 18 months at full cost, then ACA.
    Cobra,
    /// A fixed cost you specify (for example a retiree plan or coverage abroad).
    Custom {
        /// Total premiums and out-of-pocket costs per person per year, in today's dollars.
        annual: f64,
    },
}

/// Health plan metal tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    /// Lowest premiums, highest deductibles.
    Bronze,
    /// The benchmark tier.
    #[default]
    Silver,
    /// Higher premiums, lower out-of-pocket costs.
    Gold,
}

/// One line of the budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BudgetItem {
    /// Stable identifier used by patches and events, for example "groceries".
    pub id: String,
    /// Display name.
    pub name: String,
    /// Spending category. It sets the default growth rate and how relocation and
    /// cost-of-living changes apply.
    pub category: Category,
    /// Amount per period (see `frequency`) in today's dollars.
    pub amount: f64,
    /// How often the amount is spent.
    pub frequency: Frequency,
    /// Essential spending is protected when a spending rule or a shortfall forces cuts;
    /// non-essential spending is cut first.
    pub essential: bool,
    /// Annual growth above general inflation, as a decimal. Leave empty to use the
    /// category default (healthcare 0.015, education 0.01, childcare 0.01, rent 0.005,
    /// everything else 0).
    pub real_growth: Option<f64>,
    /// When this spending begins. Leave empty if it is already happening.
    pub start: Option<When>,
    /// When this spending stops. Leave empty if it never stops.
    pub end: Option<When>,
    /// What happens to this line when the household retires.
    pub in_retirement: RetirementChange,
    /// If the line belongs to one person (0 or 1) it stops when that person dies and
    /// does not start before they join the household. Leave empty for shared spending.
    pub owner: Option<usize>,
    /// "fixed" (default): inflation plus `real_growth`. "market": a rent line follows
    /// the path's simulated local rent market instead (rent category only).
    #[serde(skip_serializing_if = "super::housing_realism::is_default")]
    pub rent_growth: RentGrowth,
}

impl Default for BudgetItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            category: Category::Other,
            amount: 0.0,
            frequency: Frequency::Monthly,
            essential: false,
            real_growth: None,
            start: None,
            end: None,
            in_retirement: RetirementChange::Same,
            owner: None,
            rent_growth: RentGrowth::Fixed,
        }
    }
}

impl BudgetItem {
    /// Amount per year in today's dollars.
    pub fn annual_amount(&self) -> f64 {
        self.amount * self.frequency.per_year()
    }

    /// Amount per year in today's dollars once retired.
    pub fn annual_amount_in_retirement(&self) -> f64 {
        match self.in_retirement {
            RetirementChange::Same => self.annual_amount(),
            RetirementChange::Stops => 0.0,
            RetirementChange::Factor { factor } => self.annual_amount() * factor,
            RetirementChange::Amount { amount } => amount * self.frequency.per_year(),
        }
    }

    /// The real growth rate in force: the explicit value or the category default.
    pub fn effective_real_growth(&self) -> f64 {
        self.real_growth
            .unwrap_or_else(|| self.category.default_real_growth())
    }
}

/// How often a budget amount recurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    /// Every week (52 times a year).
    Weekly,
    /// Every two weeks (26 times a year).
    Biweekly,
    /// Every month.
    #[default]
    Monthly,
    /// Every three months.
    Quarterly,
    /// Once a year.
    Annual,
}

impl Frequency {
    /// Number of periods per year.
    pub const fn per_year(self) -> f64 {
        match self {
            Frequency::Weekly => 52.0,
            Frequency::Biweekly => 26.0,
            Frequency::Monthly => 12.0,
            Frequency::Quarterly => 4.0,
            Frequency::Annual => 1.0,
        }
    }
}

/// What a budget line does at retirement.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetirementChange {
    /// No change.
    #[default]
    Same,
    /// The line ends (commuting, work clothes).
    Stops,
    /// The line is multiplied by a factor.
    Factor {
        /// Multiplier applied from retirement on (0.5 halves it, 1.5 raises it by half).
        factor: f64,
    },
    /// The line is replaced by a new amount.
    Amount {
        /// New amount per period in today's dollars, in the item's own `frequency`.
        amount: f64,
    },
}

/// Spending category.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Rent and other housing costs not tied to a property in the profile.
    Rent,
    /// Electricity, gas, water, internet, phone.
    Utilities,
    /// Food at home.
    Groceries,
    /// Restaurants and takeaway.
    Dining,
    /// Cars, fuel, transit, rideshare.
    Transport,
    /// Health insurance and medical costs (only when healthcare mode is "in_budget").
    Healthcare,
    /// Insurance other than health and homeowner's: car, life, umbrella.
    Insurance,
    /// Daycare, nannies, after-school care.
    Childcare,
    /// Tuition, courses, school costs.
    Education,
    /// Holidays and trips.
    Travel,
    /// Streaming, hobbies, events.
    Entertainment,
    /// Clothing, grooming, gym, personal care.
    Personal,
    /// Charitable giving and gifts.
    Giving,
    /// Anything else.
    #[default]
    Other,
}

impl Category {
    /// Number of categories.
    pub const COUNT: usize = 14;

    /// All categories in declaration order; `ALL[c.index()] == c`.
    pub const ALL: [Category; Category::COUNT] = [
        Category::Rent,
        Category::Utilities,
        Category::Groceries,
        Category::Dining,
        Category::Transport,
        Category::Healthcare,
        Category::Insurance,
        Category::Childcare,
        Category::Education,
        Category::Travel,
        Category::Entertainment,
        Category::Personal,
        Category::Giving,
        Category::Other,
    ];

    /// Dense index for per-category arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Default annual growth above CPI. Healthcare, education and childcare have
    /// outpaced general inflation for decades; rents slightly.
    pub const fn default_real_growth(self) -> f64 {
        match self {
            Category::Healthcare => 0.015,
            Category::Education | Category::Childcare => 0.01,
            Category::Rent => 0.005,
            _ => 0.0,
        }
    }

    /// Whether a new item in this category is essential unless the user says otherwise.
    pub const fn default_essential(self) -> bool {
        matches!(
            self,
            Category::Rent
                | Category::Utilities
                | Category::Groceries
                | Category::Transport
                | Category::Healthcare
                | Category::Insurance
                | Category::Childcare
        )
    }
}
