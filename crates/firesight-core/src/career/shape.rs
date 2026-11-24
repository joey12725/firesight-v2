//! Opt-in shapes for the random parts of a job (`incomes[].kind.risk`).
//!
//! Responsibility: the three pure mechanisms behind `wage_shocks: fat_tailed`,
//! `layoff_pay_cut: two_part` and `job_search: monthly`. Each is compiled once per
//! job in [`super::JobModel::with_params`] and only read on the path. All three are
//! off by default, and with them off [`super::JobModel::step`] does exactly the
//! arithmetic it did before they existed.
//!
//! Contract:
//!
//! * No new random numbers. Every mechanism here turns a word that
//!   [`super::JobModel::step`] already draws into its outcome, so switching one on
//!   moves no other draw on any stream:
//!   - the wage mixture uses the January persistent-shock word;
//!   - the two-part pay cut uses the scar word (of the layoff month, or of the
//!     re-employment month when the search is monthly);
//!   - the monthly search uses the spell word of each month out of work. In the
//!     default model that word is only read in the month of a layoff, which needs
//!     the job to be held, so in the months after it the word is drawn and
//!     discarded; in the layoff month itself it is the same word the default uses
//!     for the spell length.
//! * Where one word must give two independent things (which part of a mixture, and
//!   the value inside it) the low 11 bits choose the part and the high 53 bits give
//!   the value. [`normal_from_bits`] and [`CompiledDist::sample`] read only the high
//!   53 bits, so the two are independent. A probability is therefore a whole number
//!   of 2048ths, and every mean below is computed from that rounded probability.
//! * Nothing here allocates after compilation or calls a rejection sampler.
//!
//! # The wage mixture
//!
//! The default persistent innovation is `eta = sigma_eta * Z`, `Z ~ N(0, 1)`. Here
//! `eta = sigma_eta * X`, where `X` is `N(m1, s1^2)` with probability `1 - p` (a
//! small move) and `N(m2, s2^2)` with probability `p` (a large one). With the three
//! parameters `p`, `k = s2 / s1` and `m2` given, `m1 = -p m2 / (1 - p)` makes the
//! mean zero and `s1^2 = (1 - (1 - p) m1^2 - p m2^2) / (1 - p + p k^2)` makes the
//! variance one, so the variance of the persistent shock is the default's.
//!
//! The default keeps expected pay on the curve by subtracting `Var[z] / 2`, which is
//! `ln E[exp(z)]` for a Gaussian `z`. For the mixture the same quantity is
//!
//! ```text
//! ln E[exp(z_n)] = sum over j < n of ln M(sigma_eta * rho^j)
//! M(t) = (1 - p) exp(m1 t + s1^2 t^2 / 2) + p exp(m2 t + s2^2 t^2 / 2)
//! ```
//!
//! after `n` yearly shocks, which depends only on `n`. It is tabulated at
//! compilation and subtracted in place of `Var[z] / 2`, so expected pay is the curve
//! exactly, as in the default.
//!
//! # The two-part pay cut
//!
//! The default log pay loss on re-employment is `L ~ N(mu, sd_L)`. Here, with `q`
//! the chance of no loss for the layoff's regime and age,
//!
//! ```text
//! with probability q:      L ~ N(0, no_loss_sd)
//! with probability 1 - q:  L = top * (1 - (1 - U)^(1 / b)),   U uniform,
//!                          m = mu / (1 - q),  top = max(cap, k m),  b = top / m - 1
//! ```
//!
//! The loss is `top` times a Beta(1, b) variable, whose mean is `1 / (1 + b)`, so
//! the mean loss of those who lose is `m` and `E[L] = mu`: the default's mean for
//! the same layoff (regime, age and length of spell), exactly. Its density falls
//! from zero loss to nothing at `top`, so it is right-skewed: most losses are below
//! the average loss and a few are several times it. `cap` is the log of a halving of
//! pay and `k` is 3, so `b` is at least 2 and no loss exceeds the larger of half of
//! pay and three times the mean loss of those who lose. The quantile is in closed
//! form, so one word gives one value and no table is needed.
//!
//! # The monthly search
//!
//! With `d` whole months out of work so far, the search ends this month with
//! probability
//!
//! ```text
//! f(d) = f0[regime now][age band] * (floor + (1 - floor) * exp(-d / fade_months))
//! ```
//!
//! and when it ends the part of the month spent out of work is uniform (not less
//! than a quarter of a month in the first month; the default's floor). A search
//! still running in its 36th month ends with it (the default's cap). `f0` is solved
//! by bisection so that the mean spell with the regime held fixed equals the mean
//! of the default clipped lognormal for that regime and age band
//! ([`JobSearch::mean_months`] is exact, not simulated).

