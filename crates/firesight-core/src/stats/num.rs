//! Small numeric helpers shared by the summary and the comparison: estimates with
//! intervals, quantiles of sorted samples, a fast deterministic sort, rounding, and
//! the compact JSON number encoding.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 97.5th percentile of the standard normal: every interval reported is 95%.
pub(crate) const Z95: f64 = 1.959_963_984_540_054;

/// Rounds to `decimals` decimal places (0..=6); non-finite values become NaN.
pub(crate) fn round_dp(x: f64, decimals: u32) -> f64 {
    const SCALE: [f64; 7] = [1.0, 10.0, 100.0, 1e3, 1e4, 1e5, 1e6];
    if !x.is_finite() {
        return f64::NAN;
    }
    let k = SCALE[decimals.min(6) as usize];
    (x * k).round() / k
}

/// JSON encoding for one `f64`: NaN and infinities become `null`, whole numbers are
/// written without a fraction. Reads `null` back as NaN. JSON only (not for
/// non-self-describing formats).
pub(crate) mod nan {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Largest magnitude written as an integer (2^53: every integer below is exact).
    const MAX_EXACT_INT: f64 = 9_007_199_254_740_992.0;

    pub(crate) struct Num(pub f64);

    impl Serialize for Num {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            serialize(&self.0, s)
        }
    }

    pub(crate) fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if !v.is_finite() {
            s.serialize_none()
        } else if v.trunc() == *v && v.abs() < MAX_EXACT_INT {
            s.serialize_i64(*v as i64)
        } else {
            s.serialize_f64(*v)
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
    }
}

/// [`nan`] for a `Vec<f64>`.
pub(crate) mod nan_vec {
    use super::nan::Num;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(v: &[f64], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(|x| Num(*x)))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
        let raw = Vec::<Option<f64>>::deserialize(d)?;
        Ok(raw.into_iter().map(|x| x.unwrap_or(f64::NAN)).collect())
    }
}

/// [`nan`] for a `Vec<Vec<f64>>`.
pub(crate) mod nan_vec2 {
    use super::nan::Num;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    struct Row<'a>(&'a [f64]);

    impl Serialize for Row<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_seq(self.0.iter().map(|x| Num(*x)))
        }
    }

    pub(crate) fn serialize<S: Serializer>(v: &[Vec<f64>], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(|row| Row(row)))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<f64>>, D::Error> {
        let raw = Vec::<Vec<Option<f64>>>::deserialize(d)?;
        Ok(raw
            .into_iter()
            .map(|row| row.into_iter().map(|x| x.unwrap_or(f64::NAN)).collect())
            .collect())
    }
}

/// A point estimate with its Monte Carlo standard error and 95% interval. In JSON a
/// value that cannot be computed (no paths) is `null`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Estimate {
    /// Point estimate.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub value: f64,
    /// Standard error from the finite number of paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub std_error: f64,
    /// Lower end of the 95% interval (Wilson score for proportions, normal for means
    /// and differences).
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub lo: f64,
    /// Upper end of the 95% interval.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub hi: f64,
}

impl Default for Estimate {
    fn default() -> Self {
        Estimate::NONE
    }
}

impl Estimate {
    /// "Not available".
    pub const NONE: Estimate = Estimate {
        value: f64::NAN,
        std_error: f64::NAN,
        lo: f64::NAN,
        hi: f64::NAN,
    };

