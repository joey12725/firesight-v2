//! Per-path overlays: opt-in rewrites of a generated path that have their own random
//! streams, so the generator's draws on `Market` and `Inflation` are untouched.
//!
//! Responsibility: uncertainty about the long-run averages
//! (`equity_premium_uncertainty`, `inflation_uncertainty`) and rare disasters
//! (`disasters`). Both are off by default; [`EconomyModel::has_path_overlays`] is
//! then false and nothing here runs.
//!
//! Contract: [`EconomyModel::apply_path_overlays`] is called once per path, straight
//! after `generate` and before any stress overlay. It never allocates. It draws
//!
//! * from `MarketParams`: 2 words when either long-run uncertainty is set (the
//!   equity shift, then the inflation shift, both always drawn), none otherwise;
//! * from `Disaster`: `5 x MAX_EPISODES` = 15 words when disasters are on (per
//!   episode: waiting time, depth, length, whether inflation bursts, burst size),
//!   none otherwise.
//!
//! The counts depend only on the plan, never on the path. The rewrite of month `t`
//! depends only on the drawn values and on month `t` as generated, so a longer path
//! has a shorter one as its prefix, as generation itself does.
//!
//! # Long-run uncertainty
//!
//! Each path draws an annual shift `d_e` to the equity adjustment and `d_i` to the
//! inflation adjustment, from the user's distributions with their means subtracted,
//! so the average shift across paths is zero. The path is then rewritten to what the
//! generator would have produced with `equity_premium_adjustment + d_e` and
//! `inflation_adjustment + d_i`:
//!
//! * equity: the gross returns of US and international equities are multiplied by
//!   `k_e = ((1 + adj + d_e) / (1 + adj))^(1/12)`; listed real estate moves by its
//!   equity beta times the change in the US return, the single stock by its beta
//!   inside its own formula. Bonds, cash, gold, crypto and the custom asset do not
//!   move (crypto and the custom asset have stated expected returns of their own);
//! * inflation: `1 + cpi` and the gross return of every asset and of home prices are
//!   multiplied by `k_i`, and `d_i` is added to the long yield and the mortgage
//!   rate; the bill yield follows the cash return. Every real return and real rate
//!   is unchanged.
//!
//! # Rare disasters
//!
//! Arrivals have a constant hazard: the wait to the first episode, and from the end
//! of each episode to the next, is exponential with rate `-ln(1 - annual
//! probability)` a year, rounded down to whole months. An episode has a depth `D`
//! (limited to 0..0.95) and a length of `n` months (12 to 480). Over the episode the
//! gross return of US and international equities in its month `k = 0..n-1` is
//! multiplied by `exp(ln(1 - D) x w_k)` with `w_k = 2 (n - k) / (n (n + 1))`: the
//! weights fall in a straight line and sum to 1, so by the end the index stands at
//! `1 - D` times where it would have been, three quarters of the fall (in logs)
//! coming in the first half. Nothing is given back afterwards. A factor multiplies a
//! gross return that is at least zero, so no return goes below -100%.
//!
//! Other risky assets get the same factor raised to their equity beta (the betas
//! the stress overlays use, limited to 0..3): listed real estate 0.70, the single
//! stock its beta, crypto its drawdown beta, the custom asset its own. Bonds, cash
//! and home prices keep their generated real returns. The first third of the
//! episode, at most [`DISASTER_RECESSION_MONTHS`] months, is flagged as a recession.
//!
//! With probability `inflation_probability` the episode carries an inflation burst
//! of `p` extra points a year for its whole length: `1 + cpi`, home prices and every
//! risky asset's gross return are multiplied by `(1 + p)^(1/12)` (so the burst
//! itself costs them nothing after inflation); bonds get none of it, so their real
//! return falls by the full burst; cash gets the share `s =` [`CASH_PASS_THROUGH`]
//! of it, compounded: its gross return is multiplied by `(1 + p)^(s/12)`, which is
//! `q = (1 + p)^s - 1` a year (11.8 points for a burst of 25 at a half, not 12.5).
//! The long yield and the mortgage rate rise by that same `q`, and the bill yield,
//! recomputed from the cash return `c` as `(1 + c)^12 - 1`, by `q` times one plus
//! its old level. The extra rise in prices over one episode is limited to
//! [`MAX_BURST_PRICE_FACTOR`], and `p` here is the burst after that limit. Prices
//! do not come back down afterwards.
//!
//! Simplifications: the fall has one fixed shape; there is no recovery and no
//! default on bonds; home prices are not hit; at most [`MAX_EPISODES`] episodes
//! happen on a path, which matters only at hazards far above the suggested 1%.

