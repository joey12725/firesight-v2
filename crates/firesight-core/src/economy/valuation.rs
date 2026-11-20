//! Valuation-aware expected equity returns (`market.valuation`).
//!
//! The state is the log gap between the CAPE and its normal level,
//! `v = ln(CAPE / fair)`, starting at `ln(starting_cape / fair_cape)`. Earnings
//! (a ten-year average) barely move in a month, so a month's price surprise moves
//! the CAPE one for one, and the gap closes at a monthly rate `k` set by the
//! half-life (Campbell and Shiller's finding that valuation ratios revert mostly
//! through prices):
//!
//! ```text
//! tilt_t = -k v_{t-1}                                   (log return added in month t)
//! 1 + r'_t = (1 + r_t) exp(tilt_t)
//! v_t = v_{t-1} + ln(1 + r'_t) - ln(1 + cpi_t) - m     = (1 - k) v_{t-1} + e_t
//! ```
//!
//! where `r_t` is the model's own US equity return of the month, untouched, and `m`
//! the model's mean monthly real log equity return, so `e_t` is the month's real
//! surprise. `v` is then a stationary AR(1); with `v_0 = 0` the tilt has mean zero
//! and the long-run mean return of the model is kept, and from an expensive start
//! the expected cumulative log return falls short by `v_0 (1 - (1 - k)^t)`, which
//! tends to `v_0`: the level lost while valuations normalise is not made up.
//!
//! The tilt is applied to US equities; listed real estate, the single stock and the
//! custom asset move by their equity beta (as under a stress overlay). International
//! equities, whose valuations are not those of the US market, gold and crypto are
//! left alone. No random numbers are drawn: the overlay rewrites the generated path
//! before any stress overlay is applied.

use super::stress::shifted;
use super::EconomyPath;
use crate::math;
use crate::model::{AssetClass, Valuation, N_ASSETS};

/// Asset classes that follow the US equity tilt through their equity beta.
const BETA_ASSETS: [AssetClass; 3] = [
    AssetClass::RealEstate,
    AssetClass::SingleStock,
    AssetClass::Custom,
];

/// The compiled valuation overlay of one market model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ValuationTilt {
    /// `ln(starting_cape / fair_cape)`.
    pub(super) gap0: f64,
    /// `1 - k`: the share of the gap left after a month.
    pub(super) keep: f64,
    /// Mean monthly real log return of US equities in the model (`m`).
    pub(super) mean_real_log: f64,
    /// CAPE values, for annotations.
    pub(super) starting_cape: f64,
    pub(super) fair_cape: f64,
}

impl ValuationTilt {
    /// `None` for values that cannot be used (validation reports them).
    pub(super) fn new(v: &Valuation, mean_real_log: f64) -> Option<ValuationTilt> {
        let ok = |x: f64| x.is_finite() && x > 0.0;
        if !ok(v.starting_cape) || !ok(v.fair_cape) || !ok(v.half_life_years) {
            return None;
        }
        Some(ValuationTilt {
            gap0: math::ln(v.starting_cape / v.fair_cape),
            keep: math::pow(0.5, 1.0 / (12.0 * v.half_life_years)),
            mean_real_log,
            starting_cape: v.starting_cape,
            fair_cape: v.fair_cape,
        })
    }

    /// The expected shortfall (negative) or excess of the annual log return over
    /// the first `months`, from the starting gap alone.
    pub(super) fn expected_tilt_annual(&self, months: usize) -> f64 {
        let closed = 1.0 - math::powi(self.keep, months as i32);
        -self.gap0 * closed * 12.0 / months as f64
    }

