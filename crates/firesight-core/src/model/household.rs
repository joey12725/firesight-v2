//! Who is in the household and where they live.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{AdultSupport, Arrival, CollegeCost, ParentalLeave, YearMonth};

/// The people the plan covers and where they live.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Household {
    /// The one or two adults whose finances are planned together. Other fields refer
    /// to them by position: 0 is the first person, 1 the second.
    pub people: Vec<Person>,
    /// Children the household supports, including planned children with a birth month
    /// in the future.
    pub children: Vec<Child>,
    /// Where the household lives at the start. Drives state tax, property tax defaults
    /// and health insurance prices.
    pub location: Location,
}

/// One adult in the household.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Person {
    /// Stable identifier used by patches, for example "alex". Must be unique among people.
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Month of birth as "YYYY-MM". Required; every age in the plan is derived from it.
    pub birth: YearMonth,
    /// Sex used to pick the mortality table. "unspecified" uses a blend of both.
    #[serde(default)]
    pub sex: Sex,
    /// Self-assessed health, which shifts life expectancy by a few years either way.
    #[serde(default)]
    pub health: HealthStatus,
    /// For a future partner: the month they join the household. Before this their
    /// income, accounts and spending are not counted. Leave empty for someone already
    /// in the household.
    #[serde(default)]
    pub joins: Option<YearMonth>,
    /// Social Security claiming plan and how the benefit is estimated.
    #[serde(default)]
    pub social_security: SocialSecurity,
    /// Whether a long-term disability can end this person's work before retirement.
    /// Off by default. When on, each month a worker below full retirement age faces
    /// an age-dependent chance of a disability that ends all wage and
    /// self-employment income. It pays Social Security Disability Insurance after a
    /// five-month wait (the person's benefit as of onset, converting to the
    /// retirement benefit at full retirement age) and any `ltd_insurance`, and ends
    /// with a small yearly chance of recovery.
    #[serde(default, skip_serializing_if = "crate::career_realism::is_default")]
    pub disability_risk: DisabilityRisk,
    /// Employer or private long-term disability insurance. Needs `disability_risk`
    /// on. Leave empty for none.
    #[serde(default, skip_serializing_if = "crate::career_realism::is_default")]
    pub ltd_insurance: Option<LtdInsurance>,
    /// A long-term-care insurance policy this person owns, if any. Leave empty for
    /// none (the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ltc_insurance: Option<super::LtcInsurance>,
}

/// Whether a long-term work-limiting disability is simulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisabilityRisk {
    /// No disability: the person works until the plan says otherwise (the default).
    #[default]
    Off,
    /// Disability onset at rates shaped like Social Security's disabled-worker award
    /// incidence (about 4.6 per 1,000 insured a year on average, rising steeply with
    /// age; the age profile is a judgement), a 3% yearly chance of recovery.
    Typical,
}

/// Long-term disability insurance paid after the first three months of a disability.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LtdInsurance {
    /// Share of base salary at the time of onset that the policy replaces before
    /// offsets, 0 to 1. Typical group policies replace 0.6. The benefit is reduced by
    /// the Social Security Disability Insurance the person receives (the usual
    /// offset), and is taxable (employer-paid premiums).
    pub replacement: f64,
    /// Age at which the benefit stops, for example 65. Typical policies pay to 65 or
    /// to full retirement age.
    pub to_age: f64,
}

impl Default for LtdInsurance {
    fn default() -> Self {
        Self {
            replacement: 0.6,
            to_age: 65.0,
        }
    }
}

impl Default for Person {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            birth: YearMonth::ym(1990, 1),
            sex: Sex::default(),
            health: HealthStatus::default(),
            joins: None,
            social_security: SocialSecurity::default(),
            disability_risk: DisabilityRisk::default(),
            ltd_insurance: None,
            ltc_insurance: None,
        }
    }
}

/// Sex for mortality tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    /// Female mortality table (longer life expectancy).
    Female,
    /// Male mortality table.
    Male,
    /// An even blend of the female and male tables.
    #[default]
    Unspecified,
}

/// Self-assessed health; scales mortality rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Markedly healthier than average (mortality 25% below the population table):
    /// roughly 2.5 more years of life expectancy at 65.
    Excellent,
    /// The default: somewhat healthier than the population (mortality 15% below the
    /// table, about 1.5 more years at 65). People who plan for early retirement are
    /// on average better off and longer-lived than the population the life tables
    /// describe, so planning on the unadjusted table understates longevity risk.
    #[default]
    Good,
    /// Population-average mortality, straight from the Social Security life table.
    Average,
    /// Chronic conditions: higher mortality, roughly 4 fewer years of life expectancy.
    Poor,
}

/// When a person claims Social Security and how the benefit is estimated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SocialSecurity {
    /// Age at which the person starts retirement benefits, between 62 and 70. Claiming
    /// later raises the monthly benefit for life. Default 67 (full retirement age for
    /// anyone born in 1960 or later).
    pub claim_age: f64,
    /// How the benefit amount is worked out.
    pub basis: SocialSecurityBasis,
}

impl Default for SocialSecurity {
    fn default() -> Self {
        Self {
            claim_age: 67.0,
            basis: SocialSecurityBasis::default(),
        }
    }
}

