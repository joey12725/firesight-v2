//! One-draw table samplers.
//!
//! The economy needs several random variates per simulated month. The exact samplers
//! in [`crate::rng`] cost a logarithm (normal) or a `pow` (Student-t) each, which
//! would blow the 60 ns per month budget, so the economy samples by inverse transform
//! from quantile tables built once: one `u64` gives one variate, with a fixed draw
//! count and no rejection loop.
//!
//! A table holds, for each sign, 1,024 equal-probability cells with linear
//! interpolation between the exact quantiles at the cell edges; the outermost cell on
//! each side (probability 1/2048) is refined by a second table of 1,024 cells, so the
//! distribution is resolved down to a tail probability of about 2.4e-7 per side, where
//! it is truncated. Tables for symmetric unit-variance shocks are rescaled so the
//! sampled variance is exactly 1 after interpolation and truncation.

use std::sync::OnceLock;

use crate::math;

/// Equal-probability cells per side in each level.
const CELLS: usize = 1024;
/// Knots per level (cell edges).
const KNOTS: usize = CELLS + 1;
const CELLS_F: f64 = CELLS as f64;
const MASK53: u64 = (1 << 53) - 1;
const UNIT53: f64 = 1.0 / (1u64 << 53) as f64;

/// A quantile table for one distribution. Layout: for each sign (0 = positive side,
/// 1 = negative side) a main level then a tail level, each `KNOTS` long.
#[derive(Clone, Debug)]
pub(super) struct ShockTable {
    t: Box<[f64]>,
}

impl ShockTable {
    /// Builds the table of `f(x)` where `x` is symmetric with `|x|` having quantile
    /// function `half_q(u)`, `u` in `[0, 1)` (that is, `Q(0.5 + u / 2)` of `x`).
    pub(super) fn build(half_q: impl Fn(f64) -> f64, f: impl Fn(f64) -> f64) -> ShockTable {
        let mut t = vec![0.0; 4 * KNOTS];
        let mut half = vec![0.0; 2 * KNOTS];
        for j in 0..KNOTS {
            // Main level: knots at u = j / CELLS; the last cell is replaced by the
            // tail level, so its upper knot is never read.
            half[j] = half_q((j.min(CELLS - 1)) as f64 / CELLS_F);
            // Tail level: the last main cell split again; the final knot sits half a
            // sub-cell from the end, which is where the distribution is truncated.
            let sub = if j == CELLS { CELLS_F - 0.5 } else { j as f64 };
            half[KNOTS + j] = half_q((CELLS_F - 1.0 + sub / CELLS_F) / CELLS_F);
        }
        for (k, &x) in half.iter().enumerate() {
            t[k] = f(x);
            t[2 * KNOTS + k] = f(-x);
        }
        ShockTable { t: t.into() }
    }

    /// One variate from 64 random bits: 1 sign bit, 10 cell bits, 53 bits of position
    /// inside the cell.
    #[inline(always)]
    pub(super) fn sample(&self, bits: u64) -> f64 {
        let base = (bits >> 63) as usize * (2 * KNOTS);
        let cell = ((bits >> 53) & (CELLS as u64 - 1)) as usize;
        let frac = (bits & MASK53) as f64 * UNIT53;
        if cell != CELLS - 1 {
            let a = self.t[base + cell];
            a + frac * (self.t[base + cell + 1] - a)
        } else {
            let pos = frac * CELLS_F;
            let sub = pos as usize;
            let a = self.t[base + KNOTS + sub];
            a + (pos - sub as f64) * (self.t[base + KNOTS + sub + 1] - a)
        }
    }

    /// Integrates a per-cell functional of the two knots with the cell probabilities.
    fn integrate(&self, g: impl Fn(f64, f64) -> f64) -> f64 {
        let mut total = 0.0;
        for side in 0..2 {
            let base = side * 2 * KNOTS;
            let (mut main, mut tail) = (0.0, 0.0);
            for c in 0..CELLS - 1 {
                main += g(self.t[base + c], self.t[base + c + 1]);
            }
            for c in 0..CELLS {
                tail += g(self.t[base + KNOTS + c], self.t[base + KNOTS + c + 1]);
            }
            total += 0.5 * (main / CELLS_F + tail / (CELLS_F * CELLS_F));
        }
        total
    }

    /// Exact mean of the sampled distribution.
    pub(super) fn mean(&self) -> f64 {
        self.integrate(|a, b| 0.5 * (a + b))
    }

    /// Exact second moment of the sampled distribution.
    pub(super) fn second_moment(&self) -> f64 {
        self.integrate(|a, b| (a * a + a * b + b * b) / 3.0)
    }

