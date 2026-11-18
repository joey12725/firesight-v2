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
    }
}
