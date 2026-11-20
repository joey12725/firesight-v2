//! Older workers and involuntary retirement (`risk.late_career`).
//!
//! # What is modelled
//!
//! The career model already makes layoffs of older workers worse: unemployment spells
//! have a median 1.4 times longer from 55 and the re-employment pay loss is 1.5 times
//! larger from 50 (`params.json`, `unemployment`). This feature adds, for layoffs from
//! 55 on, three things on top of those:
//!
//! * the spell is longer again, by a factor that rises from [`SPELL_MULTIPLIER`] (55 to
//!   59) to 60 and over;
//! * the pay loss on returning is larger again by [`SCAR_MULTIPLIER`];
//! * with a probability that rises with age ([`NEVER_AT_55`] plus [`NEVER_PER_YEAR`] for
//!   each year older, at most [`NEVER_MAX`]) the laid-off worker never works again:
//!   involuntary retirement. The person is retired on the spot (their jobs end, they
//!   stop contributing), and if nobody else in the household is still working the
//!   household retires the next month on what it has.
//!
//! The combined effect at 60, in a normal labour market, is a median spell of
//! `2.2 x 1.4 x 1.6 = 4.9` months and a mean pay loss of `0.08 x 1.5 x 1.5 = 18%`, and
//! about 35% of layoffs end in permanent withdrawal.
//!
//! # Parameters and status
//!
//! The direction and rough size are documented; the numbers are JUDGEMENTS. Johnson
//! and Gosselin (Urban Institute 2018, reported by ProPublica, "Forced out") found in
//! the Health and Retirement Study that 56% of workers over 50 in long-term full-time
//! jobs were pushed out before they chose to retire, and ProPublica's analysis found
//! that only about one in ten of them ever again earned as much as before. Those
//! counts include exits for poor health and use a six-month or 50%-earnings-drop
//! threshold, so they do not translate into a per-layoff probability; the rising
//! never-again probability here is shaped to be consistent with them, not estimated
//! from them. The existing layoff hazard (`risk.level`) decides how often layoffs
//! happen; this feature changes only what follows one, so the share of workers pushed
//! out stays tied to the level the user chose.

/// Youngest age at which the late-career effects apply.
pub const FROM_AGE: f64 = 55.0;
/// Extra factors on the unemployment spell from 55-59, 60-64 and 65 and over
/// (JUDGEMENT), on top of the career model's own 1.4 from age 55.
pub const SPELL_MULTIPLIER: [f64; 3] = [1.3, 1.6, 1.8];
/// Extra factor on the mean re-employment pay loss from 55 (JUDGEMENT), on top of the
/// career model's own 1.5 from age 50.
pub const SCAR_MULTIPLIER: f64 = 1.3;
/// Probability that a layoff at 55 is followed by permanent withdrawal (JUDGEMENT).
pub const NEVER_AT_55: f64 = 0.15;
/// Added to that probability for each year of age above 55 (JUDGEMENT).
pub const NEVER_PER_YEAR: f64 = 0.04;
/// Largest probability of permanent withdrawal (JUDGEMENT).
pub const NEVER_MAX: f64 = 0.6;
/// Longest spell the feature allows, months (the career model caps at 36).
pub const SPELL_CAP_MONTHS: f64 = 48.0;
/// Spell length given to a worker who never returns (months).
pub const FOREVER_MONTHS: f64 = 1.0e9;

/// What a layoff turns into for an older worker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LateOutcome {
    /// New spell length in months (a very large number: never returns).
    pub spell_months: f64,
    /// New log pay loss on return.
    pub scar: f64,
    /// True if the worker never works again.
    pub never: bool,
}

/// The late-career rules (stateless).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LateCareerModel;

impl LateCareerModel {
    /// Probability that a layoff at `age` ends in permanent withdrawal.
    pub fn never_probability(&self, age: f64) -> f64 {
        if age < FROM_AGE {
            return 0.0;
        }
        (NEVER_AT_55 + NEVER_PER_YEAR * (age - FROM_AGE)).min(NEVER_MAX)
    }

    /// Spell multiplier at `age`.
    pub fn spell_multiplier(&self, age: f64) -> f64 {
        if age < FROM_AGE {
            1.0
        } else if age < 60.0 {
            SPELL_MULTIPLIER[0]
        } else if age < 65.0 {
            SPELL_MULTIPLIER[1]
        } else {
            SPELL_MULTIPLIER[2]
        }
    }

    /// Adjusts a layoff that has just happened at `age`, given the career model's
    /// spell and pay loss and one uniform. `None` below 55.
    pub fn adjust(&self, age: f64, spell: f64, scar: f64, u_never: f64) -> Option<LateOutcome> {
        if age < FROM_AGE {
            return None;
        }
        let never = u_never < self.never_probability(age);
        Some(LateOutcome {
            spell_months: if never {
                FOREVER_MONTHS
            } else {
                (spell * self.spell_multiplier(age)).min(SPELL_CAP_MONTHS)
            },
            // The career model's scar carries a random part that can be negative (a
            // better job); only a loss is deepened, a gain is left as drawn.
            scar: if scar > 0.0 {
                scar * SCAR_MULTIPLIER
            } else {
                scar
            },
            never,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_probability_rises_with_age_and_is_capped() {
        let m = LateCareerModel;
        assert_eq!(m.never_probability(54.9), 0.0);
        assert!((m.never_probability(55.0) - 0.15).abs() < 1e-12);
        assert!((m.never_probability(60.0) - 0.35).abs() < 1e-12);
        assert!((m.never_probability(64.0) - 0.51).abs() < 1e-12);
        assert_eq!(m.never_probability(70.0), 0.6);
    }

    #[test]
    fn adjust_changes_nothing_below_55() {
        assert_eq!(LateCareerModel.adjust(54.0, 3.0, 0.1, 0.0), None);
    }

    #[test]
    fn adjust_lengthens_the_spell_and_deepens_the_scar() {
        let m = LateCareerModel;
        // Age 57, u above the never probability (0.23): a 5-month spell becomes 6.5
        // and a 0.12 scar 0.156.
        let o = m.adjust(57.0, 5.0, 0.12, 0.9).unwrap();
        assert!(!o.never);
        assert!((o.spell_months - 6.5).abs() < 1e-12);
        assert!((o.scar - 0.156).abs() < 1e-12);
        // Age 62: spell x 1.6.
        let o = m.adjust(62.0, 10.0, 0.0, 0.99).unwrap();
        assert!((o.spell_months - 16.0).abs() < 1e-12);
        // A negative scar (a pay gain on return) is not made bigger.
        let o = m.adjust(62.0, 10.0, -0.05, 0.99).unwrap();
        assert_eq!(o.scar, -0.05);
        // The cap.
        let o = m.adjust(66.0, 30.0, 0.0, 0.99).unwrap();
        assert_eq!(o.spell_months, SPELL_CAP_MONTHS);
    }

    #[test]
    fn a_low_draw_means_never_working_again() {
        let m = LateCareerModel;
        let o = m.adjust(60.0, 5.0, 0.1, 0.34).unwrap();
        assert!(o.never && o.spell_months == FOREVER_MONTHS);
        let o = m.adjust(60.0, 5.0, 0.1, 0.36).unwrap();
        assert!(!o.never);
    }
}
