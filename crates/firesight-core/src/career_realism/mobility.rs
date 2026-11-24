//! Voluntary job changes with pay jumps (`risk.job_mobility`).
//!
//! # What is modelled
//!
//! An employed person changes employer in a month with probability `h(age, cycle)`.
//! A change multiplies base pay by `exp(J)`, `J ~ N(mu(age, cycle), SD)`, and forfeits
//! unvested equity (the new employer's sign-on grant is not modelled: the jump is
//! meant to cover it). Moves are rarer and pay less in recessions.
//!
//! # No double counting with the lifecycle curve
//!
//! The lifecycle wage curve is fitted to earnings panels (Cocco, Gomes and Maenhout)
//! in which people change jobs; its average growth already contains the average gain
//! from moves. Adding jumps on top would raise the average career. The model instead
//! keeps the average on the curve and redistributes around it: every month in which
//! the person is employed and does not move, pay is multiplied by the stayer deflator
//!
//! ```text
//! d = (1 - h * E[exp(J)]) / (1 - h),   E[exp(J)] = exp(mu + SD^2 / 2)
//! ```
//!
//! so that `E[pay growth] = curve growth` exactly (a move multiplies by `exp(J)`, no
//! move by `d`, so the expected factor is `h E[e^J] + (1 - h) d = 1`). Movers then
//! gain more than stayers, as in the job-ladder evidence, while stayers fall behind
//! the curve; the mean path is the curve. What the jumps do add is dispersion between
//! careers (some climb, some stay). The persistent wage shock of the existing model
//! (`earnings.shock_process`) is calibrated to the same panel data and already
//! carries some of that dispersion, so the two overlap a little in variance; the mean
//! is not double counted.
//!
//! # Parameters
//!
//! The annual probability of changing employer by age is a judgement shaped by:
//! Fallick and Fleischman (Federal Reserve Board FEDS 2004-34, CPS dependent
//! interviewing), who find that 2.6% of employed people change employers each month
//! and that the rate falls with age and in recessions (FRBSF Economic Letter 2016-30
//! reports 1.7 to 1.9% a month for 40 to 44 year olds in SIPP data); and the BLS
//! National Longitudinal Survey of Youth 1979 count of about 12 jobs between 18 and
//! 52 (from memory: 5.7 before 25, 2.9 at 25 to 34, 2.0 at 35 to 44). The table counts
//! only voluntary moves with no unemployment spell, so it sits below those job counts
//! (which include layoffs, already simulated separately) and below the 2.6% a month
//! (which includes involuntary employer-to-employer moves).
//!
//! The pay jump (mean log gain 8% under 35, 5% at 35 to 54, 2% from 55; SD 0.12) is
//! a judgement. Direction and rough size follow Topel and Ward (QJE 1992: job
//! changes account for about a third of early-career wage growth) and the Atlanta
//! Fed Wage Growth Tracker, in which job switchers out-earn stayers by one to three
//! points a year in expansions (8.4% against 5.6% at the August 2022 peak) and the
//! premium turns negative in and after recessions. Those numbers were read from
//! summaries of the sources, not recomputed here.

use crate::math;
use crate::model::JobMobility;
use crate::rng::normal_from_bits;

/// Number of age bands: under 25, 25-34, 35-44, 45-54, 55-64, 65 and over.
pub const BANDS: usize = 6;

/// Annual probability of a voluntary employer change in an expansion, by age band at
/// the `Typical` level. JUDGEMENT shaped by Fallick and Fleischman (2004) and the
/// NLSY79 job counts, see the module documentation.
pub const ANNUAL_MOVE_PROBABILITY: [f64; BANDS] = [0.30, 0.20, 0.12, 0.08, 0.05, 0.03];
/// Multiplier on the yearly move rate at the `High` level (JUDGEMENT).
pub const HIGH_LEVEL_MULTIPLIER: f64 = 1.6;
/// Ceiling on the annual move probability at the typical level.
pub const MAX_ANNUAL_PROBABILITY: f64 = 0.6;
/// Multiplier on the move rate in a recession month (Fallick and Fleischman: the
/// rate fell in the 2001 recession; the size is a JUDGEMENT).
pub const RECESSION_RATE_MULTIPLIER: f64 = 0.6;
/// Mean log pay jump of a move in an expansion, by age band (JUDGEMENT, see above).
pub const MEAN_LOG_JUMP: [f64; BANDS] = [0.08, 0.08, 0.05, 0.05, 0.02, 0.02];
/// Reduction of the mean log jump in a recession month (the Atlanta Fed switcher
/// premium turns negative in recessions; the size is a JUDGEMENT).
pub const RECESSION_JUMP_PENALTY: f64 = 0.04;
/// Standard deviation of the log jump (JUDGEMENT).
pub const JUMP_SD: f64 = 0.12;

