//! Long-term disability (`household.people[].disability_risk`, `ltd_insurance`).
//!
//! # What is modelled
//!
//! Each month a person who is below full retirement age and has not retired faces an
//! onset hazard that rises steeply with age. Onset ends all of the person's wage and
//! self-employment income (their jobs go into an open-ended idle state, equity
//! unvested at that date is forfeited). What the household receives:
//!
//! * **SSDI** after a five-month waiting period (SSA's rule), equal to the person's
//!   disability PIA at onset, adjusted by prices, until full retirement age. The PIA
//!   is worked out as SSA does for disability: the computation years are the years
//!   from the year after turning 21 to the year before onset (the "elapsed years"),
//!   less one dropout year per five elapsed years (at most five), but never fewer than
//!   two; AIME is the best of those years' indexed earnings divided by their months
//!   (SSA POMS RS 00605.016 and the SSA Handbook section 703). The result goes into
//!   the claimant's record and stays there while disabled, so later years without
//!   earnings do not lower it (the "freeze" of the earnings record). At full retirement
//!   age the claim age is set to the full retirement age, so the existing retirement
//!   machinery pays exactly that PIA (conversion from disability to retirement
//!   benefit). Not insured (under five years of earnings recorded, or Social Security
//!   basis `none`, or already claiming retirement benefits) means no SSDI.
//! * **LTD insurance**, if the person has it: after a three-month elimination
//!   period, `replacement x base salary at onset` a month in nominal dollars (not
//!   indexed, as most policies are), less the SSDI actually received (the usual
//!   offset), until `to_age`.
//! * **Recovery** with a constant yearly probability. The person's jobs then resume
//!   after a pay scar; the Social Security record is released.
//!
//! # Parameters and status
//!
//! The onset hazard is a JUDGEMENT. What was verified: the Trustees' age-sex-adjusted
//! ultimate incidence rate is 4.6 disabled-worker awards per 1,000 exposed a year
//! (2025 Trustees Report, Alternative II; 4.8 in 2023, 4.5 in 2024), and in the 2024
//! SSA Annual Statistical Report on the SSDI Program the age distribution of
//! disabled-worker awards was: under 30 6.9%, 30-34 3.6%, 35-39 4.5%, 40-44 6.0%,
//! 45-49 7.4%, 50-54 16.2%, 55-59 28.0%, 60-61 12.6%, 62 to full retirement age 14.8%
//! (the table was read through search summaries; the SSA site refuses automated
//! access). The age-specific incidence tables (OACT long-range disability
//! assumptions) could not be retrieved, so the shape here is a judgement made to be
//! roughly consistent with those facts. Weighted by the age mix of employed people the
//! table below averages about 5.2 onsets per 1,000 a year, some 13% above the
//! Trustees' rate (which also counts insured people who are not working), and gives
//! award shares of about 17% at 50-54, 22% at 55-59 and 29% from 60: a little too few
//! at 55-59 and too many after 62, where workers who could claim retirement benefits
//! instead are in the data but not separated here. Because an SSDI award
//! needs a severe, long-lasting impairment, the rates are *lower* than the share of
//! people with some work limitation; this model covers the severe end only.
//! Recovery (3% a year), the scar (a 10% pay loss on return), the minimum insured
//! history and the waiting periods other than SSDI's are judgements as well. Excess
//! mortality of disabled people and Medicare entry after 24 months are not modelled.

use crate::benefits::{EarningsRecord, SocialSecurityModel, MAX_EARNING_YEARS};
use crate::model::YearMonth;

