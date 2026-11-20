//! User-supplied distributions: one uniform in, one value out.
//!
//! Responsibility: turn an [`Uncertain`] (the profile's way of writing "a number I am
//! unsure about") into a [`CompiledDist`] that the simulation can sample without
//! allocating, without rejection and without a transcendental function, and check an
//! `Uncertain` for the validator ([`check_uncertain`]).
//!
//! Contract:
//!
//! * Build a [`CompiledDist`] once, in `Plan::compile` ([`CompiledDist::new`] or
//!   [`CompiledDist::from_option`]). Building may allocate (one table of about 10 KB,
//!   none for `uniform` or a point) and costs up to about a millisecond (`pert`).
//! * [`CompiledDist::sample`] turns exactly one 64-bit word into one value;
//!   [`CompiledDist::quantile`] takes the uniform directly. Both are pure, never
//!   allocate, and return a finite value for every input, including `NaN`.
//! * `quantile` is monotone non-decreasing in `u`, with `quantile(0) == min()` and
//!   values never above `max()`. A larger uniform is never a smaller outcome, so a
//!   caller can reuse one uniform to make two quantities move together.
//! * [`CompiledDist::mean`] is the mean of what `quantile` returns for a uniform `u`
//!   (not of the ideal distribution), to within rounding, so dividing by it gives a
//!   multiplier whose simulated average is 1.
//! * Building never fails: parameters the validator would reject are repaired
//!   (reordered, clamped) or, when not finite, replaced by a point at zero.
//!
//! # The shapes
//!
//! With `z(u)` the standard normal quantile limited to `[-4, 4]` (outcomes beyond 4
//! standard deviations, about 1 in 31,600 on each side, are moved to the limit, which
//! keeps every percentile below that level exactly where the formula puts it):
//!
//! | Variant | `quantile(u)` | Support | Mean |
//! |---|---|---|---|
//! | `uniform` | `low + u (high - low)` | `[low, high]` | `(low + high) / 2` |
//! | `triangular` | inverse of the piecewise-quadratic CDF | `[low, high]` | `(low + mode + high) / 3` |
//! | `pert` | `low + (high - low) B^-1(u; a, b)`, `a = 1 + 4 (mode - low) / (high - low)`, `b = 6 - a` | `[low, high]` | `(low + 4 mode + high) / 6` |
//! | `normal` | `mean + sd z(u)` | `mean -/+ 4 sd` | `mean` |
//! | `lognormal` | `median exp(sigma z(u))`, `sigma = ln(p90 / median) / 1.2815515655446004` | `median exp(-/+ 4 sigma)` | close to `median exp(sigma^2 / 2)` |
//! | `quantiles` | see below | bounded | from the table |
//!
//! `quantiles { p10, p50, p90 }` is a lognormal shifted so that it passes through the
//! three points. With `a = p50 - p10`, `b = p90 - p50`, `z90 = 1.2815515655446004` and
//! `s = ln(b / a) / z90`:
//!
//! ```text
//! quantile(u) = p50 + b * expm1(s z(u)) / expm1(s z90)
//! ```
//!
//! which is `p50 - a` at 10%, `p50` at 50% and `p50 + b` at 90% exactly, tends to the
//! normal `p50 + b z / z90` as the two sides become equal, and works for either
//! direction of skew (`s < 0` when the lower side is the longer one). The skew is
//! limited to a ratio of [`MAX_SKEW_RATIO`] between the sides: beyond it `s` stops
//! growing and each side is scaled separately to its own point,
//!
//! ```text
//! u >= 1/2:  p50 + b * expm1(s z) / expm1(s z90)
//! u <  1/2:  p50 - a * expm1(s z) / expm1(-s z90)
//! ```
//!
//! which is the same curve when `s` is not limited, and otherwise two lognormal
//! halves that meet at the median with a change of slope (a split distribution). The
//! three percentiles are reproduced in every case, the quantile function changes
//! continuously with the inputs, and the support is bounded: at most 14.9 times `b`
//! above the median and 14.9 times `a` below it, 3.12 times each when symmetric.
//!
//! # The table
//!
//! `uniform` and a point are sampled in closed form. Every other shape is a table of
//! its exact quantiles with linear interpolation between them. The knots are spaced
//! geometrically towards both ends: each halving of the distance to an end,
//! `[2^-(k+1), 2^-k)`, is cut into 32 equal cells, down to `2^-21`, where the last
//! cell runs to the end of the support. The cell is found from the exponent and the
//! top mantissa bits of the uniform, so a lookup is integer arithmetic, two loads, a
//! multiply and an add. Relative to the spread of the distribution the interpolation
//! error is below 1e-4 everywhere and far smaller in the body; the reported mean is
//! the exact mean of the interpolated function.
//!
//! Simplifications: no correlation between two distributions is modelled here (a
//! caller that wants it feeds correlated uniforms); `pert` uses the standard weight
//! of 4 on the mode; tails beyond 4 standard deviations are cut for `normal`,
//! `lognormal` and `quantiles`.

