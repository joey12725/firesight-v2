//! The simulation loop: one household, one life, one month at a time.
//!
//! * [`Plan::compile`] (`plan.rs`) resolves a profile once: ids to indices, dates to
//!   month offsets, budget lines to monthly real amounts, and every subsystem model.
//! * [`run_path`] simulates one path into a [`Recorder`]; [`run_range`] a range of
//!   paths into an [`Accumulator`]; [`trace_path`] one path with its full ledger.
//!
//! # A path
//!
//! 1. **Lifetimes** are drawn first, each from its own `Longevity` stream. The path
//!    runs until the last death or the horizon (the month the youngest adult would
//!    reach `simulation.end_age`), whichever comes first. With `fixed_age` longevity
//!    the length is the same on every path.
//! 2. **The economy** is generated for exactly that many months from the `Market`
//!    and `Inflation` streams, then stress overlays with fixed dates are applied.
//! 3. **Events** are resolved on the `Events` stream: whether each happens and how
//!    far its date shifts.
//! 4. **The month loop** (`month.rs`) runs the steps of `docs/architecture.md`
//!    section 3 in order; the annual cycle is in `year.rs`, the retirement trigger
//!    in `trigger.rs`, events in `events.rs`, recording and the conservation checks
//!    in `record.rs`.
//!
//! # Contract
//!
//! * `run_path(plan, seed, i, ..)` is a pure function of its arguments: the same
//!   inputs give the same recorder calls, bit for bit, on native and wasm.
//! * Every random subsystem draws only from its own `rng::Stream`, and the number
//!   of draws never depends on a decision, so two scenarios that differ in a
//!   decision see identical markets, careers, health and lifespans.
//! * No allocation per path once the [`Workspace`] has run one path (trace mode
//!   allocates for its labels and returns). `tests/sim_no_alloc.rs` asserts it.
//! * `run_range` over `a..b` then `b..c`, merged, equals `run_range` over `a..c`.
//! * Money handed to the recorder is in today's dollars.
//! * Money is conserved: see `record.rs`. Call [`Workspace::set_check`] to assert
//!   it every month (tests do; it costs about a fifth of the run time).
//!
//! # Depletion
//!
//! When a month cannot be paid from cash, investments are sold in the conventional
//! order with the early-access rules, penalty-free sources first and penalised ones
//! after. When nothing is left to sell, spending is cut to what there is:
//! discretionary lines first, then the essentials, and the household is *depleted*
//! from the first month an essential is cut. Costs that cannot be cut (housing,
//! debt service, tax) remain as an overdraft that is reported as debt and repaid
//! from later income. A depleted household recovers in the first month its income
//! covers its planned spending again with no overdraft; the path still counts as a
//! failure.

// Per-person and per-asset state lives in parallel fixed arrays; indexing them by
// person or asset index reads better than zipping four iterators.
#![allow(clippy::needless_range_loop)]

use std::ops::Range;

use crate::economy::EconomyPath;
use crate::stats::{Accumulator, PathTrace, Recorder, TraceRecorder};

mod events;
mod family;
mod month;
mod plan;
mod preview;
mod record;
mod state;
mod trigger;
mod year;

pub use plan::{
    Plan, DEFAULT_LATEST_RETIREMENT_AGE, INCOME_DISCOUNT_RATE, PLANNING_INFLATION,
    PURCHASE_RETRY_MONTHS,
};
pub use preview::PlanPreview;

/// Reusable per-worker buffers: the economy path and the path state. Create one per
/// thread or worker and pass it to every `run_path` call.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// The current path's economy.
    pub economy: EconomyPath,
    state: state::State,
    check: bool,
}

impl Workspace {
    /// Allocates buffers sized for the plan's horizon.
    pub fn new(plan: &Plan) -> Workspace {
        Workspace {
            economy: EconomyPath::with_capacity(plan.horizon_months() as usize),
            state: state::State::new(plan),
            check: false,
        }
    }

    /// Turns the monthly conservation assertions on or off (off by default).
    pub fn set_check(&mut self, check: bool) {
        self.check = check;
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
    let detail = recorder.wants_detail();
    let Workspace {
        economy,
        state,
        check,
    } = workspace;
    state.begin(plan, seed, path_index, economy, detail, *check);
    recorder.begin_path(path_index);
    for t in 0..state.months {
        state.month(plan, economy, recorder, t);
    }
    state.finish(plan, economy, recorder);
}

/// Simulates every path in `paths` into `acc`, creating a workspace internally.
/// Equivalent to calling [`run_path`] for each index in order.
pub fn run_range(plan: &Plan, seed: u64, paths: Range<u64>, acc: &mut Accumulator) {
    let mut workspace = Workspace::new(plan);
    for i in paths {
        run_path(plan, seed, i, &mut workspace, acc);
    }
}

/// Re-simulates one path with full monthly detail.
pub fn trace_path(plan: &Plan, seed: u64, path_index: u64) -> PathTrace {
    trace_path_in(plan, seed, path_index, &mut Workspace::new(plan))
}

/// [`trace_path`] in a caller-supplied workspace (reuse it across traces, or turn
/// its conservation checks on first).
pub fn trace_path_in(
    plan: &Plan,
    seed: u64,
    path_index: u64,
    workspace: &mut Workspace,
) -> PathTrace {
    let mut recorder = TraceRecorder::new(seed, plan.start());
    run_path(plan, seed, path_index, workspace, &mut recorder);
    recorder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use crate::model::{Profile, YearMonth};

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
