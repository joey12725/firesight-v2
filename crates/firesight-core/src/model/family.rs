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
