//! Deterministic elementary functions.
//!
//! `f64::ln`, `exp`, `powf` and friends call the platform libm on native targets and a
//! different implementation on wasm32, and the two can differ in the last bit. One ulp
//! of difference in a return compounds into a different retirement month, which breaks
//! the "same seed, same bytes" contract. Everything in core therefore calls these
//! wrappers around the pure-Rust `libm` crate. `clippy.toml` bans the `std` methods.
//!
//! `sqrt`, `abs`, `floor`, `ceil`, `round`, `min`, `max` are exact IEEE operations and
//! may be used directly.

/// Natural logarithm.
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// `ln(1 + x)`, accurate for small `x`.
#[inline]
pub fn ln_1p(x: f64) -> f64 {
    libm::log1p(x)
}

/// Base-10 logarithm.
#[inline]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

/// `e^x`.
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// `e^x - 1`, accurate for small `x`.
#[inline]
pub fn exp_m1(x: f64) -> f64 {
    libm::expm1(x)
}

/// Complementary error function `1 - erf(x)`.
#[inline]
pub fn erfc(x: f64) -> f64 {
    libm::erfc(x)
}

/// Standard normal cumulative distribution function, `P(Z <= x)`, through `erfc` so
/// the lower tail keeps its relative accuracy (`normal_cdf(-6.0)` is about 9.87e-10,
/// not zero). The inverse is [`crate::rng::inverse_normal_cdf`].
///
/// `health.rs` keeps its own private copies of the normal CDF and its inverse: its
/// results are pinned bit for bit, so it must not pick up a change made here.
#[inline]
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

/// Standard normal density, `exp(-x^2 / 2) / sqrt(2 pi)`.
#[inline]
pub fn normal_pdf(x: f64) -> f64 {
    const INV_SQRT_2PI: f64 = 0.398_942_280_401_432_7;
    INV_SQRT_2PI * exp(-0.5 * x * x)
}

/// Natural logarithm of the absolute value of the gamma function, `ln |Gamma(x)|`.
/// For a positive integer `n`, `ln_gamma(n) = ln((n - 1)!)`.
#[inline]
pub fn ln_gamma(x: f64) -> f64 {
    libm::lgamma(x)
}

/// `x^y` for real `y`.
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// `x^n` by repeated squaring. Exact sequence of multiplications, so deterministic.
#[inline]
pub fn powi(x: f64, n: i32) -> f64 {
    let mut base = if n < 0 { 1.0 / x } else { x };
    let mut e = n.unsigned_abs();
    let mut acc = 1.0;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        base *= base;
        e >>= 1;
    }
    acc
}

/// Converts an annual rate to the equivalent compounding monthly rate:
/// `(1 + r)^(1/12) - 1`.
#[inline]
pub fn annual_to_monthly(rate: f64) -> f64 {
    exp_m1(ln_1p(rate) / 12.0)
}

/// Converts a monthly rate to the equivalent annual rate: `(1 + r)^12 - 1`.
#[inline]
pub fn monthly_to_annual(rate: f64) -> f64 {
    powi(1.0 + rate, 12) - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powi_matches_repeated_multiplication() {
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(2.0, -2), 0.25);
        assert_eq!(powi(7.5, 0), 1.0);
    }

    #[test]
    fn rate_conversions_round_trip() {
        let m = annual_to_monthly(0.07);
        assert!((monthly_to_annual(m) - 0.07).abs() < 1e-12);
        assert!((exp(ln(3.5)) - 3.5).abs() < 1e-12);
        assert!((pow(2.0, 0.5) - std::f64::consts::SQRT_2).abs() < 1e-15);
        assert!((log10(1000.0) - 3.0).abs() < 1e-12);
        assert_eq!(normal_cdf(0.0), 0.5);
        assert!((normal_cdf(1.0) - 0.841_344_746_068_543).abs() < 1e-12);
    }

    #[test]
    fn normal_cdf_and_pdf_match_published_values() {
        // Standard table values of the normal CDF.
        assert_eq!(normal_cdf(0.0), 0.5);
        for (x, p) in [
            (1.0, 0.841_344_746_068_542_9),
            (-1.96, 0.024_997_895_148_220_43),
            (1.281_551_565_544_600_4, 0.9),
            (2.326_347_874_040_841, 0.99),
            (-4.0, 3.167_124_183_311_996e-5),
        ] {
            assert!((normal_cdf(x) - p).abs() < 1e-14, "x={x}");
        }
        // The far lower tail keeps relative accuracy.
        assert!((normal_cdf(-6.0) / 9.865_876_450_377_014e-10 - 1.0).abs() < 1e-12);
        // Symmetry, and agreement with the inverse used for sampling (Acklam's
        // approximation, relative error 1.2e-9).
        for x in [-3.0, -1.5, -0.2, 0.7, 2.5] {
            assert!((normal_cdf(x) + normal_cdf(-x) - 1.0).abs() < 1e-15);
            assert!((crate::rng::inverse_normal_cdf(normal_cdf(x)) - x).abs() < 1e-8);
        }
        // Density: 1 / sqrt(2 pi) at the centre, and exp(-1/2) of that one sd out.
        assert!((normal_pdf(0.0) - 0.398_942_280_401_432_7).abs() < 1e-16);
        assert!((normal_pdf(1.0) - 0.241_970_724_519_143_37).abs() < 1e-15);
        assert_eq!(normal_pdf(-2.0), normal_pdf(2.0));
    }

    #[test]
    fn ln_gamma_matches_factorials_and_half_integers() {
        // Gamma(1) = Gamma(2) = 1; Gamma(5) = 4! = 24; Gamma(11) = 10! = 3,628,800.
        assert!(ln_gamma(1.0).abs() < 1e-15);
        assert!(ln_gamma(2.0).abs() < 1e-15);
        assert!((ln_gamma(5.0) - ln(24.0)).abs() < 1e-13);
        assert!((ln_gamma(11.0) - ln(3_628_800.0)).abs() < 1e-12);
        // Gamma(1/2) = sqrt(pi); Gamma(5/2) = 3 sqrt(pi) / 4.
        let sqrt_pi = std::f64::consts::PI.sqrt();
        assert!((ln_gamma(0.5) - ln(sqrt_pi)).abs() < 1e-14);
        assert!((ln_gamma(2.5) - ln(0.75 * sqrt_pi)).abs() < 1e-14);
        // The recurrence Gamma(x + 1) = x Gamma(x) at a non-integer point.
        assert!((ln_gamma(3.2) - ln_gamma(2.2) - ln(2.2)).abs() < 1e-14);
    }
}