use serde_json::Value;

use super::{CareerParams, SPELL_MAX_MONTHS, SPELL_MIN_MONTHS};
use crate::data::Data;
use crate::math;
use crate::model::{JobSearchModel, PayCutShape, WageShockShape};
use crate::rng::normal_from_bits;

/// Bits of a word that choose the part of a mixture.
const PICK_BITS: u32 = 11;
const PICK_LEVELS: u64 = 1 << PICK_BITS;
/// Yearly shocks covered by the mean-correction table. Beyond it the last value is
/// used; by then the remaining terms are below 1e-12.
const WAGE_TABLE_YEARS: usize = 512;
/// The longest spell in whole months (the default's cap).
pub(super) const SEARCH_MONTHS: usize = SPELL_MAX_MONTHS as usize;
/// `spell_months_left` while a monthly search is still running: any value above one
/// month, so the month is spent wholly out of work and the spell continues.
pub(super) const SEARCHING: f64 = 2.0;
/// No chance of an unchanged wage may exceed this, so the loss part keeps a share.
const MAX_NO_LOSS_PROBABILITY: f64 = 0.9;

/// The `career_shapes` section of `data/actuarial/params.json`: judgements, not
/// estimates (see the notes beside each number there).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeParams {
    /// Chance that a year's persistent wage shock is a large move.
    pub large_move_probability: f64,
    /// Standard deviation of a large move relative to a small one.
    pub large_to_small_sd_ratio: f64,
    /// Mean of a large move, in standard deviations of the whole shock (negative:
    /// large moves are more often down).
    pub large_move_mean_shift: f64,
    /// Chance of re-employment at about the old wage, expansion and recession
    /// regime.
    pub no_loss_probability: [f64; 2],
    /// Multiplier on that chance from age 50.
    pub no_loss_older_multiplier: f64,
    /// Standard deviation of the log wage change when there is no loss.
    pub no_loss_sd: f64,
    /// The largest lasting pay cut as a fraction of pay (0.5 = half), unless
    /// `loss_max_multiple` times the mean loss of those who lose is larger.
    pub loss_max_pay_cut: f64,
    /// Largest loss as a multiple of the mean loss of those who lose, at least.
    pub loss_max_multiple: f64,
    /// Job-finding chance after a long search relative to the first month.
    pub search_floor: f64,
    /// Months over which the job-finding chance fades towards the floor.
    pub search_fade_months: f64,
}

fn num(root: &Value, path: &[&str]) -> f64 {
    let mut v = root;
    for key in path {
        v = &v[*key];
    }
    v.as_f64().unwrap_or_else(|| {
        panic!(
            "data/actuarial/params.json: expected a number at {}",
            path.join(".")
        )
    })
}

impl ShapeParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks one of the expected numbers (a build error: the file
    /// is embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> ShapeParams {
        let a = &data.actuarial;
        let wage = |k: &str| num(a, &["career_shapes", "wage_mixture", k]);
        let cut = |k: &str| num(a, &["career_shapes", "pay_cut_two_part", k]);
        let search = |k: &str| num(a, &["career_shapes", "job_search_hazard", k]);
        ShapeParams {
            large_move_probability: wage("large_move_probability"),
            large_to_small_sd_ratio: wage("large_to_small_sd_ratio"),
            large_move_mean_shift: wage("large_move_mean_shift_in_sd"),
            no_loss_probability: [
                cut("no_loss_probability_expansion"),
                cut("no_loss_probability_recession"),
            ],
            no_loss_older_multiplier: cut("no_loss_age_50_plus_multiplier"),
            no_loss_sd: cut("no_loss_sd"),
            loss_max_pay_cut: cut("loss_max_pay_cut"),
            loss_max_multiple: cut("loss_max_as_multiple_of_mean_loss_at_least"),
            search_floor: search("floor"),
            search_fade_months: search("fade_months"),
        }
    }
}

/// The low bits of a word, a whole number below [`PICK_LEVELS`], independent of the
/// value [`normal_from_bits`] makes from the same word.
#[inline]
fn pick(bits: u64) -> u64 {
    bits & (PICK_LEVELS - 1)
}

/// A probability as a count of [`PICK_LEVELS`]: `pick(bits) < count` is true with
/// probability `count / PICK_LEVELS`.
fn pick_count(probability: f64) -> u64 {
    let p = if probability.is_finite() {
        probability.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p * PICK_LEVELS as f64).round() as u64
}

