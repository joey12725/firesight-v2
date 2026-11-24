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

use crate::health::{CareType, HealthMonth};
use crate::health_states::{HealthChain, HealthLevel, HealthStateParams, MAX_YEARS};
use crate::longevity_links::CoupleModel;
use crate::ltc_insurance::{rate_path, LtcPolicy, PolicyState};
use crate::medicaid::{CareMonth, MedicaidRules};
use crate::model::{
    CoupleDependence, HealthModelKind, HealthcareMode, Longevity, Profile, PropertyUse,
};
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
    /// Linked lifespans of the first two people (`couple_dependence: linked` with
    /// two adults and stochastic lifespans).
    pub couple: Option<CoupleModel>,
    /// Medicaid's rules for long-term care (`spending.healthcare.medicaid` with
    /// modelled healthcare and long-term care on).
    pub medicaid: Option<MedicaidRules>,
    /// Each person's long-term-care insurance policy (`person.ltc_insurance`, with
    /// modelled long-term care on).
    pub ltc: [Option<LtcPolicy>; MAX_PEOPLE],
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
        if let Longevity::Stochastic {
            couple_dependence: CoupleDependence::Linked,
            ..
        } = profile.assumptions.longevity
        {
            if people.len() >= 2 {
                care.couple = Some(CoupleModel::new(
                    [
                        people[0].mortality.survival_table(),
                        people[1].mortality.survival_table(),
                    ],
                    data,
                ));
            }
        }
        let hc = &profile.spending.healthcare;
        if hc.medicaid && hc.long_term_care && hc.mode == HealthcareMode::Modeled {
            care.medicaid = Some(MedicaidRules::from_data(data));
        }
        if hc.long_term_care && hc.mode == HealthcareMode::Modeled {
            for (slot, person) in care.ltc.iter_mut().zip(&profile.household.people) {
                *slot = person.ltc_insurance.as_ref().map(LtcPolicy::new);
            }
        }
        care.active = !care.chains.is_empty()
            || care.couple.is_some()
            || care.medicaid.is_some()
            || care.ltc.iter().any(Option::is_some);
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
    /// True for a person whose death was drawn as following the partner's within
    /// the bereavement window (linked lifespans).
    pub bereaved: [bool; MAX_PEOPLE],
    /// Medicaid's payments for care so far on this path, nominal (what estate
    /// recovery claims).
    pub medicaid_paid: f64,
    /// First month Medicaid paid; `i32::MAX` if it has not.
    pub medicaid_month: i32,
    /// True while person `i` is on Medicaid (for the event log).
    pub on_medicaid: [bool; MAX_PEOPLE],
    /// The month in which Medicaid paid for a nursing-home resident with no spouse
    /// at home: the safety net then covers the household, which is not counted as
    /// depleted.
    pub safety_net_month: i32,
    /// Community spouse's protected assets fixed in the first month of person
    /// `i`'s care (`None` before it).
    pub snapshot: [Option<f64>; MAX_PEOPLE],
    /// Each person's insurance policy on this path.
    pub policy: [PolicyState; MAX_PEOPLE],
    /// Premium factor of the household's policy class by policy year.
    pub rates: [f64; MAX_YEARS],
    /// Household wage-like income since the last policy anniversary, nominal.
    pub income_since: f64,
    /// True if the household was depleted in a month since the last anniversary.
    pub depleted_since: bool,
    /// The last month the household-wide insurance bookkeeping ran.
    pub policy_stamp: i32,
}

