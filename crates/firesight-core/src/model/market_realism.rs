//! Optional refinements of the market model (`assumptions.market`). Every one is off
//! by default, and with all of them off the market is exactly what it was before
//! they existed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where the block bootstrap takes bond returns from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BondPricing {
    /// Each month's bond return is the one of the resampled history row. Returns
    /// keep history's joint behaviour with stocks and inflation, but a path's bond
    /// returns do not follow from its own yield level. The default.
    #[default]
    Sampled,
    /// Each month's bond return is priced from the path's own 10-year yield: a
    /// month of interest at the yield, plus the price change of a 10-year bond when
    /// the yield moves (duration and convexity). The yield's moves still come from
    /// the resampled history, so stocks, bonds and inflation keep their co-movement.
    /// Only affects the block bootstrap.
    FromYield,
}