    /// A proportion `k / n` with the binomial standard error and the Wilson score
    /// interval, which behaves at 0, at 1 and for small `n` where the normal interval
    /// does not.
    pub fn proportion(k: u64, n: u64) -> Estimate {
        if n == 0 {
            return Estimate::NONE;
        }
        let nf = n as f64;
        let p = k as f64 / nf;
        let z2 = Z95 * Z95;
        let denom = 1.0 + z2 / nf;
        let centre = (p + z2 / (2.0 * nf)) / denom;
        let half = Z95 * (p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt() / denom;
        Estimate {
            value: p,
            std_error: (p * (1.0 - p) / nf).sqrt(),
            lo: (centre - half).max(0.0),
            hi: (centre + half).min(1.0),
        }
    }

    /// A value with a standard error and the normal 95% interval.
    pub fn normal(value: f64, std_error: f64) -> Estimate {
        Estimate {
            value,
            std_error,
            lo: value - Z95 * std_error,
            hi: value + Z95 * std_error,
        }
    }

    /// The mean of `values` with its standard error.
    pub fn mean(values: impl Iterator<Item = f64>) -> Estimate {
        let (n, mean, sd) = moments(values);
        if n == 0 {
            Estimate::NONE
        } else {
            Estimate::normal(mean, sd / (n as f64).sqrt())
        }
    }

    /// The ratio `sum(x) / sum(y)` over paths with the linearised (cluster-robust)
    /// standard error, for shares of path-years where years of one path are not
    /// independent.
    pub fn ratio(pairs: impl Iterator<Item = (f64, f64)> + Clone) -> Estimate {
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0u64);
        for (x, y) in pairs.clone() {
            sx += x;
            sy += y;
            n += 1;
        }
        if n == 0 || sy <= 0.0 {
            return Estimate::NONE;
        }
        let r = sx / sy;
        let ss: f64 = pairs.map(|(x, y)| (x - r * y) * (x - r * y)).sum();
        let correction = if n > 1 {
            n as f64 / (n - 1) as f64
        } else {
            1.0
        };
        let e = Estimate::normal(r, (ss * correction).sqrt() / sy);
        Estimate {
            lo: e.lo.max(0.0),
            ..e
        }
    }

    /// The same estimate rounded for output.
    pub(crate) fn rounded(self, decimals: u32) -> Estimate {
        Estimate {
            value: round_dp(self.value, decimals),
            std_error: round_dp(self.std_error, (decimals + 2).min(6)),
            lo: round_dp(self.lo, decimals),
            hi: round_dp(self.hi, decimals),
        }
    }

    /// True if zero lies inside the 95% interval (or the estimate is unavailable).
    pub(crate) fn within_noise(&self) -> bool {
        !(self.lo > 0.0 || self.hi < 0.0)
    }
}

