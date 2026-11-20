//! Career and income realism: five opt-in extensions of the income model.
//!
//! | Feature | Switch | Module |
//! |---|---|---|
//! | Voluntary job changes with pay jumps | `risk.job_mobility` | [`mobility`] |
//! | Recession pay freezes, hours cuts, smaller bonuses | `risk.recession_pay` | [`recession_pay`] |
//! | Long-term disability, SSDI and LTD | `people[].disability_risk` | [`disability`] |
//! | Older workers, involuntary retirement | `risk.late_career` | [`late_career`] |
//! | Business cycles and failure | `business_risk` on self-employment | [`business`] |
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

pub mod business;
pub mod disability;
pub mod late_career;
pub mod mobility;
pub mod recession_pay;
pub mod validate;

use crate::data::Data;
use crate::model::{IncomeKind, JobMobility, LateCareer, Profile, RecessionPay, RiskLevel};
use crate::rng::{Rng, Stream};

use business::{BusinessModel, BusinessState};
use disability::{DisabilityModel, DisabilityState};
use late_career::LateCareerModel;
use mobility::MobilityModel;
use recession_pay::{RecessionPayModel, RecessionPayState};

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
/// `Stream::CareerExt` kind: one stream per person (index into the plan's people).
pub const KIND_PERSON: u8 = 1;
/// `Stream::CareerExt` kind: one stream per self-employment income (index into the
/// plan's self-employment incomes).
pub const KIND_SE: u8 = 2;

/// Compiled realism settings of one job.
#[derive(Clone, Debug, Default)]
pub struct JobRealism {
    /// Voluntary job changes, when switched on.
    pub mobility: Option<MobilityModel>,
    /// Recession pay freezes, cuts and smaller bonuses, when switched on.
    pub recession_pay: Option<RecessionPayModel>,
    /// Older-worker layoff outcomes, when switched on.
    pub late_career: Option<LateCareerModel>,
}

impl JobRealism {
    /// True if any feature is on for this job (it then draws every month).
    pub fn any(&self) -> bool {
        self.mobility.is_some() || self.recession_pay.is_some() || self.late_career.is_some()
    }
}

/// Everything realism-related that is fixed by the profile.
#[derive(Clone, Debug, Default)]
pub struct RealismPlan {
    /// True if any feature is on anywhere: the one flag the month loop tests.
    pub any: bool,
    /// One entry per job in the plan's job order.
    pub jobs: Vec<JobRealism>,
    /// One entry per person: the disability model when switched on.
    pub people: Vec<Option<DisabilityModel>>,
    /// One entry per self-employment income: the business model when switched on.
    pub selfemp: Vec<Option<BusinessModel>>,
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
                // Recession pay follows the path's recession flag; it is switched off
                // with the other random elements of pay by `career_risk`.
                if risk_on && job.risk.recession_pay == RecessionPay::Typical {
                    jr.recession_pay = Some(RecessionPayModel::new());
                }
                if risk_on
                    && job.risk.level != RiskLevel::None
                    && job.risk.late_career == LateCareer::Realistic
                {
                    jr.late_career = Some(LateCareerModel);
                }
                plan.any |= jr.any();
                plan.jobs.push(jr);
            }
        }
        for income in &profile.incomes {
            if let IncomeKind::SelfEmployment(se) = &income.kind {
                let model = if risk_on {
                    BusinessModel::new(
                        se.business_risk,
                        se.fallback_wage.map(|f| (f.annual, f.search_months)),
                        se.years_operating.max(0.0),
                    )
                } else {
                    None
                };
                plan.any |= model.is_some();
                plan.selfemp.push(model);
            }
        }
        for person in &profile.household.people {
            let model =
                (person.disability_risk == crate::model::DisabilityRisk::Typical).then(|| {
                    DisabilityModel::new(person.ltd_insurance.map(|l| (l.replacement, l.to_age)))
                });
            plan.any |= model.is_some();
            plan.people.push(model);
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
    /// Uniform: does a recession pay freeze start.
    pub u_freeze: f64,
    /// Uniform: does a recession pay cut start.
    pub u_cut: f64,
    /// Uniform: the size of a pay cut.
    pub u_size: f64,
    /// Uniform: does a laid-off older worker never work again.
    pub u_never: f64,
}

