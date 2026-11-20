use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::*;
use crate::rng::Rng;

const N: usize = 1_000_000;

/// Largest interpolation error between two knots, as a share of the scale of the
/// distribution (its standard deviation or its range): the worst cells, next to the
/// median, are 1/128 wide, which for a normal gives 5e-5 standard deviations.
const CELL_TOL: f64 = 1e-4;
/// Largest difference between the mean of the sampler and the mean of the ideal
/// distribution, as a share of its range.
const MEAN_TOL: f64 = 1e-5;

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: got {actual}, expected {expected} +/- {tol}"
    );
}

fn uniform(low: f64, high: f64) -> Uncertain {
    Uncertain::Uniform { low, high }
}
fn triangular(low: f64, mode: f64, high: f64) -> Uncertain {
    Uncertain::Triangular { low, mode, high }
}
fn pert(low: f64, mode: f64, high: f64) -> Uncertain {
    Uncertain::Pert { low, mode, high }
}
fn normal(mean: f64, sd: f64) -> Uncertain {
    Uncertain::Normal { mean, sd }
}
fn lognormal(median: f64, p90: f64) -> Uncertain {
    Uncertain::Lognormal { median, p90 }
}
fn quantiles(p10: f64, p50: f64, p90: f64) -> Uncertain {
    Uncertain::Quantiles { p10, p50, p90 }
}

/// One of each shape, plus the awkward corners: a mode at an end, heavy skew in
/// both directions, a one-sided range, values far from zero and negative values.
fn zoo() -> Vec<Uncertain> {
    vec![
        uniform(2.0, 10.0),
        uniform(-3.0, -1.0),
        triangular(0.0, 3.0, 10.0),
        triangular(0.0, 0.0, 10.0),
        triangular(-5.0, 5.0, 5.0),
        pert(0.0, 3.0, 10.0),
        pert(0.0, 0.0, 10.0),
        pert(0.0, 10.0, 10.0),
        pert(30_000.0, 40_000.0, 80_000.0),
        pert(0.0, 1e-3, 1.0),
        normal(100.0, 15.0),
        normal(-0.01, 0.02),
        normal(1e9, 1.0),
        lognormal(100.0, 200.0),
        lognormal(5_000.0, 5_001.0),
        lognormal(1.0, 50.0),
        quantiles(-10.0, 0.0, 10.0),
        quantiles(10.0, 50.0, 130.0),
        quantiles(-130.0, -50.0, -10.0),
        quantiles(0.0, 10.0, 110.0),
        quantiles(-110.0, -10.0, 0.0),
        quantiles(5.0, 5.0, 9.0),
        quantiles(1.0, 5.0, 5.0),
        quantiles(100.0, 100.000_000_1, 100.000_000_3),
    ]
}

// ---------------------------------------------------------------------------------
// The table

#[test]
fn table_layout_is_consistent() {
    // Knots run from the end of the support to the median, doubling every SUB knots.
    assert_eq!(knot_t(0), 0.0);
    assert_eq!(knot_t(1), 1.0 / INNER_SCALE);
    assert_eq!(knot_t(1 + SUB), 2.0 / INNER_SCALE);
    assert_eq!(knot_t(SIDE - 1), 0.5);
    assert_eq!(knot_t(SIDE - 1 - SUB), 0.25);
    assert_eq!(KNOTS, 1283);
    // Every knot is found exactly, with nothing left over, and the point just below
    // it belongs to the previous cell.
    for i in 1..SIDE - 1 {
        let t = knot_t(i);
        assert!(t > knot_t(i - 1));
        assert_eq!(locate(t), (i, 0.0), "knot {i}");
        let below = f64::from_bits(t.to_bits() - 1);
        let (cell, frac) = locate(below);
        assert_eq!(cell, i - 1);
        assert!(frac > 0.999 && frac < 1.0);
        // Half-way through the cell.
        let (cell, frac) = locate(0.5 * (t + knot_t(i + 1)));
        assert_eq!((cell, frac), (i, 0.5));
    }
    // The smallest uniforms land in the last cell.
    assert_eq!(locate(f64::MIN_POSITIVE).0, 0);
    assert_eq!(locate(0.25 / INNER_SCALE), (0, 0.25));
    // Cell probabilities are the knot spacings and add up to half on each side.
    let mut total = 0.0;
    for i in 0..SIDE - 1 {
        assert_eq!(cell_prob(i), knot_t(i + 1) - knot_t(i), "cell {i}");
        total += cell_prob(i);
    }
    assert_eq!(total, 0.5);
}

#[test]
fn incomplete_beta_matches_closed_forms() {
    // Beta(1, 5): I_x = 1 - (1 - x)^5. Beta(2, 2): 3x^2 - 2x^3. Beta(3, 3):
    // 10x^3 - 15x^4 + 6x^5.
    let ln_b = |a: f64, b: f64| math::ln_gamma(a) + math::ln_gamma(b) - math::ln_gamma(a + b);
    for x in [1e-6, 0.01, 0.2, 0.5, 0.77, 0.99] {
        let exact = -math::exp_m1(5.0 * math::ln_1p(-x));
        close(
            beta_cdf(x, 1.0, 5.0, ln_b(1.0, 5.0)) / exact,
            1.0,
            1e-12,
            "I(1,5)",
        );
        let exact = 3.0 * x * x - 2.0 * x * x * x;
        close(
            beta_cdf(x, 2.0, 2.0, ln_b(2.0, 2.0)) / exact,
            1.0,
            1e-12,
            "I(2,2)",
        );
        let exact = x * x * x * (10.0 - 15.0 * x + 6.0 * x * x);
        close(
            beta_cdf(x, 3.0, 3.0, ln_b(3.0, 3.0)) / exact,
            1.0,
            1e-12,
            "I(3,3)",
        );
    }
    assert_eq!(beta_cdf(0.0, 2.2, 3.8, 0.0), 0.0);
    assert_eq!(beta_cdf(1.0, 2.2, 3.8, 0.0), 1.0);
    // Inversion: Beta(1, 5) has quantile 1 - (1 - t)^(1/5).
    let lb = ln_b(1.0, 5.0);
    for t in [1e-7, 1e-3, 0.1, 0.5] {
        let exact = -math::exp_m1(0.2 * math::ln_1p(-t));
        close(
            beta_quantile(t, 1.0, 5.0, lb, 0.0) / exact,
            1.0,
            1e-11,
            "beta quantile",
        );
        // A starting point below the root does not change the answer.
        close(
            beta_quantile(t, 1.0, 5.0, lb, 0.5 * exact) / exact,
            1.0,
            1e-11,
            "warm-started beta quantile",
        );
    }
    assert_eq!(beta_quantile(0.0, 2.0, 4.0, 0.0, 0.0), 0.0);
}

// ---------------------------------------------------------------------------------
// Each shape against hand-derived values

#[test]
fn uniform_is_exact() {
    let d = uniform(2.0, 10.0).compile();
    assert_eq!((d.min(), d.max()), (2.0, 10.0));
    assert_eq!((d.mean(), d.median()), (6.0, 6.0));
    assert_eq!(d.quantile(0.0), 2.0);
    close(d.quantile(0.1), 2.8, 1e-14, "P10");
    assert_eq!(d.quantile(0.5), 6.0);
    close(d.quantile(0.9), 9.2, 1e-14, "P90");
    assert_eq!(d.quantile(1.0), 10.0);
    assert!(!d.is_point());
    // No table: a uniform is sampled in closed form.
    assert!(d.table.is_none());
}

