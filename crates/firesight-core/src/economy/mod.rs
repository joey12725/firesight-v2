//! Joint economic scenario generator.
//!
//! Responsibility: for one path, produce every month's inflation, asset returns,
//! interest rates, national home-price change and recession flag, jointly, so that the
//! historical co-movement (stocks fall in recessions, bonds and mortgage rates follow
//! yields, inflation regimes persist) is preserved.
//!
//! Contract:
//!
//! * Output goes into a caller-owned [`EconomyPath`] (struct of arrays) that is reused
//!   across paths: `generate` does not allocate once the buffers have reached the
//!   horizon length.
//! * Draws come only from the `Market` and `Inflation` streams handed in, and their
//!   number per month is fixed for a given model kind. Nothing about the household
//!   influences the draws: all nine asset classes are always generated, and the
//!   single-stock and custom-asset parameters only rescale draws that are taken
//!   anyway. Two scenarios of one profile therefore see the same economy on the same
//!   path (common random numbers).
//!
//! # Model kinds (`assumptions.market.kind`)
//!
//! * **Bootstrap** (default): stationary bootstrap (Politis and Romano 1994) over row
//!   indices of the aligned monthly history. Each month the path either continues to
//!   the next historical month (wrapping from the last row to the first) or, with
//!   probability `1 / mean_block_months`, jumps to a uniformly random row, so block
//!   lengths are geometric. Every series of a month comes from the same historical
//!   row: US equities, bills, bonds, CPI, home prices, the long yield, the mortgage
//!   rate and the recession flag keep their cross-correlations and joint dynamics.
//!   `from_year` restricts the rows. Levels (yields, mortgage rate) jump at block
//!   seams along with everything else.
//! * **Historical**: path `i` replays the rows starting at row `i mod cohort_count`,
//!   wrapping to the first row when history runs out.
//! * **Regime** and **Lognormal**: parametric models, see [`regime`].
//!
//! # Short-history columns
//!
//! International equities start in 1975 and gold in 1968. Once, at construction, each
//! is regressed on US equities, bonds and CPI over its own sample. For a sampled month
//! without the real value, the return is the regression fit plus a residual with the
//! regression's residual volatility and Student-t(6) tails drawn on the `Market`
//! stream. Months that have the real value use it. (The residual draw is taken every
//! month so the draw count does not depend on the sampled row.)
//!
//! # Derived asset classes (all models)
//!
//! Built from the month's factors plus idiosyncratic noise on the `Market` stream:
//!
//! * `RealEstate` (listed real estate): `bill + 0.70 x (equity - bill) + 1.0 x (real
//!   home-price growth minus its mean) + noise`, the noise Student-t(6) with 14%
//!   annual volatility. No alpha: the expected return is bills plus 0.70 of the
//!   equity premium.
//! * `Crypto`: per `data/actuarial/params.json` (`crypto`). The monthly gross return is
//!   `(1 + cpi) * exp(mu / 12) * F(equity) * exp(sigma_i * t4)`: a median that keeps
//!   pace with inflation (`mu` = the "neutral" preset, 0), an equity link `F` with
//!   correlation 0.35 that steepens to 0.6 for equity months more than one standard
//!   deviation below average, Student-t(4) shocks sized so total volatility is 70% a
//!   year, and each month a `1%/12` chance of a collapse (return -100%). After a
//!   collapse the process continues: money rebalanced into the sleeve later is buying
//!   a successor asset; what was lost stays lost.
//! * `SingleStock`: `(1 + bill + beta (equity - bill)) * L - 1` with `L` lognormal,
//!   mean exactly 1, sized so total volatility is `single_stock.volatility`; plus a
//!   small monthly hazard of a total loss, interpolated from the presets in
//!   `params.json` (`equity_compensation.single_public_stock`), with survivors'
//!   returns raised by the same amount so the expected return is exactly what the
//!   market exposure implies. Concentration adds risk and volatility drag, not return.
//! * `Custom`: monthly mean from the user's annual mean, `beta = corr * vol / equity
//!   vol` on the equity return's deviation from its mean, normal residual.
//! * `Cash`: the bill return.
//!
//! # Adjustments
//!
//! `equity_premium_adjustment` multiplies the monthly gross return of US and
//! international equities by `(1 + adj)^(1/12)`; the real-estate and single-stock
//! sleeves inherit it through their betas. `inflation_adjustment` multiplies `1 + cpi`
//! and every asset's and home prices' gross return by `(1 + adj)^(1/12)` and adds
//! `adj` to the long yield, the mortgage rate and (through the cash return) the bill
//! yield, so every real return and real rate is unchanged.
//!
//! # Stress overlays
//!
//! See [`EconomyModel::apply_stress`].

mod bonds;
mod episodes;
mod fat_tailed;
mod fit;
mod income_yields;
#[cfg(test)]
mod realism_tests;
mod regime;
mod sampler;
mod stress;
#[cfg(test)]
mod tests;
mod valuation;

use std::sync::{Arc, OnceLock};

use serde_json::Value;

use crate::data::{Data, History};
use crate::math;
use crate::model::{
    AssetClass, BondPricing, CustomAsset, MarketKind, MarketModel, PortfolioPolicy, SingleStock,
    YearMonth, N_ASSETS,
};
use crate::rng::Rng;

use regime::{LognormalParams, LongRun, RegimeParams};
use sampler::ShockTable;

pub use episodes::{episode_first_month, episode_months};
pub use fat_tailed::{
    FAT_TAIL_ARCH, FAT_TAIL_DOF, FAT_TAIL_GARCH, FAT_TAIL_MIXING_CAP, FAT_TAIL_VARIANCE_CAP,
};
pub use income_yields::{
    DIVIDEND_ADJUSTMENT_ANNUAL, DIVIDEND_YIELD_RANGE, RECESSION_DIVIDEND_CUT_ANNUAL,
};
pub use stress::{ResolvedStress, CRASH_MONTHS, LOST_DECADE_MONTHS};

// ---------------------------------------------------------------------------------
// Assumptions that are not in data/. Each is a named constant with its source.
// ---------------------------------------------------------------------------------

/// Beta of listed real estate to US equities. FTSE Nareit All Equity REITs against
/// the US market since 1972: correlation about 0.6, volatility about 18-20%, which
/// with 18% market volatility is a beta near 0.7 (figures quoted from memory, not
/// re-verified; docs/v1-review.md entry 13 gives "equity-like volatility around 20%,
/// high equity correlation").
pub const REIT_EQUITY_BETA: f64 = 0.70;
/// Exposure of listed real estate to real national home-price growth in excess of its
/// long-run mean. Judgement: one for one.
pub const REIT_HOME_BETA: f64 = 1.0;
/// Annual idiosyncratic volatility of listed real estate, chosen so total volatility
/// is about 19% and the equity correlation about 0.67 with the beta above.
pub const REIT_IDIOSYNCRATIC_VOL: f64 = 0.14;

