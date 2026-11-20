//! Random numbers. We own the generator so that results are bit-identical on every
//! platform and across releases: no `rand` crate, no platform libm.
//!
//! * [`SplitMix64`] expands seeds.
//! * [`Rng`] is xoshiro256++ (Blackman and Vigna), 256 bits of state, period 2^256 - 1.
//! * [`Rng::stream`] derives an independent generator for each
//!   `(seed, path_index, Stream)`. Every random subsystem draws only from its own
//!   stream, so changing a decision in one subsystem (say, spending) cannot shift the
//!   random numbers of another (say, markets). That is what makes scenario comparison
//!   a paired experiment.
//!
//! Rules for callers:
//!
//! * Never share one `Rng` between subsystems, and never let the number of draws from a
//!   stream depend on another subsystem's state, or the pairing is lost.
//! * A stream's draw sequence is part of the engine's observable behaviour. Reordering
//!   draws changes golden results.

use crate::math;

/// SplitMix64 (Steele, Lea, Flood). Used only to turn seeds into generator state.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from any 64-bit seed.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64 random bits.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix64(self.state)
    }
}

/// The SplitMix64 output function: a bijective 64-bit finaliser with full avalanche.
#[inline]
const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A named random stream. One per stochastic subsystem (and per person or property
/// where the subsystem is replicated), so subsystems never share random numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Asset returns, interest rates, recessions.
    Market,
    /// Inflation innovations that are not part of the joint market draw.
    Inflation,
    /// Wage shocks, layoffs and re-employment for one person (index into `people`).
    Career(usize),
    /// Employer stock prices and private-company outcomes.
    EquityComp,
    /// Medical costs and long-term care for one person.
    Health(usize),
    /// Age at death for one person.
    Longevity(usize),
    /// Irregular household expenses.
    Shocks,
    /// Idiosyncratic value of one property (index into the plan's property table,
    /// which includes properties bought by events).
    Housing(usize),
    /// Whether and when probabilistic life events occur.
    Events,
    /// Lenders' response to a downturn: whether home-equity lines freeze (drawn only
    /// when `retirement.home_equity_borrowing` is a home-equity line).
    HomeEquityCredit,
    /// Tenants, market rent and repairs of one rental with realistic dynamics (index
    /// into the plan's property table).
    RentalMarket(usize),
}

impl Stream {
    /// A stable 64-bit identifier. These values are part of the reproducibility
    /// contract: never renumber them, only add new ones.
    pub const fn id(self) -> u64 {
        match self {
            Stream::Market => 1,
            Stream::Inflation => 2,
            Stream::EquityComp => 3,
            Stream::Shocks => 4,
            Stream::Events => 5,
            Stream::HomeEquityCredit => 1600,
            Stream::Career(i) => 0x0100_0000 + i as u64,
            Stream::Health(i) => 0x0200_0000 + i as u64,
            Stream::Longevity(i) => 0x0300_0000 + i as u64,
            Stream::Housing(i) => 0x0400_0000 + i as u64,
            Stream::RentalMarket(i) => 0x1600_0000 + i as u64,
        }
    }
}

/// Inverse of the standard normal CDF for `p` in `(0, 1)` by Acklam's rational
/// approximation (relative error below 1.2e-9). Deterministic: only `ln`, `sqrt` and
/// arithmetic, so one uniform gives one normal on every platform.
pub fn inverse_normal_cdf(p: f64) -> f64 {
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
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < P_LOW {
        tail((-2.0 * math::ln(p)).sqrt())
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -tail((-2.0 * math::ln(1.0 - p)).sqrt())
    }
}

/// A standard normal variate from 64 random bits ([`Rng::next_u64`]), by the inverse
/// CDF. Lets a caller draw a fixed number of words every step and turn only the ones
/// it ends up using into normals: the stream position stays independent of the
/// state, and the unused conversions cost nothing.
#[inline]
pub fn normal_from_bits(bits: u64) -> f64 {
    inverse_normal_cdf(((bits >> 11) as f64 + 0.5) * F64_UNIT)
}

/// xoshiro256++ with the distributions the model needs.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
    /// Second variate from the last polar-method pair, if not yet consumed.
    spare_normal: Option<f64>,
}

const F64_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