#[test]
fn triangular_matches_its_closed_form() {
    // low 0, mode 3, high 10. P(X <= mode) = 3/10.
    // Below the mode Q(p) = sqrt(30 p); above it Q(p) = 10 - sqrt(70 (1 - p)).
    let d = triangular(0.0, 3.0, 10.0).compile();
    assert_eq!((d.min(), d.max()), (0.0, 10.0));
    close(
        d.mean(),
        13.0 / 3.0,
        MEAN_TOL * 10.0,
        "mean (0 + 3 + 10) / 3",
    );
    close(
        d.quantile(0.1),
        3.0f64.sqrt(),
        CELL_TOL * 10.0,
        "P10 = sqrt(3)",
    );
    close(
        d.quantile(0.3),
        3.0,
        CELL_TOL * 10.0,
        "the mode sits at 30%",
    );
    close(
        d.median(),
        10.0 - 35.0f64.sqrt(),
        1e-12,
        "median = 10 - sqrt(35)",
    );
    close(d.quantile(0.5), 10.0 - 35.0f64.sqrt(), 1e-12, "P50");
    close(
        d.quantile(0.9),
        10.0 - 7.0f64.sqrt(),
        CELL_TOL * 10.0,
        "P90 = 10 - sqrt(7)",
    );
    // Knots are exact: 25% and 75% are knots.
    close(d.quantile(0.25), 7.5f64.sqrt(), 1e-12, "P25 = sqrt(7.5)");
    close(d.quantile(0.75), 10.0 - 17.5f64.sqrt(), 1e-12, "P75");
    // Far tails: Q(1e-6) = sqrt(3e-5).
    close(d.quantile(1e-6), 3e-5f64.sqrt(), 1e-6, "P0.0001");
    close(
        d.quantile(1.0 - 1e-6),
        10.0 - 7e-5f64.sqrt(),
        1e-6,
        "P99.9999",
    );

    // Mode at an end: a right triangle, Q(p) = 10 - 10 sqrt(1 - p), mean 10 / 3.
    let d = triangular(0.0, 0.0, 10.0).compile();
    close(d.mean(), 10.0 / 3.0, MEAN_TOL * 10.0, "right-triangle mean");
    close(
        d.quantile(0.5),
        10.0 - 10.0 * 0.5f64.sqrt(),
        1e-12,
        "median",
    );
    close(
        d.quantile(0.1),
        10.0 - 10.0 * 0.9f64.sqrt(),
        CELL_TOL * 10.0,
        "P10",
    );
}