/// Annual cash distributions (dividends) as a fraction of market value, by asset
/// class, for taxing distributions in taxable accounts. `Bonds` and `Cash` are not in
/// this table: their income is the prevailing long yield and bill yield of the month
/// (see [`EconomyPath::income_yields_at`]). Values are round numbers for the mid-2020s
/// quoted from memory and not re-verified: US total market funds yield about 1.3-1.5%,
/// developed ex-US funds about 3%, US REIT funds about 4%; the single stock is given
/// the market yield; gold, crypto and the custom asset distribute nothing.
pub const DIVIDEND_YIELD: [f64; N_ASSETS] = [0.015, 0.030, 0.0, 0.0, 0.040, 0.0, 0.0, 0.015, 0.0];
/// Fraction of each class's distributions taxed as qualified dividends (the rest is
/// ordinary income). US funds are almost entirely qualified, international funds
/// roughly three quarters, REIT distributions and all interest are ordinary. Round
/// numbers quoted from memory, not re-verified.
pub const QUALIFIED_FRACTION: [f64; N_ASSETS] = [0.95, 0.75, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// Fallbacks for `data/actuarial/params.json` (`crypto.model`), used only if a key is
/// missing; the values are the file's own.
const CRYPTO_SIGMA_ANNUAL: f64 = 0.70;
const CRYPTO_T_DOF: u32 = 4;
const CRYPTO_EQUITY_CORR: f64 = 0.35;
const CRYPTO_DRAWDOWN_CORR: f64 = 0.60;
const CRYPTO_RUIN_ANNUAL: f64 = 0.01;
const CRYPTO_EXPECTED_REAL_RETURN: f64 = 0.0;
/// Fallbacks for `long_run_assumptions` in `data/actuarial/params.json`.
const INTL_SPREAD_VS_US: f64 = -0.01;
const GOLD_REAL_RETURN: f64 = 0.01;
/// Bootstrap: monthly pull of the long yield and the mortgage rate toward their
/// long-run means (a half-life of about seven years; yields are very persistent).
const RATE_REVERSION: f64 = 1.0 / 120.0;
/// Bootstrap: the long yield is kept at or above this (the lowest month-end 10-year
/// yield in the history is about 0.6%).
const MIN_LONG_YIELD: f64 = 0.005;
/// Bootstrap: the mortgage rate stays within this spread over the long yield (the
/// history's spread has ranged from about 0.5 to 4 points).
const MORTGAGE_SPREAD_RANGE: (f64, f64) = (0.005, 0.04);
/// The crisis correlation applies to the part of an equity month that is more than
/// this many monthly standard deviations below the mean (judgement: "in equity
/// drawdowns" read as a worse than one-sigma month).
const CRYPTO_DRAWDOWN_SIGMAS: f64 = 1.0;
/// The monthly idiosyncratic crypto shock is capped at this many standard deviations
/// (about a 5x or a -80% month at 70% volatility). A Student-t(4) has no finite
/// exponential moment, so without a cap the average return would be set by the table
/// truncation instead of by an assumption. Bitcoin's largest calendar-month gain was
/// about 5.5x (November 2013, from memory).
const CRYPTO_SHOCK_CAP_SD: f64 = 8.0;
/// Floor on the equity-link factor of crypto (keeps the gross return positive).
const CRYPTO_LINK_FLOOR: f64 = 0.05;
/// Idiosyncratic variance of crypto is at least this share of total variance.
const CRYPTO_MIN_IDIO_SHARE: f64 = 0.25;

/// Fallback presets `(idiosyncratic volatility, annual ruin hazard)` for
/// `equity_compensation.single_public_stock.defaults` in params.json.
const STOCK_PRESETS: [(f64, f64); 3] = [(0.25, 0.003), (0.35, 0.01), (0.50, 0.03)];

/// Lowest monthly return of the additive asset classes (real estate, custom,
/// synthesised international and gold): a guard against a fat-tailed residual taking
/// a return below -100%.
const MIN_RETURN: f64 = -0.95;
/// A `from_year` that would leave fewer rows than this is moved back so this many
/// remain (ten years is the least that can stand in for a business cycle).
const MIN_BOOTSTRAP_MONTHS: usize = 120;
/// Real annual geometric returns of US equities, 10-year bonds and bills over
/// 1973-01..1981-12, computed from data/history/monthly.csv. Used only if the history
/// handed in does not cover that window; otherwise recomputed from it.
const STAGFLATION_REAL_FALLBACK: [f64; 3] = [-0.0283, -0.0524, -0.0090];
const STAGFLATION_WINDOW: (YearMonth, YearMonth) =
    (YearMonth::ym(1973, 1), YearMonth::ym(1981, 12));

const SQRT_12: f64 = 3.464_101_615_137_754_6;
const TWO_POW_32: f64 = 4_294_967_296.0;
const MASK_32: u64 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------------
// The output buffers.
// ---------------------------------------------------------------------------------

/// One path of the economy, month by month. Index `t` is months since `profile.start`.
///
/// Returns and inflation are simple monthly fractions (0.01 = +1% in the month) in
/// nominal terms. Rates are annualised decimals.
#[derive(Clone, Debug, Default)]
pub struct EconomyPath {
    /// Number of valid months. All vectors have at least this length
    /// (`price_level` has `months + 1`).
    pub months: usize,
    /// CPI inflation during month `t`.
    pub inflation: Vec<f64>,
    /// Cumulative price level at the START of month `t`; `price_level[0] == 1.0` and
    /// `price_level[t + 1] = price_level[t] * (1 + inflation[t])`. Nominal dollars in
    /// month `t` divided by `price_level[t]` are today's dollars.
    pub price_level: Vec<f64>,
    /// Nominal total return of each asset class during month `t`:
    /// `returns[asset.index()][t]`.
    pub returns: [Vec<f64>; N_ASSETS],
    /// Treasury bill yield, annualised with monthly compounding, so that
    /// `(1 + tbill[t])^(1/12) - 1` is the `Cash` return of month `t`. Cash accounts
    /// earn this plus their spread.
    pub tbill: Vec<f64>,
    /// 10-year Treasury yield at the end of month `t`, annualised.
    pub long_yield: Vec<f64>,
    /// 30-year fixed mortgage rate available to a new borrower, annualised.
    pub mortgage_rate: Vec<f64>,
    /// National home price index change during month `t`.
    pub home_national: Vec<f64>,
    /// 1 if month `t` is in a recession (drives layoff hazard), else 0.
    pub recession: Vec<u8>,
    /// With `market.valuation`: log of the CAPE of US stocks over its normal level
    /// at the end of month `t`. Empty when valuation is off.
    pub valuation_gap: Vec<f64>,
    /// With `market.dynamic_yields`: the dividend yields of US and international
    /// stocks in month `t`, annualised. Empty when off (the constant
    /// [`DIVIDEND_YIELD`] applies).
    pub dividend_yields: Vec<[f64; 2]>,
}

impl EconomyPath {
    /// An empty path; buffers grow on first use.
    pub fn new() -> Self {
        Self::default()
    }

    /// A path with buffers preallocated for `months`.
    pub fn with_capacity(months: usize) -> Self {
        let mut p = Self::default();
        p.reset(months);
        p
    }

    /// Sets the length to `months` and zeroes the contents, keeping allocations.
    pub fn reset(&mut self, months: usize) {
        fn fill<T: Clone + Default>(v: &mut Vec<T>, n: usize) {
            v.clear();
            v.resize(n, T::default());
        }
        self.months = months;
        fill(&mut self.inflation, months);
        fill(&mut self.price_level, months + 1);
        self.price_level[0] = 1.0;
        for r in &mut self.returns {
            fill(r, months);
        }
        fill(&mut self.tbill, months);
        fill(&mut self.long_yield, months);
        fill(&mut self.mortgage_rate, months);
        fill(&mut self.home_national, months);
        fill(&mut self.recession, months);
    }

    /// Sets the length to `months` without clearing: the generators overwrite every
    /// element, so zeroing first would be wasted work.
    fn prepare(&mut self, months: usize) {
        self.months = months;
        self.inflation.resize(months, 0.0);
        self.price_level.resize(months + 1, 1.0);
        self.price_level[0] = 1.0;
        for r in &mut self.returns {
            r.resize(months, 0.0);
        }
        self.tbill.resize(months, 0.0);
        self.long_yield.resize(months, 0.0);
        self.mortgage_rate.resize(months, 0.0);
        self.home_national.resize(months, 0.0);
        self.recession.resize(months, 0);
    }

    /// Return of one asset class in month `t`.
    #[inline]
    pub fn asset_return(&self, asset: AssetClass, t: usize) -> f64 {
        self.returns[asset.index()][t]
    }

    /// All asset returns for month `t` as a fixed array, for bucket updates.
    #[inline]
    pub fn returns_at(&self, t: usize) -> [f64; N_ASSETS] {
        let mut out = [0.0; N_ASSETS];
        for (o, series) in out.iter_mut().zip(&self.returns) {
            *o = series[t];
        }
        out
    }

    /// Annualised cash distribution yield of every asset class in month `t`, as a
    /// fraction of market value: [`DIVIDEND_YIELD`] for the equity-like classes, the
    /// prevailing long yield for `Bonds` (a par bond's coupon) and the bill yield for
    /// `Cash`. A taxable holding worth `v` distributes about `v * yield / 12` this
    /// month. The distribution is part of the total return in `returns`, not an
    /// addition to it. [`QUALIFIED_FRACTION`] says how much of it is qualified.
    #[inline]
    pub fn income_yields_at(&self, t: usize) -> [f64; N_ASSETS] {
        let mut out = DIVIDEND_YIELD;
        out[AssetClass::Bonds.index()] = self.long_yield[t].max(0.0);
        out[AssetClass::Cash.index()] = self.tbill[t].max(0.0);
        if let Some(&[us, intl]) = self.dividend_yields.get(t) {
            out[AssetClass::UsStocks.index()] = us;
            out[AssetClass::SingleStock.index()] = us;
            out[AssetClass::IntlStocks.index()] = intl;
        }
        out
    }

    /// Recomputes `price_level` from `inflation`. Stress overlays call this after
    /// rewriting inflation.
    pub fn rebuild_price_level(&mut self) {
        self.price_level[0] = 1.0;
        for t in 0..self.months {
            self.price_level[t + 1] = self.price_level[t] * (1.0 + self.inflation[t]);
        }
    }

    /// Converts a nominal amount in month `t` to today's dollars.
    #[inline]
    pub fn to_real(&self, nominal: f64, t: usize) -> f64 {
        nominal / self.price_level[t]
    }

    /// Converts today's dollars to nominal dollars of month `t`.
    #[inline]
    pub fn to_nominal(&self, real: f64, t: usize) -> f64 {
        real * self.price_level[t]
    }
}

/// The two random streams the economy may draw from.
pub struct EconomyStreams<'a> {
    /// `Rng::stream(seed, path, Stream::Market)`.
    pub market: &'a mut Rng,
    /// `Rng::stream(seed, path, Stream::Inflation)`.
    pub inflation: &'a mut Rng,
    /// `Rng::stream(seed, path, Stream::MarketTails)`: drawn only by the fat-tailed
    /// model.
    pub tails: &'a mut Rng,
}

