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
//!
//! A stress overlay then dictates US equity returns over its window (a crash and its
//! recovery, a lost decade, a replayed episode...). Inside the window no tilt is
//! applied and the gap moves with the stressed prices alone; after it, the tilt of
//! every later month is re-derived from the gap the stress left
//! ([`ValuationTilt::after_stress`]), so a crash lowers the CAPE and raises the
//! returns that follow instead of the path carrying on as if it had not happened.

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
        // Grow once to the workspace's capacity, so a longer later path does not
        // reallocate.
        out.valuation_gap.reserve(out.inflation.capacity().max(n));
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

    /// After a stress overlay rewrote months `from..end`: moves the gap with the
    /// stressed returns through the window (no tilt inside it) and replaces the
    /// tilt of every later month (the one the generated path got from the old gap)
    /// by the tilt of the new gap. Months before `from` are untouched. Draws
    /// nothing and allocates nothing.
    pub(super) fn after_stress(
        &self,
        out: &mut EconomyPath,
        from: usize,
        end: usize,
        betas: &[f64; N_ASSETS],
    ) {
        let n = out.months.min(out.valuation_gap.len());
        if from >= n {
            return;
        }
        let us = AssetClass::UsStocks.index();
        let k = 1.0 - self.keep;
        let gap_before = |t: usize, gaps: &[f64]| if t == 0 { self.gap0 } else { gaps[t - 1] };
        // The gap the generated path had at the start of each month, before this
        // month's entry is overwritten.
        let mut old_prev = gap_before(from, &out.valuation_gap);
        let mut v = old_prev;
        for t in from..n {
            let old_here = out.valuation_gap[t];
            if t >= end {
                // Undo the old tilt, apply the new one.
                let r = out.returns[us][t];
                let untilted = (1.0 + r) * math::exp(k * old_prev) - 1.0;
                let new = (1.0 + untilted) * math::exp(-k * v) - 1.0;
                let delta = new - r;
                out.returns[us][t] = new;
                for a in BETA_ASSETS {
                    let i = a.index();
                    out.returns[i][t] = shifted(out.returns[i][t], betas[i], delta);
                }
            }
            v += math::ln_1p(out.returns[us][t])
                - math::ln_1p(out.inflation[t])
                - self.mean_real_log;
            out.valuation_gap[t] = v;
            old_prev = old_here;
        }
    }
}

/// Months `from..end` over which a resolved stress dictates US equity returns, or
/// `None` when it changes nothing.
pub(super) fn stress_window(s: &super::ResolvedStress) -> Option<(usize, usize)> {
    use super::stress::{CRASH_MONTHS, LOST_DECADE_MONTHS};
    use super::ResolvedStress as R;
    Some(match *s {
        R::Crash {
            at_month,
            drawdown,
            recovery_months,
        } => {
            if !(drawdown.is_finite() && drawdown > 0.0) {
                return None;
            }
            let len = CRASH_MONTHS.saturating_add(recovery_months.max(1));
            (at_month, at_month.saturating_add(len))
        }
        R::LostDecade { from_month } => (from_month, from_month.saturating_add(LOST_DECADE_MONTHS)),
        R::Stagflation {
            from_month, months, ..
        }
        | R::LostGeneration { from_month, months } => {
            if months == 0 {
                return None;
            }
            (from_month, from_month.saturating_add(months))
        }
        R::HistoricalEpisode {
            from_month,
            episode,
        } => (
            from_month,
            from_month.saturating_add(super::episodes::episode_months(episode)),
        ),
    })
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
    fn a_stress_moves_the_gap_and_the_later_tilt() {
        let m = 0.005;
        let v = tilt(40.0, 20.0, 10.0, m);
        let k = 1.0 - v.keep;
        let g = math::exp(m) - 1.0;
        let us = AssetClass::UsStocks.index();
        let mut out = EconomyPath::with_capacity(40);
        out.returns[us].fill(g);
        v.apply(&mut out, &[0.0; N_ASSETS]);
        let before = out.clone();
        // With nothing rewritten, re-deriving the tilt changes nothing (to rounding).
        v.after_stress(&mut out, 10, 10, &[0.0; N_ASSETS]);
        for t in 0..40 {
            assert!((out.returns[us][t] - before.returns[us][t]).abs() < 1e-15);
            assert!((out.valuation_gap[t] - before.valuation_gap[t]).abs() < 1e-12);
        }
        // A stress sets months 10..16 to -10% each (zero inflation).
        out.returns[us][10..16].fill(-0.1);
        v.after_stress(&mut out, 10, 16, &[0.0; N_ASSETS]);
        assert_eq!(
            out.returns[us][..16],
            {
                let mut x = before.returns[us][..16].to_vec();
                x[10..16].fill(-0.1);
                x
            }[..]
        );
        // Inside the window the gap moves with prices only.
        let g15 = before.valuation_gap[9] + 6.0 * (math::ln(0.9) - m);
        assert!((out.valuation_gap[15] - g15).abs() < 1e-12);
        // After it, the month's log return is the mean less k times the new gap:
        // the crash took the gap from about +0.68 to about +0.02, so the drag nearly
        // vanishes.
        assert!((math::ln_1p(out.returns[us][16]) - (m - k * g15)).abs() < 1e-12);
        assert!(out.returns[us][16] > before.returns[us][16]);
        let g16 = (1.0 - k) * g15;
        assert!((out.valuation_gap[16] - g16).abs() < 1e-12);
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