use crate::math;
pub use crate::model::Uncertain;
use crate::sampler::normal_quantile;
use crate::validate::Report;

/// The 90th percentile of the standard normal.
const Z90: f64 = 1.281_551_565_544_600_4;

/// `normal`, `lognormal` and `quantiles` never produce a value more than this many
/// standard deviations from the centre. A judgement: far enough that the 99.99th
/// percentile is untouched, close enough that a lognormal tail stays finite money.
pub const Z_CAP: f64 = 4.0;

/// The largest ratio between the two sides of a `quantiles` distribution
/// (`(p90 - p50) / (p50 - p10)` or its inverse) that is fitted by one smooth shifted
/// lognormal. More lopsided inputs keep this much curvature and scale each side
/// separately. A judgement: at 3 the long tail reaches 14.9 times the distance from
/// the median to its percentile; allowing more makes the extremes implausibly large
/// for three hand-entered numbers.
pub const MAX_SKEW_RATIO: f64 = 3.0;

/// Halvings of the distance to each end of the unit interval that get their own
/// cells; the last cell on each side has probability `2^-(LEVELS + 1)`.
const LEVELS: usize = 20;
/// Equal cells per halving, as a power of two.
const SUB_BITS: u32 = 5;
const SUB: usize = 1 << SUB_BITS;
/// Knots on each side including the end of the support (index 0) and the median.
const SIDE: usize = LEVELS * SUB + 2;
/// Knots in a table: both sides, sharing the median.
const KNOTS: usize = 2 * SIDE - 1;
/// Mantissa bits below the cell index.
const FRAC_BITS: u32 = 52 - SUB_BITS;
const FRAC_MASK: u64 = (1 << FRAC_BITS) - 1;
const FRAC_UNIT: f64 = 1.0 / (1u64 << FRAC_BITS) as f64;
/// `2^(LEVELS + 1)`: scales the innermost cell `[0, 2^-(LEVELS + 1))` to `[0, 1)`.
const INNER_SCALE: f64 = (1u64 << (LEVELS + 1)) as f64;
const UNIT53: f64 = 1.0 / (1u64 << 53) as f64;

/// The distance from an end of the unit interval at knot `i` of a side: 0 at the end
/// of the support, then `2^(m - LEVELS - 1) * (1 + s / SUB)`, reaching 1/2 at the
/// last knot.
fn knot_t(i: usize) -> f64 {
    if i == 0 {
        return 0.0;
    }
    let (m, s) = ((i - 1) / SUB, (i - 1) % SUB);
    math::powi(2.0, m as i32 - LEVELS as i32 - 1) * (1.0 + s as f64 / SUB as f64)
}

/// Probability of the cell between knots `i` and `i + 1` of a side.
fn cell_prob(i: usize) -> f64 {
    if i == 0 {
        1.0 / INNER_SCALE
    } else {
        math::powi(2.0, ((i - 1) / SUB) as i32 - LEVELS as i32 - 1) / SUB as f64
    }
}

