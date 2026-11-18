//! Careers and earned income. **Interface stub: bodies are `todo!()`.**
//!
//! Responsibility: what each job pays each month on one path: the lifecycle wage
//! curve, persistent and transitory wage shocks, layoffs and re-employment (hazard
//! higher in recessions, re-employment at a wage scar), bonus, commission, equity
//! vesting and private-company liquidity events, career breaks, and self-employment
//! income with its own volatility.
//!
//! Contract:
//!
//! * One [`JobModel`] per job income is compiled once; one [`JobState`] per job lives
//!   in the path state. Stepping never allocates.
//! * Each person's jobs draw only from `Stream::Career(person)`; equity price paths
//!   and outcomes draw only from `Stream::EquityComp`. The number of draws per month
//!   is fixed regardless of state (draw, then decide), so that a scenario change such
//!   as retiring earlier does not shift later draws.
//! * `assumptions.career_risk == false` or `risk.level == none` makes every shock
//!   zero while still consuming the same draws.
//! * All outputs are nominal dollars for the month. Wage curves are real; the caller
//!   supplies the price level.
//! * A layoff ends employer benefits (match, HSA, unvested equity is forfeited);
//!   re-employment restores the plan with the same terms.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::data::Data;
use crate::model::{Job, LifecycleProfile, RiskLevel, SelfEmployment, YearMonth};
use crate::rng::Rng;

/// Maximum equity grants tracked per job, including refresh grants in flight.
pub const MAX_GRANTS: usize = 8;

/// Compiled parameters of one job. Built in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct JobModel {
    _private: (),
}

/// Compiled parameters of one self-employment income.
#[derive(Clone, Debug)]
pub struct SelfEmploymentModel {
    _private: (),
}

/// Employment status of a job on a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Employment {
    /// Working and being paid.
    #[default]
    Employed,
    /// Laid off and searching; `months` so far.
    Unemployed { months: u32 },
    /// On a planned career break with `months_left` to go.
    OnBreak { months_left: u32 },
    /// The job has ended for good (retirement, `end` date or an `income_change` event).
    Ended,
}

/// Per-path state of one job.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct JobState {
    /// Current status.
    pub employment: Employment,
    /// Current real annual base salary (today's dollars), including the persistent
    /// shock and any scar from past layoffs.
    pub real_salary: f64,
    /// Log of the persistent wage shock component.
    pub persistent_shock: f64,
    /// Fraction of pay received while on a break.
    pub break_income_fraction: f64,
    /// Unvested value per grant in today's dollars at the grant-date price.
    pub unvested: [f64; MAX_GRANTS],
    /// Months of vesting left per grant.
    pub vest_months_left: [u32; MAX_GRANTS],
    /// Stock price relative to the price at `profile.start` (1.0 at start).
    pub stock_price: f64,
    /// Vested private-company shares waiting for a liquidity event, at grant value.
    pub illiquid_vested: f64,
    /// Months spent unemployed so far on this path (reported as a path scalar).
    pub months_unemployed_total: u32,
}

/// What one job paid in one month, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct JobMonth {
    /// Base pay.
    pub wages: f64,
    /// Bonus paid this month (0 outside the bonus month).
    pub bonus: f64,
    /// Commission paid this month.
    pub commission: f64,
    /// Value of RSUs vesting or options exercised this month: taxed as wages.
    pub equity_income: f64,
    /// Portion of `equity_income` kept as single-stock holdings rather than sold.
    pub equity_retained: f64,
    /// Unemployment insurance received.
    pub unemployment_benefit: f64,
    /// True in the month a layoff happens (for the event log).
    pub laid_off: bool,
    /// True in the month a new job starts after unemployment.
    pub rehired: bool,
}

impl JobMonth {
    /// Total taxable compensation for the month.
    pub fn gross(&self) -> f64 {
        self.wages + self.bonus + self.commission + self.equity_income
    }
}

/// The economy as careers see it this month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CareerClimate {
    /// Calendar month.
    pub now: YearMonth,
    /// The owner's age in years.
    pub age: f64,
    /// Price level relative to `profile.start`.
    pub price_level: f64,
    /// 1 in a recession month.
    pub recession: u8,
    /// US stock return this month (drives employer stock through its beta).
    pub equity_return: f64,
}

impl JobModel {
    /// Compiles a job. `risk_enabled` is `assumptions.career_risk`.
    pub fn new(job: &Job, owner_age_at_start: f64, risk_enabled: bool, data: &Data) -> JobModel {
        todo!("career owner")
    }

    /// State at `profile.start`.
    pub fn initial_state(&self) -> JobState {
        todo!("career owner")
    }

    /// Advances one month and returns what was paid. Called for every job every
    /// month, whatever its status, so the draw count per stream is constant.
    pub fn step(
        &self,
        state: &mut JobState,
        climate: &CareerClimate,
        career: &mut Rng,
        equity: &mut Rng,
    ) -> JobMonth {
        todo!("career owner")
    }

    /// Ends the job (retirement or event). Unvested equity is forfeited.
    pub fn end(&self, state: &mut JobState) {
        todo!("career owner")
    }

    /// Starts a career break of `months` at `income_fraction` of pay.
    pub fn start_break(&self, state: &mut JobState, months: u32, income_fraction: f64) {
        todo!("career owner")
    }

    /// Applies an `income_change` event: sets or scales the real salary.
    pub fn set_real_salary(&self, state: &mut JobState, real_salary: f64) {
        todo!("career owner")
    }

    /// Expected real annual base salary at an age with no shocks: the deterministic
    /// curve used for Social Security backfill and the FI projection.
    pub fn expected_real_salary(&self, age: f64) -> f64 {
        todo!("career owner")
    }
}

impl SelfEmploymentModel {
    /// Compiles a self-employment income.
    pub fn new(se: &SelfEmployment, risk_enabled: bool) -> SelfEmploymentModel {
        todo!("career owner")
    }

    /// Net profit for the month, nominal. `years_since_start` drives real growth; the
    /// annual volatility is applied as a persistent yearly factor redrawn each January.
    pub fn step(&self, years_since_start: f64, climate: &CareerClimate, career: &mut Rng) -> f64 {
        todo!("career owner")
    }
}

/// Real wage relative to age 40 on a lifecycle curve (1.0 at 40).
pub fn lifecycle_factor(profile: LifecycleProfile, age: f64, data: &Data) -> f64 {
    todo!("career owner")
}

/// Monthly layoff probability for a risk level, in and out of recession.
pub fn layoff_hazard(level: RiskLevel, recession: bool, data: &Data) -> f64 {
    todo!("career owner")
}