/// `P(X <= x)` for PERT(0, mode, 1) by Simpson's rule on the density
/// `x^(a-1) (1-x)^(b-1)`, normalised numerically: an independent check that uses no
/// gamma function and no continued fraction.
fn pert_cdf_by_quadrature(mode: f64, x: f64) -> f64 {
    let (a, b) = (1.0 + 4.0 * mode, 5.0 - 4.0 * mode);
    let density = |t: f64| math::pow(t, a - 1.0) * math::pow(1.0 - t, b - 1.0);
    let simpson = |lo: f64, hi: f64| {
        let n = 20_000;
        let h = (hi - lo) / n as f64;
        let mut sum = density(lo) + density(hi);
        for k in 1..n {
            sum += density(lo + k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
        }
        sum * h / 3.0
    };
    simpson(0.0, x) / (simpson(0.0, x) + simpson(x, 1.0))
}

#[test]
fn pert_matches_the_beta_distribution() {
    // Symmetric: PERT(0, 5, 10) is 10 x Beta(3, 3), whose CDF is the polynomial
    // 10x^3 - 15x^4 + 6x^5. Mean 5, variance 100 * 9 / (36 * 7).
    let d = pert(0.0, 5.0, 10.0).compile();
    let cdf = |v: f64| {
        let x = v / 10.0;
        x * x * x * (10.0 - 15.0 * x + 6.0 * x * x)
    };
    assert_eq!((d.min(), d.max()), (0.0, 10.0));
    close(d.mean(), 5.0, 1e-9, "symmetric mean");
    close(d.median(), 5.0, 1e-10, "symmetric median");
    for p in [0.25, 0.75, 0.125, 1.0 / 1024.0] {
        // Knots: the inversion itself.
        close(cdf(d.quantile(p)), p, 1e-11, "CDF at a knot");
    }
    for p in [0.01, 0.1, 0.3, 0.9, 0.99] {
        close(cdf(d.quantile(p)), p, 2.0 * CELL_TOL, "CDF between knots");
    }
    close(d.quantile(0.1) + d.quantile(0.9), 10.0, 1e-9, "symmetry");

    // Mode at the low end: 10 x Beta(1, 5), Q(p) = 10 (1 - (1 - p)^(1/5)); mean
    // (0 + 0 + 10) / 6.
    let d = pert(0.0, 0.0, 10.0).compile();
    let q = |p: f64| 10.0 * (1.0 - math::pow(1.0 - p, 0.2));
    close(
        d.mean(),
        10.0 / 6.0,
        MEAN_TOL * 10.0,
        "mean with the mode at low",
    );
    close(d.quantile(0.1), q(0.1), CELL_TOL * 10.0, "P10");
    close(d.quantile(0.5), q(0.5), 1e-10, "P50");
    close(d.quantile(0.9), q(0.9), CELL_TOL * 10.0, "P90");
    // Mode at the high end is its mirror image.
    let m = pert(0.0, 10.0, 10.0).compile();
    close(
        m.mean(),
        50.0 / 6.0,
        MEAN_TOL * 10.0,
        "mean with the mode at high",
    );
    for p in [0.1, 0.5, 0.9] {
        close(m.quantile(p), 10.0 - d.quantile(1.0 - p), 1e-9, "mirror");
    }

    // General: PERT(0, 3, 10) is 10 x Beta(2.2, 3.8). Mean (0 + 12 + 10) / 6.
    let d = pert(0.0, 3.0, 10.0).compile();
    close(
        d.mean(),
        22.0 / 6.0,
        MEAN_TOL * 10.0,
        "mean (low + 4 mode + high) / 6",
    );
    for p in [0.25, 0.5, 0.75] {
        close(
            pert_cdf_by_quadrature(0.3, d.quantile(p) / 10.0),
            p,
            1e-9,
            "knot",
        );
    }
    for p in [0.1, 0.9] {
        close(
            pert_cdf_by_quadrature(0.3, d.quantile(p) / 10.0),
            p,
            2.0 * CELL_TOL,
            "P10, P90",
        );
    }
    // Shifting and scaling the three points shifts and scales the distribution.
    let e = pert(30_000.0, 42_000.0, 70_000.0).compile();
    for p in [0.03, 0.1, 0.5, 0.9, 0.999] {
        close(
            e.quantile(p),
            30_000.0 + 4_000.0 * d.quantile(p),
            1e-6,
            "affine",
        );
    }
    close(
        e.mean(),
        (30_000.0 + 4.0 * 42_000.0 + 70_000.0) / 6.0,
        MEAN_TOL * 40_000.0,
        "mean",
    );
}

#[test]
fn normal_is_cut_at_four_standard_deviations() {
    let d = normal(100.0, 15.0).compile();
    assert_eq!((d.min(), d.max()), (40.0, 160.0));
    close(d.mean(), 100.0, 1e-9, "mean");
    assert_eq!(d.median(), 100.0);
    assert_eq!(d.quantile(0.5), 100.0);
    // Standard table values: z(0.9) = 1.2815516, z(0.975) = 1.9599640,
    // z(0.999) = 3.0902323.
    close(d.quantile(0.9), 100.0 + 15.0 * Z90, 15.0 * CELL_TOL, "P90");
    close(d.quantile(0.1), 100.0 - 15.0 * Z90, 15.0 * CELL_TOL, "P10");
    close(
        d.quantile(0.975),
        100.0 + 15.0 * 1.959_963_984_540_054,
        15.0 * CELL_TOL,
        "P97.5",
    );
    close(
        d.quantile(0.001),
        100.0 - 15.0 * 3.090_232_306_167_813,
        15.0 * CELL_TOL,
        "P0.1",
    );
    // Knots are exact: z(0.75) = 0.6744898.
    close(
        d.quantile(0.75),
        100.0 + 15.0 * 0.674_489_750_196_081_7,
        1e-9,
        "P75",
    );
    // Symmetric about the mean.
    for u in [1e-9, 1e-5, 0.01, 0.2, 0.4] {
        close(d.quantile(u) + d.quantile(1.0 - u), 200.0, 1e-9, "symmetry");
    }
    // Beyond 4 sd (probability 3.17e-5 a side) every draw is the limit itself.
    assert_eq!(d.quantile(1e-5), 40.0);
    assert_eq!(d.quantile(1.0 - 1e-5), 160.0);
    assert!(d.quantile(1e-4) > 40.0);
    // Negative values are allowed.
    let d = normal(-0.01, 0.02).compile();
    close(d.min(), -0.09, 1e-15, "min");
    close(d.quantile(0.9), -0.01 + 0.02 * Z90, 0.02 * CELL_TOL, "P90");
}

/// Mean of `median * exp(sigma * Z)` with `Z` limited to `[-4, 4]`: the lognormal
/// partial expectation plus the two points of mass at the limits.
fn capped_lognormal_mean(median: f64, sigma: f64) -> f64 {
    let body = math::exp(0.5 * sigma * sigma)
        * (math::normal_cdf(Z_CAP - sigma) - math::normal_cdf(-Z_CAP - sigma));
    let ends = math::normal_cdf(-Z_CAP) * (math::exp(Z_CAP * sigma) + math::exp(-Z_CAP * sigma));
    median * (body + ends)
}

#[test]
fn lognormal_reproduces_median_and_p90() {
    // median 100, p90 200: sigma = ln 2 / 1.2815516 = 0.540866.
    let sigma = std::f64::consts::LN_2 / Z90;
    close(sigma, 0.540_866, 1e-6, "sigma");
    let d = lognormal(100.0, 200.0).compile();
    close(d.median(), 100.0, 1e-12, "median");
    close(d.quantile(0.9), 200.0, 200.0 * CELL_TOL, "P90");
    // Symmetric in logs: P10 = median^2 / P90 = 50.
    close(d.quantile(0.1), 50.0, 50.0 * CELL_TOL, "P10");
    // P97.5 = 100 exp(1.959964 sigma) = 288.66.
    close(
        d.quantile(0.975),
        100.0 * math::exp(1.959_963_984_540_054 * sigma),
        0.01,
        "P97.5",
    );
    close(d.min(), 100.0 * math::exp(-4.0 * sigma), 1e-12, "min");
    close(d.max(), 100.0 * math::exp(4.0 * sigma), 1e-9, "max");
    assert!(d.min() > 11.4 && d.min() < 11.6 && d.max() > 869.0 && d.max() < 871.0);
    // exp(sigma^2 / 2) = 1.1575: the uncapped mean is 115.75, and the cap moves it
    // by less than 0.01.
    let mean = capped_lognormal_mean(100.0, sigma);
    close(mean, 115.75, 0.01, "analytic capped mean");
    close(d.mean(), mean, MEAN_TOL * (d.max() - d.min()), "table mean");
    // Always positive.
    assert!(lognormal(1.0, 50.0).compile().min() > 0.0);
}

#[test]
fn quantiles_pass_through_the_three_points() {
    for (p10, p50, p90) in [
        (-10.0, 0.0, 10.0),
        (10.0, 50.0, 130.0),
        (-130.0, -50.0, -10.0),
        (0.0, 10.0, 110.0),
        (-110.0, -10.0, 0.0),
        (40_000.0, 100_000.0, 300_000.0),
        (5.0, 5.0, 9.0),
        (1.0, 5.0, 5.0),
        (-0.03, 0.0, 0.01),
    ] {
        let d = quantiles(p10, p50, p90).compile();
        let tol = 2.0 * CELL_TOL * (p90 - p10);
        close(d.quantile(0.1), p10, tol, "P10");
        close(d.quantile(0.5), p50, 1e-12 * (1.0 + p50.abs()), "P50");
        close(d.median(), p50, 1e-12 * (1.0 + p50.abs()), "median");
        close(d.quantile(0.9), p90, tol, "P90");
        assert!(d.min() <= p10 && d.max() >= p90);
    }
}

#[test]
fn quantiles_are_a_shifted_lognormal_with_limited_skew() {
    // Symmetric points give the normal with sd = 10 / 1.2815516: limits at
    // 4 / 1.2815516 = 3.1212 times the distance to each percentile.
    let d = quantiles(-10.0, 0.0, 10.0).compile();
    let n = normal(0.0, 10.0 / Z90).compile();
    for u in [1e-6, 0.03, 0.3, 0.6, 0.99] {
        close(
            d.quantile(u),
            n.quantile(u),
            1e-9,
            "symmetric quantiles are normal",
        );
    }
    close(d.max(), 40.0 / Z90, 1e-12, "max");
    close(d.min(), -40.0 / Z90, 1e-12, "min");
    close(d.mean(), 0.0, 1e-9, "mean");

    // (10, 50, 130): the upper side is twice the lower, so s = ln 2 / z90 and the
    // curve is -30 + 80 exp(s z): shift = 50 - 80 / (2 - 1), scale = 80 / (2 - 1).
    // Two z90 above the median that is -30 + 80 * 4 = 290.
    let d = quantiles(10.0, 50.0, 130.0).compile();
    close(
        d.quantile(math::normal_cdf(2.0 * Z90)),
        290.0,
        0.03,
        "at 2 z90",
    );
    // Two z90 below: -30 + 80 / 4 = -10.
    close(
        d.quantile(math::normal_cdf(-2.0 * Z90)),
        -10.0,
        0.01,
        "at -2 z90",
    );
    // The floor of the lognormal is -30; the cap at -4 sd stops just above it.
    let s = std::f64::consts::LN_2 / Z90;
    close(d.min(), -30.0 + 80.0 * math::exp(-4.0 * s), 1e-9, "min");
    close(d.max(), -30.0 + 80.0 * math::exp(4.0 * s), 1e-9, "max");
    assert!(d.min() > -30.0 && d.min() < -20.0);
    // Right skew: the mean is above the median.
    assert!(d.mean() > 55.0 && d.mean() < 70.0, "{}", d.mean());

    // Left skew is the mirror image.
    let m = quantiles(-130.0, -50.0, -10.0).compile();
    for u in [1e-6, 0.03, 0.3, 0.5, 0.6, 0.99] {
        close(m.quantile(u), -d.quantile(1.0 - u), 1e-9, "mirror");
    }
    close(m.mean(), -d.mean(), 1e-9, "mirrored mean");

    // (0, 10, 110): the sides differ by a factor of 10, above the limit of 3, so
    // the curvature is that of a ratio of 3 and each side is scaled to its point.
    // With k = 3^(4 / z90) = exp(4 s_max): max = 10 + 100 (k - 1) / (3 - 1),
    // min = 10 - 10 (1 - 1 / k) / (1 - 1 / 3).
    let d = quantiles(0.0, 10.0, 110.0).compile();
    let k = math::pow(MAX_SKEW_RATIO, Z_CAP / Z90);
    close(
        d.max(),
        10.0 + 100.0 * (k - 1.0) / 2.0,
        1e-9,
        "limited-skew max",
    );
    close(
        d.min(),
        10.0 - 10.0 * (1.0 - 1.0 / k) * 1.5,
        1e-9,
        "limited-skew min",
    );
    close(
        (d.max() - 10.0) / 100.0,
        14.92,
        0.01,
        "long tail is 14.9 times b",
    );
    close(
        (10.0 - d.min()) / 10.0,
        1.451,
        0.001,
        "short tail is 1.45 times a",
    );

    // A one-sided range: nothing below the median.
    let d = quantiles(5.0, 5.0, 9.0).compile();
    assert_eq!(d.min(), 5.0);
    assert_eq!(d.quantile(0.01), 5.0);
    assert_eq!(d.quantile(0.5), 5.0);
    assert!(d.quantile(0.51) > 5.0);

    // No jump at the skew limit: ratios just inside and just outside agree.
    let inside = quantiles(-1.0, 0.0, 2.999_999).compile();
    let outside = quantiles(-1.0, 0.0, 3.000_001).compile();
    for u in [0.0, 1e-4, 0.05, 0.3, 0.7, 0.95, 0.9999, 1.0] {
        close(inside.quantile(u), outside.quantile(u), 2e-4, "continuity");
    }
    // Nor as the two sides become equal (the normal limit of the formula).
    let near = quantiles(-1.0, 0.0, 1.000_000_001).compile();
    let exact = quantiles(-1.0, 0.0, 1.0).compile();
    for u in [0.0, 1e-4, 0.05, 0.3, 0.7, 0.95, 0.9999] {
        close(near.quantile(u), exact.quantile(u), 1e-7, "symmetric limit");
    }
}

// ---------------------------------------------------------------------------------
// Properties of every compiled distribution

/// A fine grid of uniforms in increasing order: 200,001 evenly spaced points merged
/// with the powers of two towards both ends and their neighbours.
fn fine_grid() -> Vec<f64> {
    let mut grid: Vec<f64> = (0..=200_000).map(|k| f64::from(k) / 200_000.0).collect();
    grid.pop();
    for j in 1..=53 {
        let t = math::powi(0.5, j);
        for x in [t, 1.0 - t] {
            let bits = x.to_bits();
            grid.extend([f64::from_bits(bits - 1), x, f64::from_bits(bits + 1)]);
        }
    }
    grid.retain(|u| (0.0..1.0).contains(u));
    grid.sort_by(f64::total_cmp);
    grid
}

#[test]
fn quantile_is_finite_monotone_and_inside_the_support() {
    let grid = fine_grid();
    assert!(grid.len() > 200_000);
    for dist in zoo() {
        let d = dist.compile();
        assert!(d.min().is_finite() && d.max().is_finite() && d.min() < d.max());
        assert_eq!(d.quantile(0.0), d.min(), "{dist:?}");
        let mut prev = d.min();
        for &u in &grid {
            let x = d.quantile(u);
            assert!(x.is_finite(), "{dist:?} at {u}");
            assert!(x >= prev, "{dist:?}: {x} after {prev} at u = {u}");
            assert!(x <= d.max(), "{dist:?}: {x} above max at u = {u}");
            prev = x;
        }
        // The top of the range is reached in the limit, and by out-of-range inputs.
        assert_eq!(d.quantile(1.0), d.max());
        assert_eq!(d.quantile(7.0), d.max());
        assert_eq!(d.quantile(f64::INFINITY), d.max());
        assert_eq!(d.quantile(-1.0), d.min());
        assert_eq!(d.quantile(f64::NEG_INFINITY), d.min());
        assert_eq!(d.quantile(f64::NAN), d.min());
        // The median and the mean are inside it.
        assert_eq!(d.quantile(0.5), d.median());
        assert!(d.mean() > d.min() && d.mean() < d.max(), "{dist:?}");
        // Random words: the smallest and largest map to the ends of the grid.
        assert_eq!(d.sample(0), d.min());
        assert_eq!(d.sample(u64::MAX), d.quantile(1.0 - UNIT53));
        assert_eq!(d.sample(1 << 63), d.median());
    }
}

#[test]
fn mean_is_the_integral_of_the_quantile_function() {
    // The quantile function is piecewise linear between binary knots, so the midpoint
    // rule is exact when no step straddles a knot: 1,024 equal steps in each halving
    // of the distance to an end (32 per cell), and one step for the innermost cell.
    // Every point is a short binary fraction, so `1 - t` is exact too.
    for dist in zoo() {
        let d = dist.compile();
        let steps = 1024;
        let mut sum = 0.0;
        for side in [false, true] {
            let at = |t: f64| d.quantile(if side { 1.0 - t } else { t }) - d.median();
            let inner = 1.0 / INNER_SCALE;
            sum += inner * at(0.5 * inner);
            for level in 0..LEVELS {
                let base = inner * math::powi(2.0, level as i32);
                let mut band = 0.0;
                for k in 0..steps {
                    band += at(base * (1.0 + (f64::from(k) + 0.5) / f64::from(steps)));
                }
                sum += base * band / f64::from(steps);
            }
        }
        close(
            d.mean(),
            d.median() + sum,
            1e-11 * (d.max() - d.min()),
            &format!("{dist:?}"),
        );
    }
}

/// Draws a million values and returns (mean, variance, share at or below each of
/// three thresholds).
fn draw(d: &CompiledDist, seed: u64, at: [f64; 3]) -> (f64, f64, [f64; 3]) {
    let mut rng = Rng::from_seed(seed);
    let (mut sum, mut sum_sq, mut below) = (0.0, 0.0, [0usize; 3]);
    let centre = d.median();
    for _ in 0..N {
        let x = d.sample(rng.next_u64());
        assert!(x >= d.min() && x <= d.max());
        sum += x - centre;
        sum_sq += (x - centre) * (x - centre);
        for (count, limit) in below.iter_mut().zip(at) {
            *count += usize::from(x <= limit);
        }
    }
    let n = N as f64;
    let mean = sum / n;
    (
        centre + mean,
        sum_sq / n - mean * mean,
        below.map(|c| c as f64 / n),
    )
}

/// Checks a million draws against the exact mean and variance and against three
/// (value, probability) points of the distribution. The tolerance on the mean is
/// four standard errors; on a share, four standard errors of a proportion.
#[track_caller]
fn check_draws(dist: Uncertain, seed: u64, mean: f64, variance: f64, points: [(f64, f64); 3]) {
    let d = dist.compile();
    let (m, v, shares) = draw(&d, seed, points.map(|p| p.0));
    let se = (variance / N as f64).sqrt();
    close(m, mean, 4.0 * se, &format!("{dist:?} sample mean"));
    close(m, d.mean(), 4.0 * se, &format!("{dist:?} mean()"));
    close(v / variance, 1.0, 0.012, &format!("{dist:?} variance"));
    for (share, (x, p)) in shares.iter().zip(points) {
        let tol = 4.0 * (p * (1.0 - p) / N as f64).sqrt() + 1e-4;
        close(*share, p, tol, &format!("{dist:?} share below {x}"));
    }
}

#[test]
fn a_million_draws_have_the_right_moments() {
    // Uniform(2, 10): mean 6, variance 8^2 / 12.
    check_draws(
        uniform(2.0, 10.0),
        11,
        6.0,
        64.0 / 12.0,
        [(2.8, 0.1), (6.0, 0.5), (9.2, 0.9)],
    );
    // Triangular(0, 3, 10): mean 13/3, variance (0 + 9 + 100 - 0 - 0 - 30) / 18.
    check_draws(
        triangular(0.0, 3.0, 10.0),
        12,
        13.0 / 3.0,
        79.0 / 18.0,
        [
            (3.0f64.sqrt(), 0.1),
            (3.0, 0.3),
            (10.0 - 7.0f64.sqrt(), 0.9),
        ],
    );
    // PERT(0, 5, 10) = 10 Beta(3, 3): mean 5, variance 100 * 9 / 252; the CDF
    // 10x^3 - 15x^4 + 6x^5 is 0.05792 at 0.2, 0.5 at 0.5 and 0.94208 at 0.8.
    check_draws(
        pert(0.0, 5.0, 10.0),
        14,
        5.0,
        900.0 / 252.0,
        [(2.0, 0.057_92), (5.0, 0.5), (8.0, 0.942_08)],
    );
    // PERT(0, 3, 10) = 10 Beta(2.2, 3.8): mean (0 + 12 + 10) / 6, variance
    // 100 * 2.2 * 3.8 / (6^2 * 7); shares from integrating the density.
    check_draws(
        pert(0.0, 3.0, 10.0),
        15,
        22.0 / 6.0,
        836.0 / 252.0,
        [2.0, 5.0, 8.0].map(|x| (x, pert_cdf_by_quadrature(0.3, x / 10.0))),
    );
    // Normal(100, 15) cut at 4 sd: variance 225 * (1 - 8 phi(4) + 30 Phi(-4)), which
    // is 225 * 0.99988. P(X <= 115) = Phi(1) = 0.84134.
    let cut = 1.0 - 8.0 * math::normal_pdf(4.0) + 30.0 * math::normal_cdf(-4.0);
    close(cut, 0.999_88, 1e-5, "variance lost to the cut");
    check_draws(
        normal(100.0, 15.0),
        16,
        100.0,
        225.0 * cut,
        [
            (100.0 - 15.0 * Z90, 0.1),
            (100.0, 0.5),
            (115.0, 0.841_344_7),
        ],
    );
    // Lognormal(median 100, p90 200): mean 115.75 (see above); the uncapped variance
    // is mean^2 (exp(sigma^2) - 1) with exp(sigma^2) = 1.33983, less a little for
    // the cap.
    let sigma = std::f64::consts::LN_2 / Z90;
    let mean = capped_lognormal_mean(100.0, sigma);
    let second = 100.0 * capped_lognormal_mean(100.0, 2.0 * sigma);
    check_draws(
        lognormal(100.0, 200.0),
        17,
        mean,
        second - mean * mean,
        [(50.0, 0.1), (100.0, 0.5), (200.0, 0.9)],
    );
    close(second - mean * mean, 4_500.0, 100.0, "lognormal variance");
    // Quantiles(10, 50, 130) = -30 + 80 LN(0, sigma) with the same sigma: mean
    // -30 + 0.8 * 115.75, variance 0.64 times the lognormal's.
    check_draws(
        quantiles(10.0, 50.0, 130.0),
        18,
        -30.0 + 0.8 * mean,
        0.64 * (second - mean * mean),
        [(10.0, 0.1), (50.0, 0.5), (130.0, 0.9)],
    );
    // Limited skew, each side scaled separately: the three points still hold.
    let d = quantiles(0.0, 10.0, 110.0).compile();
    let (m, _, shares) = draw(&d, 19, [0.0, 10.0, 110.0]);
    close(shares[0], 0.1, 0.0013, "share below p10");
    close(shares[1], 0.5, 0.0021, "share below p50");
    close(shares[2], 0.9, 0.0013, "share below p90");
    close(m, d.mean(), 1.0, "limited-skew mean");
}

#[test]
fn dividing_by_the_mean_gives_a_multiplier_that_averages_one() {
    // What a mean-preserving caller does: scale a base amount by sample / mean.
    let d = lognormal(1.0, 3.0).compile();
    let mut rng = Rng::stream(2026, 0, crate::rng::Stream::EventShape);
    let mut sum = 0.0;
    for _ in 0..N {
        sum += d.sample(rng.next_u64()) / d.mean();
    }
    // sd of the multiplier is about 1.0, so the standard error is 0.001.
    close(sum / N as f64, 1.0, 0.005, "average multiplier");
}

// ---------------------------------------------------------------------------------
// Degenerate and invalid inputs

#[test]
fn degenerate_inputs_are_points() {
    let points = [
        (Uncertain::point(7.5), 7.5),
        (uniform(3.0, 3.0), 3.0),
        (triangular(4.0, 4.0, 4.0), 4.0),
        (pert(-2.0, -2.0, -2.0), -2.0),
        (normal(100.0, 0.0), 100.0),
        (quantiles(9.0, 9.0, 9.0), 9.0),
    ];
    for (dist, value) in points {
        let d = dist.compile();
        assert!(d.is_point(), "{dist:?}");
        assert!(d.table.is_none(), "{dist:?} built a table");
        assert_eq!(
            (d.min(), d.max(), d.mean(), d.median()),
            (value, value, value, value)
        );
        for u in [0.0, 1e-12, 0.3, 0.5, 0.999_999, 1.0, f64::NAN] {
            assert_eq!(d.quantile(u), value);
        }
        assert_eq!(d.sample(0xDEAD_BEEF_0000_0001), value);
        assert_eq!(d, CompiledDist::point(value));
    }
    assert_eq!(Uncertain::point(2.0), uniform(2.0, 2.0));
    assert_eq!(CompiledDist::default(), CompiledDist::point(0.0));
    assert_eq!(CompiledDist::point(f64::NAN), CompiledDist::point(0.0));
    // "No distribution given" and "a distribution given" through one call.
    assert_eq!(
        CompiledDist::from_option(None, 12.0),
        CompiledDist::point(12.0)
    );
    let some = normal(1.0, 2.0);
    assert_eq!(CompiledDist::from_option(Some(&some), 12.0), some.compile());
    // A spread too small to change the centre is a point, not a broken table.
    let d = normal(1e17, 1.0).compile();
    assert!(d.is_point() && d.mean() == 1e17);
}

#[test]
fn invalid_inputs_are_repaired_never_propagated() {
    // Limits out of order, a mode outside them, a negative spread: put right.
    assert_eq!(uniform(10.0, 2.0).compile(), uniform(2.0, 10.0).compile());
    assert_eq!(
        triangular(10.0, 3.0, 0.0).compile(),
        triangular(0.0, 3.0, 10.0).compile()
    );
    assert_eq!(
        pert(0.0, 12.0, 10.0).compile(),
        pert(0.0, 10.0, 10.0).compile()
    );
    assert_eq!(normal(5.0, -1.0).compile(), CompiledDist::point(5.0));
    assert_eq!(lognormal(100.0, 50.0).compile(), CompiledDist::point(100.0));
    assert_eq!(
        quantiles(8.0, 5.0, 9.0).compile(),
        quantiles(5.0, 5.0, 9.0).compile()
    );
    // Not finite, or not positive where it must be: the point at zero.
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for x in bad {
        for dist in [
            uniform(x, 1.0),
            uniform(0.0, x),
            triangular(x, 1.0, 2.0),
            triangular(0.0, x, 2.0),
            pert(0.0, 1.0, x),
            pert(0.0, x, 2.0),
            normal(x, 1.0),
            normal(0.0, x),
            lognormal(x, 2.0),
            lognormal(1.0, x),
            quantiles(x, 1.0, 2.0),
            quantiles(0.0, x, 2.0),
            quantiles(0.0, 1.0, x),
        ] {
            assert_eq!(dist.compile(), CompiledDist::point(0.0), "{dist:?}");
        }
    }
    assert_eq!(lognormal(0.0, 2.0).compile(), CompiledDist::point(0.0));
    assert_eq!(lognormal(-3.0, 2.0).compile(), CompiledDist::point(0.0));
    // Ranges too wide to represent: still finite everywhere.
    for dist in [
        uniform(-f64::MAX, f64::MAX),
        triangular(-f64::MAX, 0.0, f64::MAX),
        pert(-f64::MAX, 0.0, f64::MAX),
        normal(0.0, f64::MAX),
        lognormal(1e-300, 1e300),
        quantiles(-f64::MAX, 0.0, f64::MAX),
        quantiles(0.0, 1e-300, 1e300),
    ] {
        let d = dist.compile();
        for u in [0.0, 1e-9, 0.25, 0.5, 0.75, 1.0 - 1e-9] {
            assert!(d.quantile(u).is_finite(), "{dist:?} at {u}");
        }
        assert!(d.mean().is_finite() && d.min().is_finite() && d.max().is_finite());
    }
}

// ---------------------------------------------------------------------------------
// JSON and schema

#[test]
fn json_shapes_round_trip() {
    let cases = [
        (
            uniform(1.0, 2.5),
            json!({"type": "uniform", "low": 1.0, "high": 2.5}),
        ),
        (
            triangular(1.0, 2.0, 4.0),
            json!({"type": "triangular", "low": 1.0, "mode": 2.0, "high": 4.0}),
        ),
        (
            pert(30_000.0, 40_000.0, 80_000.0),
            json!({"type": "pert", "low": 30000.0, "mode": 40000.0, "high": 80000.0}),
        ),
        (
            normal(-0.01, 0.02),
            json!({"type": "normal", "mean": -0.01, "sd": 0.02}),
        ),
        (
            lognormal(5_000.0, 20_000.0),
            json!({"type": "lognormal", "median": 5000.0, "p90": 20000.0}),
        ),
        (
            quantiles(-1.0, 0.5, 4.0),
            json!({"type": "quantiles", "p10": -1.0, "p50": 0.5, "p90": 4.0}),
        ),
    ];
    for (dist, value) in cases {
        assert_eq!(serde_json::to_value(dist).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<Uncertain>(value.clone()).unwrap(),
            dist
        );
        assert_eq!(value["type"], dist.type_name());
        // Field order in the JSON is the declaration order.
        let text = serde_json::to_string(&dist).unwrap();
        assert!(
            text.starts_with(&format!("{{\"type\":\"{}\",", dist.type_name())),
            "{text}"
        );
    }
    // Integers are accepted for numbers.
    let d: Uncertain =
        serde_json::from_str(r#"{"type":"pert","low":1,"mode":2,"high":3}"#).unwrap();
    assert_eq!(d, pert(1.0, 2.0, 3.0));
    // Misspelt and extra fields, missing fields and unknown shapes are errors.
    let err = |text: &str| {
        serde_json::from_str::<Uncertain>(text)
            .unwrap_err()
            .to_string()
    };
    assert!(err(r#"{"type":"uniform","low":1,"hihg":2}"#).contains("hihg"));
    assert!(err(r#"{"type":"uniform","low":1,"high":2,"mode":1.5}"#).contains("mode"));
    assert!(err(r#"{"type":"uniform","low":1}"#).contains("high"));
    assert!(err(r#"{"type":"normal","mean":1}"#).contains("sd"));
    assert!(err(r#"{"type":"beta","low":1,"high":2}"#).contains("beta"));
    assert!(err(r#"{"low":1,"high":2}"#).contains("type"));
    assert!(serde_json::from_str::<Uncertain>("5").is_err());
}

/// What a field of this type looks like inside a profile struct.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Wrapper {
    /// A plain amount in dollars.
    amount: f64,
    /// The amount as a range, when it is uncertain.
    amount_range: Option<Uncertain>,
    /// A range that is always present.
    growth: Uncertain,
}

impl Default for Wrapper {
    fn default() -> Self {
        Wrapper {
            amount: 1.0,
            amount_range: None,
            growth: Uncertain::point(0.0),
        }
    }
}

/// The checks of `model::profile::tests::schema_is_strict_and_fully_documented`,
/// applied to one node: closed objects, a description on every property and every
/// variant, a `type` tag on every object variant.
fn walk(name: &str, node: &Value, problems: &mut Vec<String>) {
    let Some(obj) = node.as_object() else { return };
    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        if obj.get("additionalProperties") != Some(&Value::Bool(false)) {
            problems.push(format!("{name}: object is not closed"));
        }
        for (key, prop) in props {
            if key != "type" && prop.get("description").is_none() {
                problems.push(format!("{name}.{key}: no description"));
            }
        }
    }
    if let Some(variants) = obj.get("oneOf").and_then(Value::as_array) {
        for (i, variant) in variants.iter().enumerate() {
            if variant.get("description").is_none() {
                problems.push(format!("{name}[{i}]: variant has no description"));
            }
            if variant["type"] == "object" && variant["properties"]["type"]["const"].is_null() {
                problems.push(format!("{name}[{i}]: variant has no \"type\" tag"));
            }
            walk(&format!("{name}[{i}]"), variant, problems);
        }
    }
}

#[test]
fn schema_is_strict_and_fully_documented() {
    let schema = serde_json::to_value(schemars::schema_for!(Wrapper)).unwrap();
    let mut problems = Vec::new();
    walk("Wrapper", &schema, &mut problems);
    let defs = schema["$defs"].as_object().unwrap();
    for (name, def) in defs {
        walk(name, def, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");

    // The type appears once, by name, with a description of its own, and both the
    // required and the optional field refer to it.
    assert_eq!(defs.keys().collect::<Vec<_>>(), ["Uncertain"]);
    let def = &defs["Uncertain"];
    assert!(def["description"].as_str().unwrap().len() > 100);
    assert_eq!(schema["properties"]["growth"]["$ref"], "#/$defs/Uncertain");
    assert!(schema["properties"]["amount_range"]
        .to_string()
        .contains("#/$defs/Uncertain"));
    // The wrapper's serde defaults reach the schema, which is what lets the
    // assistant's plan view hide a range that is not set.
    assert_eq!(schema["properties"]["amount_range"]["default"], Value::Null);
    assert_eq!(
        schema["properties"]["growth"]["default"],
        json!({"type": "uniform", "low": 0.0, "high": 0.0})
    );

    // Six variants, in order, each a closed object whose fields are all required
    // numbers with help text.
    let expected: [(&str, &[&str]); 6] = [
        ("uniform", &["low", "high"]),
        ("triangular", &["low", "mode", "high"]),
        ("pert", &["low", "mode", "high"]),
        ("normal", &["mean", "sd"]),
        ("lognormal", &["median", "p90"]),
        ("quantiles", &["p10", "p50", "p90"]),
    ];
    let variants = def["oneOf"].as_array().unwrap();
    assert_eq!(variants.len(), expected.len());
    for (variant, (tag, fields)) in variants.iter().zip(expected) {
        assert_eq!(variant["type"], "object", "{tag}");
        assert_eq!(variant["properties"]["type"]["const"], tag);
        assert_eq!(variant["additionalProperties"], false, "{tag}");
        assert!(variant["description"].as_str().unwrap().len() > 40, "{tag}");
        let props = variant["properties"].as_object().unwrap();
        let mut names: Vec<&str> = props.keys().map(String::as_str).collect();
        names.retain(|n| *n != "type");
        assert_eq!(names, fields, "{tag}");
        let required: Vec<&str> = variant["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for field in fields {
            assert_eq!(props[*field]["type"], "number", "{tag}.{field}");
            assert!(props[*field]["description"].as_str().unwrap().len() > 15);
            assert!(required.contains(field), "{tag}.{field} is not required");
        }
        assert!(required.contains(&"type"), "{tag}");
    }

    // And it behaves like any other profile field: omitted means the default, a
    // misspelt key inside it is refused.
    let w: Wrapper = serde_json::from_str("{}").unwrap();
    assert_eq!(w, Wrapper::default());
    let w: Wrapper =
        serde_json::from_str(r#"{"amount_range": {"type": "lognormal", "median": 3, "p90": 9}}"#)
            .unwrap();
    assert_eq!(w.amount_range, Some(lognormal(3.0, 9.0)));
    assert!(
        serde_json::from_str::<Wrapper>(r#"{"growth": {"type": "normal", "mean": 0}}"#).is_err()
    );
    assert_eq!(
        serde_json::from_value::<Wrapper>(serde_json::to_value(&w).unwrap()).unwrap(),
        w
    );
}

// ---------------------------------------------------------------------------------
// Validation

const PATH: &str = "/events/@roof/kind/amount_range";

fn check(dist: Uncertain, bounds: Bounds) -> Report {
    let mut report = Report::default();
    let ok = check_uncertain(&mut report, PATH, "Roof cost", &dist, bounds);
    assert_eq!(ok, report.errors.is_empty());
    assert!(report.warnings.is_empty());
    report
}

#[track_caller]
fn assert_rejected(dist: Uncertain, bounds: Bounds, code: &str, field: &str) -> String {
    let report = check(dist, bounds);
    let path = format!("{PATH}/{field}");
    let found = report
        .errors
        .iter()
        .find(|i| i.code == code && i.path == path);
    let Some(issue) = found else {
        panic!(
            "{dist:?}: expected {code} at {path}, got {:#?}",
            report.errors
        );
    };
    assert!(issue.message.starts_with("Roof cost"), "{}", issue.message);
    assert!(issue.message.ends_with('.'), "{}", issue.message);
    issue.message.clone()
}

#[test]
fn valid_distributions_pass_and_stay_inside_their_bounds() {
    let money = [
        uniform(0.0, 1e12),
        uniform(5.0, 5.0),
        triangular(0.0, 0.0, 10.0),
        triangular(10.0, 20.0, 20.0),
        pert(30_000.0, 40_000.0, 80_000.0),
        pert(7.0, 7.0, 7.0),
        normal(1_000.0, 250.0),
        normal(1_000.0, 0.0),
        lognormal(5_000.0, 20_000.0),
        lognormal(1.0, 1.000_001),
        quantiles(40_000.0, 100_000.0, 300_000.0),
        quantiles(0.0, 0.0, 0.0),
        quantiles(50.0, 50.0, 90.0),
    ];
    for dist in money {
        let report = check(dist, Bounds::MONEY);
        assert!(report.errors.is_empty(), "{dist:?}: {:#?}", report.errors);
        let d = dist.compile();
        assert!(d.min() >= 0.0 && d.max() <= 1e12, "{dist:?}");
    }
    // Negative values are fine where the field allows them.
    for dist in [
        normal(-0.01, 0.02),
        quantiles(-0.03, 0.0, 0.01),
        uniform(-0.5, 0.5),
        pert(-0.02, 0.0, 0.01),
    ] {
        let report = check(dist, Bounds::RATE);
        assert!(report.errors.is_empty(), "{dist:?}: {:#?}", report.errors);
    }
    assert!(check(uniform(0.0, 1.0), Bounds::FRACTION).errors.is_empty());
    assert!(check(normal(-5e11, 1e11), Bounds::ANY).errors.is_empty());
    assert_eq!(Bounds::new(2.0, 3.0), Bounds { lo: 2.0, hi: 3.0 });
}

#[test]
fn non_finite_values_are_rejected_at_their_field() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_rejected(uniform(bad, 1.0), Bounds::MONEY, "out_of_range", "low");
        assert_rejected(uniform(0.0, bad), Bounds::MONEY, "out_of_range", "high");
        assert_rejected(
            triangular(0.0, bad, 1.0),
            Bounds::MONEY,
            "out_of_range",
            "mode",
        );
        assert_rejected(pert(0.0, 1.0, bad), Bounds::MONEY, "out_of_range", "high");
        assert_rejected(normal(bad, 1.0), Bounds::ANY, "out_of_range", "mean");
        assert_rejected(normal(0.0, bad), Bounds::ANY, "out_of_range", "sd");
        assert_rejected(lognormal(bad, 2.0), Bounds::MONEY, "out_of_range", "median");
        assert_rejected(lognormal(1.0, bad), Bounds::MONEY, "out_of_range", "p90");
        assert_rejected(quantiles(bad, 1.0, 2.0), Bounds::ANY, "out_of_range", "p10");
        assert_rejected(quantiles(0.0, bad, 2.0), Bounds::ANY, "out_of_range", "p50");
        assert_rejected(quantiles(0.0, 1.0, bad), Bounds::ANY, "out_of_range", "p90");
    }
    // Every bad field is reported, and nothing else is checked until they are fixed.
    let report = check(triangular(f64::NAN, 5.0, f64::INFINITY), Bounds::MONEY);
    assert_eq!(report.errors.len(), 2);
    let msg = assert_rejected(uniform(f64::NAN, 1.0), Bounds::MONEY, "out_of_range", "low");
    assert_eq!(msg, "Roof cost: low is NaN; it must be a finite number.");
}

#[test]
fn badly_ordered_shapes_are_rejected() {
    let msg = assert_rejected(uniform(5.0, 4.0), Bounds::MONEY, "dist_order", "high");
    assert_eq!(
        msg,
        "Roof cost: high (4) is below low (5); high must be at least low."
    );
    assert_rejected(
        triangular(5.0, 4.5, 4.0),
        Bounds::MONEY,
        "dist_order",
        "high",
    );
    assert_rejected(pert(5.0, 4.5, 4.0), Bounds::MONEY, "dist_order", "high");
    let msg = assert_rejected(
        triangular(1.0, 0.5, 4.0),
        Bounds::MONEY,
        "dist_order",
        "mode",
    );
    assert!(msg.contains("between low (1) and high (4)"), "{msg}");
    assert_rejected(pert(1.0, 4.5, 4.0), Bounds::MONEY, "dist_order", "mode");
    let msg = assert_rejected(normal(10.0, -1.0), Bounds::ANY, "out_of_range", "sd");
    assert!(msg.contains("zero or positive"), "{msg}");
    let msg = assert_rejected(lognormal(0.0, 5.0), Bounds::MONEY, "out_of_range", "median");
    assert!(msg.contains("above zero"), "{msg}");
    assert_rejected(lognormal(-2.0, 5.0), Bounds::ANY, "out_of_range", "median");
    let msg = assert_rejected(lognormal(10.0, 10.0), Bounds::MONEY, "dist_order", "p90");
    assert!(msg.contains("for example 20"), "{msg}");
    assert_rejected(lognormal(10.0, 4.0), Bounds::MONEY, "dist_order", "p90");
    assert_rejected(lognormal(10.0, -4.0), Bounds::MONEY, "dist_order", "p90");
    assert_rejected(quantiles(6.0, 5.0, 9.0), Bounds::ANY, "dist_order", "p10");
    assert_rejected(quantiles(1.0, 5.0, 4.0), Bounds::ANY, "dist_order", "p90");
    // Both ends wrong: both reported.
    let report = check(quantiles(6.0, 5.0, 4.0), Bounds::ANY);
    assert_eq!(report.errors.len(), 2);
}

#[test]
fn values_outside_the_callers_bounds_are_rejected() {
    // Parameters outside the bounds.
    let msg = assert_rejected(uniform(-1.0, 5.0), Bounds::MONEY, "out_of_range", "low");
    assert_eq!(
        msg,
        "Roof cost could be as low as -1; it must stay between 0 and 1000000000000."
    );
    assert_rejected(uniform(0.0, 2e12), Bounds::MONEY, "out_of_range", "high");
    assert_rejected(pert(-5.0, 1.0, 2.0), Bounds::MONEY, "out_of_range", "low");
    assert_rejected(
        triangular(0.0, 1.0, 1.5),
        Bounds::FRACTION,
        "out_of_range",
        "high",
    );
    assert_rejected(normal(-5.0, 0.0), Bounds::MONEY, "out_of_range", "mean");
    assert_rejected(normal(-5.0, 1.0), Bounds::MONEY, "out_of_range", "mean");
    assert_rejected(normal(2.0, 0.1), Bounds::RATE, "out_of_range", "mean");
    assert_rejected(
        lognormal(2e12, 3e12),
        Bounds::MONEY,
        "out_of_range",
        "median",
    );
    assert_rejected(
        quantiles(-1.0, 5.0, 9.0),
        Bounds::MONEY,
        "out_of_range",
        "p10",
    );
    assert_rejected(
        quantiles(1.0, 5.0, 2e12),
        Bounds::MONEY,
        "out_of_range",
        "p90",
    );
    // Both ends outside: both reported.
    assert_eq!(check(uniform(-2.0, 2.0), Bounds::FRACTION).errors.len(), 2);

    // Parameters inside, but the distribution reaches outside: a normal amount
    // whose lower tail is negative (1,000 - 4 * 500 = -1,000).
    let msg = assert_rejected(normal(1_000.0, 500.0), Bounds::MONEY, "out_of_range", "sd");
    assert!(
        msg.contains("as low as -1000") && msg.contains("reduce sd"),
        "{msg}"
    );
    assert_rejected(normal(0.9, 0.1), Bounds::RATE, "out_of_range", "sd");
    // Percentiles of a symmetric amount that imply a negative tail
    // (100 - 3.12 * 100).
    let msg = assert_rejected(
        quantiles(0.0, 100.0, 200.0),
        Bounds::MONEY,
        "out_of_range",
        "p10",
    );
    assert!(msg.contains("raise p10"), "{msg}");
    // A lognormal whose p90 is 100 times its median reaches 1.7 million times the
    // median (exp(4 * ln 100 / 1.28)).
    let msg = assert_rejected(lognormal(1e9, 1e11), Bounds::MONEY, "out_of_range", "p90");
    assert!(msg.contains("bring p90 closer"), "{msg}");
    assert!(check(lognormal(1e3, 1e5), Bounds::MONEY).errors.is_empty());
    // A lognormal confined to a narrow band from below.
    assert_rejected(
        lognormal(2.0, 4.0),
        Bounds::new(1.0, 100.0),
        "out_of_range",
        "p90",
    );
    // A tail too long to be a number at all.
    let msg = assert_rejected(lognormal(1.0, 1e300), Bounds::MONEY, "out_of_range", "p90");
    assert!(msg.contains("too long to represent"), "{msg}");
    assert_rejected(
        lognormal(1.0, 1e300),
        Bounds::new(0.0, f64::INFINITY),
        "out_of_range",
        "p90",
    );
}

#[test]
fn whatever_passes_validation_samples_inside_the_bounds() {
    // A crude fuzz: random parameters, some valid and some not. Anything accepted
    // must compile to a sampler that keeps every draw finite and inside the bounds.
    let mut rng = Rng::from_seed(77);
    let mut accepted = 0;
    for case in 0..4_000 {
        let mut x = [0.0; 3];
        for v in &mut x {
            let scale = math::pow(10.0, rng.uniform_range(-3.0, 13.0));
            *v = scale * rng.uniform_range(-0.3, 1.0);
        }
        let dist = match case % 6 {
            0 => uniform(x[0], x[1]),
            1 => triangular(x[0], x[1], x[2]),
            2 => pert(x[0], x[1], x[2]),
            3 => normal(x[0], x[1]),
            4 => lognormal(x[0], x[1]),
            _ => quantiles(x[0], x[1], x[2]),
        };
        let mut report = Report::default();
        if !check_uncertain(&mut report, PATH, "Value", &dist, Bounds::MONEY) {
            assert!(!report.errors.is_empty());
            continue;
        }
        accepted += 1;
        let d = dist.compile();
        assert!(
            d.min() >= 0.0 && d.max() <= 1e12 && d.min() <= d.max(),
            "{dist:?}"
        );
        assert!(d.mean() >= d.min() && d.mean() <= d.max(), "{dist:?}");
        let mut prev = d.min();
        for k in 0..=64 {
            let v = d.quantile(f64::from(k) / 64.0);
            assert!(v.is_finite() && v >= prev && v <= d.max(), "{dist:?}");
            prev = v;
        }
        for _ in 0..16 {
            let v = d.sample(rng.next_u64());
            assert!(v >= d.min() && v <= d.max(), "{dist:?}");
        }
    }
    assert!(accepted > 100, "only {accepted} random cases were valid");
}

#[test]
fn the_checker_is_reachable_from_validate() {
    // Lanes call it through `crate::validate`, next to the other checks.
    let mut report = Report::default();
    assert!(crate::validate::check_uncertain(
        &mut report,
        "/x",
        "X",
        &pert(1.0, 2.0, 3.0),
        crate::validate::Bounds::MONEY,
    ));
    assert!(report.is_ok());
}

// ---------------------------------------------------------------------------------
// The crate-wide shock samplers

#[test]
fn shared_shock_samplers_are_the_economys_tables() {
    use crate::sampler;
    // Same objects, not copies: nothing about the economy's draws can differ.
    assert!(std::ptr::eq(
        sampler::normal(),
        crate::economy::sampler::normal()
    ));
    assert!(std::ptr::eq(sampler::student_t4(), sampler::student_t(4)));
    assert!(std::ptr::eq(sampler::student_t6(), sampler::student_t(6)));
    // One word in, one variate out; unit variance, fat tails for the t.
    let mut rng = Rng::stream(5, 0, crate::rng::Stream::CareerShape(0));
    let (mut s2, mut s4) = ([0.0; 2], [0.0; 2]);
    let tables = [&sampler::normal().table, &sampler::student_t4().table];
    for _ in 0..400_000 {
        let bits = rng.next_u64();
        for (k, table) in tables.iter().enumerate() {
            let x = table.sample(bits);
            s2[k] += x * x;
            s4[k] += x * x * x * x;
        }
    }
    let n = 400_000.0;
    close(s2[0] / n, 1.0, 0.01, "normal variance");
    close(s4[0] / n, 3.0, 0.08, "normal fourth moment");
    close(s2[1] / n, 1.0, 0.05, "t(4) variance");
    assert!(s4[1] / n > 4.5, "t(4) fourth moment {}", s4[1] / n);
    // A transformed table, normalised to mean 1 the way a caller would.
    let mut jump: sampler::ShockTable = sampler::student_t6().transformed(|x| math::exp(0.2 * x));
    jump.scale(1.0 / jump.mean());
    close(jump.mean(), 1.0, 1e-14, "normalised mean");
    assert!(jump.second_moment() > 1.0);
    assert_eq!(
        sampler::ShockTable::constant(2.5).sample(rng.next_u64()),
        2.5
    );
    close(
        sampler::normal_quantile(0.975),
        1.959_963_984_540_054,
        1e-9,
        "z(0.975)",
    );
    close(
        sampler::student_t_cdf_even(2.776_445, 4),
        0.975,
        1e-6,
        "t(4) CDF",
    );
}