impl Rng {
    /// Creates a generator from a single seed, expanding it with SplitMix64 as the
    /// xoshiro authors recommend.
    pub fn from_seed(seed: u64) -> Self {
        let mut sm = SplitMix64::new(seed);
        let s = [sm.next_u64(), sm.next_u64(), sm.next_u64(), sm.next_u64()];
        // SplitMix64 is a bijection over a full-period counter, so four consecutive
        // outputs are never all zero, the one state xoshiro cannot leave.
        Self {
            s,
            spare_normal: None,
        }
    }

    /// The generator for one subsystem on one path.
    ///
    /// The three inputs are absorbed one at a time through the bijective finaliser, so
    /// distinct `(seed, path_index, stream)` triples give unrelated states; streams are
    /// not carved out of one long sequence and cannot overlap by construction of the
    /// 256-bit state.
    pub fn stream(seed: u64, path_index: u64, stream: Stream) -> Self {
        let mut h = mix64(seed ^ 0x6A09_E667_F3BC_C908);
        h = mix64(
            h ^ path_index
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x3C6E_F372),
        );
        h = mix64(
            h ^ stream
                .id()
                .wrapping_mul(0xD6E8_FEB8_6659_FD93)
                .wrapping_add(0xA54F_F53A),
        );
        Self::from_seed(h)
    }

    /// Next 64 random bits.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in `[0, 1)` with 53 random bits.
    #[inline]
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * F64_UNIT
    }

    /// Uniform in `(0, 1]`. Safe to pass to `ln`.
    #[inline]
    pub fn uniform_open0(&mut self) -> f64 {
        ((self.next_u64() >> 11) + 1) as f64 * F64_UNIT
    }

    /// Uniform in `[lo, hi)`.
    #[inline]
    pub fn uniform_range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// True with probability `p` (clamped to `[0, 1]`). Always consumes one draw, so
    /// the stream position does not depend on `p`.
    #[inline]
    pub fn bernoulli(&mut self, p: f64) -> bool {
        self.uniform() < p
    }

    /// Uniform integer in `0..n` without modulo bias (Lemire's multiply-and-reject).
    /// Returns 0 when `n` is 0.
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        let n = n as u64;
        if n == 0 {
            return 0;
        }
        let mut m = u128::from(self.next_u64()) * u128::from(n);
        if (m as u64) < n {
            let threshold = n.wrapping_neg() % n;
            while (m as u64) < threshold {
                m = u128::from(self.next_u64()) * u128::from(n);
            }
        }
        (m >> 64) as usize
    }

    /// Standard normal variate.
    ///
    /// Marsaglia's polar method: exact (no table or tail approximation), needs one
    /// `ln` and one `sqrt` per pair of variates, and both come from deterministic
    /// implementations. A ziggurat would be roughly twice as fast, but normal draws are
    /// a small share of path time and the polar method has no tables to get wrong.
    /// The second variate of each pair is cached, so interleaving `normal` with other
    /// draws is fine but changes which uniforms each normal is built from.
    #[inline]
    pub fn normal(&mut self) -> f64 {
        if let Some(z) = self.spare_normal.take() {
            return z;
        }
        loop {
            let u = 2.0 * self.uniform() - 1.0;
            let v = 2.0 * self.uniform() - 1.0;
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                let k = (-2.0 * math::ln(s) / s).sqrt();
                self.spare_normal = Some(v * k);
                return u * k;
            }
        }
    }

    /// One uniform strictly inside `(0, 1)` from exactly one 64-bit draw.
    #[inline]
    pub fn uniform_open(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) * F64_UNIT
    }

    /// Standard normal variate from exactly one 64-bit draw, by the inverse normal
    /// CDF ([`inverse_normal_cdf`]). Use it where the number of draws from a stream
    /// must not depend on the values drawn: [`Rng::normal`] rejects a variable number
    /// of pairs and caches a spare. It does not touch the spare of `normal`.
    #[inline]
    pub fn normal_one_draw(&mut self) -> f64 {
        normal_from_bits(self.next_u64())
    }

    /// Normal variate with the given mean and standard deviation.
    #[inline]
    pub fn normal_with(&mut self, mean: f64, sd: f64) -> f64 {
        mean + sd * self.normal()
    }

    /// Student-t variate with `dof` degrees of freedom (any real `dof > 0`), by
    /// Bailey's polar method. Variance is `dof / (dof - 2)` for `dof > 2`.
    pub fn student_t(&mut self, dof: f64) -> f64 {
        loop {
            let u = 2.0 * self.uniform() - 1.0;
            let v = 2.0 * self.uniform() - 1.0;
            let w = u * u + v * v;
            if w > 0.0 && w < 1.0 {
                return u * (dof * (math::pow(w, -2.0 / dof) - 1.0) / w).sqrt();
            }
        }
    }

    /// Student-t variate rescaled to unit variance (requires `dof > 2`). Use this for
    /// fat-tailed shocks that should keep a calibrated standard deviation.
    pub fn student_t_unit(&mut self, dof: f64) -> f64 {
        debug_assert!(dof > 2.0);
        self.student_t(dof) * ((dof - 2.0) / dof).sqrt()
    }

    /// Lognormal variate: `exp(mu + sigma * Z)`. Its mean is `exp(mu + sigma^2 / 2)`.
    #[inline]
    pub fn lognormal(&mut self, mu: f64, sigma: f64) -> f64 {
        math::exp(mu + sigma * self.normal())
    }

    /// Lognormal variate with mean exactly 1 and log-standard-deviation `sigma`. A
    /// multiplicative shock that does not change the expected value.
    #[inline]
    pub fn lognormal_unit_mean(&mut self, sigma: f64) -> f64 {
        self.lognormal(-0.5 * sigma * sigma, sigma)
    }

    /// Exponential variate with the given rate (mean `1 / rate`).
    #[inline]
    pub fn exponential(&mut self, rate: f64) -> f64 {
        -math::ln(self.uniform_open0()) / rate
    }

    /// Number of independent trials up to and including the first success, each with
    /// success probability `p`: a value in `1..`, with mean `1 / p`. `p >= 1` gives 1;
    /// `p <= 0` gives `u32::MAX` (never).
    pub fn geometric(&mut self, p: f64) -> u32 {
        let u = self.uniform_open0();
        if p >= 1.0 {
            return 1;
        }
        if p <= 0.0 {
            return u32::MAX;
        }
        let k = (math::ln(u) / math::ln_1p(-p)).floor() + 1.0;
        if k >= f64::from(u32::MAX) {
            u32::MAX
        } else {
            k as u32
        }
    }

    /// Poisson variate with mean `lambda`.
    ///
    /// Knuth's multiplication method, exact, costing about `lambda + 1` uniforms. Meant
    /// for small rates (events per month or year); larger means are split into chunks
    /// of at most 30 so `exp(-lambda)` never underflows.
    pub fn poisson(&mut self, lambda: f64) -> u32 {
        if lambda.is_nan() || lambda <= 0.0 {
            return 0;
        }
        const CHUNK: f64 = 30.0;
        let mut remaining = lambda;
        let mut total = 0u32;
        while remaining > 0.0 {
            let l = remaining.min(CHUNK);
            remaining -= l;
            let limit = math::exp(-l);
            let mut product = self.uniform();
            while product > limit {
                total += 1;
                product *= self.uniform();
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 1_000_000;

    fn moments(mut draw: impl FnMut() -> f64) -> (f64, f64) {
        let (mut sum, mut sum_sq) = (0.0, 0.0);
        for _ in 0..N {
            let x = draw();
            sum += x;
            sum_sq += x * x;
        }
        let mean = sum / N as f64;
        (mean, sum_sq / N as f64 - mean * mean)
    }

    fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= tol,
            "{what}: got {actual}, expected {expected} +/- {tol}"
        );
    }

    #[test]
    fn splitmix_reference_vector() {
        // First outputs for seed 1234567, from the reference C implementation.
        let mut sm = SplitMix64::new(1_234_567);
        assert_eq!(sm.next_u64(), 6_457_827_717_110_365_317);
        assert_eq!(sm.next_u64(), 3_203_168_211_198_807_973);
        assert_eq!(sm.next_u64(), 9_817_491_932_198_370_423);
    }

    #[test]
    fn xoshiro_reference_vector() {
        // State {1, 2, 3, 4}: outputs of the reference xoshiro256++ implementation.
        let mut rng = Rng {
            s: [1, 2, 3, 4],
            spare_normal: None,
        };
        assert_eq!(rng.next_u64(), 41_943_041);
        assert_eq!(rng.next_u64(), 58_720_359);
        assert_eq!(rng.next_u64(), 3_588_806_011_781_223);
        assert_eq!(rng.next_u64(), 3_591_011_842_654_386);
    }

    #[test]
    fn same_inputs_same_sequence() {
        let mut a = Rng::stream(42, 7, Stream::Market);
        let mut b = Rng::stream(42, 7, Stream::Market);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let (mut a, mut b) = (
            Rng::stream(42, 7, Stream::Career(1)),
            Rng::stream(42, 7, Stream::Career(1)),
        );
        for _ in 0..100 {
            assert_eq!(a.normal().to_bits(), b.normal().to_bits());
            assert_eq!(a.student_t(5.0).to_bits(), b.student_t(5.0).to_bits());
            assert_eq!(a.poisson(2.5), b.poisson(2.5));
        }
    }

    #[test]
    fn stream_golden_values_are_frozen() {
        // Guards the seeding scheme: if this changes, every stored result changes.
        let mut r = Rng::stream(1, 0, Stream::Market);
        let first = [r.next_u64(), r.next_u64()];
        let mut again = Rng::stream(1, 0, Stream::Market);
        assert_eq!(first, [again.next_u64(), again.next_u64()]);
        assert_eq!(first, GOLDEN_STREAM_1_0_MARKET);
    }

    const GOLDEN_STREAM_1_0_MARKET: [u64; 2] =
        [13_782_392_658_221_407_413, 12_316_620_447_257_498_702];

    #[test]
    fn different_streams_paths_and_seeds_differ() {
        let streams = [
            Stream::Market,
            Stream::Inflation,
            Stream::Career(0),
            Stream::Career(1),
            Stream::EquityComp,
            Stream::Health(0),
            Stream::Health(1),
            Stream::Longevity(0),
            Stream::Longevity(1),
            Stream::Shocks,
            Stream::Housing(0),
            Stream::Housing(1),
            Stream::Events,
            Stream::HomeEquityCredit,
            Stream::RentalMarket(0),
            Stream::RentalMarket(1),
        ];
        let mut firsts = Vec::new();
        for seed in [0u64, 1, 2] {
            for path in [0u64, 1, 2, 1_000_000] {
                for s in streams {
                    let mut r = Rng::stream(seed, path, s);
                    firsts.push([r.next_u64(), r.next_u64(), r.next_u64()]);
                }
            }
        }
        let total = firsts.len();
        firsts.sort_unstable();
        firsts.dedup();
        assert_eq!(
            firsts.len(),
            total,
            "two streams started with the same outputs"
        );
    }

    #[test]
    fn streams_are_uncorrelated() {
        // Adjacent paths and sibling streams must not be linearly related.
        let pairs = [
            (
                Rng::stream(9, 0, Stream::Market),
                Rng::stream(9, 1, Stream::Market),
            ),
            (
                Rng::stream(9, 0, Stream::Market),
                Rng::stream(9, 0, Stream::Inflation),
            ),
            (
                Rng::stream(9, 0, Stream::Career(0)),
                Rng::stream(9, 0, Stream::Career(1)),
            ),
            (
                Rng::stream(9, 0, Stream::Shocks),
                Rng::stream(10, 0, Stream::Shocks),
            ),
        ];
        for (mut a, mut b) in pairs {
            let n = 200_000;
            let mut sum = 0.0;
            for _ in 0..n {
                sum += (a.uniform() - 0.5) * (b.uniform() - 0.5);
            }
            let corr = sum / n as f64 * 12.0;
            assert!(corr.abs() < 0.01, "correlation {corr}");
        }
    }

    #[test]
    fn uniform_is_in_range_with_right_moments() {
        let mut r = Rng::from_seed(1);
        let (mut lo, mut hi) = (1.0f64, 0.0f64);
        let (mean, var) = moments(|| {
            let u = r.uniform();
            lo = lo.min(u);
            hi = hi.max(u);
            u
        });
        assert!(lo >= 0.0 && hi < 1.0);
        assert_close(mean, 0.5, 0.002, "uniform mean");
        assert_close(var, 1.0 / 12.0, 0.001, "uniform variance");
        for _ in 0..1000 {
            let u = r.uniform_open0();
            assert!(u > 0.0 && u <= 1.0);
        }
    }

    #[test]
    fn normal_moments_and_tails() {
        let mut r = Rng::from_seed(2);
        let (mut sum3, mut sum4, mut beyond2) = (0.0, 0.0, 0usize);
        let (mean, var) = moments(|| {
            let z = r.normal();
            sum3 += z * z * z;
            sum4 += z * z * z * z;
            beyond2 += usize::from(z.abs() > 1.959_964);
            z
        });
        assert_close(mean, 0.0, 0.004, "normal mean");
        assert_close(var, 1.0, 0.006, "normal variance");
        assert_close(sum3 / N as f64, 0.0, 0.02, "normal skewness");
        assert_close(sum4 / N as f64, 3.0, 0.05, "normal kurtosis");
        assert_close(
            beyond2 as f64 / N as f64,
            0.05,
            0.001,
            "normal 95% tail mass",
        );
        let (mean, var) = moments(|| r.normal_with(3.0, 2.0));
        assert_close(mean, 3.0, 0.01, "normal_with mean");
        assert_close(var, 4.0, 0.03, "normal_with variance");
    }

    #[test]
    fn student_t_moments() {
        let mut r = Rng::from_seed(3);
        let dof = 8.0;
        let (mean, var) = moments(|| r.student_t(dof));
        assert_close(mean, 0.0, 0.005, "t mean");
        assert_close(var, dof / (dof - 2.0), 0.02, "t variance");
        let mut sum4 = 0.0;
        let (mean, var) = moments(|| {
            let x = r.student_t_unit(dof);
            sum4 += x * x * x * x;
            x
        });
        assert_close(mean, 0.0, 0.005, "unit t mean");
        assert_close(var, 1.0, 0.02, "unit t variance");
        // Kurtosis of t is 3 + 6 / (dof - 4) = 4.5: fatter tails than the normal.
        assert_close(sum4 / N as f64, 4.5, 0.4, "unit t kurtosis");
    }

    #[test]
    fn lognormal_moments() {
        let mut r = Rng::from_seed(4);
        let (mu, sigma) = (0.05f64, 0.3f64);
        let (mean, var) = moments(|| r.lognormal(mu, sigma));
        let expected_mean = math::exp(mu + sigma * sigma / 2.0);
        let expected_var = (math::exp(sigma * sigma) - 1.0) * expected_mean * expected_mean;
        assert_close(mean, expected_mean, 0.002, "lognormal mean");
        assert_close(var, expected_var, 0.003, "lognormal variance");
        let (mean, _) = moments(|| r.lognormal_unit_mean(0.5));
        assert_close(mean, 1.0, 0.003, "unit-mean lognormal");
    }

    #[test]
    fn exponential_moments() {
        let mut r = Rng::from_seed(5);
        let (mean, var) = moments(|| r.exponential(4.0));
        assert_close(mean, 0.25, 0.001, "exponential mean");
        assert_close(var, 0.0625, 0.001, "exponential variance");
    }

    #[test]
    fn geometric_moments_and_edges() {
        let mut r = Rng::from_seed(6);
        let p = 0.2;
        let mut min = u32::MAX;
        let (mean, var) = moments(|| {
            let k = r.geometric(p);
            min = min.min(k);
            f64::from(k)
        });
        assert_eq!(min, 1);
        assert_close(mean, 1.0 / p, 0.02, "geometric mean");
        assert_close(var, (1.0 - p) / (p * p), 0.2, "geometric variance");
        assert_eq!(r.geometric(1.0), 1);
        assert_eq!(r.geometric(0.0), u32::MAX);
    }

    #[test]
    fn poisson_moments() {
        let mut r = Rng::from_seed(7);
        for lambda in [0.05, 1.0, 4.5, 75.0] {
            let (mean, var) = moments(|| f64::from(r.poisson(lambda)));
            let tol = 0.01 + 0.004 * lambda;
            assert_close(mean, lambda, tol, "poisson mean");
            assert_close(var, lambda, 4.0 * tol, "poisson variance");
        }
        assert_eq!(r.poisson(0.0), 0);
        assert_eq!(r.poisson(-1.0), 0);
    }

    #[test]
    fn bernoulli_frequency() {
        let mut r = Rng::from_seed(8);
        let (mean, _) = moments(|| f64::from(u8::from(r.bernoulli(0.3))));
        assert_close(mean, 0.3, 0.002, "bernoulli frequency");
        assert!(!r.bernoulli(0.0));
        assert!(r.bernoulli(1.0));
    }

    #[test]
    fn below_is_uniform_and_in_range() {
        let mut r = Rng::from_seed(9);
        let n = 7;
        let mut counts = [0usize; 7];
        for _ in 0..N {
            counts[r.below(n)] += 1;
        }
        let expected = N as f64 / n as f64;
        for c in counts {
            assert_close(
                c as f64,
                expected,
                4.0 * expected.sqrt(),
                "below bucket count",
            );
        }
        assert_eq!(r.below(0), 0);
        assert_eq!(r.below(1), 0);
        // A range that is not a power of two and close to half the word size exercises rejection.
        let big = usize::MAX / 2 + 12_345;
        for _ in 0..1000 {
            assert!(r.below(big) < big);
        }
    }
}