    /// Multiplies every value by `k`.
    pub(super) fn scale(&mut self, k: f64) {
        for v in self.t.iter_mut() {
            *v *= k;
        }
    }

    /// A table that always returns `value`.
    pub(super) fn constant(value: f64) -> ShockTable {
        ShockTable {
            t: vec![value; 4 * KNOTS].into(),
        }
    }
}

/// Standard normal CDF.
pub(super) fn normal_cdf(x: f64) -> f64 {
    0.5 * crate::math::erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

/// Standard normal quantile: Acklam's rational approximation (relative error about
/// 1e-9) polished with two Newton steps on the exact CDF.
pub(super) fn normal_quantile(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const P_LOW: f64 = 0.024_25;
    const INV_SQRT_2PI: f64 = 0.398_942_280_401_432_7;
    let tail = |p: f64| {
        let q = (-2.0 * math::ln(p)).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    let mut x = if p < P_LOW {
        tail(p)
    } else if p > 1.0 - P_LOW {
        -tail(1.0 - p)
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    };
    for _ in 0..2 {
        let density = INV_SQRT_2PI * math::exp(-0.5 * x * x);
        if density > 1e-300 {
            x -= (normal_cdf(x) - p) / density;
        }
    }
    x
}

/// CDF of Student's t with an even number of degrees of freedom, which is algebraic:
/// `F(t) = 1/2 + (s/2) * sum_{j < dof/2} c_j * cos2^j`, with `s = t / sqrt(dof + t^2)`,
/// `cos2 = dof / (dof + t^2)`, `c_0 = 1`, `c_j = c_{j-1} * (2j - 1) / (2j)`.
pub(super) fn student_t_cdf_even(t: f64, dof: u32) -> f64 {
    debug_assert!(dof >= 2 && dof % 2 == 0);
    let nu = f64::from(dof);
    let s = t / (nu + t * t).sqrt();
    let cos2 = nu / (nu + t * t);
    let (mut term, mut sum) = (1.0, 1.0);
    for j in 1..dof / 2 {
        term *= cos2 * f64::from(2 * j - 1) / f64::from(2 * j);
        sum += term;
    }
    0.5 + 0.5 * s * sum
}

/// Quantile of `|T|` for even `dof` at `u` in `[0, 1)`, by bisection on the CDF.
fn student_t_half_quantile(u: f64, dof: u32) -> f64 {
    if u <= 0.0 {
        return 0.0;
    }
    let target = 0.5 + 0.5 * u;
    let (mut lo, mut hi) = (0.0, 1.0);
    while student_t_cdf_even(hi, dof) < target && hi < 1e12 {
        lo = hi;
        hi *= 2.0;
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if student_t_cdf_even(mid, dof) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// A unit-variance symmetric shock: its additive table plus the knots, kept so that
/// transformed tables (lognormal, capped exponentials) can be derived from the same
/// quantiles.
#[derive(Debug)]
pub(super) struct UnitShock {
    /// Additive table with variance exactly 1.
    pub(super) table: ShockTable,
    /// `|x|` quantiles at the main and tail knots, in units where the variance is 1.
    half: Box<[f64]>,
}

impl UnitShock {
    fn new(half_q: impl Fn(f64) -> f64) -> UnitShock {
        let mut table = ShockTable::build(half_q, |x| x);
        let k = 1.0 / table.second_moment().sqrt();
        table.scale(k);
        let half = table.t[..2 * KNOTS].to_vec().into();
        UnitShock { table, half }
    }

    /// The table of `f(x)` for `x` distributed as this unit shock.
    pub(super) fn transformed(&self, f: impl Fn(f64) -> f64) -> ShockTable {
        let mut t = vec![0.0; 4 * KNOTS];
        for (k, &x) in self.half.iter().enumerate() {
            t[k] = f(x);
            t[2 * KNOTS + k] = f(-x);
        }
        ShockTable { t: t.into() }
    }
}

/// Standard normal.
pub(super) fn normal() -> &'static UnitShock {
    static T: OnceLock<UnitShock> = OnceLock::new();
    T.get_or_init(|| UnitShock::new(|u| normal_quantile(0.5 + 0.5 * u)))
}

/// Student-t with 6 degrees of freedom, unit variance (excess kurtosis 3 before
/// truncation).
pub(super) fn student_t6() -> &'static UnitShock {
    static T: OnceLock<UnitShock> = OnceLock::new();
    T.get_or_init(|| UnitShock::new(|u| student_t_half_quantile(u, 6)))
}

/// Student-t with 4 degrees of freedom, unit variance.
pub(super) fn student_t4() -> &'static UnitShock {
    static T: OnceLock<UnitShock> = OnceLock::new();
    T.get_or_init(|| UnitShock::new(|u| student_t_half_quantile(u, 4)))
}

/// Unit-variance Student-t for 4 or 6 degrees of freedom; other values map to the
/// nearer of the two (the data file specifies 4).
pub(super) fn student_t(dof: u32) -> &'static UnitShock {
    if dof <= 4 {
        student_t4()
    } else {
        student_t6()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn normal_quantile_matches_published_values() {
        // Standard table values.
        for (p, x) in [
            (0.5, 0.0),
            (0.975, 1.959_963_984_540_054),
            (0.99, 2.326_347_874_040_841),
            (0.001, -3.090_232_306_167_813),
            (1e-9, -5.997_807_015_007_686),
        ] {
            assert!((normal_quantile(p) - x).abs() < 1e-9, "p={p}");
        }
        assert!((normal_cdf(1.0) - 0.841_344_746_068_543).abs() < 1e-12);
    }

    #[test]
    fn student_t_cdf_matches_published_critical_values() {
        // Two-sided 5% critical values: t(4) = 2.776445, t(6) = 2.446912; one-sided
        // 1%: t(4) = 3.746947.
        assert!((student_t_cdf_even(2.776_445, 4) - 0.975).abs() < 1e-6);
        assert!((student_t_cdf_even(2.446_912, 6) - 0.975).abs() < 1e-6);
        assert!((student_t_cdf_even(3.746_947, 4) - 0.99).abs() < 1e-6);
        assert!((student_t_half_quantile(0.95, 4) - 2.776_445).abs() < 1e-5);
        assert_eq!(student_t_cdf_even(0.0, 6), 0.5);
    }

    fn sample_moments(t: &ShockTable, n: usize) -> (f64, f64, f64) {
        let mut rng = Rng::from_seed(11);
        let (mut s1, mut s2, mut s4) = (0.0, 0.0, 0.0);
        for _ in 0..n {
            let x = t.sample(rng.next_u64());
            s1 += x;
            s2 += x * x;
            s4 += x * x * x * x;
        }
        let n = n as f64;
        (s1 / n, s2 / n, s4 / n)
    }

    #[test]
    fn tables_have_unit_variance_and_right_tails() {
        let n = normal();
        assert!(n.table.mean().abs() < 1e-15);
        assert!((n.table.second_moment() - 1.0).abs() < 1e-12);
        let (m, v, k) = sample_moments(&n.table, 400_000);
        assert!(m.abs() < 0.006, "mean {m}");
        assert!((v - 1.0).abs() < 0.01, "var {v}");
        assert!((k - 3.0).abs() < 0.08, "fourth moment {k}");

        for dof in [4, 6] {
            let t = student_t(dof);
            assert!((t.table.second_moment() - 1.0).abs() < 1e-12);
            let (m, v, k) = sample_moments(&t.table, 400_000);
            assert!(
                m.abs() < 0.006 && (v - 1.0).abs() < 0.05,
                "dof {dof}: {m} {v}"
            );
            // Fat tails: fourth moment well above the normal's 3.
            assert!(k > 4.5, "dof {dof}: fourth moment {k}");
        }

        // Tail frequency of the normal table: P(|z| > 3) = 0.0026998.
        let mut rng = Rng::from_seed(5);
        let hits = (0..2_000_000)
            .filter(|_| n.table.sample(rng.next_u64()).abs() > 3.0)
            .count();
        let freq = hits as f64 / 2e6;
        assert!((freq - 0.002_699_8).abs() < 0.000_15, "{freq}");
    }

    #[test]
    fn sampling_is_monotone_in_the_bits_within_a_side() {
        let t = &normal().table;
        let mut prev = t.sample(0);
        assert_eq!(prev, 0.0);
        for k in 1..=4096u64 {
            let bits = (k << 51).wrapping_sub(1) & (u64::MAX >> 1);
            let x = t.sample(bits);
            assert!(x >= prev);
            prev = x;
        }
        let top = t.sample(u64::MAX >> 1);
        assert!(top > 4.9 && top < 6.0, "{top}");
        assert!(t.sample(u64::MAX) < -4.9);
    }

    #[test]
    fn transformed_table_mean_is_exact() {
        let sigma = 0.1;
        let mut t = normal().transformed(|x| math::exp(sigma * x));
        // Close to the lognormal mean exp(sigma^2 / 2) before normalisation.
        assert!((t.mean() - math::exp(0.5 * sigma * sigma)).abs() < 1e-5);
        let k = 1.0 / t.mean();
        t.scale(k);
        assert!((t.mean() - 1.0).abs() < 1e-14);
        assert_eq!(ShockTable::constant(1.0).sample(0xDEAD_BEEF_0000_0001), 1.0);
    }
}
