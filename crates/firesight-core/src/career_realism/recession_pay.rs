//! Recession pay freezes, hours cuts and smaller bonuses (`risk.recession_pay`).
//!
//! # What is modelled
//!
//! Layoffs in recessions are already in the career model (a 1.6 times hazard and
//! longer spells). This adds what happens to people who keep their job. In every
//! recession month an employed worker faces, independently:
//!
//! * **A pay freeze** with a yearly probability of 25% (monthly hazard `1 - 0.75^(1/12)`
//!   while the recession flag is set; at most one a year). The raise that would have
//!   come this year is not given: base pay is permanently lower by the raise the job
//!   would have had, which is the real growth of the lifecycle curve at the person's
//!   age plus the path's inflation over the last 12 months (a nominal freeze costs the
//!   inflation too). Not made up later: that is a JUDGEMENT (the freeze literature,
//!   Daly, Hobijn and Lucking, FRBSF Economic Letter 2012-10, is about how many
//!   workers are frozen, not about later catch-up).
//! * **A temporary hours or pay cut** with a yearly probability of 15%: pay falls by
//!   5% to 20% (uniform) for nine months and then returns in full. A new cut can start
//!   once the old one has ended, while the recession lasts.
//!
//! Bonuses are 30% smaller (`BONUS_FACTOR` 0.7) in a recession and for twelve months
//! after it (the slack lasts longer than the recession itself, as in the career
//! model's spell durations). A bonus also carries the freeze, being a share of pay.
//! Commission follows the pay factor.
//!
//! # Parameters and their status
//!
//! The direction is documented: the share of workers whose pay did not change from
//! one year to the next reached a record high in and after the Great Recession (Daly,
//! Hobijn and Lucking 2012; FRBSF Economic Letter, July 2013), and an administrative
//! study of Washington State hourly stayers found 15 to 20% with no change in a year
//! even in normal times (a paper at the 2020 AEA conference). The sizes here are JUDGEMENTS:
//! no source was found that gives per-recession probabilities of a freeze, a cut or
//! their depth for salaried workers, and none of the figures was recomputed. They are
//! named constants, listed in `params.json` under `unverified_from_memory`.
//!
//! The model is not mean-preserving on purpose: it adds recession losses on top of the
//! lifecycle curve, which is an average over cycles and so already contains an average
//! recession effect of unknown size. Expect it to overstate losses slightly for that
//! reason. Pay returns to its curve only through the cut ending; frozen raises are lost.

use crate::math;

/// Probability per year that a worker in a recession year gets a pay freeze
/// (JUDGEMENT, see the module documentation).
pub const ANNUAL_FREEZE_PROBABILITY: f64 = 0.25;
/// Probability per year that a worker in a recession year takes a temporary pay or
/// hours cut (JUDGEMENT).
pub const ANNUAL_CUT_PROBABILITY: f64 = 0.15;
/// Smallest and largest cut as a fraction of pay (JUDGEMENT).
pub const CUT_MIN: f64 = 0.05;
/// See [`CUT_MIN`].
pub const CUT_MAX: f64 = 0.20;
/// Months a cut lasts before pay returns in full (JUDGEMENT).
pub const CUT_MONTHS: u32 = 9;
/// Months after a freeze during which no further freeze happens: one raise a year.
pub const FREEZE_COOLDOWN_MONTHS: u32 = 12;
/// Multiplier on bonuses in a recession and for [`BONUS_SLACK_MONTHS`] after it
/// (JUDGEMENT).
pub const BONUS_FACTOR: f64 = 0.7;
/// Months after a recession ends during which bonuses stay reduced (JUDGEMENT, the
/// same 12 months of slack the career model uses for unemployment spells).
pub const BONUS_SLACK_MONTHS: u32 = 12;

/// Compiled hazards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecessionPayModel {
    /// Monthly probability of a freeze in a recession month.
    pub freeze_monthly: f64,
    /// Monthly probability that a cut starts in a recession month.
    pub cut_monthly: f64,
}

/// Per-path, per-job state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecessionPayState {
    /// Log of the permanent level lost to freezes so far (zero or negative).
    pub freeze_log: f64,
    /// Months the current cut still has to run.
    pub cut_left: u32,
    /// Size of the current cut as a fraction of pay.
    pub cut_fraction: f64,
    /// Months before another freeze is possible.
    pub cooldown: u32,
    /// Months since the last recession month (large before the first).
    pub since_recession: u32,
}

impl Default for RecessionPayState {
    fn default() -> Self {
        Self {
            freeze_log: 0.0,
            cut_left: 0,
            cut_fraction: 0.0,
            cooldown: 0,
            since_recession: u32::MAX / 2,
        }
    }
}

/// What to multiply this month's pay by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PayFactors {
    /// Factor on base pay and commission.
    pub wage: f64,
    /// Factor on the bonus.
    pub bonus: f64,
    /// True if a freeze started this month.
    pub froze: bool,
    /// True if a cut started this month.
    pub cut: bool,
}

impl RecessionPayModel {
    /// Compiles the hazards from the yearly probabilities.
    pub fn new() -> RecessionPayModel {
        let monthly = |annual: f64| 1.0 - math::pow(1.0 - annual, 1.0 / 12.0);
        RecessionPayModel {
            freeze_monthly: monthly(ANNUAL_FREEZE_PROBABILITY),
            cut_monthly: monthly(ANNUAL_CUT_PROBABILITY),
        }
    }

