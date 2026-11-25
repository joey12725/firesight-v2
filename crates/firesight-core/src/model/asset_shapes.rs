//! Opt-in shapes for the three asset classes the engine builds from a formula rather
//! than from history: the single stock, crypto and the custom asset. Every default
//! here is the behaviour the engine had before these options existed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Uncertain;

/// How often an asset's own monthly surprises are extreme, for the same average
/// return and the same volatility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TailShape {
    /// The bell curve: a surprise four times the usual size happens about once in
    /// 1,300 years of months. The default.
    #[default]
    Normal,
    /// Fat tails: most months are a little quieter than under `normal` and a few are
    /// far wilder, with the same average return and the same volatility. A surprise
    /// four times the usual size happens about once every 17 years (single stock) or
    /// 30 years (custom asset) instead of almost never. Closer to how individual
    /// companies and private funds behave.
    Fat,
}

/// The view taken of crypto's average return. Nothing anchors it (there are no cash
/// flows), so it is an assumption to choose, not an estimate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CryptoOutlook {
    /// On average the holding loses 5% a year after inflation, collapses included.
    Skeptic,
    /// On average the holding just keeps pace with inflation, collapses included.
    /// The default.
    #[default]
    Neutral,
    /// On average the holding gains 6% a year after inflation, collapses included:
    /// about what stocks have returned.
    Believer,
}

/// What kind of crypto is held, which sets the yearly chance that it becomes
/// worthless for good.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CryptoToken {
    /// Bitcoin or ether: a 1% chance each year of a total, permanent loss (26% over
    /// 30 years). The default.
    #[default]
    BitcoinOrEther,
    /// Any other token: an 8% chance each year of a total, permanent loss (92% over
    /// 30 years). Most tokens ever listed are already worthless.
    OtherTokens,
}

/// Whether crypto's volatility is the same every month or comes in calm and wild
/// spells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CryptoDynamics {
    /// Every month has the same volatility (70% a year). The default.
    #[default]
    Steady,
    /// Calm spells alternate with wild ones (manias and the busts that follow), so
    /// big moves arrive in runs. Volatility averaged over time, the average return
    /// and the chance of collapse are the same as with `steady`; a spell says nothing
    /// about the direction of prices.
    Cycles {
        /// Average length of a calm spell, in months, from 1 to 600. Default 30.
        #[serde(default = "d_calm_months")]
        calm_months: f64,
        /// Average length of a wild spell, in months, from 1 to 600. Default 12.
        #[serde(default = "d_wild_months")]
        wild_months: f64,
        /// Volatility in a wild spell divided by volatility in a calm one, from 1 to
        /// 5. The default 2.5 gives about 50% a year when calm and 105% when wild.
        #[serde(default = "d_volatility_ratio")]
        volatility_ratio: f64,
    },
}

fn d_calm_months() -> f64 {
    CryptoDynamics::DEFAULT_CALM_MONTHS
}
fn d_wild_months() -> f64 {
    CryptoDynamics::DEFAULT_WILD_MONTHS
}
fn d_volatility_ratio() -> f64 {
    CryptoDynamics::DEFAULT_VOLATILITY_RATIO
}

impl CryptoDynamics {
    /// Default average length of a calm spell, in months. A judgement
    /// (`asset_shapes.crypto_cycles` in `data/actuarial/params.json`).
    pub const DEFAULT_CALM_MONTHS: f64 = 30.0;
    /// Default average length of a wild spell, in months. A judgement.
    pub const DEFAULT_WILD_MONTHS: f64 = 12.0;
    /// Default ratio of wild to calm volatility. A judgement.
    pub const DEFAULT_VOLATILITY_RATIO: f64 = 2.5;

    /// Cycles at the default settings.
    pub const fn cycles() -> CryptoDynamics {
        CryptoDynamics::Cycles {
            calm_months: Self::DEFAULT_CALM_MONTHS,
            wild_months: Self::DEFAULT_WILD_MONTHS,
            volatility_ratio: Self::DEFAULT_VOLATILITY_RATIO,
        }
    }
}

/// Assumptions for the "crypto" asset class. They matter only if the allocation
/// holds crypto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CryptoAssumptions {
    /// The average return assumed: `skeptic` (-5% a year after inflation), `neutral`
    /// (0%) or `believer` (+6%). Default `neutral`.
    pub outlook: CryptoOutlook,
    /// What is held: `bitcoin_or_ether` (1% a year chance of total loss) or
    /// `other_tokens` (8% a year). Default `bitcoin_or_ether`.
    pub token: CryptoToken,
    /// Whether volatility is the same every month (`steady`) or comes in calm and
    /// wild spells (`cycles`). Default `steady`.
    pub dynamics: CryptoDynamics,
}

/// An occasional sudden loss on the custom asset: a default in a credit fund, a
/// write-down of a private holding, a failed property deal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LossEvent {
    /// Chance that a loss happens in any one year, as a decimal from 0 to 0.5
    /// (0.02 = once in fifty years on average). Default 0.02.
    pub annual_probability: f64,
    /// Share of the holding lost when it happens, as a decimal from 0 to 1 (0.3 =
    /// 30% lost). The loss is permanent. Default: most likely 0.3, between 0.1 and
    /// 0.6.
    pub loss: Uncertain,
    /// What the asset's `mean` describes. True: the average return with these
    /// losses counted, so ordinary months earn a little more to make up for them
    /// and the long-run average is still `mean`. False: the return of ordinary
    /// months only, so the losses come on top and the long-run average is lower
    /// than `mean` (by about the yearly chance times the typical loss). Default
    /// true.
    pub mean_includes_losses: bool,
}

impl Default for LossEvent {
    fn default() -> Self {
        Self {
            annual_probability: 0.02,
            loss: Uncertain::Pert {
                low: 0.1,
                mode: 0.3,
                high: 0.6,
            },
            mean_includes_losses: true,
        }
    }
}
