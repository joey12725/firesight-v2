//! Joint economic scenario generator. **Interface stub: bodies marked `todo!()`.**
//!
//! Responsibility: for one path, produce every month's inflation, asset returns,
//! interest rates, national home-price change and recession flag, jointly, so that the
//! historical co-movement (stocks fall in recessions, bonds and mortgage rates follow
//! yields, inflation regimes persist) is preserved.
//!
//! Contract:
//!
//! * Output goes into a caller-owned [`EconomyPath`] (struct of arrays) that is reused
//!   across paths: `generate` must not allocate once the buffers have reached the
//!   horizon length.
//! * Draws come only from the `Market` and `Inflation` streams handed in. Nothing about
//!   the household may influence the draws, so two scenarios of the same profile see
//!   the same economy on the same path (common random numbers).
//! * Model kinds (`assumptions.market.kind`): stationary block bootstrap of
//!   `data.history` (default), regime switching, i.i.d. lognormal, and historical
//!   cohorts where path `i` replays history starting at month `i mod cohort_count`.
//! * Derived asset classes are built from factor exposures plus idiosyncratic noise:
//!   REITs, gold before 1968, international before 1975, crypto, the single stock
//!   (`beta` to US stocks, residual volatility), and the custom asset (`mean`,
//!   `volatility`, `equity_corr`).
//! * `equity_premium_adjustment` and `inflation_adjustment` shift monthly means after
//!   generation. Stress overlays are applied by [`EconomyModel::apply_stress`], which
//!   `sim` may call mid-path (a crash "at retirement" is only known when retirement
//!   triggers); an overlay may only rewrite months at or after its start.
//! * Calibration tests compare simulated moments with `data/history/calibration.json`.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::data::Data;
use crate::model::{AssetClass, MarketModel, PortfolioPolicy, N_ASSETS};
use crate::rng::Rng;

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
    /// Treasury bill yield, annualised. Cash accounts earn this plus their spread.
    pub tbill: Vec<f64>,
    /// 10-year Treasury yield, annualised.
    pub long_yield: Vec<f64>,
    /// 30-year fixed mortgage rate available to a new borrower, annualised.
    pub mortgage_rate: Vec<f64>,
    /// National home price index change during month `t`.
    pub home_national: Vec<f64>,
    /// 1 if month `t` is in a recession (drives layoff hazard), else 0.
    pub recession: Vec<u8>,
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

    /// Recomputes `price_level` from `inflation`. Generators and stress overlays call
    /// this after writing inflation.
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
}

/// A stress overlay with its dates resolved to month indices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResolvedStress {
    /// Equities fall by `drawdown` over the 12 months from `at_month`, then recover to
    /// the pre-crash level over `recovery_months`.
    Crash {
        at_month: usize,
        drawdown: f64,
        recovery_months: usize,
    },
    /// 120 months from `from_month` in which US and international stocks return zero
    /// after inflation in total.
    LostDecade { from_month: usize },
    /// `months` of inflation at `inflation` (annual) with stocks and bonds flat in
    /// real terms.
    Stagflation {
        from_month: usize,
        months: usize,
        inflation: f64,
    },
}

/// The compiled scenario generator for one profile. Built once in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct EconomyModel {
    _private: (),
}

impl EconomyModel {
    /// Builds the generator for a market model: slices and aligns history for the
    /// bootstrap, fits or loads regime parameters, precomputes factor loadings of the
    /// derived asset classes with library defaults for single stock and custom.
    pub fn new(market: &MarketModel, data: &Data) -> EconomyModel {
        todo!("economy owner")
    }

    /// Applies the profile's single-stock and custom-asset parameters. Call after
    /// [`EconomyModel::new`]; without it the defaults of those types are used.
    pub fn with_assets(self, portfolio: &PortfolioPolicy) -> EconomyModel {
        todo!("economy owner")
    }

    /// Number of distinct start months available in `historical` mode (0 for other
    /// kinds). A run with more paths than cohorts wraps around.
    pub fn cohort_count(&self) -> usize {
        todo!("economy owner")
    }

    /// Fills `out` with `months` months. `path_index` selects the cohort in
    /// `historical` mode and is otherwise unused (randomness comes from `streams`).
    /// Allocation-free once `out` has capacity for `months`.
    pub fn generate(
        &self,
        streams: &mut EconomyStreams<'_>,
        path_index: u64,
        months: usize,
        out: &mut EconomyPath,
    ) {
        todo!("economy owner")
    }

    /// Rewrites `out` from the overlay's start month onward. Deterministic: draws no
    /// random numbers. Safe to call mid-path for months not yet simulated.
    pub fn apply_stress(&self, stress: &ResolvedStress, out: &mut EconomyPath) {
        todo!("economy owner")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_buffers_reset_and_convert() {
        let mut p = EconomyPath::with_capacity(24);
        assert_eq!(p.price_level.len(), 25);
        p.inflation.iter_mut().for_each(|x| *x = 0.01);
        p.rebuild_price_level();
        assert!((p.price_level[2] - 1.0201).abs() < 1e-12);
        assert!((p.to_real(p.to_nominal(100.0, 12), 12) - 100.0).abs() < 1e-9);
        p.returns[AssetClass::Bonds.index()][3] = 0.02;
        assert_eq!(p.returns_at(3)[AssetClass::Bonds.index()], 0.02);
        assert_eq!(p.asset_return(AssetClass::Bonds, 3), 0.02);
        p.reset(12);
        assert_eq!((p.months, p.inflation.len(), p.inflation[0]), (12, 12, 0.0));
    }
}
