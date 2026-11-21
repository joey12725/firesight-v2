//! Dividend yields that move with the market (`market.dynamic_yields`).
//!
//! Off, US and international stocks distribute a constant [`DIVIDEND_YIELD`] of
//! their value. On, each one's yield is a state on the path, after Lintner (1956,
//! American Economic Review 46(2)): companies smooth dividends, so a fall in prices
//! raises the yield at once, and dividends then adjust part of the way toward their
//! target each year. Each month
//!
//! `ln y' = ln y - (ln(1 + r) - mu) + k (ln y* - ln y) - c [recession]`
//!
//! where `r` is the month's total return, `mu` the model's long-run mean monthly log
//! return (so only the surprise moves the yield), `k` the monthly pull of
//! [`DIVIDEND_ADJUSTMENT_ANNUAL`] a year, and `c` the monthly dividend cut of
//! [`RECESSION_DIVIDEND_CUT_ANNUAL`] a year in recession months. The target `y*` is
//! today's yield, raised by the long-run effect of the recession cuts so that the
//! average yield stays today's; with `market.valuation` the US target also moves
//! inversely with the CAPE gap, so as an expensive market returns to its normal
//! valuation the yield rises toward `today's yield x CAPE / normal CAPE`.
//!
//! Only the split of the total return between income and price changes: `returns`
//! is never touched. The single stock follows the US yield. Bonds and cash already
//! distribute the path's own 10-year and bill yields (`income_yields_at`); listed
//! real estate keeps its constant yield. No random numbers are drawn.

use super::{EconomyPath, DIVIDEND_YIELD};
use crate::math;
use crate::model::AssetClass;

/// Share of the gap between dividends and their target closed in a year. Judgement:
/// Lintner's partial-adjustment model has this form, and published speeds vary
/// widely across studies and markets; 0.3 a year is a middle value.
pub const DIVIDEND_ADJUSTMENT_ANNUAL: f64 = 0.3;
/// Annual rate at which dividends are cut during recession months, on top of the
/// adjustment to lower prices. Calibrated so that replaying 2007-11 to 2013-01 cuts
/// the US dividend run rate by about 21% from peak to trough, as S&P 500 dividends
/// per share fell from $28.39 in 2008 to $22.41 in 2009 (Shiller's data, as
/// tabulated by DQYDJ).
pub const RECESSION_DIVIDEND_CUT_ANNUAL: f64 = 0.10;
/// Bounds on a dividend yield. Judgement: they only stop a collapsed index from
/// producing an absurd yield.
pub const DIVIDEND_YIELD_RANGE: (f64, f64) = (0.002, 0.15);

const US: usize = 0;
const INTL: usize = 1;
const CLASSES: [AssetClass; 2] = [AssetClass::UsStocks, AssetClass::IntlStocks];

/// The compiled dividend model of one market model.
#[derive(Clone, Copy, Debug)]
pub(super) struct DividendModel {
    /// Long-run mean monthly log total return of US and international stocks.
    mu: [f64; 2],
    /// `ln` of the target yields before valuation.
    target: [f64; 2],
    /// Monthly pull.
    k: f64,
    /// Monthly log cut in a recession month.
    cut: f64,
    /// With `market.valuation`: log CAPE gap before month 0.
    gap0: Option<f64>,
}

impl DividendModel {
    /// `mu`: long-run mean monthly log total returns; `recession_share`: long-run
    /// share of recession months; `gap0`: the initial log CAPE gap when valuation is
    /// on.
    pub(super) fn new(mu: [f64; 2], recession_share: f64, gap0: Option<f64>) -> DividendModel {
        let k = 1.0 - math::pow(1.0 - DIVIDEND_ADJUSTMENT_ANNUAL, 1.0 / 12.0);
        let cut = -math::ln_1p(-RECESSION_DIVIDEND_CUT_ANNUAL) / 12.0;
        // Mean of ln y - ln y* is -cut * share / k; lift the target by as much.
        let lift = cut * recession_share / k;
        DividendModel {
            mu,
            target: CLASSES.map(|a| math::ln(DIVIDEND_YIELD[a.index()]) + lift),
            k,
            cut,
            gap0,
        }
    }

    /// Fills `out.dividend_yields` from the path's returns, recessions and (with
    /// valuation) CAPE gap. Allocation-free once the buffer has grown.
    pub(super) fn fill(&self, out: &mut EconomyPath) {
        let n = out.months;
        out.dividend_yields.clear();
        out.dividend_yields.resize(n, [0.0; 2]);
        let (lo, hi) = (
            math::ln(DIVIDEND_YIELD_RANGE.0),
            math::ln(DIVIDEND_YIELD_RANGE.1),
        );
        let mut ly = CLASSES.map(|a| math::ln(DIVIDEND_YIELD[a.index()]));
        let with_gap = self.gap0.is_some() && out.valuation_gap.len() >= n;
        let gap0 = self.gap0.unwrap_or(0.0);
        for t in 0..n {
            out.dividend_yields[t] = [math::exp(ly[US]), math::exp(ly[INTL])];
            let shift = if with_gap {
                let before = if t == 0 {
                    gap0
                } else {
                    out.valuation_gap[t - 1]
                };
                before - gap0
            } else {
                0.0
            };
            let recession = if out.recession[t] != 0 { self.cut } else { 0.0 };
            for (i, a) in CLASSES.iter().enumerate() {
                let g = 1.0 + out.returns[a.index()][t];
                let surprise = math::ln(g.max(1e-9)) - self.mu[i];
                let target = self.target[i] - if i == US { shift } else { 0.0 };
                ly[i] = (ly[i] - surprise + self.k * (target - ly[i]) - recession).clamp(lo, hi);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn parts(&self) -> (f64, f64, [f64; 2]) {
        (self.k, self.cut, self.target)
    }
}