/// Age band of an age in years.
#[inline]
pub fn band(age: f64) -> usize {
    if age < 25.0 {
        0
    } else if age < 35.0 {
        1
    } else if age < 45.0 {
        2
    } else if age < 55.0 {
        3
    } else if age < 65.0 {
        4
    } else {
        5
    }
}

/// Compiled tables, indexed `[recession][age band]`.
#[derive(Clone, Debug, PartialEq)]
pub struct MobilityModel {
    /// Probability of a move in the month.
    pub hazard: [[f64; BANDS]; 2],
    /// Mean of the log jump.
    pub mean_log_jump: [[f64; BANDS]; 2],
    /// Pay multiplier applied in every month without a move (see module docs).
    pub stayer_deflator: [[f64; BANDS]; 2],
}

/// What one month of mobility does to pay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobilityStep {
    /// Factor to multiply base pay by (the stayer deflator, or the jump).
    pub factor: f64,
    /// True if the person changed employer this month.
    pub moved: bool,
}

impl MobilityModel {
    /// Compiles the tables for a level. `Off` gives a model that never moves (it is
    /// not normally built).
    pub fn new(level: JobMobility) -> MobilityModel {
        let scale = match level {
            JobMobility::Off => 0.0,
            JobMobility::Typical => 1.0,
            JobMobility::High => HIGH_LEVEL_MULTIPLIER,
        };
        let mut hazard = [[0.0; BANDS]; 2];
        let mut mean = [[0.0; BANDS]; 2];
        let mut deflator = [[1.0; BANDS]; 2];
        for rec in 0..2 {
            for b in 0..BANDS {
                // The annual probability becomes a yearly rate, which the level
                // scales; the monthly probability follows from it.
                let annual = ANNUAL_MOVE_PROBABILITY[b].min(MAX_ANNUAL_PROBABILITY);
                let yearly_rate = -math::ln_1p(-annual) * scale;
                let cycle = if rec == 1 {
                    RECESSION_RATE_MULTIPLIER
                } else {
                    1.0
                };
                let h = (1.0 - math::exp(-yearly_rate / 12.0 * cycle)).clamp(0.0, 1.0);
                let mu = MEAN_LOG_JUMP[b]
                    - if rec == 1 {
                        RECESSION_JUMP_PENALTY
                    } else {
                        0.0
                    };
                let gain = math::exp(mu + 0.5 * JUMP_SD * JUMP_SD);
                hazard[rec][b] = h;
                mean[rec][b] = mu;
                deflator[rec][b] = (1.0 - h * gain) / (1.0 - h);
            }
        }
        MobilityModel {
            hazard,
            mean_log_jump: mean,
            stayer_deflator: deflator,
        }
    }