/// The cell holding `t` (a distance from one end, `0 < t < 1/2`) and the position
/// inside it in `[0, 1)`. Exact: the cells are binary, so the index is the exponent
/// and the top mantissa bits, and the position is the remaining mantissa.
#[inline(always)]
fn locate(t: f64) -> (usize, f64) {
    let bits = t.to_bits();
    // Exponent of t relative to the largest cell group [1/4, 1/2).
    let m = ((bits >> 52) & 0x7ff) as i32 - 1023 + LEVELS as i32 + 1;
    if m < 0 {
        (0, t * INNER_SCALE)
    } else {
        let mantissa = bits & ((1 << 52) - 1);
        (
            1 + m as usize * SUB + (mantissa >> FRAC_BITS) as usize,
            (mantissa & FRAC_MASK) as f64 * FRAC_UNIT,
        )
    }
}

/// A distribution ready to sample: see the module documentation for the contract.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledDist {
    /// Smallest value `quantile` returns.
    lo: f64,
    /// Largest value `quantile` can approach or return.
    hi: f64,
    mean: f64,
    median: f64,
    /// Quantiles at the knots in increasing order of probability, or `None` for a
    /// distribution that is linear in the uniform (`uniform`, a point).
    table: Option<Box<[f64]>>,
}

impl Default for CompiledDist {
    /// The point at zero.
    fn default() -> Self {
        CompiledDist::point(0.0)
    }
}

impl CompiledDist {
    /// A distribution that is always exactly `x`. Does not allocate. A non-finite
    /// `x` gives the point at zero.
    pub fn point(x: f64) -> CompiledDist {
        let x = if x.is_finite() { x } else { 0.0 };
        CompiledDist {
            lo: x,
            hi: x,
            mean: x,
            median: x,
            table: None,
        }
    }

    /// Compiles `dist` when one is given, else the point at `fallback`: lets a caller
    /// treat "a plain number" and "a range" the same way.
    pub fn from_option(dist: Option<&Uncertain>, fallback: f64) -> CompiledDist {
        match dist {
            Some(d) => CompiledDist::new(d),
            None => CompiledDist::point(fallback),
        }
    }