/// A uniform on `[0, 1)` from the high 53 bits of a word.
#[inline]
fn unit(bits: u64) -> f64 {
    (bits >> PICK_BITS) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// A two-part normal mixture with mean 0 and variance 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct UnitMixture {
    /// Probability of the second (large) part, a whole number of 2048ths.
    pub(super) p: f64,
    /// Means of the small and large parts.
    pub(super) mean: [f64; 2],
    /// Standard deviations of the small and large parts.
    pub(super) sd: [f64; 2],
}

impl UnitMixture {
    /// Solves the two free values (the small part's mean and standard deviation) so
    /// that the mixture has mean 0 and variance 1. Out-of-range inputs are clamped.
    pub(super) fn new(probability: f64, sd_ratio: f64, large_mean: f64) -> UnitMixture {
        let p = pick_count(probability) as f64 / PICK_LEVELS as f64;
        if p <= 0.0 || p >= 1.0 {
            // One part only: the standard normal.
            return UnitMixture {
                p: 0.0,
                mean: [0.0, 0.0],
                sd: [1.0, 1.0],
            };
        }
        let k = if sd_ratio.is_finite() {
            sd_ratio.clamp(1.0, 20.0)
        } else {
            1.0
        };
        // The spread between the two means may use at most half the variance.
        let limit = (0.5 * (1.0 - p) / p).sqrt();
        let m2 = if large_mean.is_finite() {
            large_mean.clamp(-limit, limit)
        } else {
            0.0
        };
        let m1 = -p * m2 / (1.0 - p);
        let between = (1.0 - p) * m1 * m1 + p * m2 * m2;
        let s1 = ((1.0 - between) / (1.0 - p + p * k * k)).sqrt();
        UnitMixture {
            p,
            mean: [m1, m2],
            sd: [s1, k * s1],
        }
    }

    /// `ln E[exp(t X)]`.
    pub(super) fn log_mgf(&self, t: f64) -> f64 {
        let part = |i: usize| self.mean[i] * t + 0.5 * self.sd[i] * self.sd[i] * t * t;
        let (a, b) = (part(0), part(1));
        // Factor out the larger exponent so nothing overflows.
        let top = a.max(b);
        top + math::ln((1.0 - self.p) * math::exp(a - top) + self.p * math::exp(b - top))
    }

    /// The raw moment `E[X^n]` for `n` from 1 to 4.
    #[cfg(test)]
    pub(super) fn moment(&self, n: u32) -> f64 {
        let part = |i: usize| {
            let (m, v) = (self.mean[i], self.sd[i] * self.sd[i]);
            match n {
                1 => m,
                2 => m * m + v,
                3 => m * m * m + 3.0 * m * v,
                _ => m * m * m * m + 6.0 * m * m * v + 3.0 * v * v,
            }
        };
        (1.0 - self.p) * part(0) + self.p * part(1)
    }
}

/// `wage_shocks: fat_tailed` for one job.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WageMixture {
    /// `pick(bits) < large_below` chooses the large part.
    large_below: u64,
    /// Means of the two parts in log-pay units (`sigma_eta` times the unit means).
    mean: [f64; 2],
    /// Standard deviations of the two parts in log-pay units.
    sd: [f64; 2],
    /// `ln E[exp(z_n)]` after `n` yearly shocks, `n` from 0.
    log_mean: Vec<f64>,
}

impl WageMixture {
    pub(super) fn new(sigma_eta: f64, rho: f64, p: &ShapeParams) -> WageMixture {
        let unit = UnitMixture::new(
            p.large_move_probability,
            p.large_to_small_sd_ratio,
            p.large_move_mean_shift,
        );
        let sigma = sigma_eta.max(0.0);
        let mut log_mean = Vec::with_capacity(WAGE_TABLE_YEARS + 1);
        let mut total = 0.0;
        let mut weight = 1.0;
        log_mean.push(0.0);
        for _ in 0..WAGE_TABLE_YEARS {
            total += unit.log_mgf(sigma * weight);
            weight *= rho;
            log_mean.push(total);
        }
        WageMixture {
            large_below: pick_count(unit.p),
            mean: [sigma * unit.mean[0], sigma * unit.mean[1]],
            sd: [sigma * unit.sd[0], sigma * unit.sd[1]],
            log_mean,
        }
    }

    /// This year's persistent innovation, in log-pay units, from one word.
    #[inline]
    pub(super) fn draw(&self, bits: u64) -> f64 {
        let part = usize::from(pick(bits) < self.large_below);
        self.mean[part] + self.sd[part] * normal_from_bits(bits)
    }