// ---------------------------------------------------------------------------------
// Internal representation.
// ---------------------------------------------------------------------------------

/// The factor series of one month before adjustments and derived assets.
#[derive(Clone, Copy, Debug, Default)]
struct Core {
    eq: f64,
    intl: f64,
    bond: f64,
    cash: f64,
    cpi: f64,
    home: f64,
    gold: f64,
    long_yield: f64,
    mortgage: f64,
    recession: u8,
}

/// One month ready to emit: adjusted factors, the bill yield, and the deterministic
/// part of each derived asset class (the noise is applied when the month is emitted).
#[derive(Clone, Copy, Debug, Default)]
struct Month {
    eq: f64,
    intl: f64,
    bond: f64,
    cash: f64,
    cpi: f64,
    home: f64,
    gold: f64,
    tbill: f64,
    long_yield: f64,
    mortgage: f64,
    /// Real estate return before its noise.
    re: f64,
    /// Single-stock gross return before its lognormal shock.
    stock: f64,
    /// Crypto gross return before its shock.
    crypto: f64,
    /// Custom asset return before its noise.
    custom: f64,
    recession: u8,
}

/// A row of the bootstrap table: a month plus the residual volatility of the
/// synthesised columns (zero where the history has the real value).
#[derive(Clone, Copy, Debug, Default)]
struct Row {
    m: Month,
    intl_sd: f64,
    gold_sd: f64,
    /// Change of the long yield and of the mortgage rate from the row before it in
    /// the history (0 for the first row).
    d_long: f64,
    d_mortgage: f64,
}

/// The equity premium and inflation adjustments as monthly factors.
#[derive(Clone, Copy, Debug)]
struct Adjust {
    /// Monthly gross factor on equity returns.
    equity: f64,
    /// Monthly gross factor on the price level and on every nominal return.
    inflation: f64,
    /// Added to annualised nominal rate levels.
    inflation_annual: f64,
}

/// Parameters of the derived asset classes, in monthly units.
#[derive(Clone, Copy, Debug)]
struct AssetParams {
    /// Mean monthly US equity return after adjustments.
    eq_mean: f64,
    re_beta_eq: f64,
    re_beta_home: f64,
    /// Mean monthly real home-price growth.
    home_real_mean: f64,
    re_sigma: f64,
    stock_beta: f64,
    /// `1 / (1 - monthly ruin probability)`.
    stock_survivor_boost: f64,
    /// Monthly ruin probability times 2^32.
    stock_ruin: u64,
    crypto_beta: f64,
    /// Extra beta beyond the drawdown threshold.
    crypto_beta_extra: f64,
    /// Distance below the mean equity return at which the extra beta starts.
    crypto_threshold: f64,
    /// `exp(mu / 12)` divided by the geometric mean of the equity-link factor.
    crypto_norm: f64,
    crypto_ruin: u64,
    custom_mean: f64,
    custom_beta: f64,
    custom_sigma: f64,
}

/// Everything derived from the history that does not depend on the adjustments or the
/// portfolio.
#[derive(Debug)]
struct HistBase {
    /// The selected rows; where international equities or gold are missing the value
    /// is the factor-model fit.
    rows: Vec<Core>,
    intl_sd: Vec<f64>,
    gold_sd: Vec<f64>,
    /// Mean and standard deviation of monthly US equity returns over the rows.
    eq_mean: f64,
    eq_sd: f64,
    home_real_mean: f64,
    /// Factor fits `[intercept, equity_us, bond10, cpi]` and residual sd (kept for
    /// diagnostics; the fitted values are already in `rows`).
    #[cfg_attr(not(test), allow(dead_code))]
    intl_fit: ([f64; 4], f64),
    #[cfg_attr(not(test), allow(dead_code))]
    gold_fit: ([f64; 4], f64),
    /// Simple regression betas on US equities, for stress overlays.
    intl_beta: f64,
    gold_beta: f64,
    /// Monthly real gross returns of equities, bonds and bills in 1973-81.
    stagflation_real: [f64; 3],
    crypto: CryptoSpec,
    stock_presets: [(f64, f64); 3],
    /// Long yield and mortgage rate in the last row of the history (today's levels)
    /// and their means over the rows used.
    rate_now: (f64, f64),
    rate_mean: (f64, f64),
    /// The constants every month's gross international-equity and gold return was
    /// multiplied by to set their long-run level (1 in the historical replay).
    #[cfg_attr(not(test), allow(dead_code))]
    short_scale: (f64, f64),
}

/// The crypto assumptions read from params.json.
#[derive(Clone, Copy, Debug)]
struct CryptoSpec {
    sigma_annual: f64,
    dof: u32,
    equity_corr: f64,
    drawdown_corr: f64,
    ruin_annual: f64,
    /// Expected (arithmetic mean) real return per year, collapse risk included.
    expected_real_return: f64,
}

#[derive(Clone, Debug)]
enum Kind {
    Bootstrap {
        /// Restart probability times 2^32.
        restart: u64,
    },
    Historical,
    Regime(Arc<RegimeParams>),
    Lognormal(Arc<LognormalParams>),
    FatTailed(Arc<fat_tailed::FatTailedParams>),
}