/// Lower edges of the age bands of [`INCIDENCE_PER_1000`] (years).
pub const BAND_FROM_AGE: [f64; 8] = [20.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0];
/// Yearly disability onsets per 1,000 working people by age band (JUDGEMENT, shaped by
/// the Trustees' 4.6 per 1,000 average and the age shares of awards; see above).
pub const INCIDENCE_PER_1000: [f64; 8] = [1.0, 1.6, 2.2, 3.6, 5.5, 8.0, 12.0, 14.0];
/// Probability per year of recovering and returning to work (JUDGEMENT).
pub const ANNUAL_RECOVERY: f64 = 0.03;
/// Log pay loss on returning to work after a disability (JUDGEMENT).
pub const RETURN_SCAR: f64 = 0.10;
/// Months of waiting before SSDI is paid (SSA: five full calendar months).
pub const SSDI_WAIT_MONTHS: u32 = 5;
/// Months of elimination period before LTD pays (JUDGEMENT; 90 days is typical).
pub const LTD_WAIT_MONTHS: u32 = 3;
/// Years of recorded earnings needed to count as insured (JUDGEMENT standing in for
/// SSA's 20 quarters of work in the last 40).
pub const MIN_INSURED_YEARS: usize = 5;
/// Youngest age at which onset is modelled.
pub const MIN_ONSET_AGE: f64 = 20.0;
/// Open-ended idle spell (months) given to a disabled person's jobs.
pub const DISABLED_SPELL_MONTHS: f64 = 1.0e9;

/// Compiled per-person model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisabilityModel {
    /// Monthly onset probability by age band.
    pub hazard: [f64; 8],
    /// Monthly recovery probability.
    pub recovery: f64,
    /// Share of base salary replaced by LTD, if the person has a policy.
    pub ltd_replacement: f64,
    /// Age at which LTD stops (0 with no policy).
    pub ltd_to_age: f64,
}

/// Per-path, per-person state.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DisabilityState {
    /// Currently disabled.
    pub disabled: bool,
    /// Months since onset (0 in the onset month).
    pub months: u32,
    /// SSDI per month in today's dollars (0: not insured).
    pub ssdi_real: f64,
    /// LTD per month, nominal dollars, fixed at onset (0: none).
    pub ltd_nominal: f64,
    /// The claim age to put back on recovery.
    pub saved_claim_age: f64,
    /// True while the claimant record is frozen at the disability PIA.
    pub frozen: bool,
    /// True once an SSDI payment has been made (for the trace event).
    pub ssdi_started: bool,
    /// Onsets so far on this path.
    pub onsets: u32,
}

/// What happened this month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Nothing.
    None,
    /// Disability began.
    Onset,
    /// The person recovered.
    Recovery,
}

impl DisabilityModel {
    /// Compiles the hazards. `ltd` is `(replacement, to_age)`.
    pub fn new(ltd: Option<(f64, f64)>) -> DisabilityModel {
        let mut hazard = [0.0; 8];
        for (h, per_1000) in hazard.iter_mut().zip(INCIDENCE_PER_1000) {
            *h = monthly(per_1000 / 1000.0);
        }
        let (ltd_replacement, ltd_to_age) = ltd.unwrap_or((0.0, 0.0));
        DisabilityModel {
            hazard,
            recovery: monthly(ANNUAL_RECOVERY),
            ltd_replacement,
            ltd_to_age,
        }
    }

    /// Monthly onset probability at an age (0 below [`MIN_ONSET_AGE`]).
    pub fn onset_hazard(&self, age: f64) -> f64 {
        if age < MIN_ONSET_AGE {
            return 0.0;
        }
        let mut band = 0;
        for (i, from) in BAND_FROM_AGE.iter().enumerate() {
            if age >= *from {
                band = i;
            }
        }
        self.hazard[band]
    }

    /// One month. `at_risk` says the person could become disabled (alive, present,
    /// not retired, below full retirement age); `can_recover` that they could return
    /// to work (the same conditions: past full retirement age the benefit has
    /// converted to a retirement benefit for good). Both uniforms are always drawn by the caller.
    pub fn step(
        &self,
        state: &mut DisabilityState,
        age: f64,
        at_risk: bool,
        can_recover: bool,
        (u_onset, u_recover): (f64, f64),
    ) -> Change {
        if state.disabled {
            state.months = state.months.saturating_add(1);
            if can_recover && u_recover < self.recovery {
                state.disabled = false;
                return Change::Recovery;
            }
            return Change::None;
        }
        if at_risk && u_onset < self.onset_hazard(age) {
            state.disabled = true;
            state.months = 0;
            state.onsets += 1;
            return Change::Onset;
        }
        Change::None
    }

