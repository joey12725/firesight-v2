//! Opt-in market options that widen what the scenario generator can produce: how
//! bootstrap blocks are joined, and rare disasters. (The per-path uncertainty about
//! long-run averages is two fields on `MarketModel` itself.)

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Uncertain;

/// How the bootstrap chooses the month a new block of history starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapJoins {
    /// Any month of the record, all equally likely. A deep recession can be followed
    /// at once by a boom from another era. The default.
    #[default]
    Uniform,
    /// A month that followed conditions like the present ones: the same recession
    /// state, and similar inflation and stock returns over the year before. Recessions
    /// and inflation episodes then run on across the joins instead of being cut short.
    /// Long-run average returns are the same as with `uniform`.
    Matched,
}

/// Rare market disasters beyond anything in the US record: long episodes in which
/// stocks lose a large part of their value for good, as happened to several other
/// countries in the last century. Off unless `annual_probability` is above zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Disasters {
    /// Chance that a disaster begins in any one year, as a decimal. 0 switches
    /// disasters off; 0.01 (one a century on average) is a typical setting, and at
    /// that level about a quarter of 30-year retirements meet one. This is extra
    /// risk on top of the market model, so it lowers average stock returns. At most
    /// three disasters happen in one simulated life. Default 0.
    pub annual_probability: f64,
    /// How much of their value stocks lose by the end of a disaster, after
    /// inflation, compared with where they would otherwise have been, as a decimal
    /// (0.4 = 40% lost). The loss is not made up afterwards. Default: most likely
    /// 0.4, between 0.2 and 0.75.
    pub depth: Uncertain,
    /// How long a disaster lasts, in years. Most of the loss comes early and the
    /// rest tails off. Default: most likely 8 years, between 3 and 20.
    pub years: Uncertain,
    /// Chance that a disaster also brings a burst of inflation, as a decimal.
    /// Default 0.3.
    pub inflation_probability: f64,
    /// Extra inflation per year for as long as the disaster lasts, when it brings an
    /// inflation burst, as a decimal (0.05 = five points a year on top of normal
    /// inflation). Bonds are not compensated for it and cash only half. Default:
    /// most likely 0.05, between 0.02 and 0.12.
    pub inflation: Uncertain,
}

impl Default for Disasters {
    fn default() -> Self {
        Self {
            annual_probability: 0.0,
            depth: Uncertain::Pert {
                low: 0.2,
                mode: 0.4,
                high: 0.75,
            },
            years: Uncertain::Pert {
                low: 3.0,
                mode: 8.0,
                high: 20.0,
            },
            inflation_probability: 0.3,
            inflation: Uncertain::Pert {
                low: 0.02,
                mode: 0.05,
                high: 0.12,
            },
        }
    }
}
