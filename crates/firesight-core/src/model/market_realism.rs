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

/// Default of [`Valuation::starting_cape`]: the S&P 500's cyclically adjusted
/// price-earnings ratio (Shiller CAPE) read between 38 and 42 through 2026 (YCharts
/// 39.8 in February, Financer 38.2 in March, multpl.com about 40.5); rounded to 40.
/// Update it with the published value.
pub const DEFAULT_STARTING_CAPE: f64 = 40.0;
/// Default of [`Valuation::fair_cape`]: the mean of Shiller's CAPE since 1881
/// (about 17.3, Shiller's online data as summarised by multpl.com and Financer).
/// Reverting all the way to it is the bearish reading; many analysts argue that
/// accounting changes and lower rates justify a higher normal level.
pub const DEFAULT_FAIR_CAPE: f64 = 17.3;
/// Default of [`Valuation::half_life_years`]. Judgement: Campbell and Shiller (1998,
/// 2001) find that valuation ratios forecast stock price changes over ten-year
/// horizons and revert mostly through prices rather than earnings, but the speed is
/// not pinned down by their results; 15 years is the middle of a 10 to 20 year
/// range, not an estimate.
pub const DEFAULT_VALUATION_HALF_LIFE_YEARS: f64 = 15.0;

/// Start expected stock returns from today's valuation (Campbell and Shiller). The
/// cyclically adjusted price-earnings ratio (CAPE) of US stocks drifts back toward
/// a normal level; while it is above that level stocks return less than their
/// long-run average, and more while it is below. Price moves on the path feed
/// back: after a crash valuations are lower and expected returns higher. The
/// long-run average return is kept; the level lost while an expensive market
/// returns to normal is lost for good.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Valuation {
    /// Today's CAPE of US stocks (S&P 500). Default 40, about its level in 2026.
    pub starting_cape: f64,
    /// The level the CAPE drifts back to. Default 17.3, its average since 1881;
    /// a higher value (25 to 30) assumes part of today's valuation is permanent.
    pub fair_cape: f64,
    /// Years for half of the gap between the CAPE and its normal level to close
    /// (when prices do nothing unusual). Default 15, a judgement; 10 to 20 is a
    /// reasonable range.
    pub half_life_years: f64,
}

impl Default for Valuation {
    fn default() -> Self {
        Self {
            starting_cape: DEFAULT_STARTING_CAPE,
            fair_cape: DEFAULT_FAIR_CAPE,
            half_life_years: DEFAULT_VALUATION_HALF_LIFE_YEARS,
        }
    }
}