/// How a Social Security benefit is estimated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SocialSecurityBasis {
    /// Estimate from earnings: past earnings are reconstructed from today's wage and a
    /// typical career curve, future earnings come from the simulation.
    FromEarnings {
        /// Age at which the person started full-time work and paying into Social
        /// Security. Default 22.
        #[serde(default = "default_career_start_age")]
        career_start_age: f64,
    },
    /// Use the figure from the person's SSA statement.
    KnownBenefit {
        /// Monthly benefit at full retirement age, in today's dollars, as shown on the
        /// statement at ssa.gov. Typical values are 1,500 to 4,000.
        monthly_at_fra: f64,
    },
    /// No Social Security (not eligible, or deliberately excluded from the plan).
    None,
}

fn default_career_start_age() -> f64 {
    22.0
}

impl Default for SocialSecurityBasis {
    fn default() -> Self {
        Self::FromEarnings {
            career_start_age: default_career_start_age(),
        }
    }
}

/// A child the household supports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Child {
    /// Stable identifier used by patches, for example "kid1". Must be unique among children.
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Month of birth as "YYYY-MM". A month in the future means a planned child.
    pub birth: YearMonth,
    /// Whether the engine adds the cost of raising the child, or the budget already
    /// includes it.
    #[serde(default)]
    pub costs: ChildCosts,
    /// Monthly childcare or daycare cost for ages 0 to 4, in today's dollars. Leave
    /// empty to use the modelled figure for the household's location. Only used when
    /// `costs` is "modeled".
    #[serde(default)]
    pub childcare_monthly: Option<f64>,
    /// What kind of college, if any, the plan should pay for from age 18 to 21.
    #[serde(default)]
    pub college: College,
    /// Fraction of college costs the parents pay (the rest is covered by the student,
    /// aid or scholarships). 1.0 means parents pay everything.
    #[serde(default = "one")]
    pub college_share: f64,
    /// For a planned child (birth month in the future): whether the child is certain to
    /// arrive on that date or the birth is uncertain (may not happen, may be later,
    /// may be twins). Default certain.
    #[serde(default, skip_serializing_if = "Arrival::is_certain")]
    pub arrival: Arrival,
    /// Whether the college cost is the plan's fixed figure or uncertain (random tuition
    /// growth, income-dependent aid, whether the child attends, how many years).
    /// Default fixed.
    #[serde(default, skip_serializing_if = "CollegeCost::is_fixed")]
    pub college_cost: CollegeCost,
    /// Opt-in: after school the child may return home or need money for a random spell
    /// (a "boomerang" child), which adds spending for those years. Empty (the default)
    /// means the child is never supported as an adult.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adult_support: Option<AdultSupport>,
    /// Opt-in: when this child is born during the plan (a child already born has no
    /// leave), the named person's wages drop to a fraction of pay for some months.
    /// Retirement deferrals, payroll tax and Social Security earnings follow the pay
    /// actually received. Twins are one leave. State paid-family-leave programmes are
    /// not modelled. Empty (the default) means no leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parental_leave: Option<ParentalLeave>,
    /// Opt-in: a second leave at the same birth, for the other parent (name a different
    /// `person` than `parental_leave` does). It starts in the birth month too, with
    /// its own length and pay fraction. Empty (the default) means no second leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_parental_leave: Option<ParentalLeave>,
}

fn one() -> f64 {
    1.0
}

impl Default for Child {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            birth: YearMonth::ym(2020, 1),
            costs: ChildCosts::default(),
            childcare_monthly: None,
            college: College::default(),
            college_share: 1.0,
            arrival: Arrival::Certain,
            college_cost: CollegeCost::Fixed,
            adult_support: None,
            parental_leave: None,
            second_parental_leave: None,
        }
    }
}

/// How the cost of raising a child enters the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChildCosts {
    /// The engine adds a cost by age and household income tier (USDA "Expenditures on
    /// Children" style), ending at 18. Do not also list child costs in the budget.
    #[default]
    Modeled,
    /// Child costs are already in the budget items; the engine adds nothing except
    /// college.
    InBudget,
}

/// College plan for a child.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum College {
    /// No college costs in the plan.
    #[default]
    None,
    /// Four years at an in-state public university (tuition, fees, room and board).
    PublicInState,
    /// Four years at an out-of-state public university.
    PublicOutOfState,
    /// Four years at a private university.
    Private,
    /// Four years at a cost you specify.
    Custom {
        /// Total cost per year in today's dollars, including room and board.
        annual: f64,
    },
}

/// Where the household lives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Location {
    /// Country as an ISO 3166-1 two-letter code. "US" enables the full US tax model;
    /// anywhere else uses `foreign_tax_rate`.
    pub country: String,
    /// Two-letter US state or district code such as "CO" or "DC". Leave empty outside
    /// the US.
    pub state: Option<String>,
    /// City name, used for cost-of-living and housing defaults. Optional.
    pub city: Option<String>,
    /// Flat effective income tax rate applied when the household lives outside the US,
    /// as a fraction of income (0.25 = 25%). Ignored in the US.
    pub foreign_tax_rate: Option<f64>,
}

impl Default for Location {
    fn default() -> Self {
        Self {
            country: "US".into(),
            state: None,
            city: None,
            foreign_tax_rate: None,
        }
    }
}

impl Location {
    /// True when the US federal and state tax model applies.
    pub fn is_us(&self) -> bool {
        self.country.eq_ignore_ascii_case("US")
    }
}

/// Two-letter codes of the 50 US states and DC, the only values accepted for
/// `Location::state`.
pub const US_STATE_CODES: [&str; 51] = [
    "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "DC", "FL", "GA", "HI", "ID", "IL", "IN", "IA",
    "KS", "KY", "LA", "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ", "NM",
    "NY", "NC", "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VT", "VA", "WA",
    "WV", "WI", "WY",
];