    /// Compiles a distribution. Call it in `Plan::compile`, never per path. Inputs
    /// that [`check_uncertain`] would reject are repaired rather than refused: limits
    /// are put in order, the mode is moved inside them, a negative `sd` counts as
    /// zero, and anything not finite gives the point at zero.
    pub fn new(dist: &Uncertain) -> CompiledDist {
        match *dist {
            Uncertain::Uniform { low, high } => {
                if !(low.is_finite() && high.is_finite()) {
                    return CompiledDist::point(0.0);
                }
                let (low, high) = (low.min(high), low.max(high));
                let width = high - low;
                if !width.is_finite() {
                    return CompiledDist::point(0.0);
                }
                let mid = low + 0.5 * width;
                CompiledDist {
                    lo: low,
                    hi: high,
                    mean: mid,
                    median: mid,
                    table: None,
                }
            }
            Uncertain::Triangular { low, mode, high } => {
                let Some((low, mode, high)) = three_points(low, mode, high) else {
                    return CompiledDist::point(0.0);
                };
                if low == high {
                    return CompiledDist::point(low);
                }
                let width = high - low;
                let (left, right) = (mode - low, high - mode);
                // P(X <= mode), and the two closed-form branches measured from the
                // nearer end so neither loses precision in its tail.
                let below = left / width;
                let rising = |p: f64| low + (p * width * left).sqrt();
                let falling = |q: f64| high - (q * width * right).sqrt();
                CompiledDist::tabulate(
                    |t| {
                        if t < below {
                            rising(t)
                        } else {
                            falling(1.0 - t)
                        }
                    },
                    |t| {
                        if t <= 1.0 - below {
                            falling(t)
                        } else {
                            rising(1.0 - t)
                        }
                    },
                    low,
                    high,
                )
            }
            Uncertain::Pert { low, mode, high } => {
                let Some((low, mode, high)) = three_points(low, mode, high) else {
                    return CompiledDist::point(0.0);
                };
                if low == high {
                    return CompiledDist::point(low);
                }
                let width = high - low;
                let a = (1.0 + 4.0 * (mode - low) / width).clamp(1.0, 5.0);
                let b = 6.0 - a;
                // ln B(a, b) with a + b = 6, so Gamma(a + b) = 5! = 120.
                let ln_beta = math::ln_gamma(a) + math::ln_gamma(b) - math::ln(120.0);
                // Knots are visited in increasing distance from each end, so each
                // inversion starts from the previous root.
                let (mut from_low, mut from_high) = (0.0, 0.0);
                CompiledDist::tabulate(
                    |t| {
                        from_low = beta_quantile(t, a, b, ln_beta, from_low);
                        low + width * from_low
                    },
                    |t| {
                        from_high = beta_quantile(t, b, a, ln_beta, from_high);
                        high - width * from_high
                    },
                    low,
                    high,
                )
            }
            Uncertain::Normal { mean, sd } => {
                if !(mean.is_finite() && sd.is_finite()) {
                    return CompiledDist::point(0.0);
                }
                let sd = sd.max(0.0);
                CompiledDist::tabulate(
                    |t| mean + sd * capped_z(t),
                    |t| mean - sd * capped_z(t),
                    mean - Z_CAP * sd,
                    mean + Z_CAP * sd,
                )
            }
            Uncertain::Lognormal { median, p90 } => {
                if !(median.is_finite() && p90.is_finite()) || median <= 0.0 {
                    return CompiledDist::point(0.0);
                }
                let sigma = if p90 > median {
                    math::ln(p90 / median) / Z90
                } else {
                    0.0
                };
                CompiledDist::tabulate(
                    |t| median * math::exp(sigma * capped_z(t)),
                    |t| median * math::exp(-sigma * capped_z(t)),
                    median * math::exp(-Z_CAP * sigma),
                    median * math::exp(Z_CAP * sigma),
                )
            }
            Uncertain::Quantiles { p10, p50, p90 } => {
                if !(p10.is_finite() && p50.is_finite() && p90.is_finite()) {
                    return CompiledDist::point(0.0);
                }
                let (below, above) = ((p50 - p10).max(0.0), (p90 - p50).max(0.0));
                let max_s = math::ln(MAX_SKEW_RATIO) / Z90;
                let s = if below > 0.0 && above > 0.0 {
                    (math::ln(above / below) / Z90).clamp(-max_s, max_s)
                } else if above > below {
                    max_s
                } else if below > above {
                    -max_s
                } else {
                    0.0
                };
                // expm1(s z) / expm1(s z90), with its limit z / z90 as s goes to 0.
                let curve = |z: f64, at: f64| {
                    if s.abs() < 1e-9 {
                        z / at
                    } else {
                        math::exp_m1(s * z) / math::exp_m1(s * at)
                    }
                };
                let lower = |z: f64| p50 - below * curve(z, -Z90);
                let upper = |z: f64| p50 + above * curve(z, Z90);
                CompiledDist::tabulate(
                    |t| lower(capped_z(t)),
                    |t| upper(-capped_z(t)),
                    lower(-Z_CAP),
                    upper(Z_CAP),
                )
            }
        }
    }

    /// Builds the table from the exact quantile function given from each end:
    /// `lower(t)` is the quantile at probability `t` and `upper(t)` the quantile at
    /// `1 - t`, both for `0 < t <= 1/2` in increasing order of `t`. `min` and `max`
    /// are the ends of the support.
    fn tabulate(
        mut lower: impl FnMut(f64) -> f64,
        mut upper: impl FnMut(f64) -> f64,
        min: f64,
        max: f64,
    ) -> CompiledDist {
        if !(min.is_finite() && max.is_finite() && (max - min).is_finite()) {
            return CompiledDist::point(0.0);
        }
        if min >= max {
            return CompiledDist::point(min);
        }
        let mut q = vec![0.0; KNOTS];
        q[0] = min;
        q[KNOTS - 1] = max;
        for i in 1..SIDE {
            let t = knot_t(i);
            q[i] = lower(t);
            // The two sides meet at the median; the lower side's value is kept.
            if i < SIDE - 1 {
                q[KNOTS - 1 - i] = upper(t);
            }
        }
        // Whatever rounding did, the table is finite, inside the support and sorted.
        let mut floor = min;
        for v in q.iter_mut() {
            if !v.is_finite() {
                return CompiledDist::point(0.0);
            }
            floor = floor.max(v.min(max));
            *v = floor;
        }
        // Exact mean of the piecewise-linear quantile function, smallest cells first.
        let mut mean = 0.0;
        for i in 0..SIDE - 1 {
            let low_side = q[i] - min + (q[i + 1] - min);
            let high_side = q[KNOTS - 1 - i] - min + (q[KNOTS - 2 - i] - min);
            mean += 0.5 * cell_prob(i) * (low_side + high_side);
        }
        CompiledDist {
            lo: min,
            hi: max,
            mean: (min + mean).clamp(min, max),
            median: q[SIDE - 1],
            table: Some(q.into()),
        }
    }

