//! The opt-in health, longevity and care features, as they enter one path: health
//! states (`assumptions.health_model: states`), linked lifespans
//! (`longevity.couple_dependence: linked`), Medicaid for long-term care
//! (`spending.healthcare.medicaid`) and long-term-care insurance
//! (`person.ltc_insurance`).
//!
//! The month loop reaches this file through two hooks, each behind
//! `plan.care.active`, a flag fixed at compile time: [`State::care_begin`] when a
//! path starts and [`State::care_month`] in the health step. With every feature
//! off neither runs and the path is the one it always was.

use crate::health::HealthMonth;
use crate::health_states::{HealthChain, HealthLevel, HealthStateParams, MAX_YEARS};
use crate::model::{HealthModelKind, Profile};
use crate::rng::{Rng, Stream};
use crate::stats::TraceEventKind;
use crate::MAX_PEOPLE;

use super::plan::{PersonPlan, Plan};
use super::state::State;

/// The compiled features.
#[derive(Clone, Debug, Default)]
pub(crate) struct CarePlan {
    /// True when any feature is on: the hooks run.
    pub active: bool,
    /// Health-state chain of each person (empty unless `health_model: states`).
    pub chains: Vec<HealthChain>,
    /// True when the state scales costs and care incidence (states on and
    /// `health_risk` on: with health risk off costs are averages).
    pub state_costs: bool,
}

impl CarePlan {
    /// Compiles the features of `profile` for its people.
    pub(crate) fn compile(
        profile: &Profile,
        people: &[PersonPlan],
        data: &crate::data::Data,
    ) -> CarePlan {
        let mut care = CarePlan::default();
        if profile.assumptions.health_model == HealthModelKind::States {
            let params = HealthStateParams::from_data(data);
            care.chains = people
                .iter()
                .map(|p| {
                    HealthChain::new(
                        &params,
                        p.sex,
                        p.age0.max(0) as u32,
                        p.mortality.survival_table(),
                        data,
                    )
                })
                .collect();
            care.state_costs = profile.assumptions.health_risk;
        }
        care.active = !care.chains.is_empty();
        care
    }
}

/// The features' state on one path.
#[derive(Clone, Debug)]
pub(crate) struct CareState {
    /// Health state of each person in each simulation year.
    pub level: [[u8; MAX_YEARS]; MAX_PEOPLE],
    /// Multiplier on each person's care-episode probability, from the state they
    /// die in.
    pub ltc_scale: [f64; MAX_PEOPLE],
}

impl Default for CareState {
    fn default() -> Self {
        CareState {
            level: [[0; MAX_YEARS]; MAX_PEOPLE],
            ltc_scale: [1.0; MAX_PEOPLE],
        }
    }
}

impl State {
    /// Path start, after the lifetimes are drawn and before the health model's
    /// initial state: draws each person's health-state path given their death.
    pub(crate) fn care_begin(&mut self, plan: &Plan, seed: u64, path: u64) {
        let care = &plan.care;
        for i in 0..MAX_PEOPLE {
            self.care.ltc_scale[i] = 1.0;
        }
        for (i, chain) in care.chains.iter().enumerate() {
            let mut rng = Rng::stream(seed, path, Stream::HealthStates(i));
            let year = chain.sample(self.death_month[i], &mut rng, &mut self.care.level[i]);
            if care.state_costs {
                self.care.ltc_scale[i] = chain.ltc_factor(year, self.care.level[i][year]);
            }
        }
    }

    /// The health step of present person `i` in month `t`: scales this month's
    /// out-of-pocket cost by their health state.
    pub(crate) fn care_month(&mut self, plan: &Plan, i: usize, t: usize, hm: &mut HealthMonth) {
        let care = &plan.care;
        if let Some(chain) = care.chains.get(i) {
            let k = t / 12;
            let level = self.care.level[i][k.min(MAX_YEARS - 1)];
            if care.state_costs {
                hm.out_of_pocket *= chain.oop_factor(k, level);
            }
            if self.detail && t % 12 == 0 {
                let before = if k == 0 {
                    u8::MAX
                } else {
                    self.care.level[i][(k - 1).min(MAX_YEARS - 1)]
                };
                if level != before && (k > 0 || level != HealthLevel::Healthy as u8) {
                    let now = plan.start + t as i32;
                    let label = HealthLevel::from_index(level).label();
                    self.event(now, TraceEventKind::HealthState, None, || {
                        format!("Person {} health: {label}", i + 1)
                    });
                }
            }
        }
    }
}
