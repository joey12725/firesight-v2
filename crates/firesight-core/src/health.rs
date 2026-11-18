//! Health costs and mortality. **Interface stub: bodies are `todo!()`.**
//!
//! Responsibility: when each person dies on a path; health insurance premiums and
//! out-of-pocket costs by phase of life (employer plan while working, COBRA or ACA
//! between retirement and 65, Medicare with supplemental coverage after); random
//! medical cost shocks; long-term care episodes late in life.
//!
//! Contract:
//!
//! * Lifetimes are drawn once per path from `Stream::Longevity(person)`, before the
//!   monthly loop, by inverting the cohort survival curve (period life table from
//!   `data.life_table`, improved forward with `data.mortality_improvement` when
//!   `improvement` is on, scaled by `HealthStatus`). The draw uses exactly one uniform
//!   per person so longevity is paired across scenarios.
//! * Monthly cost draws come only from `Stream::Health(person)`, a fixed number per
//!   month.
//! * Costs are returned in today's dollars; `sim` inflates them with CPI plus the
//!   healthcare category's real growth.
//! * `spending.healthcare.mode == in_budget` disables premiums and routine
//!   out-of-pocket costs but not long-term care (when enabled).
//! * ACA premiums are returned gross with the benchmark premium alongside; the subsidy
//!   is the tax module's job because it depends on MAGI.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::data::Data;
use crate::model::{HealthStatus, Healthcare, Longevity, Sex, YearMonth};
use crate::rng::Rng;

/// Compiled mortality model: survival curves per person. Built in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct MortalityModel {
    _private: (),
}

impl MortalityModel {
    /// Builds cohort survival curves for a person from the life table.
    pub fn new(
        birth: YearMonth,
        sex: Sex,
        health: HealthStatus,
        start: YearMonth,
        longevity: &Longevity,
        data: &Data,
    ) -> MortalityModel {
        todo!("health owner")
    }

    /// Number of months from `start` until death, drawn with one uniform. With
    /// `Longevity::FixedAge` the draw is still consumed and ignored.
    pub fn draw_months_to_death(&self, rng: &mut Rng) -> u32 {
        todo!("health owner")
    }

    /// Probability of being alive `months` after `start`, for present values and for
    /// the survival curves in `Summary`.
    pub fn survival(&self, months: u32) -> f64 {
        todo!("health owner")
    }

    /// Remaining life expectancy in years at `start`.
    pub fn life_expectancy(&self) -> f64 {
        todo!("health owner")
    }
}

/// Where a person's health insurance comes from this month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CoveragePhase {
    /// An employer plan (their own or their partner's).
    #[default]
    Employer,
    /// COBRA continuation after leaving a job.
    Cobra,
    /// An ACA marketplace plan.
    Aca,
    /// Medicare with Part B, Part D and supplemental coverage.
    Medicare,
    /// A fixed user-specified cost.
    Custom,
    /// Outside the modelled system (healthcare is in the budget).
    None,
}

/// One person's health costs for one month, in today's dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HealthMonth {
    /// Phase the costs belong to.
    pub phase: CoveragePhase,
    /// Premium before any subsidy.
    pub premium: f64,
    /// ACA benchmark (second-lowest silver) premium for this person, 0 off-marketplace.
    pub aca_benchmark: f64,
    /// Out-of-pocket medical spending.
    pub out_of_pocket: f64,
    /// Long-term care cost.
    pub long_term_care: f64,
    /// True in the month a long-term care episode starts (for the event log).
    pub ltc_started: bool,
}

/// Per-path health state of one person.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HealthState {
    /// Months of long-term care still needed (0 when not in care).
    pub ltc_months_left: u32,
    /// Level of care cost in today's dollars per month for the current episode.
    pub ltc_monthly_cost: f64,
    /// Persistent component of medical spending (people with a bad year tend to have
    /// another).
    pub cost_factor: f64,
}

/// Facts about a person this month that decide their coverage.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HealthContext {
    /// Age in years.
    pub age: f64,
    /// True if this person or their partner has an employer plan this month.
    pub employer_coverage: bool,
    /// Months since employer coverage ended (for COBRA's 18-month limit).
    pub months_since_coverage_ended: u32,
    /// True if the household lives in the US.
    pub in_us: bool,
}

/// Compiled health cost model for the household.
#[derive(Clone, Debug)]
pub struct HealthModel {
    _private: (),
}

impl HealthModel {
    /// Compiles premiums and cost distributions for the household's settings.
    /// `risk_enabled` is `assumptions.health_risk`.
    pub fn new(
        settings: &Healthcare,
        state: Option<&str>,
        risk_enabled: bool,
        data: &Data,
    ) -> HealthModel {
        todo!("health owner")
    }

    /// The coverage phase implied by a person's situation.
    pub fn phase(&self, ctx: &HealthContext) -> CoveragePhase {
        todo!("health owner")
    }

    /// One person's costs for the month, advancing their state.
    pub fn step(&self, state: &mut HealthState, ctx: &HealthContext, rng: &mut Rng) -> HealthMonth {
        todo!("health owner")
    }

    /// Expected annual cost in today's dollars at an age in a phase with no shocks,
    /// used to project retirement spending for the FI test.
    pub fn expected_annual_cost(&self, age: f64, phase: CoveragePhase) -> f64 {
        todo!("health owner")
    }
}