    /// One month. `employed` says whether the worker holds the job and is paid (new
    /// freezes and cuts only start then); `recession` is the path's flag;
    /// `raise_forgone` is the raise a freeze would withhold, as a fraction (real curve
    /// growth over the next year, at least zero, plus the last 12 months' inflation,
    /// at least zero); the three uniforms are this month's variates.
    pub fn step(
        &self,
        state: &mut RecessionPayState,
        employed: bool,
        recession: bool,
        raise_forgone: f64,
        (u_freeze, u_cut, u_size): (f64, f64, f64),
    ) -> PayFactors {
        if recession {
            state.since_recession = 0;
        } else {
            state.since_recession = state.since_recession.saturating_add(1);
        }
        let mut froze = false;
        let mut cut = false;
        if employed && recession {
            if state.cooldown == 0 && u_freeze < self.freeze_monthly {
                state.freeze_log -= math::ln_1p(raise_forgone.max(0.0));
                state.cooldown = FREEZE_COOLDOWN_MONTHS;
                froze = true;
            }
            if state.cut_left == 0 && u_cut < self.cut_monthly {
                state.cut_left = CUT_MONTHS;
                state.cut_fraction = CUT_MIN + (CUT_MAX - CUT_MIN) * u_size;
                cut = true;
            }
        }
        let level = math::exp(state.freeze_log);
        let wage = level
            * if state.cut_left > 0 {
                1.0 - state.cut_fraction
            } else {
                1.0
            };
        let bonus = level
            * if state.since_recession <= BONUS_SLACK_MONTHS {
                BONUS_FACTOR
            } else {
                1.0
            };
        if employed {
            state.cut_left = state.cut_left.saturating_sub(1);
            state.cooldown = state.cooldown.saturating_sub(1);
        }
        PayFactors {
            wage,
            bonus,
            froze,
            cut,
        }
    }
}

impl Default for RecessionPayModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> RecessionPayModel {
        RecessionPayModel::new()
    }

    #[test]
    fn hazards_match_the_yearly_probabilities() {
        let m = model();
        assert!((1.0 - math::powi(1.0 - m.freeze_monthly, 12) - 0.25).abs() < 1e-12);
        assert!((1.0 - math::powi(1.0 - m.cut_monthly, 12) - 0.15).abs() < 1e-12);
        // Hand value: 1 - 0.75^(1/12) = 0.023688.
        assert!((m.freeze_monthly - 0.023_688).abs() < 1e-6);
    }

    #[test]
    fn nothing_happens_outside_a_recession_or_when_not_employed() {
        let m = model();
        let mut s = RecessionPayState::default();
        let f = m.step(&mut s, true, false, 0.05, (0.0, 0.0, 0.5));
        assert_eq!((f.wage, f.bonus, f.froze, f.cut), (1.0, 1.0, false, false));
        let mut s = RecessionPayState::default();
        let f = m.step(&mut s, false, true, 0.05, (0.0, 0.0, 0.5));
        assert!(!f.froze && !f.cut);
        assert_eq!(f.wage, 1.0);
        // The recession still starts the bonus slack clock.
        assert_eq!(f.bonus, BONUS_FACTOR);
    }

    #[test]
    fn a_freeze_lowers_pay_permanently_by_the_forgone_raise() {
        let m = model();
        let mut s = RecessionPayState::default();
        // Raise forgone 5%: log loss ln(1.05) = 0.048790.
        let f = m.step(&mut s, true, true, 0.05, (0.0, 0.99, 0.5));
        assert!(f.froze && !f.cut);
        assert!((f.wage - 1.0 / 1.05).abs() < 1e-12);
        // Twelve and more months later, out of the recession, still lower.
        let mut last = f;
        for _ in 0..40 {
            last = m.step(&mut s, true, false, 0.05, (0.99, 0.99, 0.5));
        }
        assert!((last.wage - 1.0 / 1.05).abs() < 1e-12);
        // No second freeze inside the cooldown, even with a freezing draw.
        let mut s2 = RecessionPayState::default();
        m.step(&mut s2, true, true, 0.05, (0.0, 0.99, 0.5));
        let again = m.step(&mut s2, true, true, 0.05, (0.0, 0.99, 0.5));
        assert!(!again.froze);
        assert!((again.wage - 1.0 / 1.05).abs() < 1e-12);
    }

    #[test]
    fn a_cut_lasts_nine_months_and_recovers_in_full() {
        let m = model();
        let mut s = RecessionPayState::default();
        // u_size 0.5 gives a cut of 5% + 0.5 * 15% = 12.5%.
        let f = m.step(&mut s, true, true, 0.0, (0.99, 0.0, 0.5));
        assert!(f.cut);
        assert!((f.wage - 0.875).abs() < 1e-12);
        for month in 1..CUT_MONTHS {
            let f = m.step(&mut s, true, false, 0.0, (0.99, 0.99, 0.5));
            assert!((f.wage - 0.875).abs() < 1e-12, "month {month}");
        }
        let after = m.step(&mut s, true, false, 0.0, (0.99, 0.99, 0.5));
        assert_eq!(after.wage, 1.0);
    }

    #[test]
    fn bonuses_are_smaller_in_a_recession_and_for_a_year_after() {
        let m = model();
        let mut s = RecessionPayState::default();
        m.step(&mut s, true, true, 0.0, (0.99, 0.99, 0.5));
        let mut bonus = Vec::new();
        for _ in 0..16 {
            bonus.push(m.step(&mut s, true, false, 0.0, (0.99, 0.99, 0.5)).bonus);
        }
        // Months 1..=12 after the recession are reduced, then back to 1.
        assert!(bonus[..12].iter().all(|b| *b == BONUS_FACTOR));
        assert!(bonus[12..].iter().all(|b| *b == 1.0));
    }
}