use crate::dist::CompiledDist;
use crate::math;
use crate::model::{AssetClass, MarketModel, N_ASSETS};
use crate::rng::{Rng, Stream};

use super::{probability_bits, EconomyModel, EconomyPath, MASK_32, MIN_RETURN};

/// Most disaster episodes on one path. The number of draws is fixed by it.
pub const MAX_EPISODES: usize = 3;
/// Words drawn from the `Disaster` stream per possible episode.
pub const DISASTER_WORDS_PER_EPISODE: usize = 5;
/// Share of an inflation burst that cash returns (and with them the bill yield, the
/// long yield and the mortgage rate) keep up with, applied in logs: a burst of `p`
/// a year adds `(1 + p)^share - 1` a year to each of them. A judgement between the 1940s,
/// when bill rates were pegged and kept up with none of it, and the 1970s, when
/// they kept up with most (`rare_disasters` in `data/actuarial/params.json`).
pub const CASH_PASS_THROUGH: f64 = 0.5;
/// Longest stretch at the start of an episode that is flagged as a recession. A
/// judgement: the 1929-33 contraction lasted 43 months.
pub const DISASTER_RECESSION_MONTHS: usize = 36;
/// Limit on the extra rise in the price level over one episode's inflation burst.
pub const MAX_BURST_PRICE_FACTOR: f64 = 10.0;
/// Deepest fall one episode can impose.
pub const MAX_DISASTER_DEPTH: f64 = 0.95;
/// Shortest and longest episode, in months.
pub const DISASTER_MONTHS_RANGE: (usize, usize) = (12, 480);
/// Largest extra annual inflation of a burst.
pub const MAX_BURST_INFLATION: f64 = 0.25;
/// The equity betas other assets respond with are limited to this range.
const DISASTER_BETA_RANGE: (f64, f64) = (0.0, 3.0);
/// Lowest total adjustment, as for the fixed adjustments.
const MIN_ADJUSTMENT: f64 = -0.9;

const UNIT53: f64 = 1.0 / (1u64 << 53) as f64;

/// The compiled long-run uncertainty and disaster settings of one market model.
#[derive(Clone, Debug)]
pub(super) struct Overlay {
    long_run: Option<LongRunSpec>,
    disasters: Option<DisasterSpec>,
}

#[derive(Clone, Debug)]
struct LongRunSpec {
    /// The fixed annual adjustments, as `Adjust::new` reads them.
    base_equity: f64,
    base_inflation: f64,
    equity: CompiledDist,
    equity_mean: f64,
    inflation: CompiledDist,
    inflation_mean: f64,
}

#[derive(Clone, Debug)]
struct DisasterSpec {
    /// Arrival rate per month.
    rate: f64,
    depth: CompiledDist,
    years: CompiledDist,
    /// Probability of an inflation burst times 2^32.
    burst: u64,
    inflation: CompiledDist,
}

/// One disaster episode on one path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Episode {
    /// First month of the episode.
    pub start: usize,
    /// Length in months.
    pub months: usize,
    /// Share of real equity value lost by the end.
    pub depth: f64,
    /// Extra annual inflation over the episode (0 without a burst).
    pub inflation: f64,
}