    /// The value at probability `u`: the inverse cumulative distribution function.
    /// `u` is a uniform in `[0, 1)`; anything below 0 (or `NaN`) counts as 0 and
    /// anything from 1 up as the top of the support. Finite and monotone
    /// non-decreasing in `u`; never allocates.
    #[inline]
    pub fn quantile(&self, u: f64) -> f64 {
        let Some(q) = self.table.as_deref() else {
            let u = if u > 0.0 { u.min(1.0) } else { 0.0 };
            return (self.lo + u * (self.hi - self.lo)).min(self.hi);
        };
        if u < 0.5 {
            if u > 0.0 {
                let (i, frac) = locate(u);
                let (a, b) = (q[i], q[i + 1]);
                (a + frac * (b - a)).min(b)
            } else {
                self.lo
            }
        } else if u < 1.0 {
            // Exact for u >= 1/2, so the upper tail is resolved as finely as the lower.
            let t = 1.0 - u;
            if t >= 0.5 {
                return q[SIDE - 1];
            }
            let (i, frac) = locate(t);
            let (a, b) = (q[KNOTS - 1 - i], q[KNOTS - 2 - i]);
            (a + frac * (b - a)).max(b)
        } else if u >= 1.0 {
            self.hi
        } else {
            // NaN.
            self.lo
        }
    }

    /// One value from one 64-bit random word (`Rng::next_u64`): the quantile at a
    /// uniform built from the top 53 bits. Exactly one draw per value, whatever the
    /// distribution, so the stream position never depends on the profile.
    #[inline]
    pub fn sample(&self, bits: u64) -> f64 {
        self.quantile((bits >> 11) as f64 * UNIT53)
    }

    /// The mean of the sampled values: the integral of [`CompiledDist::quantile`]
    /// over `u`. Exact for the sampler as built, so `sample(bits) / mean()` averages
    /// to 1 (when the mean is not zero).
    #[inline]
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// The median: `quantile(0.5)`.
    #[inline]
    pub fn median(&self) -> f64 {
        self.median
    }

    /// The smallest value that can be sampled: `quantile(0.0)`.
    #[inline]
    pub fn min(&self) -> f64 {
        self.lo
    }

    /// The upper end of the sampled values: `quantile(u)` approaches it as `u`
    /// approaches 1 and never exceeds it.
    #[inline]
    pub fn max(&self) -> f64 {
        self.hi
    }

    /// True when every sample is the same value.
    #[inline]
    pub fn is_point(&self) -> bool {
        self.lo == self.hi
    }
}

impl Uncertain {
    /// Compiles this distribution into its sampler: [`CompiledDist::new`].
    pub fn compile(&self) -> CompiledDist {
        CompiledDist::new(self)
    }
}

/// Puts a three-point estimate in order (`low <= mode <= high`), or `None` when a
/// value is not finite or the range is too wide to subtract.
fn three_points(low: f64, mode: f64, high: f64) -> Option<(f64, f64, f64)> {
    if !(low.is_finite() && mode.is_finite() && high.is_finite()) {
        return None;
    }
    let (low, high) = (low.min(high), low.max(high));
    if !(high - low).is_finite() {
        return None;
    }
    Some((low, mode.clamp(low, high), high))
}