    /// LTD paid this month, nominal, given the SSDI received this month.
    pub fn ltd_benefit(&self, state: &DisabilityState, age: f64, ssdi_nominal: f64) -> f64 {
        if !state.disabled
            || state.ltd_nominal <= 0.0
            || state.months < LTD_WAIT_MONTHS
            || age >= self.ltd_to_age
        {
            return 0.0;
        }
        (state.ltd_nominal - ssdi_nominal).max(0.0)
    }
}

/// Monthly probability with the given yearly probability.
fn monthly(annual: f64) -> f64 {
    1.0 - crate::math::pow(1.0 - annual, 1.0 / 12.0)
}

/// Computation years SSA uses for a disability benefit: elapsed years (from the year
/// after the 21st birthday through the year before onset) less one dropout year per
/// five elapsed years (at most five), never fewer than two.
pub fn computation_years(birth_year: i32, onset_year: i32) -> usize {
    let elapsed = (onset_year - 1 - (birth_year + 21)).max(0) as usize;
    let dropout = (elapsed / 5).min(5);
    (elapsed - dropout).clamp(2, MAX_EARNING_YEARS)
}

/// Disability PIA per month in today's dollars for onset at `onset` (see the module
/// documentation). `price` is the price level at onset (1.0 at the plan's start).
/// Runs once per onset: it copies and partially sorts the record.
pub fn ssdi_pia_real(
    ss: &SocialSecurityModel,
    record: &EarningsRecord,
    birth: YearMonth,
    onset: YearMonth,
    price: f64,
) -> f64 {
    let n = computation_years(birth.year(), onset.year());
    let have = record.years.min(MAX_EARNING_YEARS);
    let mut top = record.indexed;
    let used = &mut top[..have];
    let take = n.min(have);
    if take > 0 && have > take {
        used.select_nth_unstable_by(take - 1, |a, b| b.total_cmp(a));
    }
    let sum: f64 = used[..take].iter().sum();
    let aime_indexed = sum / (n as f64 * 12.0);
    // Indexing year: two years before onset, but not after the year of turning 60.
    let index_year = (onset.year() - 2).min(birth.year() + 60);
    let aime = (aime_indexed * ss.wage_index(index_year, price) + 1e-6).floor();
    let bends = ss.bend_points(onset.year(), ss.average_wage(onset.year() - 2, price));
    ss.pia_from_bend_points(aime, bends) / price
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;

    fn ss() -> SocialSecurityModel {
        SocialSecurityModel::new(Data::embedded(), None, YearMonth::ym(2026, 1))
    }

    #[test]
    fn computation_years_follow_the_ssa_rule() {
        // Born 1990, onset 2030: elapsed 2012..=2029 = 18, dropout 3, so 15 years.
        assert_eq!(computation_years(1990, 2030), 15);
        // Elapsed 25 or more: five dropout years. Born 1970, onset 2040: elapsed
        // 1992..=2039 = 48 years, less 5.
        assert_eq!(computation_years(1970, 2040), 43);
        // Very young onset: at least two years.
        assert_eq!(computation_years(2000, 2023), 2);
        // Elapsed 4: no dropout.
        assert_eq!(computation_years(2000, 2026), 4);
    }

    #[test]
    fn hazard_is_the_yearly_incidence_by_band() {
        let m = DisabilityModel::new(None);
        let annual = |age: f64| 1.0 - crate::math::powi(1.0 - m.onset_hazard(age), 12);
        assert!((annual(57.0) - 0.012).abs() < 1e-12);
        assert!((annual(32.0) - 0.0016).abs() < 1e-12);
        assert_eq!(m.onset_hazard(19.9), 0.0);
        assert!(m.onset_hazard(60.0) > m.onset_hazard(50.0));
        assert!((1.0 - crate::math::powi(1.0 - m.recovery, 12) - 0.03).abs() < 1e-12);
    }

    #[test]
    fn onset_recovery_and_waiting_periods() {
        let m = DisabilityModel::new(Some((0.6, 65.0)));
        let mut s = DisabilityState::default();
        // A draw above the hazard does nothing.
        assert_eq!(m.step(&mut s, 50.0, true, true, (0.5, 0.0)), Change::None);
        // Not at risk: no onset even with a zero draw.
        assert_eq!(m.step(&mut s, 50.0, false, true, (0.0, 0.0)), Change::None);
        assert_eq!(m.step(&mut s, 50.0, true, true, (0.0, 0.99)), Change::Onset);
        assert!(s.disabled && s.months == 0 && s.onsets == 1);
        // While disabled, the onset draw is ignored and the clock runs.
        assert_eq!(m.step(&mut s, 50.0, true, true, (0.0, 0.99)), Change::None);
        assert_eq!(s.months, 1);
        // Retired people cannot recover into work.
        assert_eq!(m.step(&mut s, 50.0, true, false, (0.0, 0.0)), Change::None);
        assert_eq!(
            m.step(&mut s, 50.0, true, true, (0.0, 0.0)),
            Change::Recovery
        );
        assert!(!s.disabled);
    }

    #[test]
    fn ltd_pays_after_the_wait_less_ssdi_until_to_age() {
        let m = DisabilityModel::new(Some((0.6, 65.0)));
        let mut s = DisabilityState {
            disabled: true,
            ltd_nominal: 3000.0,
            ..Default::default()
        };
        s.months = 2;
        assert_eq!(m.ltd_benefit(&s, 50.0, 0.0), 0.0);
        s.months = 3;
        assert_eq!(m.ltd_benefit(&s, 50.0, 0.0), 3000.0);
        assert_eq!(m.ltd_benefit(&s, 50.0, 1800.0), 1200.0);
        assert_eq!(m.ltd_benefit(&s, 50.0, 3500.0), 0.0);
        assert_eq!(m.ltd_benefit(&s, 65.0, 0.0), 0.0);
        s.disabled = false;
        assert_eq!(m.ltd_benefit(&s, 50.0, 0.0), 0.0);
    }

    #[test]
    fn disability_pia_uses_only_the_computation_years() {
        let ss = ss();
        let birth = YearMonth::ym(1990, 1);
        let onset = YearMonth::ym(2030, 1);
        let mut rec = EarningsRecord::default();
        for _ in 0..10 {
            rec.indexed[rec.years] = 60_000.0;
            rec.years += 1;
        }
        // 15 computation years: AIME = 10 * 60,000 / (15 * 12) in index-year dollars.
        let price = 1.3;
        let aime = (10.0 * 60_000.0 / 180.0 * ss.wage_index(2028, price) + 1e-6).floor();
        let bends = ss.bend_points(2030, ss.average_wage(2028, price));
        let want = ss.pia_from_bend_points(aime, bends) / price;
        let got = ssdi_pia_real(&ss, &rec, birth, onset, price);
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        assert!(got > 500.0 && got < 5000.0);
        // Dividing by 35 years (the retirement rule) would give a lower PIA.
        let mut longer = rec;
        longer.years = 10;
        let retirement_aime = longer.aime();
        assert!(retirement_aime * ss.wage_index(2028, price) < aime);
    }

    #[test]
    fn disability_pia_does_not_fall_when_the_record_is_longer_than_needed() {
        let ss = ss();
        let birth = YearMonth::ym(1990, 1);
        let onset = YearMonth::ym(2030, 1);
        let mut rec = EarningsRecord::default();
        // 18 years on record but only the best 15 count.
        for i in 0..18 {
            rec.indexed[rec.years] = if i < 15 { 50_000.0 } else { 1.0 };
            rec.years += 1;
        }
        let long = ssdi_pia_real(&ss, &rec, birth, onset, 1.0);
        let mut best = EarningsRecord::default();
        for _ in 0..15 {
            best.indexed[best.years] = 50_000.0;
            best.years += 1;
        }
        assert_eq!(long, ssdi_pia_real(&ss, &best, birth, onset, 1.0));
    }
}
