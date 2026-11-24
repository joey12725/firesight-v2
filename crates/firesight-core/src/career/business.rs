//! Opt-in risks of a self-employment income (`incomes[].kind.failure` and
//! `incomes[].kind.monthly_lumpiness`).
//!
//! Responsibility: whether the business is open this month, at what share of its
//! unbroken profit, and how this month's profit differs from the average month of
//! its year. Compiled once per income in [`super::SelfEmploymentModel`] and only
//! read on the path. Both options are off by default; with both off no
//! [`Business`] exists, [`super::SelfEmploymentModel::business_step`] is never
//! called and profit is exactly what it was before these options existed.
//!
//! Contract:
//!
//! * Draws only from `Stream::Business(k)`, `k` being the position of the income in
//!   `profile.incomes`: exactly [`WORDS_PER_MONTH`] words every month the owner is
//!   alive, whether or not the income has started, is open or has ended, so a
//!   change to the plan (retiring earlier, ending the income) never moves a later
//!   draw. The first word decides a closure, the second is the month's lump.
//! * Nothing here touches `Stream::Career`: the yearly profit factor of
//!   [`super::SelfEmploymentModel::step`] is the same with these options on or off.
//! * `assumptions.career_risk == false` switches both off (no [`Business`] is
//!   built, so nothing is drawn).
//!
//! # Closure
//!
//! With `p` the entered chance per year (the young one while the venture has been
//! open for fewer than `young_years`, the mature one after), the chance of closing
//! in a month the income is active is
//!
//! ```text
//! q = 1 - (1 - p)^(1/12)                      outside recessions
//! q = min(1, recession_multiplier * q)        in a recession month
//! ```
//!
//! so the entered figure is the yearly chance in a year with no recession month. A
//! closure pays nothing from that month on. With `then: stops` that is for good.
//! With `then: restart` profit is zero for `gap_months` months in all, then resumes
//! at `income_fraction` times what it would have been (the cut is lasting and
//! compounds over repeated closures), and the venture's age starts again from
//! zero, so it is young again. The age of a venture counts the months it is open
//! and the income is active.
//!
//! This is deliberately not mean-preserving: it is a risk the default model does
//! not have, and it lowers expected profit. [`Failure::expected_level`] gives the
//! exact expected share of unbroken profit after a number of months without
//! recessions. Planning figures that read the path (the tax plan for the rest of
//! the year, the income the lifestyle ratchet follows) use the current level: zero
//! while closed, the reduced level after a restart.
//!
//! # Lumpiness
//!
//! Each month's profit is multiplied by an independent factor with mean exactly 1:
//! a lognormal with the entered coefficient of variation, limited to
//! `max_month_as_multiple_of_average` times its mean (12: no month pays more than
//! about a year of expected profit) and rescaled to mean 1 after the limit. The
//! expected total of a year is therefore unchanged; the year's actual total varies
//! by about the entered figure divided by `sqrt(12)` on top of the yearly factor.
//! The lognormal is a judgement: it never pays exactly nothing in a month.

use serde_json::Value;

use crate::data::Data;
use crate::math;
use crate::model::{AfterFailure, BusinessFailure, SelfEmployment};
use crate::rng::Rng;
use crate::sampler::{self, ShockTable};

/// Words drawn from `Stream::Business(k)` per month: closure, lump.
pub const WORDS_PER_MONTH: usize = 2;
/// `closed_months_left` of a business that will not reopen.
pub(super) const CLOSED_FOR_GOOD: u32 = u32::MAX;
/// Largest yearly closure chance accepted (validation allows less).
const MAX_ANNUAL_PROBABILITY: f64 = 0.95;

/// The self-employment part of `business_and_equity` in
/// `data/actuarial/params.json`: a judgement, not an estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusinessParams {
    /// No month's lump exceeds this many times the average month (before the
    /// rescaling that restores the mean).
    pub lump_max_multiple: f64,
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

impl BusinessParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks the expected number (a build error: the file is
    /// embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> BusinessParams {
        BusinessParams {
            lump_max_multiple: num(
                &data.actuarial,
                &[
                    "business_and_equity",
                    "monthly_lumpiness",
                    "max_month_as_multiple_of_average",
                ],
            ),
        }
    }
}

/// Monthly probability with the same chance of at least one event in twelve months
/// as `annual`.
pub(super) fn monthly_probability(annual: f64) -> f64 {
    let p = if annual.is_finite() {
        annual.clamp(0.0, MAX_ANNUAL_PROBABILITY)
    } else {
        0.0
    };
    1.0 - math::pow(1.0 - p, 1.0 / 12.0)
}