/// The standard normal quantile at probability `t <= 1/2`, limited to `-Z_CAP`. The
/// upper half is its mirror image.
fn capped_z(t: f64) -> f64 {
    if t <= 0.0 {
        -Z_CAP
    } else if t >= 0.5 {
        0.0
    } else {
        normal_quantile(t).clamp(-Z_CAP, 0.0)
    }
}

/// Regularised incomplete beta function `I_x(a, b)`: the CDF of the beta
/// distribution, by the continued fraction of Numerical Recipes (section 6.4), which
/// keeps relative accuracy in the lower tail. `ln_beta` is `ln B(a, b)`.
fn beta_cdf(x: f64, a: f64, b: f64, ln_beta: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front = math::exp(a * math::ln(x) + b * math::ln_1p(-x) - ln_beta);
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_fraction(x, a, b) / a
    } else {
        1.0 - front * beta_fraction(1.0 - x, b, a) / b
    }
}

/// The continued fraction of the incomplete beta function, by the modified Lentz
/// method. Converges in a few dozen terms for `x < (a + 1) / (a + b + 2)`.
fn beta_fraction(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let guard = |v: f64| if v.abs() < TINY { TINY } else { v };
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 / guard(1.0 - qab * x / qap);
    let mut h = d;
    for m in 1..=300 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        let even = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 / guard(1.0 + even * d);
        c = guard(1.0 + even / c);
        h *= d * c;
        let odd = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 / guard(1.0 + odd * d);
        c = guard(1.0 + odd / c);
        let step = d * c;
        h *= step;
        if (step - 1.0).abs() < 1e-15 {
            break;
        }
    }
    h
}

/// The `t` quantile of the beta distribution, `0 <= t <= 1/2`, by Newton's method
/// kept inside a bracket. `floor` is a point known to be at or below the root (the
/// root for a smaller `t`, or 0).
fn beta_quantile(t: f64, a: f64, b: f64, ln_beta: f64, floor: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    let (mut lo, mut hi) = (floor.clamp(0.0, 1.0), 1.0);
    // Start from the previous root, or from the leading term of the CDF near zero,
    // x^a / (a B(a, b)).
    let mut x = if lo > 0.0 {
        lo
    } else {
        math::exp((math::ln(t * a) + ln_beta) / a).min(0.5)
    };
    for _ in 0..100 {
        let err = beta_cdf(x, a, b, ln_beta) - t;
        if err == 0.0 {
            break;
        }
        if err < 0.0 {
            lo = x;
        } else {
            hi = x;
        }
        let density = math::exp((a - 1.0) * math::ln(x) + (b - 1.0) * math::ln_1p(-x) - ln_beta);
        let mut next = x - err / density;
        if !(next > lo && next < hi) {
            next = 0.5 * (lo + hi);
        }
        let moved = (next - x).abs();
        x = next;
        if moved <= 1e-13 * x || hi - lo <= 1e-15 * hi {
            break;
        }
    }
    x
}

/// The range of values a field accepts, for [`check_uncertain`]: every value the
/// distribution can produce must lie in `lo..=hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Smallest acceptable value.
    pub lo: f64,
    /// Largest acceptable value.
    pub hi: f64,
}

impl Bounds {
    /// Dollar amounts: zero to a trillion.
    pub const MONEY: Bounds = Bounds::new(0.0, 1e12);
    /// A fraction or probability: zero to one.
    pub const FRACTION: Bounds = Bounds::new(0.0, 1.0);
    /// An annual rate or a rate adjustment as a decimal: minus one to one.
    pub const RATE: Bounds = Bounds::new(-1.0, 1.0);
    /// Any finite number up to a trillion in size, of either sign.
    pub const ANY: Bounds = Bounds::new(-1e12, 1e12);

    /// Values from `lo` to `hi` inclusive.
    pub const fn new(lo: f64, hi: f64) -> Bounds {
        Bounds { lo, hi }
    }
}