impl Default for CareState {
    fn default() -> Self {
        CareState {
            level: [[0; MAX_YEARS]; MAX_PEOPLE],
            ltc_scale: [1.0; MAX_PEOPLE],
            bereaved: [false; MAX_PEOPLE],
            medicaid_paid: 0.0,
            medicaid_month: i32::MAX,
            on_medicaid: [false; MAX_PEOPLE],
            safety_net_month: i32::MIN,
            snapshot: [None; MAX_PEOPLE],
            policy: [PolicyState::default(); MAX_PEOPLE],
            rates: [1.0; MAX_YEARS],
            income_since: 0.0,
            depleted_since: false,
            policy_stamp: i32::MIN,
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
            self.care.bereaved[i] = false;
            self.care.on_medicaid[i] = false;
            self.care.snapshot[i] = None;
        }
        self.care.medicaid_paid = 0.0;
        self.care.medicaid_month = i32::MAX;
        self.care.safety_net_month = i32::MIN;
        self.care.income_since = 0.0;
        self.care.depleted_since = false;
        self.care.policy_stamp = i32::MIN;
        if care.ltc.iter().any(Option::is_some) {
            // Two uniforms a policy year, always, whatever the household does.
            let mut rng = Rng::stream(seed, path, Stream::LtcPremiumRates);
            rate_path(&mut rng, &mut self.care.rates);
            for (state, policy) in self.care.policy.iter_mut().zip(&care.ltc) {
                *state = policy
                    .as_ref()
                    .map_or(PolicyState::default(), PolicyState::start);
            }
        }
        if let Some(couple) = &care.couple {
            // The same first uniform of each person's longevity stream that the
            // independent draw used, and two uniforms of the couple stream.
            let u = [0, 1].map(|i| Rng::stream(seed, path, Stream::Longevity(i)).uniform());
            let mut rng = Rng::stream(seed, path, Stream::CoupleLongevity);
            let (pick, delay) = (rng.uniform(), rng.uniform());
            let curves = [
                plan.people[0].mortality.survival_table(),
                plan.people[1].mortality.survival_table(),
            ];
            let (months, follower) = couple.draw(curves, u, pick, delay);
            self.death_month[0] = months[0];
            self.death_month[1] = months[1];
            if let Some(j) = follower {
                self.care.bereaved[j] = true;
            }
        }
        for (i, chain) in care.chains.iter().enumerate() {
            let mut rng = Rng::stream(seed, path, Stream::HealthStates(i));
            let year = chain.sample(self.death_month[i], &mut rng, &mut self.care.level[i]);
            if care.state_costs {
                self.care.ltc_scale[i] = chain.ltc_factor(year, self.care.level[i][year]);
            }
        }
    }