    /// Rewrites US equities (and the beta assets) of every month of `out` and
    /// fills `out.valuation_gap`. Draws nothing.
    pub(super) fn apply(&self, out: &mut EconomyPath, betas: &[f64; N_ASSETS]) {
        let n = out.months;
        out.valuation_gap.clear();
        out.valuation_gap.resize(n, 0.0);
        let us = AssetClass::UsStocks.index();
        let k = 1.0 - self.keep;
        let mut v = self.gap0;
        for t in 0..n {
            let r = out.returns[us][t];
            let tilt = -k * v;
            let new = (1.0 + r) * math::exp(tilt) - 1.0;
            let delta = new - r;
            out.returns[us][t] = new;
            for a in BETA_ASSETS {
                let i = a.index();
                out.returns[i][t] = shifted(out.returns[i][t], betas[i], delta);
            }
            v += math::ln_1p(new) - math::ln_1p(out.inflation[t]) - self.mean_real_log;
            out.valuation_gap[t] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tilt(start: f64, fair: f64, half: f64, m: f64) -> ValuationTilt {
        ValuationTilt::new(
            &Valuation {
                starting_cape: start,
                fair_cape: fair,
                half_life_years: half,
            },
            m,
        )
        .unwrap()
    }

    #[test]
    fn the_gap_halves_over_the_half_life_when_returns_are_average() {
        // A path whose real log return is exactly the mean every month.
        let m = 0.005;
        let v = tilt(40.0, 20.0, 10.0, m);
        assert!((v.gap0 - math::ln(2.0)).abs() < 1e-12);
        let months = 240;
        let mut out = EconomyPath::with_capacity(months);
        let g = math::exp(m) - 1.0;
        out.returns[AssetClass::UsStocks.index()].fill(g);
        out.returns[AssetClass::RealEstate.index()].fill(0.004);
        v.apply(&mut out, &[0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0, 0.0]);
        // CAPE 40 -> 28.28 after ten years -> 24.5 after twenty.
        let gap = &out.valuation_gap;
        assert!(
            (gap[119] - 0.5 * math::ln(2.0)).abs() < 1e-9,
            "{}",
            gap[119]
        );
        assert!((gap[239] - 0.25 * math::ln(2.0)).abs() < 1e-9);
        // The cumulative log return falls short of the mean by exactly the gap
        // closed: ln(40/20) - ln(28.28/20) = ln 2 / 2 over the first decade.
        let us = &out.returns[AssetClass::UsStocks.index()];
        let cum: f64 = us[..120].iter().map(|r| math::ln_1p(*r)).sum();
        assert!((cum - (120.0 * m - 0.5 * math::ln(2.0))).abs() < 1e-9);
        assert!((v.expected_tilt_annual(120) * 10.0 + 0.5 * math::ln(2.0)).abs() < 1e-12);
        // Beta assets move by beta times the change in the equity return.
        let re = &out.returns[AssetClass::RealEstate.index()];
        assert!((re[0] - (0.004 + 0.5 * (us[0] - g))).abs() < 1e-15);
        // International equities (beta not applied) are untouched.
        assert_eq!(out.returns[AssetClass::IntlStocks.index()][0], 0.0);
    }

    #[test]
    fn a_crash_lowers_the_valuation_and_raises_later_returns() {
        let m = 0.005;
        let v = tilt(20.0, 20.0, 15.0, m);
        let mut out = EconomyPath::with_capacity(3);
        let g = math::exp(m) - 1.0;
        out.returns[AssetClass::UsStocks.index()].copy_from_slice(&[-0.4, g, g]);
        v.apply(&mut out, &[0.0; N_ASSETS]);
        let us = &out.returns[AssetClass::UsStocks.index()];
        // Fairly valued: no tilt in the first month.
        assert_eq!(us[0], -0.4);
        let gap = out.valuation_gap[0];
        assert!((gap - (math::ln(0.6) - m)).abs() < 1e-12);
        // Afterwards the gap is negative and the tilt positive: k * |gap|.
        let k = 1.0 - v.keep;
        assert!((math::ln_1p(us[1]) - (m - k * gap)).abs() < 1e-12);
        assert!(us[1] > g);
    }

    #[test]
    fn a_collapsed_asset_stays_collapsed() {
        let v = tilt(40.0, 17.3, 15.0, 0.005);
        let mut out = EconomyPath::with_capacity(2);
        out.returns[AssetClass::SingleStock.index()].copy_from_slice(&[-1.0, 0.01]);
        v.apply(&mut out, &[0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 1.2, 0.0]);
        assert_eq!(out.returns[AssetClass::SingleStock.index()][0], -1.0);
        assert!(ValuationTilt::new(
            &Valuation {
                fair_cape: 0.0,
                ..Valuation::default()
            },
            0.0
        )
        .is_none());
    }
}
