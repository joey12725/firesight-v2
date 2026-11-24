//! Opt-in family uncertainty: whether and when a planned child arrives, what college
//! really costs, whether an adult child needs support, and parental leave. Each type
//! here is a field of [`super::Child`] and defaults to "off".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Default chance that a planned child arrives (a judgement near the 78 to 92% of
/// couples who conceive within a year in time-to-pregnancy studies, lower for the
/// couples who postpone).
pub const DEFAULT_ARRIVAL_PROBABILITY: f64 = 0.85;
/// Default mean months of delay beyond the planned birth month (a judgement: monthly
/// fecundability of 0.15 to 0.25 means 4 to 6 months to conception among those who
/// do conceive).
pub const DEFAULT_ARRIVAL_DELAY_MONTHS: f64 = 4.0;
/// Default chance a birth is twins: 30.1 per 1,000 births in 2024 (CDC NCHS, "A
/// Decade of Decline in Twin Childbearing in the United States, 2014-2024").
pub const DEFAULT_TWINS_PROBABILITY: f64 = 0.03;

/// Whether a planned child is certain to arrive on the planned date.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Arrival {
    /// The child is born in the stated month (the default, and the only choice for a
    /// child already born).
    #[default]
    Certain,
    /// The birth is uncertain: for a child whose birth month is in the future, the
    /// child may not arrive, may arrive later than planned, and a birth may be twins.
    /// Costs, the child tax credit, dependants, parental leave and college all follow
    /// the births that actually happen on each path. Ignored (with a warning) for a
    /// child already born.
    Uncertain {
        /// Chance that the child arrives at all, between 0 and 1. Default 0.85.
        #[serde(default = "default_probability")]
        probability: f64,
        /// Average months by which the birth comes after the stated month (the delay
        /// is random, exponential; 0 means the child arrives exactly on time when
        /// they arrive). Default 4.
        #[serde(default = "default_delay")]
        delay_mean_months: f64,
        /// Chance that the birth is twins (the child's costs and college are then
        /// incurred twice), between 0 and 1. Default 0.03, the US twin birth rate.
        #[serde(default = "default_twins")]
        twins_probability: f64,
    },
}

fn default_probability() -> f64 {
    DEFAULT_ARRIVAL_PROBABILITY
}
fn default_delay() -> f64 {
    DEFAULT_ARRIVAL_DELAY_MONTHS
}
fn default_twins() -> f64 {
    DEFAULT_TWINS_PROBABILITY
}

/// Default chance that a child with a college plan attends at all (a judgement; the
/// plan itself says the family expects college).
pub const DEFAULT_ATTEND_PROBABILITY: f64 = 0.95;
/// Default mean real growth of college prices above CPI, a year: the engine's
/// deterministic default (`children.college.inflation_over_cpi.default_pct` in
/// `params.json`, 1.0), between +3% a year from the 1980s to the mid-2010s and about
/// 0 over the last decade (College Board, Trends in College Pricing 2025).
pub const DEFAULT_REAL_TUITION_GROWTH: f64 = 0.01;
/// Default standard deviation of the persistent part of real tuition growth, a year
/// (a judgement chosen so decade averages range over about plus or minus 1.2 points,
/// which spans the recent decade and the long-run record).
pub const DEFAULT_TUITION_VOLATILITY: f64 = 0.012;

/// How the cost of a child's college is modelled.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollegeCost {
    /// The cost is the plan's cost for four years, growing at CPI plus one point
    /// (the default).
    #[default]
    Fixed,
    /// The cost of the college plan is uncertain: real tuition growth above CPI is
    /// random and persistent (it comes in runs of fast and slow years), the aid the
    /// family receives is a random discount on the published price that shrinks as
    /// household income (its income two years earlier, plus 12% of its cash and taxable
    /// investments, as the federal aid formula counts assets) rises, the child may not attend,
    /// and the child takes 4, 5 or 6 years. Needs a `college` plan. The cost the
    /// parents pay is the published price times the tuition index times one minus the
    /// aid discount, times `college_share`, for each year enrolled.
    Stochastic {
        /// Chance the child attends college at all, between 0 and 1. Default 0.95.
        #[serde(default = "default_attend")]
        attend_probability: f64,
        /// Average yearly growth of the price above CPI, as a fraction. Default 0.01.
        #[serde(default = "default_growth")]
        real_tuition_growth: f64,
        /// Standard deviation of the persistent yearly deviation of that growth,
        /// as a fraction. Default 0.012.
        #[serde(default = "default_volatility")]
        tuition_volatility: f64,
        /// Scale on the aid discount: 1 uses the schedule fitted to net prices from
        /// NPSAS 2019-20 as reported by Brookings (about 57% off the published price
        /// below $50,000 of household income, 31% at $125,000, 21% from $200,000,
        /// the average of public and private colleges; cash and taxable investments
        /// count as income at 12% a year), 0 means no aid, and 0.5 half
        /// of the schedule. Use 0 when the college cost entered is already a net
        /// price, or when `college_share` already allows for aid. Default 1.
        #[serde(default = "one")]
        aid_scale: f64,
    },
}