/// Count, mean and sample standard deviation.
pub(crate) fn moments(values: impl Iterator<Item = f64>) -> (u64, f64, f64) {
    // Welford's update: one pass, numerically stable, deterministic in input order.
    let (mut n, mut mean, mut m2) = (0u64, 0.0f64, 0.0f64);
    for x in values {
        n += 1;
        let d = x - mean;
        mean += d / n as f64;
        m2 += d * (x - mean);
    }
    let sd = if n > 1 {
        (m2 / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    (n, mean, sd)
}

fn sort_key(v: f32) -> u32 {
    let b = v.to_bits();
    if b >> 31 == 1 {
        !b
    } else {
        b | 0x8000_0000
    }
}

fn sort_unkey(k: u32) -> f32 {
    f32::from_bits(if k >> 31 == 1 { k & 0x7fff_ffff } else { !k })
}

/// Below this length a comparison sort beats the radix passes.
const RADIX_MIN: usize = 2_048;

/// The finite values of a column in ascending order. Three-pass LSD radix sort on the
/// order-preserving integer image of the floats: about ten times faster than a
/// comparison sort at 100,000 values, and independent of the input order.
pub(crate) fn sorted_finite(values: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut keys: Vec<u32> = values.filter(|v| v.is_finite()).map(sort_key).collect();
    if keys.len() < RADIX_MIN {
        keys.sort_unstable();
    } else {
        const DIGITS: [(u32, u32); 3] = [(0, 11), (11, 11), (22, 10)];
        // The three digit histograms in one pass over the keys.
        let mut counts = [vec![0usize; 1 << 11], vec![0usize; 1 << 11], vec![0usize; 1 << 10]];
        for &k in &keys {
            for (c, (shift, bits)) in counts.iter_mut().zip(DIGITS) {
                c[((k >> shift) & ((1 << bits) - 1)) as usize] += 1;
            }
        }
        let mut scratch = vec![0u32; keys.len()];
        for (counts, (shift, bits)) in counts.iter_mut().zip(DIGITS) {
            // A digit every key shares (the high bits of a column of small whole
            // numbers, say) leaves the stable pass an identity: skip it.
            if counts.contains(&keys.len()) {
                continue;
            }
            let mask = (1u32 << bits) - 1;
            let mut at = 0;
            for c in counts.iter_mut() {
                let n = *c;
                *c = at;
                at += n;
            }
            for &k in &keys {
                let slot = &mut counts[((k >> shift) & mask) as usize];
                scratch[*slot] = k;
                *slot += 1;
            }
            std::mem::swap(&mut keys, &mut scratch);
        }
    }
    keys.into_iter().map(sort_unkey).collect()
}

/// Quantile `q` of an ascending sample by linear interpolation between order
/// statistics (the common "type 7" definition); NaN for an empty sample.
pub(crate) fn quantile_sorted(sorted: &[f32], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let i = pos.floor() as usize;
    let frac = pos - i as f64;
    let a = f64::from(sorted[i]);
    match sorted.get(i + 1) {
        Some(&b) if frac > 0.0 => a + frac * (f64::from(b) - a),
        _ => a,
    }
}

/// Guards `ceil(q * n)` against `q * n` landing a hair above an integer.
const RANK_EPS: f64 = 1e-9;

/// Quantile `q` of a right-censored sample: `sorted` holds the observed values in
/// ascending order out of `total` paths, the rest never reached the event and count
/// as later than every observed value. Returns the smallest observed value `v` such
/// that at least `q` of all paths have a value `<= v`, or NaN if fewer than `q` of
/// the paths ever reached the event.
pub(crate) fn censored_quantile(sorted: &[f32], total: u64, q: f64) -> f64 {
    if total == 0 {
        return f64::NAN;
    }
    let rank = ((q * total as f64 - RANK_EPS).ceil().max(1.0)) as usize;
    sorted.get(rank - 1).map_or(f64::NAN, |v| f64::from(*v))
}

/// Distribution-free 95% interval for quantile `q` from order statistics: the values
/// at ranks `q n -/+ z sqrt(n q (1 - q))`. `total` is the number of paths (equal to
/// `sorted.len()` unless the sample is censored). NaN where the rank falls among the
/// censored paths.
pub(crate) fn quantile_interval(sorted: &[f32], total: u64, q: f64) -> (f64, f64) {
    if total == 0 || sorted.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let n = total as f64;
    let half = Z95 * (n * q * (1.0 - q)).sqrt();
    let lo_rank = ((q * n - half).floor().max(1.0)) as usize;
    let hi_rank = ((q * n + half).ceil().min(n).max(1.0)) as usize;
    let at = |rank: usize| sorted.get(rank - 1).map_or(f64::NAN, |v| f64::from(*v));
    (at(lo_rank), at(hi_rank))
}

/// Five quantiles of a per-path quantity, with a 95% interval on the median. `null`
/// in JSON where a quantile does not exist (no data, or for a censored quantity such
/// as FI age, fewer paths than that share ever got there).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Spread {
    /// 10th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p10: f64,
    /// 25th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p25: f64,
    /// Median.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p50: f64,
    /// 75th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p75: f64,
    /// 90th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p90: f64,
    /// Lower end of the 95% interval on the median.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p50_lo: f64,
    /// Upper end of the 95% interval on the median.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p50_hi: f64,
    /// Number of paths with a value.
    pub count: u64,
}

impl Default for Spread {
    fn default() -> Self {
        Spread {
            p10: f64::NAN,
            p25: f64::NAN,
            p50: f64::NAN,
            p75: f64::NAN,
            p90: f64::NAN,
            p50_lo: f64::NAN,
            p50_hi: f64::NAN,
            count: 0,
        }
    }
}

impl Spread {
    /// Quantiles among the paths that have a value.
    pub(crate) fn of(sorted: &[f32], decimals: u32) -> Spread {
        let q = |q| round_dp(quantile_sorted(sorted, q), decimals);
        let (lo, hi) = quantile_interval(sorted, sorted.len() as u64, 0.5);
        Spread {
            p10: q(0.10),
            p25: q(0.25),
            p50: q(0.50),
            p75: q(0.75),
            p90: q(0.90),
            p50_lo: round_dp(lo, decimals),
            p50_hi: round_dp(hi, decimals),
            count: sorted.len() as u64,
        }
    }

    /// Quantiles over all `total` paths, counting those without a value as later
    /// than any observed value (right-censored).
    pub(crate) fn censored(sorted: &[f32], total: u64, decimals: u32) -> Spread {
        let q = |q| round_dp(censored_quantile(sorted, total, q), decimals);
        let (lo, hi) = quantile_interval(sorted, total, 0.5);
        Spread {
            p10: q(0.10),
            p25: q(0.25),
            p50: q(0.50),
            p75: q(0.75),
            p90: q(0.90),
            p50_lo: round_dp(lo, decimals),
            p50_hi: round_dp(hi, decimals),
            count: sorted.len() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn wilson_interval_matches_published_values() {
        // 95 of 100: (0.8882, 0.9785); 5 of 10: (0.2366, 0.7634). Both are the
        // standard textbook values for the Wilson score interval without continuity
        // correction (Brown, Cai and DasGupta 2001; Newcombe 1998 method 3).
        let e = Estimate::proportion(95, 100);
        assert_eq!(e.value, 0.95);
        assert!((e.std_error - 0.021_794_5).abs() < 1e-6);
        assert!((e.lo - 0.8882).abs() < 1e-4, "{}", e.lo);
        assert!((e.hi - 0.9785).abs() < 1e-4, "{}", e.hi);
        let e = Estimate::proportion(5, 10);
        assert!((e.lo - 0.2366).abs() < 1e-4);
        assert!((e.hi - 0.7634).abs() < 1e-4);
        // Zero successes: the lower bound is 0 and the upper bound is
        // z^2 / (n + z^2) = 3.8415 / 103.8415.
        let e = Estimate::proportion(0, 100);
        assert!(e.lo.abs() < 1e-12);
        assert!((e.hi - 0.036_994).abs() < 1e-5);
        assert_eq!(e.std_error, 0.0);
        let e = Estimate::proportion(100, 100);
        assert!((e.hi - 1.0).abs() < 1e-12);
        assert!((e.lo - (1.0 - 0.036_994)).abs() < 1e-5);
        assert!(Estimate::proportion(0, 0).value.is_nan());
    }

    #[test]
    fn mean_and_ratio_estimates() {
        // 2, 4, 4, 4, 5, 5, 7, 9: mean 5, sample sd sqrt(32 / 7).
        let v = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let e = Estimate::mean(v.iter().copied());
        assert!((e.value - 5.0).abs() < 1e-12);
        let se = (32.0f64 / 7.0).sqrt() / 8.0f64.sqrt();
        assert!((e.std_error - se).abs() < 1e-12);
        assert!((e.hi - (5.0 + Z95 * se)).abs() < 1e-12);
        assert!(Estimate::mean(std::iter::empty()).value.is_nan());

        // Three paths with (years cut, years retired) = (0, 10), (5, 10), (1, 20):
        // ratio 6 / 40 = 0.15; residuals x - r y = -1.5, 3.5, -2;
        // SE = sqrt((2.25 + 12.25 + 4) * 3 / 2) / 40.
        let pairs = [(0.0, 10.0), (5.0, 10.0), (1.0, 20.0)];
        let e = Estimate::ratio(pairs.iter().copied());
        assert!((e.value - 0.15).abs() < 1e-12);
        assert!((e.std_error - (18.5f64 * 1.5).sqrt() / 40.0).abs() < 1e-12);
        assert_eq!(e.lo, 0.0, "clamped at zero");
        assert!(Estimate::ratio([(0.0, 0.0)].iter().copied()).value.is_nan());
        assert!(Estimate::normal(0.1, 0.1).within_noise());
        assert!(!Estimate::normal(0.3, 0.1).within_noise());
        assert!(Estimate::NONE.within_noise());
    }

    #[test]
    fn radix_sort_matches_comparison_sort() {
        let mut rng = Rng::from_seed(3);
        let mut v: Vec<f32> = (0..50_000)
            .map(|i| match i % 7 {
                0 => f32::NAN,
                1 => 0.0,
                2 => -(rng.lognormal(10.0, 2.0) as f32),
                3 => (i % 40) as f32,
                _ => rng.lognormal(12.0, 1.5) as f32,
            })
            .collect();
        v.push(f32::INFINITY);
        v.push(-0.0);
        v.push(f32::MAX);
        v.push(f32::MIN);
        let got = sorted_finite(v.iter().copied());
        let mut want: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
        want.sort_by(f32::total_cmp);
        assert_eq!(got.len(), want.len());
        assert!(got
            .iter()
            .zip(&want)
            .all(|(a, b)| a.to_bits() == b.to_bits()));
        // Small inputs take the comparison path.
        assert_eq!(
            sorted_finite([3.0, f32::NAN, -1.0, 2.0].into_iter()),
            [-1.0, 2.0, 3.0]
        );
        assert!(sorted_finite(std::iter::empty()).is_empty());
    }

    #[test]
    fn sample_quantiles() {
        let s = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile_sorted(&s, 0.0), 1.0);
        assert_eq!(quantile_sorted(&s, 0.5), 3.0);
        assert_eq!(quantile_sorted(&s, 1.0), 5.0);
        assert_eq!(quantile_sorted(&s, 0.1), 1.4);
        assert_eq!(quantile_sorted(&[7.0], 0.9), 7.0);
        assert!(quantile_sorted(&[], 0.5).is_nan());
    }

    #[test]
    fn censored_median_counts_paths_that_never_get_there() {
        // Ten paths. Six reach FI at 40, 42, 45, 50, 55 and 60; four never do.
        // v1 dropped the four and reported the median of the six: 47.5.
        // Over all ten paths, half are FI by 55 (the 5th of 10), so the median is 55;
        // the 75th percentile would be the 8th path, which never gets there.
        let fi = [40.0f32, 42.0, 45.0, 50.0, 55.0, 60.0];
        assert_eq!(quantile_sorted(&fi, 0.5), 47.5);
        assert_eq!(censored_quantile(&fi, 10, 0.5), 55.0);
        assert_eq!(censored_quantile(&fi, 10, 0.1), 40.0);
        assert_eq!(censored_quantile(&fi, 10, 0.25), 45.0);
        assert_eq!(censored_quantile(&fi, 10, 0.6), 60.0);
        assert!(censored_quantile(&fi, 10, 0.75).is_nan());
        assert!(censored_quantile(&fi, 10, 0.9).is_nan());
        assert!(censored_quantile(&fi, 0, 0.5).is_nan());
        // With nothing censored it is the plain lower order statistic.
        assert_eq!(censored_quantile(&fi, 6, 0.5), 45.0);

        let s = Spread::censored(&fi, 10, 2);
        assert_eq!((s.p10, s.p25, s.p50), (40.0, 45.0, 55.0));
        assert!(s.p75.is_nan() && s.p90.is_nan());
        assert_eq!(s.count, 6);
        // Median interval: ranks 5 -/+ 1.96 * sqrt(2.5) = 1.9 .. 8.1 -> 1st and 9th;
        // the 9th path is censored.
        assert_eq!(s.p50_lo, 40.0);
        assert!(s.p50_hi.is_nan());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"p10":40,"p25":45,"p50":55,"p75":null,"p90":null,"p50_lo":40,"p50_hi":null,"count":6}"#
        );
        let back: Spread = serde_json::from_str(&json).unwrap();
        assert_eq!(back.p50, 55.0);
        assert!(back.p75.is_nan());
    }

    #[test]
    fn quantile_interval_covers_the_median() {
        // 1..=100: median interval ranks 50 -/+ 9.8 -> 40th and 60th values.
        let s: Vec<f32> = (1..=100).map(|i| i as f32).collect();
        assert_eq!(quantile_interval(&s, 100, 0.5), (40.0, 60.0));
        let sp = Spread::of(&s, 1);
        assert_eq!(sp.p50, 50.5);
        assert_eq!((sp.p50_lo, sp.p50_hi), (40.0, 60.0));
        assert_eq!(sp.p10, 10.9);
        assert!(Spread::of(&[], 0).p50.is_nan());
    }

    #[test]
    fn compact_number_encoding() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct T {
            #[serde(with = "nan")]
            a: f64,
            #[serde(with = "nan_vec")]
            b: Vec<f64>,
            #[serde(with = "nan_vec2")]
            c: Vec<Vec<f64>>,
        }
        let t = T {
            a: f64::NAN,
            b: vec![1_234_567.0, 0.25, f64::INFINITY, -3.0, 1e300],
            c: vec![vec![1.0, f64::NAN], vec![]],
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(
            json,
            r#"{"a":null,"b":[1234567,0.25,null,-3,1e+300],"c":[[1,null],[]]}"#
        );
        let back: T = serde_json::from_str(&json).unwrap();
        assert!(back.a.is_nan());
        assert_eq!(back.b[0], 1_234_567.0);
        assert!(back.b[2].is_nan());
        assert!(back.c[0][1].is_nan());
        assert_eq!(round_dp(1.23456, 2), 1.23);
        assert_eq!(round_dp(-2.5, 0), -3.0);
        assert!(round_dp(f64::INFINITY, 2).is_nan());
        assert_eq!(moments([1.0, 2.0, 3.0].into_iter()), (3, 2.0, 1.0));
    }
}