/// Whole months in a number of years, never negative.
fn months(years: f64) -> u32 {
    if years.is_finite() {
        (years.clamp(0.0, 200.0) * 12.0).round() as u32
    } else {
        0
    }
}

/// `failure: hazard` for one income.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Failure {
    /// Monthly chance of closing, `[0 young, 1 mature][recession]`.
    pub(super) hazard: [[f64; 2]; 2],
    /// A venture is young while it has been open for fewer months than this.
    pub(super) young_months: u32,
    /// Age of the business in months when the income starts.
    pub(super) age0_months: u32,
    /// `(gap months, share of the old profit)` when work starts again.
    pub(super) restart: Option<(u32, f64)>,
}

impl Failure {
    fn new(failure: &BusinessFailure, already_running: bool) -> Option<Failure> {
        let BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            years_in_business,
            recession_multiplier,
            then,
        } = *failure
        else {
            return None;
        };
        let cycle = if recession_multiplier.is_finite() {
            recession_multiplier.clamp(0.0, 100.0)
        } else {
            1.0
        };
        let row = |annual: f64| {
            let q = monthly_probability(annual);
            [q, (q * cycle).min(1.0)]
        };
        let young_months = months(young_years);
        Some(Failure {
            hazard: [
                row(young_annual_probability),
                row(mature_annual_probability),
            ],
            young_months,
            age0_months: match years_in_business {
                Some(years) => months(years),
                // Unstated: an income already being received is an established
                // business, one that starts later is new.
                None if already_running => young_months,
                None => 0,
            },
            restart: match then {
                AfterFailure::Stops => None,
                AfterFailure::Restart {
                    gap_months,
                    income_fraction,
                } => Some((
                    gap_months.min(1200),
                    if income_fraction.is_finite() {
                        income_fraction.clamp(0.0, 10.0)
                    } else {
                        0.0
                    },
                )),
            },
        })
    }

    /// The exact expected share of unbroken profit paid in each of the first
    /// `horizon` months of an income that is active throughout, with no recession
    /// month: a sum over the ways the business can have closed and reopened.
    pub fn expected_level(&self, horizon: usize) -> Vec<f64> {
        // Mass and mass-weighted level by venture age (open) and by months of gap
        // left (closed). Ages at or beyond `young_months` are one state.
        let cap = self.young_months as usize;
        let gap = self.restart.map_or(0, |(g, _)| g as usize);
        let mut open = vec![(0.0, 0.0); cap + 1];
        let mut closed = vec![(0.0, 0.0); gap + 1];
        open[(self.age0_months as usize).min(cap)] = (1.0, 1.0);
        let mut out = Vec::with_capacity(horizon);
        for _ in 0..horizon {
            let mut next_open = vec![(0.0, 0.0); cap + 1];
            let mut next_closed = vec![(0.0, 0.0); gap + 1];
            let mut paid = 0.0;
            let add = |slot: &mut (f64, f64), mass: f64, level: f64| {
                slot.0 += mass;
                slot.1 += level;
            };
            // Businesses that were closed: one month nearer to reopening. A gap
            // that runs out this month reopens and pays this month.
            for (left, &(mass, level)) in closed.iter().enumerate().skip(1) {
                if left == 1 {
                    paid += level;
                    add(&mut next_open[0], mass, level);
                } else {
                    add(&mut next_closed[left - 1], mass, level);
                }
            }
            for (age, &(mass, level)) in open.iter().enumerate() {
                if mass == 0.0 {
                    continue;
                }
                let q = self.hazard[usize::from(age >= cap)][0];
                // Survivors are paid and age by a month.
                paid += (1.0 - q) * level;
                add(
                    &mut next_open[(age + 1).min(cap)],
                    (1.0 - q) * mass,
                    (1.0 - q) * level,
                );
                if let Some((g, fraction)) = self.restart {
                    if g == 0 {
                        // Reopens at once at the lower level, and is paid.
                        paid += q * level * fraction;
                        add(&mut next_open[0], q * mass, q * level * fraction);
                    } else {
                        add(&mut next_closed[g as usize], q * mass, q * level * fraction);
                    }
                }
            }
            out.push(paid);
            open = next_open;
            closed = next_closed;
        }
        out
    }
}

/// What the business did in one month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusinessMonth {
    /// This month's profit relative to the average month of its year (mean 1).
    pub lump: f64,
    /// True in the month the business closes.
    pub closed: bool,
    /// True in the month work starts again.
    pub reopened: bool,
}

