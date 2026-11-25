//! Bond returns priced from a yield path.
//!
//! The 10-year bond of every model is a constant-maturity par bond: each month it
//! earns a month of interest at the yield it started the month at, and its price
//! moves with the yield by its modified duration and convexity,
//!
//! ```text
//! r_t = y_{t-1} / 12 - D(y_{t-1}) (y_t - y_{t-1}) + C(y_{t-1}) (y_t - y_{t-1})^2 / 2
//! ```
//!
//! which is how `data/history` builds the `bond10` column from Treasury yields and
//! how the regime model prices its bond (`regime::RegimeParams::bond_return`). This
//! module holds the same duration and convexity tables for the other uses: the
//! block bootstrap's `bond_pricing: from_yield` and the stress overlays that move
//! yields (`LostGeneration`).

use super::regime::{par_bond_convexity, par_bond_duration};
use super::EconomyPath;
use crate::model::AssetClass;

/// Grid step of the tables (the regime model's).
const GRID: f64 = 0.001;
/// Highest yield in the tables; higher yields use the last entry.
const MAX_YIELD: f64 = 0.25;

/// Duration and convexity of the 10-year par bond on a yield grid.
#[derive(Clone, Debug)]
pub(super) struct BondPricer {
    duration: Vec<f64>,
    convexity: Vec<f64>,
}

impl BondPricer {
    pub(super) fn new() -> BondPricer {
        let n = (MAX_YIELD / GRID).ceil() as usize + 2;
        BondPricer {
            duration: (0..n).map(|i| par_bond_duration(i as f64 * GRID)).collect(),
            convexity: (0..n)
                .map(|i| par_bond_convexity(i as f64 * GRID))
                .collect(),
        }
    }

    /// Return over a month in which the yield moves from `y0` to `y1`. Yields below
    /// zero are priced with the duration at zero (ten years) and earn no interest.
    #[inline]
    pub(super) fn bond_return(&self, y0: f64, y1: f64) -> f64 {
        let pos = (y0 / GRID).clamp(0.0, (self.duration.len() - 2) as f64);
        let i = pos as usize;
        let f = pos - i as f64;
        let d = self.duration[i] + f * (self.duration[i + 1] - self.duration[i]);
        let c = self.convexity[i] + f * (self.convexity[i + 1] - self.convexity[i]);
        let dy = y1 - y0;
        y0.max(0.0) / 12.0 - d * dy + 0.5 * c * dy * dy
    }

    /// Rewrites the bond returns of months `from..to` of `out` from its long-yield
    /// path. `y_before` is the yield at the start of month `from` (the end of the
    /// month before). Draws nothing.
    pub(super) fn reprice(&self, out: &mut EconomyPath, from: usize, to: usize, y_before: f64) {
        let to = to.min(out.months);
        let bond = AssetClass::Bonds.index();
        let mut y0 = y_before;
        for t in from..to {
            let y1 = out.long_yield[t];
            out.returns[bond][t] = self.bond_return(y0, y1);
            y0 = y1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;

    #[test]
    fn a_flat_yield_earns_its_coupon_and_a_rise_costs_its_duration() {
        let p = BondPricer::new();
        // Flat 6%: half a percent a month, no price change.
        assert!((p.bond_return(0.06, 0.06) - 0.005).abs() < 1e-15);
        // Duration of a 10-year 6% semi-annual par bond: (1 - 1.03^-20) / 0.06 =
        // 7.4387 years. A 1-point rise costs 7.44% less a little convexity.
        let d = (1.0 - math::powi(1.03, -20)) / 0.06;
        assert!((d - 7.438_7).abs() < 1e-4);
        let r = p.bond_return(0.06, 0.07);
        let c = par_bond_convexity(0.06);
        let expected = 0.005 - d * 0.01 + 0.5 * c * 0.0001;
        assert!((r - expected).abs() < 1e-9, "{r} {expected}");
        // A fall gains more than a rise loses (convexity).
        assert!(p.bond_return(0.06, 0.05) - 0.005 > 0.005 - r);
        // Between grid points the tables interpolate.
        let mid = p.bond_return(0.0605, 0.0605);
        assert!((mid - 0.0605 / 12.0).abs() < 1e-15);
        // Negative yields earn nothing and are priced at the 10-year duration.
        assert!((p.bond_return(-0.002, -0.002)).abs() < 1e-15);
        // (Duration 10 years; convexity at a zero yield adds about 5e-5.)
        let r = p.bond_return(-0.002, -0.001);
        let c0 = par_bond_convexity(0.0);
        assert!((r - (-10.0 * 0.001 + 0.5 * c0 * 1e-6)).abs() < 1e-12, "{r}");
    }

    #[test]
    fn reprice_follows_the_yield_path() {
        let p = BondPricer::new();
        let mut out = EconomyPath::with_capacity(4);
        out.long_yield.copy_from_slice(&[0.05, 0.06, 0.06, 0.04]);
        p.reprice(&mut out, 0, 4, 0.05);
        let b = &out.returns[AssetClass::Bonds.index()];
        assert!((b[0] - 0.05 / 12.0).abs() < 1e-15);
        assert_eq!(b[1], p.bond_return(0.05, 0.06));
        assert!((b[2] - 0.005).abs() < 1e-15);
        assert_eq!(b[3], p.bond_return(0.06, 0.04));
        assert!(b[1] < 0.0 && b[3] > 0.1);
    }
}