/// The compiled scenario generator for one profile. Built once in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct EconomyModel {
    kind: Kind,
    base: Arc<HistBase>,
    adjust: Adjust,
    assets: AssetParams,
    /// Bootstrap and historical: the adjusted rows with derived-asset bases.
    rows: Arc<[Row]>,
    stock_shock: Arc<ShockTable>,
    crypto_shock: Arc<ShockTable>,
    /// Sensitivity of each asset class to an equity move imposed by a stress overlay.
    stress_beta: [f64; N_ASSETS],
    /// Duration and convexity tables, for overlays that move yields and for
    /// `bond_pricing: from_yield`.
    bonds: Arc<bonds::BondPricer>,
    /// Block bootstrap with `bond_pricing: from_yield`: bond returns are repriced
    /// from the path's yield after generation.
    bonds_from_yield: bool,
    /// `market.valuation`, compiled; `None` when off or for the historical replay.
    valuation: Option<valuation::ValuationTilt>,
    /// The record's months of the historical episodes, when a stress replays one.
    episodes: Option<Arc<episodes::EpisodeTables>>,
    /// `market.dynamic_yields`, compiled; `None` when off.
    dividends: Option<income_yields::DividendModel>,
}

/// The tables and model pieces the per-month emit step needs, borrowed once per path.
struct Emit<'a> {
    assets: &'a AssetParams,
    normal: &'a ShockTable,
    t6: &'a ShockTable,
    stock: &'a ShockTable,
    crypto: &'a ShockTable,
}

/// Mutable views of the output buffers, exactly `months` long.
struct Sink<'a> {
    inflation: &'a mut [f64],
    price_level: &'a mut [f64],
    us: &'a mut [f64],
    intl: &'a mut [f64],
    bond: &'a mut [f64],
    cash: &'a mut [f64],
    re: &'a mut [f64],
    gold: &'a mut [f64],
    crypto: &'a mut [f64],
    stock: &'a mut [f64],
    custom: &'a mut [f64],
    tbill: &'a mut [f64],
    long_yield: &'a mut [f64],
    mortgage: &'a mut [f64],
    home: &'a mut [f64],
    recession: &'a mut [u8],
    level: f64,
}

impl<'a> Sink<'a> {
    fn new(out: &'a mut EconomyPath) -> Sink<'a> {
        let n = out.months;
        let [us, intl, bond, cash, re, gold, crypto, stock, custom] = &mut out.returns;
        Sink {
            inflation: &mut out.inflation[..n],
            price_level: &mut out.price_level[..n + 1],
            us: &mut us[..n],
            intl: &mut intl[..n],
            bond: &mut bond[..n],
            cash: &mut cash[..n],
            re: &mut re[..n],
            gold: &mut gold[..n],
            crypto: &mut crypto[..n],
            stock: &mut stock[..n],
            custom: &mut custom[..n],
            tbill: &mut out.tbill[..n],
            long_yield: &mut out.long_yield[..n],
            mortgage: &mut out.mortgage_rate[..n],
            home: &mut out.home_national[..n],
            recession: &mut out.recession[..n],
            level: 1.0,
        }
    }
}

impl Emit<'_> {
    /// Writes month `t`: applies the derived assets' noise (4 variates and 1 hazard
    /// draw from `rng`, always) and stores everything.
    #[inline(always)]
    fn put(&self, sink: &mut Sink<'_>, t: usize, m: &Month, intl: f64, gold: f64, rng: &mut Rng) {
        let a = self.assets;
        let re = (m.re + a.re_sigma * self.t6.sample(rng.next_u64())).max(MIN_RETURN);
        let mut crypto = m.crypto * self.crypto.sample(rng.next_u64()) - 1.0;
        let mut stock = m.stock * self.stock.sample(rng.next_u64()) - 1.0;
        let custom =
            (m.custom + a.custom_sigma * self.normal.sample(rng.next_u64())).max(MIN_RETURN);
        let hazard = rng.next_u64();
        if (hazard >> 32) < a.crypto_ruin {
            crypto = -1.0;
        }
        if (hazard & MASK_32) < a.stock_ruin {
            stock = -1.0;
        }
        sink.inflation[t] = m.cpi;
        sink.level *= 1.0 + m.cpi;
        sink.price_level[t + 1] = sink.level;
        sink.us[t] = m.eq;
        sink.intl[t] = intl;
        sink.bond[t] = m.bond;
        sink.cash[t] = m.cash;
        sink.re[t] = re;
        sink.gold[t] = gold;
        sink.crypto[t] = crypto;
        sink.stock[t] = stock;
        sink.custom[t] = custom;
        sink.tbill[t] = m.tbill;
        sink.long_yield[t] = m.long_yield;
        sink.mortgage[t] = m.mortgage;
        sink.home[t] = m.home;
        sink.recession[t] = m.recession;
    }
}

impl Adjust {
    fn new(market: &MarketModel) -> Adjust {
        let finite = |x: f64| if x.is_finite() { x.max(-0.9) } else { 0.0 };
        let e = finite(market.equity_premium_adjustment);
        let i = finite(market.inflation_adjustment);
        Adjust {
            equity: 1.0 + math::annual_to_monthly(e),
            inflation: 1.0 + math::annual_to_monthly(i),
            inflation_annual: i,
        }
    }
}

impl AssetParams {
    /// Adjusts a month's factors and computes the deterministic part of each derived
    /// asset class.
    #[inline(always)]
    fn month(&self, adjust: &Adjust, c: &Core) -> Month {
        let ki = adjust.inflation;
        let ke = adjust.equity * ki;
        let eq = scaled(c.eq, ke);
        let cash = scaled(c.cash, ki);
        let cpi = scaled(c.cpi, ki);
        let home = scaled(c.home, ki);
        let d = eq - self.eq_mean;
        let link = 1.0
            + self.crypto_beta * d
            + self.crypto_beta_extra * (d + self.crypto_threshold).min(0.0);
        Month {
            eq,
            intl: scaled(c.intl, ke),
            bond: scaled(c.bond, ki),
            cash,
            cpi,
            home,
            gold: scaled(c.gold, ki),
            tbill: math::powi(1.0 + cash, 12) - 1.0,
            long_yield: c.long_yield + adjust.inflation_annual,
            mortgage: c.mortgage + adjust.inflation_annual,
            re: cash
                + self.re_beta_eq * (eq - cash)
                + self.re_beta_home * (home - cpi - self.home_real_mean),
            stock: (1.0 + cash + self.stock_beta * (eq - cash)).max(0.0)
                * self.stock_survivor_boost,
            crypto: link.max(CRYPTO_LINK_FLOOR) * (1.0 + cpi) * self.crypto_norm,
            custom: self.custom_mean + self.custom_beta * d,
            recession: c.recession,
        }
    }
}

/// `(1 + r) * k - 1`, returning `r` itself (bit for bit) when there is no adjustment,
/// so that an unadjusted historical path reproduces the data exactly.
#[inline(always)]
fn scaled(r: f64, k: f64) -> f64 {
    if k == 1.0 {
        r
    } else {
        (1.0 + r) * k - 1.0
    }
}

fn json_f64(root: &Value, pointer: &str, fallback: f64) -> f64 {
    root.pointer(pointer)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(fallback)
}

impl CryptoSpec {
    fn from_json(actuarial: &Value) -> CryptoSpec {
        let preset = actuarial
            .pointer("/crypto/model/default_preset")
            .and_then(Value::as_str)
            .unwrap_or("neutral");
        CryptoSpec {
            sigma_annual: json_f64(actuarial, "/crypto/model/sigma_annual", CRYPTO_SIGMA_ANNUAL)
                .max(0.0),
            dof: json_f64(
                actuarial,
                "/crypto/model/student_t_df",
                f64::from(CRYPTO_T_DOF),
            ) as u32,
            equity_corr: json_f64(
                actuarial,
                "/crypto/model/correlation_with_equities",
                CRYPTO_EQUITY_CORR,
            )
            .clamp(-1.0, 1.0),
            drawdown_corr: json_f64(
                actuarial,
                "/crypto/model/correlation_in_equity_drawdowns",
                CRYPTO_DRAWDOWN_CORR,
            )
            .clamp(-1.0, 1.0),
            ruin_annual: json_f64(
                actuarial,
                "/crypto/model/ruin_hazard_annual/bitcoin_or_ether",
                CRYPTO_RUIN_ANNUAL,
            )
            .clamp(0.0, 1.0),
            expected_real_return: json_f64(
                actuarial,
                &format!("/crypto/model/expected_real_return_presets/{preset}"),
                CRYPTO_EXPECTED_REAL_RETURN,
            )
            .max(-0.99),
        }
    }
}