    /// The health step of present person `i` in month `t` (`price`: price level
    /// since the start): scales this month's out-of-pocket cost by their health
    /// state, and lets Medicaid pay for care once savings are spent down.
    pub(crate) fn care_month(
        &mut self,
        plan: &Plan,
        i: usize,
        t: usize,
        price: f64,
        hm: &mut HealthMonth,
    ) {
        let care = &plan.care;
        if self.detail && self.care.bereaved[i] && t + 1 == self.death_month[i] as usize {
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::Bereavement, None, || {
                format!(
                    "Person {} dies within six months of being widowed (bereavement effect)",
                    i + 1
                )
            });
        }
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
        if let Some(rules) = &care.medicaid {
            // The spousal snapshot is taken in the first month of care, even when an
            // insurance policy pays that month's cost.
            if hm.long_term_care > 0.0 && self.care.snapshot[i].is_none() {
                let (countable, _) = self.medicaid_assets();
                self.care.snapshot[i] = Some(rules.protected(countable, price));
            }
        }
        if care.ltc.iter().any(Option::is_some) {
            self.ltc_insurance_month(plan, i, t, price, hm);
        }
        if let Some(rules) = &care.medicaid {
            self.medicaid_month(rules, plan, i, t, price, hm);
        }
    }

    /// Long-term-care insurance in month `t` for person `i` (`ltc_insurance.rs`):
    /// the household-wide anniversary test (rate increases, lapse) once a month,
    /// then this person's premium or benefit. The benefit pays the provider, so it
    /// lowers the care cost the household pays; the premium is healthcare spending.
    fn ltc_insurance_month(
        &mut self,
        plan: &Plan,
        i: usize,
        t: usize,
        price: f64,
        hm: &mut HealthMonth,
    ) {
        let now = plan.start + t as i32;
        if self.care.policy_stamp != t as i32 {
            self.care.policy_stamp = t as i32;
            if t > 0 && t % 12 == 0 {
                self.ltc_anniversary(plan, t, now);
            }
            self.care.income_since += self.wage_like;
            self.care.depleted_since |= self.depleted_now;
        }
        let Some(policy) = plan.care.ltc[i] else {
            return;
        };
        let year = (t / 12).min(MAX_YEARS - 1);
        let m = self.care.policy[i].month(&policy, hm.long_term_care, year, self.care.rates[year]);
        if m.premium > 0.0 {
            self.spend(crate::stats::YearSeries::SpendHealthcare, m.premium);
        }
        if m.benefit > 0.0 {
            hm.long_term_care -= m.benefit;
            if m.first_benefit {
                self.event(
                    now,
                    TraceEventKind::LtcInsurance,
                    Some(m.benefit / price),
                    || {
                        format!(
                            "Long-term-care insurance starts paying for person {}",
                            i + 1
                        )
                    },
                );
            }
            if m.exhausted {
                self.event(now, TraceEventKind::LtcInsurance, None, || {
                    format!(
                        "Person {}'s long-term-care insurance benefits are used up",
                        i + 1
                    )
                });
            }
        }
    }

    /// A policy anniversary: a class-wide premium increase takes effect, and a
    /// policy whose premium the household can no longer afford lapses.
    fn ltc_anniversary(&mut self, plan: &Plan, t: usize, now: crate::model::YearMonth) {
        let year = (t / 12).min(MAX_YEARS - 1);
        let rate = self.care.rates[year];
        let raised = rate > self.care.rates[year - 1];
        let investable = self.acc.investable().max(0.0);
        for j in 0..self.n {
            let Some(policy) = plan.care.ltc[j] else {
                continue;
            };
            let st = self.care.policy[j];
            // A partner who has left (a divorce) takes their policy with them.
            if !st.in_force || st.care_months > 0 || !self.alive[j] || !self.present[j] {
                continue;
            }
            let factor = if policy.increase_risk { rate } else { 1.0 };
            if policy.increase_risk && raised {
                let pct = (rate / self.care.rates[year - 1] - 1.0) * 100.0;
                self.event(now, TraceEventKind::LtcInsurance, None, || {
                    format!(
                        "Long-term-care premiums of person {} raised by {pct:.0}%",
                        j + 1
                    )
                });
            }
            let premium = policy.monthly_premium * factor * 12.0;
            let resources =
                self.care.income_since + crate::ltc_insurance::LAPSE_ASSET_DRAW * investable;
            if self.care.depleted_since
                || premium > crate::ltc_insurance::LAPSE_INCOME_SHARE * resources
            {
                self.care.policy[j].in_force = false;
                self.care.policy[j].lapsed = true;
                self.event(now, TraceEventKind::LtcInsurance, None, || {
                    format!(
                        "Person {}'s long-term-care insurance lapses: the premium is unaffordable",
                        j + 1
                    )
                });
            }
        }
        self.care.income_since = 0.0;
        self.care.depleted_since = false;
    }

    /// True while person `j` is in a paid care episode in month `t`.
    fn in_care(&self, j: usize, t: usize) -> bool {
        let h = &self.health[j];
        h.ltc_start_month != u32::MAX && (h.ltc_start_month..h.ltc_end_month).contains(&(t as u32))
    }

    /// Income person `i` receives this month: Social Security and the pensions,
    /// annuities and other streams they own (paid earlier in the month).
    fn own_income(&self, plan: &Plan, i: usize, t: usize, price: f64) -> f64 {
        let mut x = 0.0;
        if t >= self.ss_from && self.ss_cache_price > 0.0 {
            x += self.ss_cache.paid[i] * price / self.ss_cache_price;
        }
        for (sp, rt) in plan.streams.iter().zip(&self.streams) {
            if sp.owner == i {
                x += rt.last;
            }
        }
        x
    }

    /// The household's countable assets for Medicaid (investable accounts plus the
    /// equity in property other than the home) and its home equity, nominal. A
    /// home-equity line or reverse mortgage (`housing_decisions.rs`) is a lien on its
    /// property and reduces that property's equity.
    fn medicaid_assets(&self) -> (f64, f64) {
        let (mut home_equity, mut other_property) = (0.0, 0.0);
        for (slot, p) in self.props.iter().enumerate() {
            if p.owned {
                let lien = if self.hd.any_line {
                    self.hd.lien(slot)
                } else {
                    0.0
                };
                let equity = (p.value - p.mortgage_balance() - lien).max(0.0);
                if matches!(p.usage, PropertyUse::Primary) {
                    home_equity += equity;
                } else {
                    other_property += equity;
                }
            }
        }
        (self.acc.investable() + other_property, home_equity)
    }

    /// Medicaid's share of person `i`'s care this month (`medicaid.rs`): the
    /// household pays what is above the asset limit (the spend-down) and, once
    /// eligible, the person's income less the allowances; Medicaid the rest.
    fn medicaid_month(
        &mut self,
        rules: &MedicaidRules,
        plan: &Plan,
        i: usize,
        t: usize,
        price: f64,
        hm: &mut HealthMonth,
    ) {
        if hm.long_term_care <= 0.0 {
            return;
        }
        let spouse = (0..self.n)
            .find(|&j| j != i && self.alive[j] && self.present[j] && !self.in_care(j, t));
        let (countable, home_equity) = self.medicaid_assets();
        let snapshot = *self.care.snapshot[i].get_or_insert(rules.protected(countable, price));
        let own_income = self.own_income(plan, i, t, price);
        let spouse_income = spouse.map_or(0.0, |j| self.own_income(plan, j, t, price));
        let month = CareMonth {
            cost: hm.long_term_care,
            // This month's income is already in the cash balance; Medicaid counts it
            // as income (the liability below), not also as a resource.
            countable: countable - own_income - spouse_income,
            home_equity,
            spouse: spouse.is_some(),
            own_income,
            spouse_income,
            own_health: hm.premium + hm.out_of_pocket + self.irmaa_m[i],
            nursing_home: self.health[i].ltc_care == CareType::NursingHome,
            price,
            snapshot: Some(snapshot),
        };
        let split = rules.split(&month);
        if !split.eligible || split.medicaid <= 0.0 {
            // Eligible but the spend-down and the person's income still cover the
            // month: nothing changes.
            return;
        }
        hm.long_term_care = split.household;
        self.care.medicaid_paid += split.medicaid;
        if spouse.is_none() && month.nursing_home {
            // Only a nursing home covers room and board: someone on Medicaid there
            // with no spouse at home lives on the safety net. Medicaid paying for
            // care at home or in assisted living leaves rent and food to the
            // household, so running short there is still depletion.
            self.care.safety_net_month = t as i32;
        }
        if self.care.medicaid_month == i32::MAX {
            self.care.medicaid_month = t as i32;
        }
        if !self.care.on_medicaid[i] {
            self.care.on_medicaid[i] = true;
            let now = plan.start + t as i32;
            self.event(
                now,
                TraceEventKind::Medicaid,
                Some(split.medicaid / price),
                || {
                    format!(
                        "Savings spent down: Medicaid pays for person {}'s care",
                        i + 1
                    )
                },
            );
        }
    }

    /// Settlement: true when this month's shortfall is covered by the safety net
    /// (Medicaid paying for a nursing-home resident with no spouse at home) and so
    /// is not depletion.
    #[inline]
    pub(crate) fn care_safety_net(&self, t: usize) -> bool {
        self.care.safety_net_month == t as i32
    }

    /// Path end: Medicaid's estate recovery after the deaths of the recipient and
    /// any spouse. `estate` is the nominal estate before recovery and `everyone_died`
    /// says whether the path ended with every death; returns what the state takes.
    pub(crate) fn care_estate_recovery(
        &mut self,
        plan: &Plan,
        months: usize,
        estate: f64,
        everyone_died: bool,
        end_price: f64,
    ) -> f64 {
        if self.care.medicaid_paid <= 0.0 || !everyone_died {
            return 0.0;
        }
        let taken = crate::medicaid::estate_recovery(self.care.medicaid_paid, estate);
        if taken > 0.0 {
            let now = plan.start + months.saturating_sub(1) as i32;
            self.event(
                now,
                TraceEventKind::Medicaid,
                Some(taken / end_price),
                || "Medicaid estate recovery from the estate".to_string(),
            );
        }
        taken
    }
}