impl Overlay {
    /// `None` when every option is off.
    pub(super) fn new(market: &MarketModel) -> Option<Overlay> {
        let finite = |x: f64| {
            if x.is_finite() {
                x.max(MIN_ADJUSTMENT)
            } else {
                0.0
            }
        };
        let long_run = (market.equity_premium_uncertainty.is_some()
            || market.inflation_uncertainty.is_some())
        .then(|| {
            let equity = CompiledDist::from_option(market.equity_premium_uncertainty.as_ref(), 0.0);
            let inflation = CompiledDist::from_option(market.inflation_uncertainty.as_ref(), 0.0);
            LongRunSpec {
                base_equity: finite(market.equity_premium_adjustment),
                base_inflation: finite(market.inflation_adjustment),
                equity_mean: equity.mean(),
                inflation_mean: inflation.mean(),
                equity,
                inflation,
            }
        });
        let d = &market.disasters;
        let p = if d.annual_probability.is_finite() {
            d.annual_probability.clamp(0.0, 0.5)
        } else {
            0.0
        };
        let disasters = (p > 0.0).then(|| DisasterSpec {
            rate: -math::ln_1p(-p) / 12.0,
            depth: d.depth.compile(),
            years: d.years.compile(),
            burst: probability_bits(if d.inflation_probability.is_finite() {
                d.inflation_probability
            } else {
                0.0
            }),
            inflation: d.inflation.compile(),
        });
        (long_run.is_some() || disasters.is_some()).then_some(Overlay {
            long_run,
            disasters,
        })
    }
}

impl LongRunSpec {
    /// The path's two shifts: two words from `MarketParams`, always both.
    fn draw(&self, seed: u64, path_index: u64) -> (f64, f64) {
        let mut rng = Rng::stream(seed, path_index, Stream::MarketParams);
        let (equity_word, inflation_word) = (rng.next_u64(), rng.next_u64());
        (
            self.equity.sample(equity_word) - self.equity_mean,
            self.inflation.sample(inflation_word) - self.inflation_mean,
        )
    }
}

impl DisasterSpec {
    /// The path's episodes: the fixed set of words from `Disaster`, then decide.
    fn draw(&self, seed: u64, path_index: u64) -> [Option<Episode>; MAX_EPISODES] {
        let mut rng = Rng::stream(seed, path_index, Stream::Disaster);
        let mut words = [0u64; MAX_EPISODES * DISASTER_WORDS_PER_EPISODE];
        for w in &mut words {
            *w = rng.next_u64();
        }
        self.episodes(&words)
    }

    /// Turns the fixed set of words into the path's episodes, in time order. Unused
    /// slots are `None`; an episode may start beyond the end of the path.
    fn episodes(
        &self,
        words: &[u64; MAX_EPISODES * DISASTER_WORDS_PER_EPISODE],
    ) -> [Option<Episode>; MAX_EPISODES] {
        let mut out = [None; MAX_EPISODES];
        // Months, as a float: a wait can be far longer than any path.
        let mut cursor = 0.0f64;
        for (slot, w) in out
            .iter_mut()
            .zip(words.chunks_exact(DISASTER_WORDS_PER_EPISODE))
        {
            // Uniform in (0, 1], so the logarithm is finite.
            let u = ((w[0] >> 11) + 1) as f64 * UNIT53;
            let start = cursor + (-math::ln(u) / self.rate).floor();
            if start.is_nan() || start >= 1e9 {
                break;
            }
            let depth = self.depth.sample(w[1]).clamp(0.0, MAX_DISASTER_DEPTH);
            let months = (self.years.sample(w[2]) * 12.0).round().clamp(
                DISASTER_MONTHS_RANGE.0 as f64,
                DISASTER_MONTHS_RANGE.1 as f64,
            ) as usize;
            let inflation = if (w[3] & MASK_32) < self.burst {
                self.inflation.sample(w[4]).clamp(0.0, MAX_BURST_INFLATION)
            } else {
                0.0
            };
            *slot = Some(Episode {
                start: start as usize,
                months,
                depth,
                inflation,
            });
            cursor = start + months as f64;
        }
        out
    }
}

