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

impl BondPricing {
    /// Whether this is the default; the field is then left out of saved profiles,
    /// which stay as they were before the field existed.
    pub fn is_sampled(&self) -> bool {
        *self == BondPricing::Sampled
    }
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

/// A historical market episode a [`Stress::HistoricalEpisode`](super::Stress)
/// replays. Each runs from the first month after the US stock market's peak to the
/// month its real value regained the peak, except the 1966-82 stagflation, which
/// has no such clean end in the total-market record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Episode {
    /// September 1929 to February 1945 (186 months): stocks fell 79% after
    /// inflation by June 1932 and took until 1945 to recover, with deflation in the
    /// early years.
    GreatDepression1929,
    /// January 1966 to July 1982 (199 months): inflation averaged 7% a year, stocks
    /// lost about 19% and 10-year bonds about 36% after inflation, and the 10-year yield rose
    /// from 4.7% to 13.7%.
    Stagflation1966,
    /// April 2000 to February 2013 (155 months): the dot-com bust and the 2008 crisis
    /// back to back; stocks returned nothing after inflation over the 13 years while
    /// bonds did well and yields fell from 6% to 1.9%.
    DotCom2000,
    /// November 2007 to January 2013 (63 months): stocks fell 51% after inflation by
    /// February 2009 and recovered by early 2013; bill rates went to zero.
    Financial2008,
}

/// Default of `Stress::LostGeneration::years`: Japan's stagnation is usually dated
/// from the 1990 crash into the early 2010s. Judgement: 20 years, the "two lost
/// decades".
pub const DEFAULT_LOST_GENERATION_YEARS: f64 = 20.0;
/// Annual real total return of stocks over a lost generation. Judgement informed by
/// Japan: TOPIX fell from 2,881 at the end of 1989 to 1,086 at the end of 1998
/// (Cabinet Office of Japan, Annual Report on the Japanese Economy 2002), about -10%
/// a year in price, and from the end of 1990 to August 2013 it returned -1.96% a year
/// in price (CFA Institute, Enterprising Investor, 2013), with dividends of about 1%
/// and inflation near zero. -2% sits between the post-crash and peak-dated readings.
pub const LOST_GENERATION_EQUITY_REAL: f64 = -0.02;
/// Annual inflation over a lost generation: Japan's consumer prices fell 0.28% a
/// year on average over 1999-2012 (World Bank, consumer price inflation for Japan,
/// via FRED series FPCPITOTLZGJPN). Rounded to -0.3%.
pub const LOST_GENERATION_INFLATION: f64 = -0.003;
/// 10-year yield the path's yield glides down to: Japan's 10-year government bond
/// yield averaged 1.0% in 2003 and 0.7 to 1.7% a year in 1998-2013 (OECD via FRED,
/// IRLTLT01JPA156N).
pub const LOST_GENERATION_LONG_YIELD: f64 = 0.01;
/// Years for the yield to glide from the path's level to the floor: Japan's 10-year
/// yield fell from 7.0% in 1990 to 1.5% in 1998 (same source).
pub const LOST_GENERATION_GLIDE_YEARS: f64 = 8.0;
/// Bill yield the path's bill yield glides down to. Judgement: the Bank of Japan's
/// call rate was at or near zero from 1999 (zero-interest-rate policy); 0.1% stands
/// in for "near zero" in a model whose bills cannot go negative.
pub const LOST_GENERATION_BILL_YIELD: f64 = 0.001;