/// The per-path state of the closure process, part of
/// [`super::SelfEmploymentState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusinessState {
    /// Share of unbroken profit paid now: 0 while closed, 1 before any closure,
    /// the product of the restart fractions after reopening.
    pub level: f64,
    /// The level the business pays (or will pay) when open.
    pub open_level: f64,
    /// Months until work starts again; 0 while open.
    pub closed_months_left: u32,
    /// Months the current venture has been open and active.
    pub venture_months: u32,
    /// Closures so far on this path.
    pub closures: u32,
}

impl BusinessState {
    /// The share of unbroken profit the business pays when open, or 1 if it has
    /// closed for good: what a "set the profit to this amount" event divides by so
    /// that the amount it sets is the amount paid.
    pub fn restart_share(&self) -> f64 {
        if self.open_level > 0.0 {
            self.open_level
        } else {
            1.0
        }
    }
}

impl Default for BusinessState {
    fn default() -> Self {
        Self {
            level: 1.0,
            open_level: 1.0,
            closed_months_left: 0,
            venture_months: 0,
            closures: 0,
        }
    }
}

/// The opt-in risks one self-employment income has switched on.
#[derive(Clone, Debug)]
pub(super) struct Business {
    pub(super) failure: Option<Failure>,
    /// The monthly lump factor, mean exactly 1.
    pub(super) lump: Option<ShockTable>,
}

impl Business {
    /// `None` when neither option is on. `already_running` says whether the income
    /// is being received at `profile.start`.
    pub(super) fn new(
        se: &SelfEmployment,
        already_running: bool,
        params: &BusinessParams,
    ) -> Option<Business> {
        let failure = Failure::new(&se.failure, already_running);
        let lump = lump_table(se.monthly_lumpiness, params.lump_max_multiple);
        (failure.is_some() || lump.is_some()).then_some(Business { failure, lump })
    }

    /// State at `profile.start`.
    pub(super) fn initial_state(&self) -> BusinessState {
        BusinessState {
            venture_months: self.failure.map_or(0, |f| f.age0_months),
            ..BusinessState::default()
        }
    }

    /// One month. `active` says whether the income would be paid this month if the
    /// business were open (started, not ended, owner not retired). Draws exactly
    /// [`WORDS_PER_MONTH`] words.
    #[inline]
    pub(super) fn step(
        &self,
        s: &mut BusinessState,
        active: bool,
        recession: bool,
        rng: &mut Rng,
    ) -> BusinessMonth {
        let u_close = rng.uniform();
        let r_lump = rng.next_u64();
        let mut out = BusinessMonth {
            lump: 1.0,
            closed: false,
            reopened: false,
        };
        if let Some(f) = &self.failure {
            if s.closed_months_left > 0 {
                if s.closed_months_left != CLOSED_FOR_GOOD {
                    s.closed_months_left -= 1;
                    if s.closed_months_left == 0 {
                        s.level = s.open_level;
                        s.venture_months = 0;
                        out.reopened = true;
                    }
                }
            } else if active {
                let mature = usize::from(s.venture_months >= f.young_months);
                if u_close < f.hazard[mature][usize::from(recession)] {
                    s.closures = s.closures.saturating_add(1);
                    out.closed = true;
                    match f.restart {
                        None => {
                            s.level = 0.0;
                            s.open_level = 0.0;
                            s.closed_months_left = CLOSED_FOR_GOOD;
                        }
                        Some((gap, fraction)) => {
                            s.open_level *= fraction;
                            s.venture_months = 0;
                            if gap == 0 {
                                s.level = s.open_level;
                                out.reopened = true;
                            } else {
                                s.level = 0.0;
                                s.closed_months_left = gap;
                            }
                        }
                    }
                } else {
                    s.venture_months = s.venture_months.saturating_add(1);
                }
            }
        }
        if let Some(table) = &self.lump {
            out.lump = table.sample(r_lump);
        }
        out
    }
}

/// The monthly lump factor for a coefficient of variation `cv`: a lognormal limited
/// to `max_multiple` times its mean, rescaled to mean exactly 1. `None` for no
/// lumpiness.
pub(super) fn lump_table(cv: f64, max_multiple: f64) -> Option<ShockTable> {
    if !cv.is_finite() || cv <= 0.0 {
        return None;
    }
    let sigma = super::log_sd_from_cv(cv.min(10.0));
    let max = if max_multiple.is_finite() {
        max_multiple.max(1.0)
    } else {
        1.0
    };
    // In units of the unlimited lognormal's median: its mean is exp(sigma^2 / 2).
    let cap = max * math::exp(0.5 * sigma * sigma);
    let mut table = sampler::normal().transformed(|z| math::exp(sigma * z).min(cap));
    let k = 1.0 / table.mean();
    table.scale(k);
    Some(table)
}

#[cfg(test)]
#[path = "business_tests.rs"]
mod tests;