impl EconomyModel {
    /// True when the market model has long-run uncertainty or disasters switched on,
    /// so [`EconomyModel::apply_path_overlays`] has work to do.
    #[inline]
    pub fn has_path_overlays(&self) -> bool {
        self.overlay.is_some()
    }

    /// Applies the opt-in per-path overlays to a freshly generated path: the
    /// long-run shifts, then disasters. Draws from the `MarketParams` and `Disaster`
    /// streams of this seed and path (see the module documentation for the counts);
    /// does nothing, and draws nothing, when the options are off. Allocation-free.
    pub fn apply_path_overlays(&self, seed: u64, path_index: u64, out: &mut EconomyPath) {
        let Some(overlay) = self.overlay.as_deref() else {
            return;
        };
        if let Some(spec) = &overlay.long_run {
            let (d_equity, d_inflation) = spec.draw(seed, path_index);
            self.apply_long_run_shift(out, spec, d_equity, d_inflation);
        }
        if let Some(spec) = &overlay.disasters {
            let mut burst = false;
            for episode in spec.draw(seed, path_index).iter().flatten() {
                if episode.start >= out.months {
                    break;
                }
                self.apply_disaster(out, episode);
                burst |= episode.inflation > 0.0;
            }
            if burst {
                out.rebuild_price_level();
            }
        }
        // Income yields that move with the market follow the shifted returns.
        if self.dividends.is_some() {
            self.refresh_dividends(out);
        }
    }

    /// The long-run shifts path `path_index` gets: `(equity, inflation)` in annual
    /// decimal points, both zero when the option is off. For tests and diagnostics;
    /// draws the same words as [`EconomyModel::apply_path_overlays`].
    pub fn long_run_shift(&self, seed: u64, path_index: u64) -> (f64, f64) {
        match self.overlay.as_deref().and_then(|o| o.long_run.as_ref()) {
            Some(spec) => spec.draw(seed, path_index),
            None => (0.0, 0.0),
        }
    }

    /// The disaster episodes path `path_index` gets, in time order (some may start
    /// after the path ends). For tests and diagnostics; draws the same words as
    /// [`EconomyModel::apply_path_overlays`].
    pub fn disaster_episodes(&self, seed: u64, path_index: u64) -> [Option<Episode>; MAX_EPISODES] {
        match self.overlay.as_deref().and_then(|o| o.disasters.as_ref()) {
            Some(spec) => spec.draw(seed, path_index),
            None => [None; MAX_EPISODES],
        }
    }

    /// Rewrites the whole path to what the generator would have produced with the
    /// annual adjustments moved by `d_equity` and `d_inflation`.
    fn apply_long_run_shift(
        &self,
        out: &mut EconomyPath,
        spec: &LongRunSpec,
        d_equity: f64,
        d_inflation: f64,
    ) {
        let equity = (spec.base_equity + d_equity).max(MIN_ADJUSTMENT);
        let inflation = (spec.base_inflation + d_inflation).max(MIN_ADJUSTMENT);
        let ke = (1.0 + math::annual_to_monthly(equity)) / self.adjust.equity;
        let ki = (1.0 + math::annual_to_monthly(inflation)) / self.adjust.inflation;
        let rate_shift = inflation - spec.base_inflation;
        let months = out.months;
        if ke != 1.0 && ke.is_finite() {
            let [us, intl, _, cash, re, _, _, stock, _] = &mut out.returns;
            let re_beta = self.assets.re_beta_eq;
            let stock_beta = self.assets.stock_beta;
            for t in 0..months {
                let old = us[t];
                let new = (1.0 + old) * ke - 1.0;
                let delta = new - old;
                us[t] = new;
                intl[t] = (1.0 + intl[t]) * ke - 1.0;
                re[t] = (re[t] + re_beta * delta).max(MIN_RETURN);
                // The single stock's gross return is its market part times its own
                // shock: swap the market part. A total loss stays one.
                let base = 1.0 + cash[t] + stock_beta * (old - cash[t]);
                if stock[t] > -1.0 && base > 0.0 {
                    let moved = (base + stock_beta * delta).max(0.0);
                    stock[t] = (1.0 + stock[t]) * (moved / base) - 1.0;
                }
            }
        }
        if ki != 1.0 && ki.is_finite() {
            let cash = AssetClass::Cash.index();
            for series in &mut out.returns {
                for r in &mut series[..months] {
                    *r = (1.0 + *r) * ki - 1.0;
                }
            }
            for t in 0..months {
                out.inflation[t] = (1.0 + out.inflation[t]) * ki - 1.0;
                out.home_national[t] = (1.0 + out.home_national[t]) * ki - 1.0;
                out.tbill[t] = math::powi(1.0 + out.returns[cash][t], 12) - 1.0;
                out.long_yield[t] += rate_shift;
                out.mortgage_rate[t] += rate_shift;
            }
            out.rebuild_price_level();
        }
    }

