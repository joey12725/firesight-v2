//! Household behaviour and decisions that are not mechanical rules: going back to
//! work when retirement savings fall short (`work.rs`), panic selling in a crash
//! (`panic.rs`), adaptive Social Security claiming (`claiming.rs`), stopping saving
//! in distress (`distress.rs`) and not retiring into a crash (`confirm.rs`).
//!
//! Every feature here is opt-in through a profile field whose default leaves it off.
//! With every feature off, [`BehaviorPlan::any`] is false, nothing in this module
//! runs, no random stream is created and the path is bit-identical to an engine
//! without the module.
//!
//! Random numbers come only from the module's own streams
//! ([`crate::rng::Stream::ReturnToWork`], [`crate::rng::Stream::PanicSelling`],
//! [`crate::rng::Stream::OneMoreYear`]) and
//! are drawn at a fixed rate (one draw per person, or per household, per month while
//! a feature is on), whatever the household decides, so two scenarios that differ in
//! a decision stay paired.

use crate::career::CareerParams;
use crate::data::Data;
use crate::model::{InvestorBehavior, JobSearch, Profile};
use crate::rng::{Rng, Stream};
use crate::MAX_PEOPLE;

pub(crate) mod claiming;
pub(crate) mod confirm;
pub(crate) mod distress;
pub(crate) mod panic;
pub(crate) mod work;

pub(crate) use panic::{PanicPlan, PanicState};
pub(crate) use work::{RtwPlan, RtwState};

/// The compiled behaviour settings of a profile.
#[derive(Clone, Debug, Default)]
pub(crate) struct BehaviorPlan {
    /// True when any behaviour feature is on (the month loop's single test).
    pub any: bool,
    /// `retirement.return_to_work`.
    pub rtw: Option<RtwPlan>,
    /// `portfolio.behavior: panic_prone`.
    pub panic: Option<PanicPlan>,
    /// `social_security.claiming: adaptive`, per person.
    pub claim: Option<claiming::ClaimPlan>,
    /// `portfolio.pause_deferrals_in_distress`.
    pub distress: Option<distress::DistressPlan>,
    /// `retirement.trigger.when_funded.confirmation` and `one_more_year`.
    pub confirm: Option<confirm::ConfirmPlan>,
}

impl BehaviorPlan {
    /// Compiles the behaviour settings. `age0` is each person's age in months at the
    /// start.
    pub(crate) fn compile(profile: &Profile, data: &Data, age0: &[i32]) -> BehaviorPlan {
        let rtw = profile.retirement.return_to_work.map(|r| {
            let params = CareerParams::from_data(data);
            let mut until_month = [i32::MIN; MAX_PEOPLE];
            for (i, a) in age0.iter().enumerate().take(MAX_PEOPLE) {
                until_month[i] = (r.until_age * 12.0).round() as i32 - a;
            }
            RtwPlan {
                ratio_below: r.funded_ratio_below,
                until_month,
                pay_fraction: r.pay_fraction.clamp(0.0, 1.5),
                former_pay: r.former_annual_pay.map(|x| x.max(0.0)),
                fixed_months: match r.search {
                    JobSearch::Typical => None,
                    JobSearch::Fixed { months } => Some(months.max(0.0)),
                },
                spell_median: params.spell_median,
                spell_sigma: params.spell_sigma,
                spell_age_multiplier: params.spell_age_multiplier,
            }
        });
        let panic = match &profile.portfolio.behavior {
            InvestorBehavior::Disciplined => None,
            InvestorBehavior::PanicProne(p) => Some(PanicPlan::compile(p)),
        };
        let claim = claiming::compile(profile);
        let distress = distress::DistressPlan::compile(&profile.portfolio);
        let confirm = confirm::ConfirmPlan::compile(profile);
        BehaviorPlan {
            any: rtw.is_some()
                || panic.is_some()
                || claim.is_some()
                || distress.is_some()
                || confirm.is_some(),
            confirm,
            rtw,
            panic,
            claim,
            distress,
        }
    }
}

/// The behaviour state of one path.
#[derive(Clone, Debug)]
pub(crate) struct BehaviorState {
    /// Return to work, per person.
    pub rtw: [RtwState; MAX_PEOPLE],
    /// Panic selling.
    pub panic: PanicState,
    pub panic_rng: Rng,
    /// Stopping saving in distress: this month counts as one of distress, and the
    /// year's penalised early distributions as last seen.
    pub distress: bool,
    pub penalty_seen: f64,
    /// Months the last penalty still counts for.
    pub penalty_months: u8,
    /// Confirming retirement.
    pub confirm: confirm::ConfirmState,
    pub confirm_rng: Rng,
}

impl Default for BehaviorState {
    fn default() -> Self {
        Self {
            rtw: Default::default(),
            panic: PanicState::default(),
            panic_rng: Rng::from_seed(0),
            distress: false,
            penalty_seen: 0.0,
            penalty_months: 0,
            confirm: confirm::ConfirmState::default(),
            confirm_rng: Rng::from_seed(0),
        }
    }
}

impl BehaviorState {
    /// Resets the state for path `path`. Creates the streams only for the features
    /// that are on.
    pub(crate) fn begin(&mut self, plan: &BehaviorPlan, seed: u64, path: u64) {
        if !plan.any {
            return;
        }
        for (i, s) in self.rtw.iter_mut().enumerate() {
            *s = RtwState::default();
            if plan.rtw.is_some() {
                s.rng = Rng::stream(seed, path, Stream::ReturnToWork(i));
            }
        }
        self.panic = PanicState::default();
        self.distress = false;
        self.penalty_seen = 0.0;
        self.penalty_months = 0;
        self.confirm = confirm::ConfirmState::default();
        if matches!(
            plan.confirm,
            Some(confirm::ConfirmPlan {
                one_more_year: Some(_),
                ..
            })
        ) {
            self.confirm_rng = Rng::stream(seed, path, Stream::OneMoreYear);
        }
        if plan.panic.is_some() {
            self.panic_rng = Rng::stream(seed, path, Stream::PanicSelling);
        }
    }
}

impl super::state::State {
    /// Behaviour that adds income in month `t`: called after careers and before
    /// pensions and Social Security, only when some feature is on.
    pub(crate) fn behavior_income(
        &mut self,
        plan: &super::plan::Plan,
        econ: &crate::economy::EconomyPath,
        t: usize,
    ) {
        if plan.beh.rtw.is_some() {
            self.return_to_work_month(plan, econ, t);
        }
        if plan.beh.confirm.is_some() {
            self.confirm_draw(plan);
        }
        if plan.beh.claim.is_some() {
            self.adaptive_claiming_month(plan, t, econ.price_level[t]);
        }
    }

    /// Behaviour after the month's returns (step 10), only when some feature is on.
    pub(crate) fn behavior_after_returns(
        &mut self,
        plan: &super::plan::Plan,
        econ: &crate::economy::EconomyPath,
        t: usize,
    ) {
        if plan.beh.panic.is_some() {
            self.panic_month(plan, t, econ.price_level[t + 1]);
        }
    }
}