/// Checks a user-supplied distribution and records every problem as an error on
/// `report`. Returns true when it is valid.
///
/// * `path` is the patch path of the `Uncertain` object itself
///   (`/events/@roof/kind/amount_range`); each error is reported at the field to
///   change (`.../amount_range/high`), which is the path the editor binds to.
/// * `what` names the quantity in messages ("Windfall amount").
/// * `bounds` is the range the field accepts. It is applied to the values the
///   distribution can actually produce ([`CompiledDist::min`] and
///   [`CompiledDist::max`]), not only to the parameters: a normal with mean 1,000 and
///   sd 500 fails `Bounds::MONEY` because it can go negative.
///
/// Codes: `out_of_range` (a value is not finite, a spread is negative, a lognormal
/// parameter is not positive, or the distribution reaches outside `bounds`) and
/// `dist_order` (limits or percentiles in the wrong order, a mode outside its
/// limits).
pub fn check_uncertain(
    report: &mut Report,
    path: &str,
    what: &str,
    dist: &Uncertain,
    bounds: Bounds,
) -> bool {
    let issues = uncertain_issues(what, dist, bounds);
    for (code, field, message) in &issues {
        report.error(code, &format!("{path}/{field}"), message.as_str());
    }
    issues.is_empty()
}

/// The problems with a distribution as `(code, field, message)`. Later checks assume
/// the earlier ones passed, so the list stops at the first stage that finds anything:
/// finite values, then a sound shape, then the reach of the distribution.
fn uncertain_issues(
    what: &str,
    dist: &Uncertain,
    bounds: Bounds,
) -> Vec<(&'static str, &'static str, String)> {
    const RANGE: &str = "out_of_range";
    const ORDER: &str = "dist_order";
    let mut found = Vec::new();

    let (fields, n): ([(&'static str, f64); 3], usize) = match *dist {
        Uncertain::Uniform { low, high } => ([("low", low), ("high", high), ("", 0.0)], 2),
        Uncertain::Triangular { low, mode, high } | Uncertain::Pert { low, mode, high } => {
            ([("low", low), ("mode", mode), ("high", high)], 3)
        }
        Uncertain::Normal { mean, sd } => ([("mean", mean), ("sd", sd), ("", 0.0)], 2),
        Uncertain::Lognormal { median, p90 } => ([("median", median), ("p90", p90), ("", 0.0)], 2),
        Uncertain::Quantiles { p10, p50, p90 } => ([("p10", p10), ("p50", p50), ("p90", p90)], 3),
    };
    for &(name, v) in &fields[..n] {
        if !v.is_finite() {
            found.push((
                RANGE,
                name,
                format!("{what}: {name} is {v}; it must be a finite number."),
            ));
        }
    }
    if !found.is_empty() {
        return found;
    }

    match *dist {
        Uncertain::Uniform { low, high } => {
            if low > high {
                found.push((
                    ORDER,
                    "high",
                    format!(
                        "{what}: high ({high}) is below low ({low}); high must be at least low."
                    ),
                ));
            }
        }
        Uncertain::Triangular { low, mode, high } | Uncertain::Pert { low, mode, high } => {
            if low > high {
                found.push((
                    ORDER,
                    "high",
                    format!(
                        "{what}: high ({high}) is below low ({low}); high must be at least low."
                    ),
                ));
            } else if mode < low || mode > high {
                found.push((
                    ORDER,
                    "mode",
                    format!(
                        "{what}: mode ({mode}) is outside the range; the most likely value must be between low ({low}) and high ({high})."
                    ),
                ));
            }
        }
        Uncertain::Normal { sd, .. } => {
            if sd < 0.0 {
                found.push((
                    RANGE,
                    "sd",
                    format!("{what}: sd is {sd}; a standard deviation must be zero or positive."),
                ));
            }
        }
        Uncertain::Lognormal { median, p90 } => {
            if median <= 0.0 {
                found.push((
                    RANGE,
                    "median",
                    format!(
                        "{what}: median is {median}; a lognormal value must be above zero. Use the normal or quantiles shape for values that can be zero or negative."
                    ),
                ));
            } else if p90 <= median {
                found.push((
                    ORDER,
                    "p90",
                    format!(
                        "{what}: p90 ({p90}) is not above median ({median}); the 90th percentile must be greater than the median, for example {}.",
                        2.0 * median
                    ),
                ));
            }
        }
        Uncertain::Quantiles { p10, p50, p90 } => {
            if p10 > p50 {
                found.push((
                    ORDER,
                    "p10",
                    format!("{what}: p10 ({p10}) is above p50 ({p50}); the 10th percentile must not exceed the median."),
                ));
            }
            if p90 < p50 {
                found.push((
                    ORDER,
                    "p90",
                    format!("{what}: p90 ({p90}) is below p50 ({p50}); the 90th percentile must not be less than the median."),
                ));
            }
        }
    }
    if !found.is_empty() {
        return found;
    }

    // The shape is sound: every value it can produce must be acceptable. Each error
    // goes to the field that sets the offending end, with what to do about it.
    let Bounds { lo, hi } = bounds;
    let compiled = CompiledDist::new(dist);
    let (min, max) = (compiled.min(), compiled.max());
    let ((low_field, low_why), (high_field, high_why)) = match *dist {
        Uncertain::Uniform { .. } | Uncertain::Triangular { .. } | Uncertain::Pert { .. } => {
            (("low", ""), ("high", ""))
        }
        Uncertain::Normal { mean, .. } => (
            if mean < lo {
                ("mean", "")
            } else {
                (
                    "sd",
                    " (the mean less 4 standard deviations); reduce sd, or use the pert or lognormal shape",
                )
            },
            if mean > hi {
                ("mean", "")
            } else {
                ("sd", " (the mean plus 4 standard deviations); reduce sd")
            },
        ),
        Uncertain::Lognormal { median, .. } => (
            if median < lo {
                ("median", "")
            } else {
                (
                    "p90",
                    " (the lower tail runs well below the median when p90 is many times the median); bring p90 closer to the median",
                )
            },
            if median > hi {
                ("median", "")
            } else {
                (
                    "p90",
                    " (the upper tail runs far beyond p90 when p90 is many times the median); bring p90 closer to the median",
                )
            },
        ),
        Uncertain::Quantiles { p10, p90, .. } => (
            if p10 < lo {
                ("p10", "")
            } else {
                (
                    "p10",
                    " (the lower tail runs beyond p10); raise p10 towards p50",
                )
            },
            if p90 > hi {
                ("p90", "")
            } else {
                (
                    "p90",
                    " (the upper tail runs beyond p90); bring p90 closer to p50",
                )
            },
        ),
    };
    if min < lo {
        found.push((
            RANGE,
            low_field,
            format!(
                "{what} could be as low as {min}{low_why}; it must stay between {lo} and {hi}."
            ),
        ));
    }
    // A tail too long to represent as a number compiles to a point; say so rather
    // than accept it.
    let representable = !compiled.is_point() || is_degenerate(dist);
    if !representable {
        found.push((
            RANGE,
            high_field,
            format!("{what} has a tail too long to represent{high_why}; it must stay between {lo} and {hi}."),
        ));
    } else if max > hi {
        found.push((
            RANGE,
            high_field,
            format!(
                "{what} could be as high as {max}{high_why}; it must stay between {lo} and {hi}."
            ),
        ));
    }
    found
}

/// True when a valid distribution has no spread by its own parameters.
fn is_degenerate(dist: &Uncertain) -> bool {
    match *dist {
        Uncertain::Uniform { low, high }
        | Uncertain::Triangular { low, high, .. }
        | Uncertain::Pert { low, high, .. } => low == high,
        Uncertain::Normal { mean, sd } => sd == 0.0 || mean - Z_CAP * sd == mean + Z_CAP * sd,
        Uncertain::Lognormal { .. } => false,
        Uncertain::Quantiles { p10, p90, .. } => p10 == p90,
    }
}

// Beside this file, not in a `dist/` folder: the repository ignores every directory
// of that name (web build output).
#[cfg(test)]
#[path = "dist_tests.rs"]
mod tests;