fn default_attend() -> f64 {
    DEFAULT_ATTEND_PROBABILITY
}
fn default_growth() -> f64 {
    DEFAULT_REAL_TUITION_GROWTH
}
fn default_volatility() -> f64 {
    DEFAULT_TUITION_VOLATILITY
}
fn one() -> f64 {
    1.0
}

/// Default chance that an adult child needs support for a spell (a judgement: 18% of
/// 25 to 34 year olds lived in a parent's home in 2023 (Pew Research Center, April 2025,
/// from Census data), and a larger share live there at some point).
pub const DEFAULT_SUPPORT_PROBABILITY: f64 = 0.3;
/// Default yearly support in today's dollars (a judgement: the extra food, utilities,
/// phone and insurance of an adult at home, or an allowance of $1,000 a month).
pub const DEFAULT_SUPPORT_ANNUAL: f64 = 12_000.0;
/// Default mean length of a spell in years (a judgement).
pub const DEFAULT_SUPPORT_YEARS: f64 = 2.0;

/// Support for an adult child who returns home or needs money for a spell.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AdultSupport {
    /// Chance that the child needs support at some point after finishing school, between
    /// 0 and 1. Default 0.3.
    pub probability: f64,
    /// What the household spends on the child each year of the spell, in today's dollars.
    /// Default 12,000.
    pub annual: f64,
    /// Average length of a spell in years (the length is random, exponential, at least
    /// one month). The spell starts at a random month in the six years after the child
    /// finishes school. Default 2.
    pub mean_years: f64,
}

impl Default for AdultSupport {
    fn default() -> Self {
        Self {
            probability: DEFAULT_SUPPORT_PROBABILITY,
            annual: DEFAULT_SUPPORT_ANNUAL,
            mean_years: DEFAULT_SUPPORT_YEARS,
        }
    }
}

/// Default months of leave (a judgement: the FMLA entitles eligible workers to up to
/// 12 weeks of unpaid, job-protected leave for a birth (US Department of Labor), about
/// three months).
pub const DEFAULT_LEAVE_MONTHS: u32 = 3;
/// Default share of normal pay received during the leave (a judgement: employer paid
/// leave and short-term disability typically replace 60% to 100% of pay for part of
/// the time; unpaid FMLA leave is 0).
pub const DEFAULT_LEAVE_PAY_FRACTION: f64 = 0.6;

/// Time off work around the birth of a child.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ParentalLeave {
    /// Who takes the leave: 0 for the first person, 1 for the second.
    pub person: usize,
    /// Length of the leave in months, starting in the month the child is born.
    /// Default 3.
    pub months: u32,
    /// Share of normal pay received during the leave, between 0 (unpaid) and 1 (full
    /// pay). Default 0.6. State paid-family-leave programmes are not modelled: enter
    /// their effect here.
    pub pay_fraction: f64,
}

impl Default for ParentalLeave {
    fn default() -> Self {
        Self {
            person: 0,
            months: DEFAULT_LEAVE_MONTHS,
            pay_fraction: DEFAULT_LEAVE_PAY_FRACTION,
        }
    }
}

impl Arrival {
    /// True for the default, so profiles that do not use uncertain births serialise
    /// exactly as they did before the field existed.
    pub fn is_certain(&self) -> bool {
        matches!(self, Arrival::Certain)
    }
}

impl CollegeCost {
    /// True for the default, so profiles that do not use stochastic college costs
    /// serialise exactly as they did before the field existed.
    pub fn is_fixed(&self) -> bool {
        matches!(self, CollegeCost::Fixed)
    }
}