fn stock_presets_from_json(actuarial: &Value) -> [(f64, f64); 3] {
    let mut out = STOCK_PRESETS;
    for (slot, name) in
        out.iter_mut()
            .zip(["large_cap", "typical_tech_or_growth", "small_or_recent_ipo"])
    {
        let base = format!("/equity_compensation/single_public_stock/defaults/{name}");
        slot.0 = json_f64(actuarial, &format!("{base}/sigma_idio"), slot.0);
        slot.1 = json_f64(actuarial, &format!("{base}/ruin_hazard"), slot.1);
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Annual hazard of a total loss for a stock with the given idiosyncratic volatility:
/// piecewise linear through the origin and the presets, flat beyond the last.
fn stock_ruin_hazard(presets: &[(f64, f64); 3], idio_vol: f64) -> f64 {
    let mut prev = (0.0, 0.0);
    for &(vol, hazard) in presets {
        if idio_vol <= vol {
            let span = vol - prev.0;
            return if span > 0.0 {
                prev.1 + (hazard - prev.1) * (idio_vol - prev.0) / span
            } else {
                hazard
            };
        }
        prev = (vol, hazard);
    }
    prev.1
}

/// A probability as a threshold for comparing with 32 random bits.
fn probability_bits(p: f64) -> u64 {
    (p.clamp(0.0, 1.0) * TWO_POW_32) as u64
}

/// Converts an annual probability to the monthly probability with the same annual
/// survival: `1 - (1 - p)^(1/12)`.
fn monthly_hazard(annual: f64) -> f64 {
    let p = annual.clamp(0.0, 1.0);
    if p >= 1.0 {
        1.0
    } else {
        -math::exp_m1(math::ln_1p(-p) / 12.0)
    }
}

impl HistBase {
    /// `rescale`: set the long-run level of international equities and gold by
    /// assumption (the resampling models). The historical replay passes false and
    /// keeps the record as it was.
    fn new(h: &History, first_row: usize, actuarial: &Value, rescale: bool) -> HistBase {
        assert!(!h.is_empty(), "economy: the market history is empty");
        let n = h.len();
        let home = regime::forward_fill(&h.home);

        // Factor models for the short columns, on their own samples.
        let fit_short = |series: &[f64]| -> ([f64; 4], f64, f64) {
            let (mut x, mut y, mut x1) = (Vec::new(), Vec::new(), Vec::new());
            for t in 0..n {
                if series[t].is_finite() {
                    x.push([1.0, h.equity_us[t], h.bond10[t], h.cpi[t]]);
                    x1.push([1.0, h.equity_us[t]]);
                    y.push(series[t]);
                }
            }
            if y.len() < 24 {
                // No usable sample: stand in with US equities' own moments.
                return ([0.0, 1.0, 0.0, 0.0], 0.0, 1.0);
            }
            let (coef, sd) = fit::ols(&x, &y);
            let (simple, _) = fit::ols(&x1, &y);
            (coef, sd, simple[1])
        };
        let (intl_coef, intl_res, intl_beta) = fit_short(&h.equity_intl);
        let (gold_coef, gold_res, gold_beta) = fit_short(&h.gold);

        let first = first_row.min(n - 1);
        let spread = regime::mortgage_spread(h);
        let mut rows = Vec::with_capacity(n - first);
        let mut intl_sd = Vec::with_capacity(n - first);
        let mut gold_sd = Vec::with_capacity(n - first);
        for t in first..n {
            let factors = [1.0, h.equity_us[t], h.bond10[t], h.cpi[t]];
            let fitted =
                |coef: &[f64; 4]| -> f64 { coef.iter().zip(&factors).map(|(a, b)| a * b).sum() };
            let short = |value: f64, coef: &[f64; 4], sd: f64| {
                if value.is_finite() {
                    (value, 0.0)
                } else {
                    (fitted(coef), sd)
                }
            };
            let (intl, isd) = short(h.equity_intl[t], &intl_coef, intl_res);
            let (gold, gsd) = short(h.gold[t], &gold_coef, gold_res);
            let long_yield = h.long_yield[t];
            rows.push(Core {
                eq: h.equity_us[t],
                intl,
                bond: h.bond10[t],
                cash: h.tbill[t],
                cpi: h.cpi[t],
                home: home[t],
                gold,
                long_yield,
                mortgage: if h.mortgage30[t].is_finite() {
                    h.mortgage30[t]
                } else {
                    long_yield + spread
                },
                recession: h.recession[t],
            });
            intl_sd.push(isd);
            gold_sd.push(gsd);
        }
        // International equities and gold: keep each month's variation, set the
        // long-run level by assumption. The series are short (1975 and 1968) and
        // partly synthesised, so their own means are not comparable with a
        // full-sample US mean or full-sample inflation: scale every month's gross
        // return by one constant so that the geometric mean over the rows used is
        // the US one plus the stated spread (international) or inflation plus the
        // stated real return (gold).
        let long_run = long_run_from_json(actuarial);
        let mut short_scale = (1.0, 1.0);
        if rescale {
            let count = rows.len().max(1) as f64;
            let (mut us, mut intl, mut gold, mut cpi) = (0.0, 0.0, 0.0, 0.0);
            for (r, (isd, gsd)) in rows.iter().zip(intl_sd.iter().zip(&gold_sd)) {
                us += math::ln_1p(r.eq);
                cpi += math::ln_1p(r.cpi);
                // E ln(1 + r + s e) is about ln(1 + r) - s^2 / (2 (1 + r)^2).
                let gi = 1.0 + r.intl;
                let gg = 1.0 + r.gold;
                intl += math::ln(gi) - 0.5 * isd * isd / (gi * gi);
                gold += math::ln(gg) - 0.5 * gsd * gsd / (gg * gg);
            }
            let k_intl = math::exp((us - intl) / count + math::ln_1p(long_run.intl_spread) / 12.0);
            let k_gold = math::exp((cpi - gold) / count + math::ln_1p(long_run.gold_real) / 12.0);
            for (r, (isd, gsd)) in rows.iter_mut().zip(intl_sd.iter_mut().zip(&mut gold_sd)) {
                r.intl = (1.0 + r.intl) * k_intl - 1.0;
                r.gold = (1.0 + r.gold) * k_gold - 1.0;
                *isd *= k_intl;
                *gsd *= k_gold;
            }
            short_scale = (k_intl, k_gold);
        }
        let rate_now = rows
            .last()
            .map_or((0.0, 0.0), |r| (r.long_yield, r.mortgage));
        let rate_mean = (
            fit::mean_sd(rows.iter().map(|r| r.long_yield)).0,
            fit::mean_sd(rows.iter().map(|r| r.mortgage)).0,
        );
        let (eq_mean, eq_sd, _) = fit::mean_sd(rows.iter().map(|r| r.eq));
        let home_real_mean = fit::mean_sd(rows.iter().map(|r| r.home - r.cpi)).0;

        // Real returns in the 1973-81 stagflation, for the stress overlay.
        let mut stagflation_real = STAGFLATION_REAL_FALLBACK
            .map(math::annual_to_monthly)
            .map(|r| 1.0 + r);
        if let (Some(a), Some(b)) = (
            h.index_of(STAGFLATION_WINDOW.0),
            h.index_of(STAGFLATION_WINDOW.1),
        ) {
            let months = (b - a + 1) as f64;
            let mut logs = [0.0; 3];
            for t in a..=b {
                let deflator = math::ln_1p(h.cpi[t]);
                logs[0] += math::ln_1p(h.equity_us[t]) - deflator;
                logs[1] += math::ln_1p(h.bond10[t]) - deflator;
                logs[2] += math::ln_1p(h.tbill[t]) - deflator;
            }
            stagflation_real = logs.map(|l| math::exp(l / months));
        }

        HistBase {
            rows,
            intl_sd,
            gold_sd,
            eq_mean,
            eq_sd,
            home_real_mean,
            intl_fit: (intl_coef, intl_res),
            gold_fit: (gold_coef, gold_res),
            intl_beta,
            gold_beta,
            stagflation_real,
            crypto: CryptoSpec::from_json(actuarial),
            stock_presets: stock_presets_from_json(actuarial),
            rate_now,
            rate_mean,
            short_scale,
        }
    }

    /// The crypto equity-link factor for an unadjusted equity return.
    fn crypto_link(&self, beta: f64, beta_extra: f64, eq: f64) -> f64 {
        let d = eq - self.eq_mean;
        (1.0 + beta * d + beta_extra * (d + CRYPTO_DRAWDOWN_SIGMAS * self.eq_sd).min(0.0))
            .max(CRYPTO_LINK_FLOOR)
    }
}

/// Paths and months of the simulation that estimates the regime model's mean real
/// equity return for the valuation overlay (a standard error of about 0.05 points
/// a year).
const REGIME_ANCHOR_PATHS: u64 = 2000;
const REGIME_ANCHOR_MONTHS: usize = 720;

/// The regime model's mean monthly real log equity return before adjustments,
/// computed once per process for the embedded data.
fn regime_equity_anchor(p: &RegimeParams, embedded: bool) -> f64 {
    static EMBEDDED: OnceLock<f64> = OnceLock::new();
    let compute = || p.real_equity_log_mean(REGIME_ANCHOR_PATHS, REGIME_ANCHOR_MONTHS);
    if embedded {
        *EMBEDDED.get_or_init(compute)
    } else {
        compute()
    }
}

/// The bond duration and convexity tables, built once per process.
fn bond_pricer() -> Arc<bonds::BondPricer> {
    static TABLES: OnceLock<Arc<bonds::BondPricer>> = OnceLock::new();
    TABLES
        .get_or_init(|| Arc::new(bonds::BondPricer::new()))
        .clone()
}

/// The regime calibration of the embedded history, computed once per process.
fn regime_params(data: &Data) -> Arc<RegimeParams> {
    static EMBEDDED: OnceLock<Arc<RegimeParams>> = OnceLock::new();
    let long_run = long_run_from_json(&data.actuarial);
    if std::ptr::eq(data, Data::embedded()) {
        EMBEDDED
            .get_or_init(|| Arc::new(RegimeParams::calibrate(&data.history, &long_run)))
            .clone()
    } else {
        Arc::new(RegimeParams::calibrate(&data.history, &long_run))
    }
}

/// The long-run assumptions for the short series (`long_run_assumptions` in
/// `data/actuarial/params.json`).
fn long_run_from_json(actuarial: &Value) -> LongRun {
    LongRun {
        intl_spread: json_f64(
            actuarial,
            "/long_run_assumptions/international_equity_return_vs_us",
            INTL_SPREAD_VS_US,
        )
        .clamp(-0.2, 0.2),
        gold_real: json_f64(
            actuarial,
            "/long_run_assumptions/gold_real_return",
            GOLD_REAL_RETURN,
        )
        .clamp(-0.2, 0.2),
    }
}

/// Shortest resampling window, in years, that gets no warning.
pub const SHORT_HISTORY_YEARS: i32 = 50;

/// A warning for a bootstrap `from_year` that leaves less than
/// [`SHORT_HISTORY_YEARS`] of history to resample: what the window actually is and
/// what it returned against the full record. `None` when the window is long enough.
pub fn short_history_warning(from_year: i32) -> Option<String> {
    let h = &Data::embedded().history;
    let n = h.len();
    if n == 0 {
        return None;
    }
    let last_year = (h.start + (n as i32 - 1)).year();
    if from_year <= last_year - SHORT_HISTORY_YEARS {
        return None;
    }
    let offset =
        (i64::from(from_year) - i64::from(h.start.year())) * 12 - (i64::from(h.start.month()) - 1);
    let first = usize::try_from(offset)
        .unwrap_or(0)
        .min(n.saturating_sub(MIN_BOOTSTRAP_MONTHS));
    let real = |series: &[f64], from: usize| -> f64 {
        let mut log = 0.0;
        for t in from..n {
            log += math::ln_1p(series[t]) - math::ln_1p(h.cpi[t]);
        }
        (math::exp(log * 12.0 / (n - from) as f64) - 1.0) * 100.0
    };
    let start = h.start + first as i32;
    Some(format!(
        "Resampling history from {start} only ({:.0} years). Over that window US stocks returned {:.1}% a year after inflation and bonds {:.1}%, against {:.1}% and {:.1}% over the whole record from {}: results will look like that period, not like history. Use at least {SHORT_HISTORY_YEARS} years.",
        (n - first) as f64 / 12.0,
        real(&h.equity_us, first),
        real(&h.bond10, first),
        real(&h.equity_us, 0),
        real(&h.bond10, 0),
        h.start.year(),
    ))
}

impl EconomyModel {
    /// Builds the generator for a market model: slices and aligns history for the
    /// bootstrap, fits the regime or lognormal parameters from the history, fits the
    /// factor models of the short columns, and sets up the derived asset classes with
    /// the library defaults for single stock and custom asset
    /// (`SingleStock::default()`, `CustomAsset::default()`).
    pub fn new(market: &MarketModel, data: &Data) -> EconomyModel {
        let h = &data.history;
        let first_row = match market.kind {
            MarketKind::Bootstrap {
                from_year: Some(year),
                ..
            } => {
                // Months from the first row to January of `year`, without building a
                // `YearMonth` (any i32 is accepted here; validation reports odd ones).
                let offset = (i64::from(year) - i64::from(h.start.year())) * 12
                    - (i64::from(h.start.month()) - 1);
                let latest = h.len().saturating_sub(MIN_BOOTSTRAP_MONTHS);
                usize::try_from(offset).unwrap_or(0).min(latest)
            }
            _ => 0,
        };
        let base = Arc::new(HistBase::new(
            h,
            first_row,
            &data.actuarial,
            !matches!(market.kind, MarketKind::Historical),
        ));
        let kind = match market.kind {
            MarketKind::Bootstrap {
                mean_block_months, ..
            } => Kind::Bootstrap {
                restart: probability_bits(1.0 / f64::from(mean_block_months.max(1))),
            },
            MarketKind::Historical => Kind::Historical,
            MarketKind::Regime => Kind::Regime(regime_params(data)),
            MarketKind::Lognormal => Kind::Lognormal(Arc::new(LognormalParams::calibrate(
                h,
                &long_run_from_json(&data.actuarial),
            ))),
            MarketKind::FatTailed => {
                Kind::FatTailed(Arc::new(fat_tailed::FatTailedParams::calibrate(
                    LognormalParams::calibrate(h, &long_run_from_json(&data.actuarial)),
                    h,
                )))
            }
        };
        let adjust = Adjust::new(market);

        // Crypto: equity link sized for the two correlations, idiosyncratic volatility
        // for the rest of the total, median real growth as assumed.
        let spec = base.crypto;
        let sigma_c = spec.sigma_annual;
        let eq_vol = (base.eq_sd * SQRT_12).max(1e-6);
        let crypto_beta = spec.equity_corr * sigma_c / eq_vol;
        let crypto_beta_extra = (spec.drawdown_corr - spec.equity_corr).max(0.0) * sigma_c / eq_vol;
        let (link_mean, link_sd, _) = fit::mean_sd(
            base.rows
                .iter()
                .map(|r| math::ln(base.crypto_link(crypto_beta, crypto_beta_extra, r.eq))),
        );
        let total_var = sigma_c * sigma_c / 12.0;
        let idio_var = (total_var - link_sd * link_sd).max(CRYPTO_MIN_IDIO_SHARE * total_var);
        let idio_sd = idio_var.sqrt();
        let crypto_shock = Arc::new(sampler::student_t(spec.dof).transformed(|x| {
            math::exp(idio_sd * x.clamp(-CRYPTO_SHOCK_CAP_SD, CRYPTO_SHOCK_CAP_SD))
        }));
        // The preset fixes the expected return, not the median: the mean of the
        // equity link, the mean of the fat-tailed shock and the chance of a collapse
        // are all divided out, so that the average real return of the sleeve is the
        // stated one. With 70% volatility the typical path then loses most of its
        // value; a few gain enormously; on average the holding keeps what the
        // preset says.
        let link_arithmetic = fit::mean_sd(
            base.rows
                .iter()
                .map(|r| base.crypto_link(crypto_beta, crypto_beta_extra, r.eq)),
        )
        .0;
        let crypto_ruin = probability_bits(monthly_hazard(spec.ruin_annual));
        let survive = 1.0 - crypto_ruin as f64 / TWO_POW_32;
        let _ = link_mean;
        let crypto_norm = math::pow(1.0 + spec.expected_real_return, 1.0 / 12.0)
            / (link_arithmetic * crypto_shock.mean() * survive).max(1e-9);

        let assets = AssetParams {
            eq_mean: (1.0 + base.eq_mean) * adjust.equity * adjust.inflation - 1.0,
            re_beta_eq: REIT_EQUITY_BETA,
            re_beta_home: REIT_HOME_BETA,
            home_real_mean: base.home_real_mean,
            re_sigma: REIT_IDIOSYNCRATIC_VOL / SQRT_12,
            stock_beta: 1.0,
            stock_survivor_boost: 1.0,
            stock_ruin: 0,
            crypto_beta,
            crypto_beta_extra,
            crypto_threshold: CRYPTO_DRAWDOWN_SIGMAS * base.eq_sd,
            crypto_norm,
            crypto_ruin,
            custom_mean: 0.0,
            custom_beta: 0.0,
            custom_sigma: 0.0,
        };
        let model = EconomyModel {
            kind,
            base,
            adjust,
            assets,
            rows: Arc::from(Vec::new()),
            stock_shock: Arc::new(ShockTable::constant(1.0)),
            crypto_shock,
            stress_beta: [0.0; N_ASSETS],
            bonds: bond_pricer(),
            bonds_from_yield: matches!(market.kind, MarketKind::Bootstrap { .. })
                && market.bond_pricing == BondPricing::FromYield,
            valuation: None,
            episodes: None,
            dividends: None,
        };
        let mut model = model.with_asset_params(&SingleStock::default(), &CustomAsset::default());
        if market
            .stress
            .iter()
            .any(|s| matches!(s, crate::model::Stress::HistoricalEpisode { .. }))
        {
            model.episodes = Some(Arc::new(episodes::EpisodeTables::new(h)));
        }
        if let Some(v) = &market.valuation {
            let embedded = std::ptr::eq(data, Data::embedded());
            model.valuation = model
                .real_equity_log_mean(embedded)
                .and_then(|m| valuation::ValuationTilt::new(v, m));
        }
        if market.dynamic_yields {
            model.dividends = Some(model.dividend_model(h));
        }
        model
    }

    /// Mean monthly real log return of US equities in this model, adjustments
    /// included: the anchor of the valuation overlay. `None` for the historical
    /// replay, which the overlay does not apply to.
    fn real_equity_log_mean(&self, data_is_embedded: bool) -> Option<f64> {
        let adjust = math::ln(self.adjust.equity);
        match &self.kind {
            Kind::Bootstrap { .. } => {
                let n = self.rows.len().max(1) as f64;
                let sum: f64 = self
                    .rows
                    .iter()
                    .map(|r| math::ln_1p(r.m.eq) - math::ln_1p(r.m.cpi))
                    .sum();
                Some(sum / n)
            }
            Kind::Historical => None,
            Kind::Regime(p) => Some(regime_equity_anchor(p, data_is_embedded) + adjust),
            Kind::Lognormal(p) => Some(p.real_equity_log_mean() + adjust),
            Kind::FatTailed(p) => Some(p.ln.real_equity_log_mean() + adjust),
        }
    }

    /// `(starting CAPE, normal CAPE, expected annual shortfall of the US equity log
    /// return over the first ten years)` when the valuation overlay is on.
    pub fn valuation_outlook(&self) -> Option<(f64, f64, f64)> {
        self.valuation
            .map(|v| (v.starting_cape, v.fair_cape, v.expected_tilt_annual(120)))
    }

    /// Whether this is the fat-tailed parametric model.
    pub fn is_fat_tailed(&self) -> bool {
        matches!(self.kind, Kind::FatTailed(_))
    }

    /// Applies the profile's single-stock and custom-asset parameters. Call after
    /// [`EconomyModel::new`]; without it the defaults of those types are used. Only
    /// the scaling of those two asset classes changes: the random draws, and every
    /// other series, are identical.
    pub fn with_assets(self, portfolio: &PortfolioPolicy) -> EconomyModel {
        let custom = portfolio.custom_asset.clone().unwrap_or_default();
        self.with_asset_params(&portfolio.single_stock, &custom)
    }

    fn with_asset_params(mut self, stock: &SingleStock, custom: &CustomAsset) -> EconomyModel {
        let finite = |x: f64, fallback: f64| if x.is_finite() { x } else { fallback };
        let eq_vol = (self.base.eq_sd * SQRT_12).max(1e-6);

        // Single stock: total volatility = market part + idiosyncratic lognormal.
        let beta = finite(stock.beta, 1.0);
        let total_vol = finite(stock.volatility, 0.0).max(0.0);
        let idio_vol = (total_vol * total_vol - beta * beta * eq_vol * eq_vol)
            .max(0.0)
            .sqrt();
        let ruin = monthly_hazard(stock_ruin_hazard(&self.base.stock_presets, idio_vol));
        let sigma = idio_vol / SQRT_12;
        self.stock_shock = Arc::new(if sigma > 0.0 {
            let mut table = sampler::normal().transformed(|x| math::exp(sigma * x));
            let k = 1.0 / table.mean();
            table.scale(k);
            table
        } else {
            ShockTable::constant(1.0)
        });
        self.assets.stock_beta = beta;
        self.assets.stock_ruin = probability_bits(ruin);
        // Use the probability actually applied (after rounding to 32 bits) so the
        // expected return is preserved exactly.
        self.assets.stock_survivor_boost = 1.0 / (1.0 - self.assets.stock_ruin as f64 / TWO_POW_32);

        // Custom asset.
        let corr = finite(custom.equity_corr, 0.0).clamp(-1.0, 1.0);
        let vol = finite(custom.volatility, 0.0).max(0.0);
        self.assets.custom_mean = (1.0
            + math::annual_to_monthly(finite(custom.mean, 0.0).max(-0.99)))
            * self.adjust.inflation
            - 1.0;
        self.assets.custom_beta = corr * vol / eq_vol;
        self.assets.custom_sigma = vol * (1.0 - corr * corr).sqrt() / SQRT_12;

        self.stress_beta = [0.0; N_ASSETS];
        self.stress_beta[AssetClass::UsStocks.index()] = 1.0;
        self.stress_beta[AssetClass::IntlStocks.index()] = self.base.intl_beta;
        self.stress_beta[AssetClass::RealEstate.index()] = self.assets.re_beta_eq;
        self.stress_beta[AssetClass::Gold.index()] = self.base.gold_beta;
        self.stress_beta[AssetClass::Crypto.index()] =
            self.assets.crypto_beta + self.assets.crypto_beta_extra;
        self.stress_beta[AssetClass::SingleStock.index()] = beta;
        self.stress_beta[AssetClass::Custom.index()] = self.assets.custom_beta;

        if matches!(self.kind, Kind::Bootstrap { .. } | Kind::Historical) {
            let ke = self.adjust.equity * self.adjust.inflation;
            let ki = self.adjust.inflation;
            let rows: Vec<Row> = self
                .base
                .rows
                .iter()
                .zip(self.base.intl_sd.iter().zip(&self.base.gold_sd))
                .map(|(core, (&isd, &gsd))| Row {
                    m: self.assets.month(&self.adjust, core),
                    intl_sd: isd * ke,
                    gold_sd: gsd * ki,
                    d_long: 0.0,
                    d_mortgage: 0.0,
                })
                .collect();
            let mut rows = rows;
            for i in 1..rows.len() {
                rows[i].d_long = self.base.rows[i].long_yield - self.base.rows[i - 1].long_yield;
                rows[i].d_mortgage = self.base.rows[i].mortgage - self.base.rows[i - 1].mortgage;
            }
            self.rows = rows.into();
        }
        self
    }

    /// Number of distinct start months available in `historical` mode (0 for other
    /// kinds). A run with more paths than cohorts wraps around.
    pub fn cohort_count(&self) -> usize {
        match self.kind {
            Kind::Historical => self.rows.len(),
            _ => 0,
        }
    }

    /// Fills `out` with `months` months. `path_index` selects the cohort in
    /// `historical` mode and is otherwise unused (randomness comes from `streams`).
    /// Allocation-free once `out` has capacity for `months`.
    ///
    /// Draws per month from the `Market` stream: bootstrap 8, historical 7, regime
    /// 17, lognormal and fat-tailed 12 (plus one at the start of a path). From the `Inflation` stream: 1 in the regime, lognormal
    /// and fat-tailed models, none otherwise. From `MarketTails`: 3 in the fat-tailed model,
    /// none otherwise.
    pub fn generate(
        &self,
        streams: &mut EconomyStreams<'_>,
        path_index: u64,
        months: usize,
        out: &mut EconomyPath,
    ) {
        out.prepare(months);
        let emit = Emit {
            assets: &self.assets,
            normal: &sampler::normal().table,
            t6: &sampler::student_t6().table,
            stock: &self.stock_shock,
            crypto: &self.crypto_shock,
        };
        let mut sink = Sink::new(out);
        // The generators run on copies of the streams, written back at the end: the
        // state of a stream behind a `&mut` would otherwise be stored to memory after
        // every draw (a bounds-check panic could observe it).
        let mut market = streams.market.clone();
        let mut inflation = streams.inflation.clone();
        let rng = &mut market;
        match &self.kind {
            Kind::Bootstrap { restart } => {
                let rows = &self.rows[..];
                let n = rows.len();
                let mut i = 0usize;
                // Returns are resampled; rate levels are not. A block that begins
                // in 1981 brings 1981's returns, not a 17% mortgage rate overnight:
                // the long yield and the mortgage rate start from today's values and
                // move by each sampled month's own change, with a weak pull toward
                // their long-run means that keeps them bounded.
                let shift = self.adjust.inflation_annual;
                let (mut long, mut mortgage) = self.base.rate_now;
                let (long_mean, mortgage_mean) = self.base.rate_mean;
                for t in 0..months {
                    let bits = rng.next_u64();
                    if t == 0 || (bits >> 32) < *restart {
                        i = (((bits & MASK_32) * n as u64) >> 32) as usize;
                    } else {
                        i += 1;
                        if i == n {
                            i = 0;
                        }
                    }
                    let row = &rows[i];
                    if t > 0 {
                        long = (long + row.d_long + RATE_REVERSION * (long_mean - long))
                            .max(MIN_LONG_YIELD);
                        mortgage = (mortgage
                            + row.d_mortgage
                            + RATE_REVERSION * (mortgage_mean - mortgage))
                            .clamp(
                                long + MORTGAGE_SPREAD_RANGE.0,
                                long + MORTGAGE_SPREAD_RANGE.1,
                            );
                    }
                    self.put_row(&emit, &mut sink, t, row, rng);
                    sink.long_yield[t] = long + shift;
                    sink.mortgage[t] = mortgage + shift;
                }
            }
            Kind::Historical => {
                let rows = &self.rows[..];
                let n = rows.len();
                let mut i = (path_index % n as u64) as usize;
                for t in 0..months {
                    self.put_row(&emit, &mut sink, t, &rows[i], rng);
                    i += 1;
                    if i == n {
                        i = 0;
                    }
                }
            }
            Kind::Regime(p) => {
                let mut state = p.start;
                for t in 0..months {
                    let core = p.step(&mut state, rng, &mut inflation, emit.normal);
                    let m = self.assets.month(&self.adjust, &core);
                    emit.put(&mut sink, t, &m, m.intl, m.gold, rng);
                }
            }
            Kind::Lognormal(p) => {
                let mut recession = u8::from(rng.uniform() < p.recession_share());
                for t in 0..months {
                    let core = p.step(&mut recession, rng, &mut inflation, emit.normal);
                    let m = self.assets.month(&self.adjust, &core);
                    emit.put(&mut sink, t, &m, m.intl, m.gold, rng);
                }
            }
            Kind::FatTailed(p) => {
                let mut state = p.start(rng);
                for t in 0..months {
                    let core = p.step(&mut state, rng, &mut inflation, streams.tails, emit.normal);
                    let m = self.assets.month(&self.adjust, &core);
                    emit.put(&mut sink, t, &m, m.intl, m.gold, rng);
                }
            }
        }
        *streams.market = market;
        *streams.inflation = inflation;
        if self.bonds_from_yield {
            // Block bootstrap only. The path starts at today's yield, so month 0
            // earns its carry and no price change.
            let y_before = self.base.rate_now.0 + self.adjust.inflation_annual;
            self.bonds.reprice(out, 0, months, y_before);
        }
        if let Some(v) = &self.valuation {
            v.apply(out, &self.stress_beta);
        }
        self.refresh_dividends(out);
    }

    /// The dividend model of `market.dynamic_yields`: the long-run mean monthly
    /// log returns of US and international stocks (the record's rows with this
    /// model's adjustments) and the record's share of recession months.
    fn dividend_model(&self, h: &crate::data::History) -> income_yields::DividendModel {
        let n = self.base.rows.len().max(1) as f64;
        let adjust = math::ln(self.adjust.equity) + math::ln(self.adjust.inflation);
        let mean = |f: fn(&Core) -> f64| -> f64 {
            self.base
                .rows
                .iter()
                .map(|r| math::ln_1p(f(r)))
                .sum::<f64>()
                / n
                + adjust
        };
        let share = h.recession.iter().map(|&r| f64::from(r)).sum::<f64>()
            / h.recession.len().max(1) as f64;
        income_yields::DividendModel::new(
            [mean(|r| r.eq), mean(|r| r.intl)],
            share,
            self.valuation.map(|v| v.gap0),
        )
    }

    /// Recomputes the path's dividend yields after its returns changed (generation
    /// or a stress overlay); clears them when `dynamic_yields` is off.
    #[inline]
    pub(super) fn refresh_dividends(&self, out: &mut EconomyPath) {
        match &self.dividends {
            Some(d) => d.fill(out),
            None => out.dividend_yields.clear(),
        }
    }

    /// Emits one table row: the residuals of the synthesised columns (2 draws, used
    /// only where the row lacks the real value), then the derived assets.
    #[inline(always)]
    fn put_row(&self, emit: &Emit<'_>, sink: &mut Sink<'_>, t: usize, row: &Row, rng: &mut Rng) {
        let intl = (row.m.intl + row.intl_sd * emit.t6.sample(rng.next_u64())).max(MIN_RETURN);
        let gold = (row.m.gold + row.gold_sd * emit.t6.sample(rng.next_u64())).max(MIN_RETURN);
        emit.put(sink, t, &row.m, intl, gold, rng);
    }
}