    /// One month for an employed person aged `age` (years), in a recession month or
    /// not, given this month's two variates.
    #[inline]
    pub fn step(&self, age: f64, recession: bool, u_move: f64, r_jump: u64) -> MobilityStep {
        let (r, b) = (usize::from(recession), band(age));
        if u_move < self.hazard[r][b] {
            let log_jump = self.mean_log_jump[r][b] + JUMP_SD * normal_from_bits(r_jump);
            MobilityStep {
                factor: math::exp(log_jump),
                moved: true,
            }
        } else {
            MobilityStep {
                factor: self.stayer_deflator[r][b],
                moved: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_hazard_matches_the_annual_probability() {
        let m = MobilityModel::new(JobMobility::Typical);
        for b in 0..BANDS {
            let annual = 1.0 - math::powi(1.0 - m.hazard[0][b], 12);
            assert!(
                (annual - ANNUAL_MOVE_PROBABILITY[b]).abs() < 1e-12,
                "band {b}"
            );
        }
        // Hand value: 1 - 0.8^(1/12) = 0.018424.
        assert!((m.hazard[0][1] - 0.018_424).abs() < 1e-6);
    }

    #[test]
    fn the_hazard_declines_with_age_and_in_recessions() {
        for level in [JobMobility::Typical, JobMobility::High] {
            let m = MobilityModel::new(level);
            for b in 1..BANDS {
                assert!(m.hazard[0][b] < m.hazard[0][b - 1]);
            }
            for b in 0..BANDS {
                assert!(m.hazard[1][b] < m.hazard[0][b]);
            }
        }
        let t = MobilityModel::new(JobMobility::Typical);
        let h = MobilityModel::new(JobMobility::High);
        // High scales the yearly rate: -ln(0.8) * 1.6, so 1 - 0.8^1.6 a year.
        let annual_high = 1.0 - math::powi(1.0 - h.hazard[0][1], 12);
        assert!((annual_high - (1.0 - math::pow(0.8, 1.6))).abs() < 1e-12);
        assert!(h.hazard[0][1] > t.hazard[0][1]);
    }

    #[test]
    fn the_stayer_deflator_keeps_the_mean_on_the_curve() {
        let m = MobilityModel::new(JobMobility::Typical);
        for rec in 0..2 {
            for b in 0..BANDS {
                let h = m.hazard[rec][b];
                let gain = math::exp(m.mean_log_jump[rec][b] + 0.5 * JUMP_SD * JUMP_SD);
                // What `step` does: a move multiplies by exp(J) (mean `gain`), a
                // month without one by the deflator.
                let mean = h * gain + (1.0 - h) * m.stayer_deflator[rec][b];
                assert!((mean - 1.0).abs() < 1e-12, "cell {rec},{b}: {mean}");
                // Under 1 where movers gain on average; a little over 1 for older
                // workers in a recession, whose moves lose pay on average.
                assert!((m.stayer_deflator[rec][b] - 1.0).abs() < 0.01);
                assert_eq!(m.stayer_deflator[rec][b] < 1.0, gain > 1.0);
            }
        }
        // Hand value for a 30-year-old in an expansion: h = 0.018424, gain =
        // exp(0.08 + 0.0072) = 1.091121, deflator = (1 - 0.018424 * 1.091121) /
        // (1 - 0.018424) = 0.998290.
        assert!((m.stayer_deflator[0][1] - 0.998290).abs() < 1e-6);
    }

    #[test]
    fn step_is_mean_preserving_over_the_two_variates() {
        // Integrates `step` itself over a grid of both variates: the expected pay
        // factor of a month is 1 in every cell (this fails if the mover's factor and
        // the deflator do not belong together).
        let m = MobilityModel::new(JobMobility::High);
        let n = 4000u64;
        for (age, rec) in [(23.0, false), (30.0, false), (40.0, true), (58.0, false)] {
            let (r, b) = (usize::from(rec), band(age));
            let h = m.hazard[r][b];
            let mut moved = 0.0;
            for i in 0..n {
                let q = (i as f64 + 0.5) / n as f64;
                let bits = ((q * (1u64 << 53) as f64) as u64) << 11;
                moved += m.step(age, rec, 0.0, bits).factor;
            }
            let stay = m.step(age, rec, 1.0 - 1e-12, 0).factor;
            let mean = h * moved / n as f64 + (1.0 - h) * stay;
            assert!(
                (mean - 1.0).abs() < 2e-5,
                "age {age} recession {rec}: {mean}"
            );
        }
    }

    #[test]
    fn a_draw_below_the_hazard_moves_and_above_stays() {
        let m = MobilityModel::new(JobMobility::Typical);
        let h = m.hazard[0][1];
        let stay = m.step(30.0, false, h + 1e-9, 0);
        assert!(!stay.moved);
        assert_eq!(stay.factor, m.stayer_deflator[0][1]);
        // The median word gives z = 0, so the jump is exp(mu).
        let go = m.step(30.0, false, h - 1e-9, 1u64 << 63);
        assert!(go.moved);
        assert!((go.factor - math::exp(0.08)).abs() < 1e-9);
        // In a recession the mean jump is lower by the penalty.
        let go_rec = m.step(30.0, true, 0.0, 1u64 << 63);
        assert!((go_rec.factor - math::exp(0.04)).abs() < 1e-9);
    }

    #[test]
    fn jump_distribution_is_what_is_documented() {
        let m = MobilityModel::new(JobMobility::Typical);
        let n = 20_000u64;
        let (mut sum, mut sum_sq) = (0.0, 0.0);
        for i in 0..n {
            let q = (i as f64 + 0.5) / n as f64; // evenly spaced quantiles
            let bits = ((q * (1u64 << 53) as f64) as u64) << 11;
            let l = math::ln(m.step(30.0, false, 0.0, bits).factor);
            sum += l;
            sum_sq += l * l;
        }
        let mean = sum / n as f64;
        let sd = (sum_sq / n as f64 - mean * mean).sqrt();
        assert!((mean - 0.08).abs() < 0.002, "{mean}");
        assert!((sd - JUMP_SD).abs() < 0.003, "{sd}");
    }
}