    /// `ln E[exp(z)]` of the persistent shock after `shocks` yearly innovations:
    /// what to subtract so that expected pay stays on the curve.
    #[inline]
    pub(super) fn log_mean(&self, shocks: u32) -> f64 {
        self.log_mean[(shocks as usize).min(self.log_mean.len() - 1)]
    }
}

/// `layoff_pay_cut: two_part` for one job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PayCut {
    /// `pick(bits) < no_loss_below[class]` means re-employment at about the old
    /// wage. `class = 2 * regime + (1 from age 50)`.
    no_loss_below: [u64; 4],
    /// `1 - q` for each class, from the rounded probability.
    loss_share: [f64; 4],
    no_loss_sd: f64,
    /// Log loss of the largest ordinary pay cut (`-ln(1 - loss_max_pay_cut)`).
    cap: f64,
    /// The largest loss is at least this multiple of the mean loss of those who
    /// lose (2 or more, so the loss is never left-skewed).
    multiple: f64,
}

impl PayCut {
    pub(super) fn new(p: &ShapeParams) -> PayCut {
        let mut no_loss_below = [0; 4];
        let mut loss_share = [1.0; 4];
        for class in 0..4 {
            let older = if class % 2 == 1 {
                p.no_loss_older_multiplier
            } else {
                1.0
            };
            let q = p.no_loss_probability[class / 2] * older;
            let q = if q.is_finite() {
                q.clamp(0.0, MAX_NO_LOSS_PROBABILITY)
            } else {
                0.0
            };
            no_loss_below[class] = pick_count(q);
            loss_share[class] = 1.0 - no_loss_below[class] as f64 / PICK_LEVELS as f64;
        }
        let finite_or = |x: f64, fallback: f64| if x.is_finite() { x } else { fallback };
        PayCut {
            no_loss_below,
            loss_share,
            no_loss_sd: finite_or(p.no_loss_sd, 0.0).max(0.0),
            cap: -math::ln(1.0 - finite_or(p.loss_max_pay_cut, 0.5).clamp(0.0, 0.95)),
            multiple: finite_or(p.loss_max_multiple, 3.0).clamp(2.0, 10.0),
        }
    }

    /// The largest loss and the shape `b` of the loss part when the mean loss of
    /// those who lose is `m > 0`.
    #[inline]
    fn loss_shape(&self, m: f64) -> (f64, f64) {
        let top = self.cap.max(self.multiple * m);
        (top, top / m - 1.0)
    }

    /// The log pay loss on re-employment whose average over draws is `mean`, from
    /// one word. Negative values are raises.
    #[inline]
    pub(super) fn draw(&self, class: usize, mean: f64, bits: u64) -> f64 {
        let class = class.min(3);
        if pick(bits) < self.no_loss_below[class] {
            return self.no_loss_sd * normal_from_bits(bits);
        }
        let m = mean / self.loss_share[class];
        if m.is_nan() || m <= 0.0 {
            // Nothing to lose on average (not the case with the shipped numbers).
            return mean;
        }
        let (top, b) = self.loss_shape(m);
        // The quantile of Beta(1, b): 1 - (1 - u)^(1 / b).
        top * (1.0 - math::pow(1.0 - unit(bits), 1.0 / b))
    }

    /// Chance of no loss for a class, as simulated.
    #[cfg(test)]
    pub(super) fn no_loss_probability(&self, class: usize) -> f64 {
        1.0 - self.loss_share[class]
    }

    /// The largest loss when the overall mean is `mean`, for a class.
    #[cfg(test)]
    pub(super) fn largest(&self, class: usize, mean: f64) -> f64 {
        self.loss_shape(mean / self.loss_share[class]).0
    }
}

/// `job_search: monthly` for one job.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct JobSearch {
    /// Chance that the search ends in a month, `[regime][age band][months so far]`.
    chance: [[[f64; SEARCH_MONTHS]; 3]; 2],
}

