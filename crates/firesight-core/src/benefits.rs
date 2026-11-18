//! Social Security. **Interface stub: bodies are `todo!()`.**
//!
//! Responsibility: turn an earnings history into a retirement benefit the way SSA
//! does (wage-indexed earnings, best 35 years, AIME, bend-point PIA formula, early or
//! delayed claiming adjustment), plus spousal and survivor benefits, the earnings test
//! before full retirement age, annual COLA, and the optional trust-fund haircut.
//!
//! Contract:
//!
//! * [`EarningsRecord`] is fixed-size and lives in the path state: one entry per year
//!   of covered earnings, updated each December from simulated wages, so a layoff or
//!   early retirement lowers the benefit on that path.
//! * Past earnings (before `profile.start`) are reconstructed once at compile time
//!   from today's wage and the lifecycle curve ([`EarningsRecord::backfill`]).
//! * Bend points and the wage base are wage-indexed; the engine approximates the
//!   national average wage index by CPI plus the real wage growth assumption in
//!   `data.actuarial`.
//! * Benefits returned are nominal dollars per month for the year asked.
//! * Unit tests reproduce PIA examples published by SSA.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::data::Data;
use crate::model::{SocialSecurityHaircut, YearMonth};
use crate::MAX_PEOPLE;

/// Maximum years of covered earnings tracked per person (ages 16 to 75).
pub const MAX_EARNING_YEARS: usize = 60;

/// One person's covered earnings by year, already capped at each year's wage base and
/// expressed in wage-indexed dollars of the indexing year.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EarningsRecord {
    /// Indexed earnings per year of work; only the first `years` entries are used.
    pub indexed: [f64; MAX_EARNING_YEARS],
    /// Number of years recorded.
    pub years: usize,
}

impl Default for EarningsRecord {
    fn default() -> Self {
        Self {
            indexed: [0.0; MAX_EARNING_YEARS],
            years: 0,
        }
    }
}

impl EarningsRecord {
    /// Reconstructs earnings from `career_start_age` up to `current_age` given today's
    /// annual earnings, walking the lifecycle wage curve backwards.
    pub fn backfill(
        current_earnings: f64,
        current_age: f64,
        career_start_age: f64,
        model: &SocialSecurityModel,
    ) -> EarningsRecord {
        todo!("benefits owner")
    }

    /// Adds one simulated year of covered earnings. `wage_index` is the ratio of that
    /// year's average wage to the base year's; earnings above `wage_base` are ignored.
    pub fn record_year(&mut self, nominal_earnings: f64, wage_index: f64, wage_base: f64) {
        todo!("benefits owner")
    }

    /// Average indexed monthly earnings: the best 35 years, divided by 420.
    pub fn aime(&self) -> f64 {
        todo!("benefits owner")
    }
}

/// How a person's own benefit is determined, compiled from `SocialSecurityBasis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BenefitBasis {
    /// Computed from the path's [`EarningsRecord`].
    FromEarnings,
    /// Fixed primary insurance amount in today's dollars per month.
    KnownPia(f64),
    /// Not eligible.
    None,
}

/// Compiled Social Security rules. Built once in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct SocialSecurityModel {
    _private: (),
}

/// A household's benefits for one month, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MonthlyBenefits {
    /// Benefit paid to each person: own retirement benefit, topped up to the spousal
    /// or survivor benefit where that is higher, after the earnings test and haircut.
    pub paid: [f64; MAX_PEOPLE],
    /// Amount withheld from each person by the earnings test this month.
    pub withheld: [f64; MAX_PEOPLE],
}

/// The state of one person that benefits depend on, supplied by `sim` each month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claimant {
    /// Birth month.
    pub birth: YearMonth,
    /// Planned claim age in years (62 to 70).
    pub claim_age: f64,
    /// Primary insurance amount in today's dollars per month, or `None` if not yet
    /// computed (before age 62) or not eligible.
    pub pia_real: Option<f64>,
    /// False once the person has died.
    pub alive: bool,
    /// Death month, for survivor benefits.
    pub died: Option<YearMonth>,
    /// Earned income this calendar year so far, nominal (earnings test).
    pub earnings_this_year: f64,
}

impl SocialSecurityModel {
    /// Loads bend points, the full-retirement-age table, claiming adjustment factors,
    /// the earnings-test thresholds and the wage-growth assumption.
    pub fn new(data: &Data, haircut: Option<SocialSecurityHaircut>) -> SocialSecurityModel {
        todo!("benefits owner")
    }

    /// Full retirement age in months for a birth year (67 years for 1960 and later).
    pub fn full_retirement_age_months(&self, birth_year: i32) -> u32 {
        todo!("benefits owner")
    }

    /// Primary insurance amount per month for an AIME, in dollars of the year the
    /// person turns 62 (`wage_index_at_62` scales the base-year bend points).
    pub fn pia(&self, aime: f64, wage_index_at_62: f64) -> f64 {
        todo!("benefits owner")
    }

    /// Multiplier on PIA for claiming at `claim_age_months` given the full retirement
    /// age: below 1 for early claiming (down to 0.70 at 62), up to 1.24 at 70.
    pub fn claim_factor(&self, claim_age_months: u32, fra_months: u32) -> f64 {
        todo!("benefits owner")
    }

    /// Fraction of scheduled benefits payable in a calendar year (1.0, or the haircut
    /// factor from its start year).
    pub fn payable_fraction(&self, year: i32) -> f64 {
        todo!("benefits owner")
    }

    /// Benefits paid in month `now` to up to two people, including spousal top-up,
    /// survivor step-up and the earnings test. `price_level` converts today's-dollar
    /// PIAs to nominal (benefits get full CPI COLA).
    pub fn monthly(
        &self,
        now: YearMonth,
        people: &[Option<Claimant>; MAX_PEOPLE],
        price_level: f64,
    ) -> MonthlyBenefits {
        todo!("benefits owner")
    }

    /// Present value in today's dollars of a person's expected lifetime benefits if
    /// claimed as planned, used by the `when_funded` trigger when
    /// `count_future_income` is set. `discount_rate` is real and annual.
    pub fn present_value(
        &self,
        claimant: &Claimant,
        now: YearMonth,
        discount_rate: f64,
        survival: &[f64],
    ) -> f64 {
        todo!("benefits owner")
    }
}