    /// Rewrites the months of one episode. The caller rebuilds the price level when
    /// the episode carried inflation.
    fn apply_disaster(&self, out: &mut EconomyPath, episode: &Episode) {
        let n = episode.months.max(1);
        let end = episode.start.saturating_add(n).min(out.months);
        let log_depth = math::ln_1p(-episode.depth.clamp(0.0, MAX_DISASTER_DEPTH));
        let weight_scale = 2.0 / (n as f64 * (n as f64 + 1.0));
        // Exponent of the equity factor per asset class.
        let mut beta = [0.0; N_ASSETS];
        for a in [
            AssetClass::RealEstate,
            AssetClass::Gold,
            AssetClass::Crypto,
            AssetClass::SingleStock,
            AssetClass::Custom,
        ] {
            let b = self.stress_beta[a.index()];
            if b.is_finite() {
                beta[a.index()] = b.clamp(DISASTER_BETA_RANGE.0, DISASTER_BETA_RANGE.1);
            }
        }
        beta[AssetClass::UsStocks.index()] = 1.0;
        beta[AssetClass::IntlStocks.index()] = 1.0;
        // The inflation burst as monthly factors, the price rise capped over the
        // whole episode.
        let burst = episode.inflation > 0.0;
        let (g, g_cash, rate_shift) = if burst {
            let cap = math::ln(MAX_BURST_PRICE_FACTOR) / n as f64;
            let log_g = (math::ln_1p(episode.inflation) / 12.0).min(cap);
            // One definition of the share for cash and for the yields: the
            // compounded one, `(1 + p)^share - 1` a year.
            let log_cash = CASH_PASS_THROUGH * log_g;
            (
                math::exp(log_g),
                math::exp(log_cash),
                math::exp_m1(12.0 * log_cash),
            )
        } else {
            (1.0, 1.0, 0.0)
        };
        let (bonds, cash) = (AssetClass::Bonds.index(), AssetClass::Cash.index());
        let recession_months = (n / 3).min(DISASTER_RECESSION_MONTHS);
        for t in episode.start..end {
            let k = t - episode.start;
            let x = log_depth * (weight_scale * (n - k) as f64);
            let fall = math::exp(x);
            for i in 0..N_ASSETS {
                if i == bonds || i == cash {
                    continue;
                }
                let b = beta[i];
                let f = if b == 1.0 {
                    fall
                } else if b == 0.0 {
                    1.0
                } else {
                    math::exp(b * x)
                };
                out.returns[i][t] = (1.0 + out.returns[i][t]) * (f * g) - 1.0;
            }
            if k < recession_months {
                out.recession[t] = 1;
            }
            if burst {
                out.inflation[t] = (1.0 + out.inflation[t]) * g - 1.0;
                out.home_national[t] = (1.0 + out.home_national[t]) * g - 1.0;
                out.returns[cash][t] = (1.0 + out.returns[cash][t]) * g_cash - 1.0;
                out.tbill[t] = math::powi(1.0 + out.returns[cash][t], 12) - 1.0;
                out.long_yield[t] += rate_shift;
                out.mortgage_rate[t] += rate_shift;
            }
        }
    }
}
