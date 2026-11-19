//! Career and income realism: five opt-in extensions of the income model.
//!
//! | Feature | Switch | Module |
//! |---|---|---|
//! | Voluntary job changes with pay jumps | `risk.job_mobility` | [`mobility`] |
//!
//! (Later features register here as they land.)
//!
//! # Contract
//!
//! * Every feature is off unless its schema field is set, and with every feature off
//!   nothing here runs: [`RealismPlan::any`] is false and the simulation never calls
//!   in. Profiles that existed before these features reproduce bit for bit.
//! * Randomness comes only from [`Stream::CareerExt`] streams of their own, never from
//!   the `Career` or `EquityComp` streams, so switching a feature on does not move
//!   any existing random number. A job that uses any feature draws [`JOB_WORDS`]
//!   words from its own stream every month, whatever its state and whatever the
//!   household decides, so switching one feature on never moves another feature's
//!   draws, and paired scenarios see the same variates.
//! * Nothing here allocates once a path is running: per-path buffers are sized in
//!   [`RealismState::new`].
//! * Transcendental functions go through [`crate::math`].
//!
//! The simulation-side hooks live in `sim/realism.rs` (they need the path state); this
//! module holds the pure mechanisms, which are unit-tested on their own.

pub mod mobility;
pub mod validate;

use crate::data::Data;
use crate::model::{IncomeKind, JobMobility, Profile, RiskLevel};
use crate::rng::{Rng, Stream};

use mobility::MobilityModel;

/// Serde helper: the opt-in fields of this module are left out of serialised
/// profiles while they hold their default (off), so profiles written before the
/// features existed serialise exactly as they did.
pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// 64-bit words each realism-enabled job draws per month (see [`JobDraw`]).
pub const JOB_WORDS: usize = 6;

/// `Stream::CareerExt` kind: one stream per job income (index into the plan's jobs).
pub const KIND_JOB: u8 = 0;

/// Compiled realism settings of one job.
#[derive(Clone, Debug, Default)]
pub struct JobRealism {
    /// Voluntary job changes, when switched on.
    pub mobility: Option<MobilityModel>,
}

impl JobRealism {
    /// True if any feature is on for this job (it then draws every month).
    pub fn any(&self) -> bool {
        self.mobility.is_some()
    }
}

/// Everything realism-related that is fixed by the profile.
#[derive(Clone, Debug, Default)]
pub struct RealismPlan {
    /// True if any feature is on anywhere: the one flag the month loop tests.
    pub any: bool,
    /// One entry per job in the plan's job order.
    pub jobs: Vec<JobRealism>,
}

impl RealismPlan {
    /// Compiles the realism settings of a profile. Jobs appear in the order of the
    /// profile's income list, like `Plan::jobs`. `_data` is reserved for parameter
    /// tables read from `params.json`.
    pub fn compile(profile: &Profile, _data: &Data) -> RealismPlan {
        let mut plan = RealismPlan::default();
        let risk_on = profile.assumptions.career_risk;
        for income in &profile.incomes {
            if let IncomeKind::Job(job) = &income.kind {
                let mut jr = JobRealism::default();
                // Voluntary moves are part of career risk: they follow the risk
                // switches of the job and of the assumptions.
                if risk_on && job.risk.level != RiskLevel::None {
                    jr.mobility = match job.risk.job_mobility {
                        JobMobility::Off => None,
                        level => Some(MobilityModel::new(level)),
                    };
                }
                plan.any |= jr.any();
                plan.jobs.push(jr);
            }
        }
        plan
    }
}

/// The variates one job draws in one month, in stream order.
#[derive(Clone, Copy, Debug, Default)]
pub struct JobDraw {
    /// Uniform: does a voluntary move happen.
    pub u_move: f64,
    /// Raw word: the size of the pay jump (a normal if a move happens).
    pub r_jump: u64,
    /// Reserved for later features (see the module table).
    pub u_a: f64,
    /// Reserved.
    pub u_b: f64,
    /// Reserved.
    pub u_c: f64,
    /// Reserved.
    pub u_d: f64,
}

/// Per-path realism state.
#[derive(Clone, Debug, Default)]
pub struct RealismState {
    /// One generator per job (empty when no feature is on).
    pub job_rng: Vec<Rng>,
    /// This month's variates per job.
    pub job_draw: Vec<JobDraw>,
    /// Voluntary job changes so far on this path, per job.
    pub moves: Vec<u32>,
}

impl RealismState {
    /// Buffers sized for a plan.
    pub fn new(plan: &RealismPlan) -> RealismState {
        let n = if plan.any { plan.jobs.len() } else { 0 };
        RealismState {
            job_rng: vec![Rng::from_seed(0); n],
            job_draw: vec![JobDraw::default(); n],
            moves: vec![0; n],
        }
    }

    /// Resets for path `path`.
    pub fn begin(&mut self, plan: &RealismPlan, seed: u64, path: u64) {
        for (j, rng) in self.job_rng.iter_mut().enumerate() {
            if plan.jobs[j].any() {
                *rng = Rng::stream(seed, path, Stream::CareerExt(KIND_JOB, j));
            }
        }
        self.moves.fill(0);
    }

    /// Draws this month's variates for every job that uses a feature: always
    /// [`JOB_WORDS`] words, whatever happens.
    #[inline]
    pub fn draw_month(&mut self, plan: &RealismPlan) {
        for (j, jr) in plan.jobs.iter().enumerate() {
            if !jr.any() {
                continue;
            }
            let rng = &mut self.job_rng[j];
            self.job_draw[j] = JobDraw {
                u_move: rng.uniform(),
                r_jump: rng.next_u64(),
                u_a: rng.uniform(),
                u_b: rng.uniform(),
                u_c: rng.uniform(),
                u_d: rng.uniform(),
            };
        }
    }
}