/// The variates one person draws in one month.
#[derive(Clone, Copy, Debug, Default)]
pub struct PersonDraw {
    /// Uniform: does a disability begin.
    pub u_onset: f64,
    /// Uniform: does a disabled person recover.
    pub u_recover: f64,
}

/// The variates one business draws in one month.
#[derive(Clone, Copy, Debug, Default)]
pub struct BusinessDraw {
    /// Uniform: does the business fail.
    pub u_fail: f64,
    /// Standard normal: the January regime innovation.
    pub z: f64,
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
    /// Recession pay state per job.
    pub pay: Vec<RecessionPayState>,
    /// One generator per person (empty when no feature is on).
    pub person_rng: Vec<Rng>,
    /// This month's variates per person.
    pub person_draw: Vec<PersonDraw>,
    /// Disability state per person.
    pub dis: Vec<DisabilityState>,
    /// True once a person has been laid off and will never work again (F19).
    pub never: Vec<bool>,
    /// One generator per self-employment income (empty when no feature is on).
    pub se_rng: Vec<Rng>,
    /// This month's variates per self-employment income.
    pub se_draw: Vec<BusinessDraw>,
    /// Business state per self-employment income.
    pub biz: Vec<BusinessState>,
}

impl RealismState {
    /// Buffers sized for a plan.
    pub fn new(plan: &RealismPlan) -> RealismState {
        let n = if plan.any { plan.jobs.len() } else { 0 };
        let np = if plan.any { plan.people.len() } else { 0 };
        let ns = if plan.any { plan.selfemp.len() } else { 0 };
        RealismState {
            job_rng: vec![Rng::from_seed(0); n],
            job_draw: vec![JobDraw::default(); n],
            moves: vec![0; n],
            pay: vec![RecessionPayState::default(); n],
            person_rng: vec![Rng::from_seed(0); np],
            person_draw: vec![PersonDraw::default(); np],
            dis: vec![DisabilityState::default(); np],
            never: vec![false; np],
            se_rng: vec![Rng::from_seed(0); ns],
            se_draw: vec![BusinessDraw::default(); ns],
            biz: vec![BusinessState::default(); ns],
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
        self.pay.fill(RecessionPayState::default());
        for (i, rng) in self.person_rng.iter_mut().enumerate() {
            if plan.people[i].is_some() {
                *rng = Rng::stream(seed, path, Stream::CareerExt(KIND_PERSON, i));
            }
        }
        self.dis.fill(DisabilityState::default());
        self.never.fill(false);
        for (k, rng) in self.se_rng.iter_mut().enumerate() {
            if plan.selfemp[k].is_some() {
                *rng = Rng::stream(seed, path, Stream::CareerExt(KIND_SE, k));
            }
        }
        self.biz.fill(BusinessState::default());
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
                u_freeze: rng.uniform(),
                u_cut: rng.uniform(),
                u_size: rng.uniform(),
                u_never: rng.uniform(),
            };
        }
        // Two draws per business-enabled self-employment income, whatever happens.
        for (k, model) in plan.selfemp.iter().enumerate() {
            if model.is_some() {
                let rng = &mut self.se_rng[k];
                let u_fail = rng.uniform();
                let u = rng.uniform_open();
                self.se_draw[k] = BusinessDraw {
                    u_fail,
                    z: crate::rng::inverse_normal_cdf(u),
                };
            }
        }
        // Two words per disability-enabled person, whatever happens.
        for (i, model) in plan.people.iter().enumerate() {
            if model.is_some() {
                let rng = &mut self.person_rng[i];
                self.person_draw[i] = PersonDraw {
                    u_onset: rng.uniform(),
                    u_recover: rng.uniform(),
                };
            }
        }
    }
}