impl JobSearch {
    pub(super) fn new(career: &CareerParams) -> JobSearch {
        let p = &career.shapes;
        let floor = if p.search_floor.is_finite() {
            p.search_floor.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let fade = p.search_fade_months;
        let mut shape = [1.0; SEARCH_MONTHS];
        if fade.is_finite() && fade > 0.0 {
            for (d, g) in shape.iter_mut().enumerate() {
                *g = floor + (1.0 - floor) * math::exp(-(d as f64) / fade);
            }
        }
        let mut chance = [[[0.0; SEARCH_MONTHS]; 3]; 2];
        for (regime, by_age) in chance.iter_mut().enumerate() {
            for (band, row) in by_age.iter_mut().enumerate() {
                let target = clipped_lognormal_mean(
                    career.spell_median[regime] * career.spell_age_multiplier[band],
                    career.spell_sigma[regime],
                );
                // The mean spell falls as the first-month chance rises: bisect.
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if Self::mean_months(&scaled(&shape, mid)) > target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                *row = scaled(&shape, 0.5 * (lo + hi));
            }
        }
        JobSearch { chance }
    }

    /// The exact mean spell, in months, when the search ends in month `d` with
    /// chance `chance[d]`: what [`JobSearch::step`] produces with the regime and age
    /// band held fixed.
    pub(super) fn mean_months(chance: &[f64; SEARCH_MONTHS]) -> f64 {
        // A uniform part of a month, not less than the floor in the first month.
        let first = SPELL_MIN_MONTHS * SPELL_MIN_MONTHS
            + 0.5 * (1.0 - SPELL_MIN_MONTHS * SPELL_MIN_MONTHS);
        let mut still_searching = 1.0;
        let mut mean = 0.0;
        for (d, f) in chance.iter().enumerate() {
            let part = if d == 0 { first } else { 0.5 };
            mean += still_searching * (f * part + (1.0 - f));
            still_searching *= 1.0 - f;
        }
        mean
    }

    /// One month of search. `months` is the whole months out of work so far (0 in
    /// the month of the layoff). Returns the part of this month spent out of work
    /// when the search ends in it, `None` while it goes on.
    #[inline]
    pub(super) fn step(&self, regime: usize, band: usize, months: u32, bits: u64) -> Option<f64> {
        let d = (months as usize).min(SEARCH_MONTHS - 1);
        let f = self.chance[regime][band][d];
        let u = unit(bits);
        if u < f {
            let part = u / f;
            Some(if months == 0 {
                part.max(SPELL_MIN_MONTHS)
            } else {
                part
            })
        } else if months as usize >= SEARCH_MONTHS - 1 {
            Some(1.0)
        } else {
            None
        }
    }

    /// The monthly chances for a regime and age band.
    #[cfg(test)]
    pub(super) fn chances(&self, regime: usize, band: usize) -> &[f64; SEARCH_MONTHS] {
        &self.chance[regime][band]
    }
}

fn scaled(shape: &[f64; SEARCH_MONTHS], first: f64) -> [f64; SEARCH_MONTHS] {
    let mut out = [0.0; SEARCH_MONTHS];
    for (o, g) in out.iter_mut().zip(shape) {
        *o = (first * g).clamp(0.0, 1.0);
    }
    out
}

/// Mean of the default spell: a lognormal with this median and log standard
/// deviation, limited to the default's floor and cap.
pub(super) fn clipped_lognormal_mean(median: f64, sigma: f64) -> f64 {
    let (lo, hi) = (SPELL_MIN_MONTHS, SPELL_MAX_MONTHS);
    if median.is_nan() || sigma.is_nan() || median <= 0.0 || sigma <= 0.0 {
        return if median.is_nan() { lo } else { median.clamp(lo, hi) };
    }
    let mu = math::ln(median);
    let a = (math::ln(lo) - mu) / sigma;
    let b = (math::ln(hi) - mu) / sigma;
    lo * math::normal_cdf(a)
        + math::exp(mu + 0.5 * sigma * sigma)
            * (math::normal_cdf(b - sigma) - math::normal_cdf(a - sigma))
        + hi * (1.0 - math::normal_cdf(b))
}

/// The shapes one job has switched on. All `None` is the default model.
#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct Shapes {
    pub(super) wage: Option<WageMixture>,
    pub(super) pay_cut: Option<PayCut>,
    pub(super) search: Option<JobSearch>,
}

impl Shapes {
    /// `sigma_eta` is the job's persistent-shock standard deviation. Call only for a
    /// job whose wage risk is simulated; otherwise use `Shapes::default()`.
    pub(super) fn new(
        wage: WageShockShape,
        pay_cut: PayCutShape,
        search: JobSearchModel,
        sigma_eta: f64,
        career: &CareerParams,
    ) -> Shapes {
        Shapes {
            wage: match wage {
                WageShockShape::Normal => None,
                WageShockShape::FatTailed => {
                    Some(WageMixture::new(sigma_eta, career.rho, &career.shapes))
                }
            },
            pay_cut: match pay_cut {
                PayCutShape::Normal => None,
                PayCutShape::TwoPart => Some(PayCut::new(&career.shapes)),
            },
            search: match search {
                JobSearchModel::DrawnAtLayoff => None,
                JobSearchModel::Monthly => Some(JobSearch::new(career)),
            },
        }
    }
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
