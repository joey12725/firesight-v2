//! The simulation loop. **Interface stub**: [`Plan::compile`] validates and computes
//! the horizon; the path functions are `todo!()` for the sim owner.
//!
//! Responsibility: compile a profile once into a [`Plan`] (ids resolved to indices,
//! frequencies converted to monthly, tax tables and wage curves prepared), then
//! simulate lives month by month as described in `docs/architecture.md` section 3.
//!
//! Contract:
//!
//! * `run_path(plan, seed, i, ..)` is a pure function of its arguments: the same
//!   inputs give the same recorder calls, bit for bit, on native and wasm. All
//!   randomness comes from `Rng::stream(seed, i, stream)`.
//! * No allocation inside the monthly loop. Everything a path needs lives in the
//!   [`Workspace`], created once per worker and reused.
//! * `run_range` over `a..b` then `b..c`, merged, equals `run_range` over `a..c`.
//! * The loop never stores per-path time series; `trace_path` re-simulates one path
//!   with a detail recorder instead.
//! * Money handed to the recorder is in today's dollars.
//! * A `Plan` owns copies of everything it needs from `Data`, so it has no lifetime
//!   and can be moved into a worker.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use std::ops::Range;

use crate::data::Data;
use crate::economy::EconomyPath;
use crate::model::{Profile, YearMonth};
use crate::stats::{Accumulator, PathTrace, Recorder};
use crate::validate::{validate, Report};

/// A profile compiled for simulation. Immutable; shared by reference across paths.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The validated profile with `start` resolved.
    profile: Profile,
    start: YearMonth,
    horizon_months: u32,
    // The sim owner adds the compiled tables here: EconomyModel, TaxLaw,
    // SocialSecurityModel, JobModels, PropertyModels, MortalityModels, HealthModel,
    // the monthly budget table, resolved events, the opening Accounts.
}

impl Plan {
    /// Validates and compiles a profile.
    ///
    /// Fails with the validation report if there are errors, or with a single
    /// `start_unresolved` error if `profile.start` is empty: core has no clock, so
    /// the caller must fill it first (`Profile::with_start`).
    pub fn compile(profile: &Profile, data: &Data) -> Result<Plan, Report> {
        let Some(start) = profile.start else {
            return Err(Report::single_error(
                "start_unresolved",
                "/start",
                "The profile has no start month. Set \"start\" to the current month (YYYY-MM) before simulating.",
            ));
        };
        let report = validate(profile);
        if !report.is_ok() {
            return Err(report);
        }
        // The horizon runs until the youngest adult would reach end_age; individual
        // paths end earlier when everyone has died.
        let youngest = profile
            .household
            .people
            .iter()
            .map(|p| p.birth)
            .max()
            .expect("validated: at least one person");
        let end = youngest + (profile.simulation.end_age * 12) as i32;
        let horizon_months = (end - start).max(1) as u32;
        Ok(Plan {
            profile: profile.clone(),
            start,
            horizon_months,
        })
    }

    /// The compiled profile.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// First simulated month.
    pub fn start(&self) -> YearMonth {
        self.start
    }

    /// Maximum number of months any path can run.
    pub fn horizon_months(&self) -> u32 {
        self.horizon_months
    }

    /// Number of calendar years touched by the horizon; the length of every per-year
    /// array in the results.
    pub fn horizon_years(&self) -> u32 {
        let last = self.start + (self.horizon_months as i32 - 1);
        (last.year() - self.start.year() + 1) as u32
    }

    /// The seed from `profile.simulation.seed`.
    pub fn seed(&self) -> u64 {
        self.profile.simulation.seed
    }

    /// The number of paths requested by `profile.simulation.paths`.
    pub fn paths(&self) -> u64 {
        u64::from(self.profile.simulation.paths)
    }

    /// An empty accumulator sized for this plan.
    pub fn new_accumulator(&self) -> Accumulator {
        Accumulator::new(self.horizon_years())
    }
}

/// Reusable per-worker buffers: the economy path and the path state. Create one per
/// thread or worker and pass it to every `run_path` call.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// The current path's economy.
    pub economy: EconomyPath,
    // The sim owner adds the path state here (accounts, job states, property states,
    // health states, earnings records, tax-year tallies).
}

impl Workspace {
    /// Allocates buffers sized for the plan's horizon.
    pub fn new(plan: &Plan) -> Workspace {
        Workspace {
            economy: EconomyPath::with_capacity(plan.horizon_months() as usize),
        }
    }
}

/// Simulates path `path_index` and reports it to `recorder`: `begin_path`, one
/// `record_year` per calendar year until the household's last death or the horizon,
/// then `end_path`.
pub fn run_path<R: Recorder>(
    plan: &Plan,
    seed: u64,
    path_index: u64,
    workspace: &mut Workspace,
    recorder: &mut R,
) {
    todo!("sim owner")
}

/// Simulates every path in `paths` into `acc`, creating a workspace internally.
/// Equivalent to calling [`run_path`] for each index in order.
pub fn run_range(plan: &Plan, seed: u64, paths: Range<u64>, acc: &mut Accumulator) {
    todo!("sim owner")
}

/// Re-simulates one path with full monthly detail.
pub fn trace_path(plan: &Plan, seed: u64, path_index: u64) -> PathTrace {
    todo!("sim owner")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_computes_horizon_and_rejects_bad_profiles() {
        let profile = Profile::example();
        let plan = Plan::compile(&profile, Data::embedded()).unwrap();
        // Daniel is the younger adult: born 1990-04, so 100 in 2090-04.
        assert_eq!(plan.start(), YearMonth::ym(2026, 11));
        assert_eq!(plan.horizon_months(), ((2090 - 2026) * 12 + 4 - 11) as u32);
        assert_eq!(plan.horizon_years(), 65);
        assert_eq!(plan.new_accumulator().years, 65);
        assert_eq!(
            Workspace::new(&plan).economy.months,
            plan.horizon_months() as usize
        );

        let mut no_start = profile.clone();
        no_start.start = None;
        let err = Plan::compile(&no_start, Data::embedded()).unwrap_err();
        assert_eq!(err.errors[0].code, "start_unresolved");

        let err = Plan::compile(
            &Profile::default().with_start(YearMonth::ym(2026, 1)),
            Data::embedded(),
        )
        .unwrap_err();
        assert_eq!(err.errors[0].code, "people_count");
    }
}
