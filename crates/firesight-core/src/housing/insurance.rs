//! Insurance premium drift: a persistent real increase with occasional jumps.
//!
//! The premium of a property is `value * insurance_rate / 12` (CPI-linked through
//! the value). With `insurance_real_growth: stochastic` it is multiplied by a path
//! index `F` that follows, in logs and per month,
//!
//! ```text
//! ln F += g / 12 + [jump with prob JUMP_RATE / 12] exp(JUMP_LOG_MEDIAN + JUMP_LOG_SD z2)
//! g'    = MU + RHO (g - MU) + SIGMA z          (annual real growth rate, persistent)
//! ```
//!
//! There is one index per path, shared by every property that opts in (the
//! repricing of 2021 to 2025 was national in character, and a household that sells
//! one home and buys another keeps the premium level of its day). It runs from the
//! plan's start whether or not a property is owned.
//!
//! Calibration. Urban Institute (Housing Finance at a Glance, September 2026; see
//! `data/actuarial/params.json` `housing.homeowners_insurance_pct_of_value`): the
//! average premium on newly originated agency mortgages rose 46% from 2018 to 2024,
//! about 6.5% a year against CPI of about 3.8% a year: about 2.6 points a year in
//! real terms over six years, an episode regarded as exceptional. The parameters
//! below give a long-run real drift of about 1.5% a year in logs (0.5% persistent
//! plus 8% a year of jumps averaging 12.6% in logs), below the 2018 to 2024 pace
//! because repricing cannot continue forever. The persistent growth rate has a
//! stationary sd of about 1.6 points a year, so that a six-year episode like 2018
//! to 2024 is roughly half a standard deviation above the mean and a 30-year index
//! is 1.1 at the 10th percentile, 1.5 at the median and 2.2 at the 90th (measured
//! over 3,000 paths). Every
//! figure is a JUDGEMENT anchored to that evidence.
//!
//! Randomness: 3 words a month from `Stream::InsuranceDrift`.

use crate::math;
use crate::rng::normal_from_bits;

/// Words drawn each month.
pub const DRAWS_PER_MONTH: usize = 3;
/// Long-run mean of the persistent real growth rate (annual). JUDGEMENT.
pub const MU: f64 = 0.005;
/// Monthly persistence of the growth rate deviation (about 0.7 a year). JUDGEMENT.
pub const RHO: f64 = 0.97;
/// Monthly innovation sd of the annual growth rate (stationary sd
/// `SIGMA / sqrt(1 - RHO^2)`, about 1.6 points a year). JUDGEMENT.
pub const SIGMA: f64 = 0.004;
/// Expected repricing jumps per year. JUDGEMENT.
pub const JUMP_RATE: f64 = 0.08;
/// Median log size of a jump (e^-2.2 = 0.11). JUDGEMENT.
pub const JUMP_LOG_MEDIAN: f64 = -2.2;
/// Sd of the log size of a jump. JUDGEMENT.
pub const JUMP_LOG_SD: f64 = 0.5;
/// The index is kept within these bounds.
pub const FACTOR_RANGE: (f64, f64) = (0.5, 5.0);

const F64_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

/// The premium index of a path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InsState {
    /// Deviation of the annual growth rate from [`MU`].
    pub dev: f64,
    /// Natural log of the index.
    pub log_index: f64,
    /// The index itself, `exp(log_index)`. Zero before the first month.
    pub factor: f64,
}

/// Advances the index one month with three random words and returns the size of the
/// jump (a log increment, 0 if none).
pub fn advance(st: &mut InsState, words: &[u64; DRAWS_PER_MONTH]) -> f64 {
    let u = (words[0] >> 11) as f64 * F64_UNIT;
    let z = normal_from_bits(words[1]);
    let z2 = normal_from_bits(words[2]);
    st.log_index += (MU + st.dev) / 12.0;
    st.dev = RHO * st.dev + SIGMA * z;
    let mut jump = 0.0;
    if u < JUMP_RATE / 12.0 {
        jump = math::exp(JUMP_LOG_MEDIAN + JUMP_LOG_SD * z2);
        st.log_index += jump;
    }
    st.log_index = st
        .log_index
        .clamp(math::ln(FACTOR_RANGE.0), math::ln(FACTOR_RANGE.1));
    st.factor = math::exp(st.log_index);
    jump
}
