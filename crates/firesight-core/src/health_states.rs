//! Health states that drive both costs and mortality (`assumptions.health_model:
//! states`).
//!
//! Each adult moves once a year through a Markov chain of three states, healthy,
//! chronic (two or more chronic conditions) and frail, with age-dependent
//! transitions (`params.json` `health_mortality_care.health_states`). The state
//! multiplies the mortality hazard, the out-of-pocket cost and the chance that the
//! end of life is spent in paid long-term care.
//!
//! Contract:
//!
//! * **Mortality is unchanged on average, exactly.** For every simulation year `k`
//!   the state hazards are `q_k(s) = c_k * r_s * q_k`, with `q_k` the person's own
//!   one-year death probability from their survival curve and `c_k` chosen so that
//!   the hazards averaged over the states of the people still alive (`pi_k`, carried
//!   forward through the chain with these same hazards) equal `q_k`. The marginal
//!   survival curve is therefore the one `MortalityModel` built, to the month.
//! * **Deaths are not redrawn.** The age at death still comes from the person's
//!   `Longevity` stream; the state path is then drawn *given* that death by forward
//!   filtering and backward sampling (exact for the joint model: a frail year is
//!   more likely just before a death). So turning the feature on leaves every
//!   lifetime, and everything that depends only on lifetimes, bit for bit the same.
//! * **Costs are unchanged on average.** The out-of-pocket multiplier of a state is
//!   divided by its average over the living at that year (`pi_k`); the care
//!   multiplier by its average over the states of the people who die in that year.
//! * Draws: one uniform per simulated year up to the death year from the new
//!   `Stream::HealthStates(person)`; no other stream is touched.

use crate::data::Data;
use crate::math;
use crate::model::Sex;
use crate::rng::Rng;

/// Number of health states.
pub const N_STATES: usize = 3;

/// Most simulation years a chain covers (ages 0 to the end of the 120-year table).
pub const MAX_YEARS: usize = 128;

/// Share of the chronic-to-frail transition rate at which a healthy person becomes
/// frail directly. A judgement (frailty mostly follows chronic disease), registered
/// in `params.json` `unverified_from_memory.health_mortality_care_judgements`.
pub const HEALTHY_TO_FRAIL_SHARE: f64 = 0.25;

/// Annual probability that a frail person recovers to the chronic state. A
/// judgement: frailty is partly reversible in the cohort studies, the rate is not
/// sourced.
pub const FRAIL_RECOVERY: f64 = 0.1;

/// Relative chance, by state at death, that the end of life is spent in paid
/// long-term care (healthy, chronic, frail), normalised to keep the population
/// incidence. A judgement: frailty and disability are the gateway to care.
pub const LTC_STATE_FACTOR: [f64; N_STATES] = [0.5, 1.0, 2.0];

/// Age at which the population chain that sets a person's starting state begins,
/// everyone healthy.
const CHAIN_START_AGE: usize = 18;

/// A health state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum HealthLevel {
    /// No more than one chronic condition.
    #[default]
    Healthy = 0,
    /// Two or more chronic conditions, not frail.
    Chronic = 1,
    /// Frail (the Fried phenotype).
    Frail = 2,
}

impl HealthLevel {
    /// The state with this index (0, 1, 2; anything else is frail).
    pub fn from_index(i: u8) -> HealthLevel {
        match i {
            0 => HealthLevel::Healthy,
            1 => HealthLevel::Chronic,
            _ => HealthLevel::Frail,
        }
    }

    /// Lower-case name for the event log.
    pub fn label(self) -> &'static str {
        match self {
            HealthLevel::Healthy => "healthy",
            HealthLevel::Chronic => "chronic conditions",
            HealthLevel::Frail => "frail",
        }
    }
}

/// The chain's parameters from `params.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HealthStateParams {
    onset_intercept: f64,
    onset_slope: f64,
    frailty_intercept: f64,
    frailty_slope: f64,
    /// Mortality hazard ratio by state.
    pub hazard: [f64; N_STATES],
    /// Out-of-pocket multiplier by state (before normalisation).
    pub oop: [f64; N_STATES],
}

fn num(v: &serde_json::Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("embedded data: health_states.{key}"))
}

fn by_state(v: &serde_json::Value) -> [f64; N_STATES] {
    [num(v, "healthy"), num(v, "chronic"), num(v, "frail")]
}

fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + math::exp(-x))
}

impl HealthStateParams {
    /// Reads `health_mortality_care.health_states`.
    pub fn from_data(data: &Data) -> HealthStateParams {
        let hs = &data.actuarial["health_mortality_care"]["health_states"];
        let tr = &hs["transitions"];
        HealthStateParams {
            onset_intercept: num(tr, "onset_intercept"),
            onset_slope: num(tr, "onset_slope"),
            frailty_intercept: num(tr, "frailty_intercept"),
            frailty_slope: num(tr, "frailty_slope"),
            hazard: by_state(&hs["mortality_hazard_ratio"]),
            oop: by_state(&hs["out_of_pocket_multiplier"]),
        }
    }

    /// One-year transition matrix at an age in whole years (rows: from).
    pub fn transition(&self, age: u32) -> [[f64; N_STATES]; N_STATES] {
        let a = f64::from(age);
        let onset = logistic(self.onset_intercept + self.onset_slope * (a - 50.0));
        let frailty = logistic(self.frailty_intercept + self.frailty_slope * (a - 80.0));
        let direct = HEALTHY_TO_FRAIL_SHARE * frailty;
        [
            [1.0 - onset - direct, onset, direct],
            [0.0, 1.0 - frailty, frailty],
            [0.0, FRAIL_RECOVERY, 1.0 - FRAIL_RECOVERY],
        ]
    }

    /// State hazards for one year: `min(1, c * r_s * q)` with `c` such that their
    /// average over `pi` is `q` (bisection when a hazard would pass 1).
    pub fn state_hazards(&self, q: f64, pi: &[f64; N_STATES]) -> [f64; N_STATES] {
        let r = self.hazard;
        let avg: f64 = (0..N_STATES).map(|s| pi[s] * r[s]).sum();
        if q <= 0.0 || avg <= 0.0 {
            return [q.max(0.0); N_STATES];
        }
        if q >= 1.0 {
            return [1.0; N_STATES];
        }
        let at = |c: f64| -> [f64; N_STATES] {
            [
                (c * r[0] * q).min(1.0),
                (c * r[1] * q).min(1.0),
                (c * r[2] * q).min(1.0),
            ]
        };
        let c0 = 1.0 / avg;
        let first = at(c0);
        if first.iter().all(|&h| h < 1.0) {
            return first;
        }
        let mean = |h: &[f64; N_STATES]| -> f64 { (0..N_STATES).map(|s| pi[s] * h[s]).sum() };
        // The mean is continuous and increasing in c, below q at 0 and at least q
        // once every hazard is 1.
        let mut hi = c0;
        while mean(&at(hi)) < q {
            hi *= 2.0;
        }
        let mut lo = 0.0;
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            if mean(&at(mid)) < q {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        at(hi)
    }

    /// Distribution over states at an age in whole years for someone of this sex
    /// alive then: the chain run on the period table from age 18, everyone healthy.
    pub fn prevalence(&self, sex: Sex, age: u32, data: &Data) -> [f64; N_STATES] {
        let mut pi = [1.0, 0.0, 0.0];
        for a in CHAIN_START_AGE..age as usize {
            let q = data.life_table.qx(sex, a);
            pi = self.step(&pi, q, a as u32).0;
        }
        pi
    }

    /// Carries the survivors' distribution one year forward. Returns the new
    /// distribution (normalised) and the year's state hazards.
    fn step(&self, pi: &[f64; N_STATES], q: f64, age: u32) -> ([f64; N_STATES], [f64; N_STATES]) {
        let h = self.state_hazards(q, pi);
        let p = self.transition(age);
        let mut next = [0.0; N_STATES];
        for s in 0..N_STATES {
            let alive = pi[s] * (1.0 - h[s]);
            for (n, slot) in next.iter_mut().enumerate() {
                *slot += alive * p[s][n];
            }
        }
        let total: f64 = next.iter().sum();
        if total > 0.0 {
            for x in &mut next {
                *x /= total;
            }
        } else {
            next = *pi;
        }
        (next, h)
    }
}

/// One person's compiled chain, year by year from the plan's start.
#[derive(Clone, Debug, PartialEq)]
pub struct HealthChain {
    /// Distribution over states at the start.
    pi0: [f64; N_STATES],
    /// State hazards of dying within simulation year `k`.
    hazard: Vec<[f64; N_STATES]>,
    /// Transition from year `k` to year `k + 1`.
    trans: Vec<[[f64; N_STATES]; N_STATES]>,
    /// Out-of-pocket multiplier by state in year `k` (mean 1 over the living).
    oop: Vec<[f64; N_STATES]>,
    /// Care-incidence multiplier by state at death in year `k` (mean 1 over the
    /// people who die that year).
    ltc: Vec<[f64; N_STATES]>,
    /// Distribution over states of the living at the start of year `k`.
    pi: Vec<[f64; N_STATES]>,
}

impl HealthChain {
    /// Builds the chain for a person with this survival curve (`survival[m]` is the
    /// probability of being alive `m` months after the start, as in
    /// `MortalityModel::survival_table`) and age in whole months at the start.
    pub fn new(
        params: &HealthStateParams,
        sex: Sex,
        age_months_at_start: u32,
        survival: &[f64],
        data: &Data,
    ) -> HealthChain {
        let age_years = age_months_at_start / 12;
        let pi0 = params.prevalence(sex, age_years, data);
        let months = survival.len().saturating_sub(1);
        let years = months.div_ceil(12).clamp(1, MAX_YEARS);
        let s_at = |m: usize| survival.get(m).copied().unwrap_or(0.0);
        let mut chain = HealthChain {
            pi0,
            hazard: Vec::with_capacity(years),
            trans: Vec::with_capacity(years),
            oop: Vec::with_capacity(years),
            ltc: Vec::with_capacity(years),
            pi: Vec::with_capacity(years),
        };
        let mut pi = pi0;
        for k in 0..years {
            let s0 = s_at(12 * k);
            let s1 = s_at(12 * (k + 1));
            let q = if s0 > 0.0 { 1.0 - s1 / s0 } else { 1.0 };
            let age = (age_months_at_start + 12 * k as u32) / 12;
            let h = params.state_hazards(q.clamp(0.0, 1.0), &pi);
            let oop_mean: f64 = (0..N_STATES).map(|s| pi[s] * params.oop[s]).sum();
            let mut oop = [1.0; N_STATES];
            for s in 0..N_STATES {
                oop[s] = params.oop[s] / oop_mean;
            }
            let dying: f64 = (0..N_STATES).map(|s| pi[s] * h[s]).sum();
            let mut ltc = [1.0; N_STATES];
            if dying > 0.0 {
                let mean: f64 = (0..N_STATES)
                    .map(|s| pi[s] * h[s] / dying * LTC_STATE_FACTOR[s])
                    .sum();
                for s in 0..N_STATES {
                    ltc[s] = LTC_STATE_FACTOR[s] / mean;
                }
            }
            let p = params.transition(age);
            chain.pi.push(pi);
            chain.hazard.push(h);
            chain.trans.push(p);
            chain.oop.push(oop);
            chain.ltc.push(ltc);
            pi = params.step(&pi, q.clamp(0.0, 1.0), age).0;
        }
        chain
    }

    /// Number of years covered.
    pub fn years(&self) -> usize {
        self.hazard.len()
    }

    /// Distribution over states at the start.
    pub fn initial(&self) -> [f64; N_STATES] {
        self.pi0
    }

    /// State hazards of year `k`.
    pub fn hazards(&self, k: usize) -> [f64; N_STATES] {
        self.hazard[k.min(self.hazard.len() - 1)]
    }

    /// Distribution over states of the living at the start of year `k`.
    pub fn living(&self, k: usize) -> [f64; N_STATES] {
        self.pi[k.min(self.pi.len() - 1)]
    }

    /// Out-of-pocket multiplier of a state in year `k`.
    #[inline]
    pub fn oop_factor(&self, k: usize, level: u8) -> f64 {
        self.oop[k.min(self.oop.len() - 1)][usize::from(level).min(N_STATES - 1)]
    }

    /// Care-incidence multiplier of the state at death in year `k`.
    pub fn ltc_factor(&self, k: usize, level: u8) -> f64 {
        self.ltc[k.min(self.ltc.len() - 1)][usize::from(level).min(N_STATES - 1)]
    }

    /// Draws the person's state in every year up to the year of death, given that
    /// they die at the start of month `months_to_death` (alive in months
    /// `0..months_to_death`), into `out[0..=death_year]`; later entries repeat the
    /// last state. Uses one uniform per year up to the death year. Returns the death
    /// year.
    pub fn sample(&self, months_to_death: u32, rng: &mut Rng, out: &mut [u8; MAX_YEARS]) -> usize {
        let last = self.years() - 1;
        let death_year = ((months_to_death.max(1) - 1) / 12) as usize;
        let death_year = death_year.min(last);
        // Backward pass: beta[k][s] = P(survive years k..K-1, die in K | state s in
        // k), rescaled each year (only ratios matter).
        let mut beta = [[0.0f64; N_STATES]; MAX_YEARS];
        beta[death_year] = self.hazard[death_year];
        for k in (0..death_year).rev() {
            let h = &self.hazard[k];
            let p = &self.trans[k];
            let mut b = [0.0; N_STATES];
            let mut top = 0.0f64;
            for s in 0..N_STATES {
                let mut ahead = 0.0;
                for n in 0..N_STATES {
                    ahead += p[s][n] * beta[k + 1][n];
                }
                b[s] = (1.0 - h[s]) * ahead;
                top = top.max(b[s]);
            }
            if top > 0.0 {
                for x in &mut b {
                    *x /= top;
                }
            }
            beta[k] = b;
        }
        // Forward sampling.
        let pick = |weights: [f64; N_STATES], u: f64| -> u8 {
            let total: f64 = weights.iter().sum();
            if total <= 0.0 {
                return 0;
            }
            let mut acc = 0.0;
            for (s, w) in weights.iter().enumerate() {
                acc += w / total;
                if u < acc {
                    return s as u8;
                }
            }
            (N_STATES - 1) as u8
        };
        let mut w = [0.0; N_STATES];
        for s in 0..N_STATES {
            w[s] = self.pi0[s] * beta[0][s];
        }
        let mut state = pick(w, rng.uniform());
        out[0] = state;
        for k in 1..=death_year {
            let p = &self.trans[k - 1];
            for n in 0..N_STATES {
                w[n] = p[usize::from(state)][n] * beta[k][n];
            }
            state = pick(w, rng.uniform());
            out[k] = state;
        }
        for slot in out.iter_mut().skip(death_year + 1) {
            *slot = state;
        }
        death_year
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::MortalityModel;
    use crate::model::{HealthStatus, Longevity, YearMonth};
    use crate::rng::Stream;

    fn params() -> HealthStateParams {
        HealthStateParams::from_data(Data::embedded())
    }

    #[test]
    fn transitions_are_stochastic_matrices_with_the_documented_form() {
        let p = params();
        for age in [20, 50, 65, 80, 95, 110] {
            let m = p.transition(age);
            for row in m {
                assert!((row.iter().sum::<f64>() - 1.0).abs() < 1e-12);
                assert!(row.iter().all(|&x| (0.0..=1.0).contains(&x)));
            }
        }
        // At 50 onset is logistic(-4.1549) = 1 / (1 + e^4.1549) = 0.015445; at 80 frailty is
        // logistic(-2.7433) = 0.060466 and the direct healthy-to-frail rate a
        // quarter of it.
        let m50 = p.transition(50);
        assert!((m50[0][1] - 0.015445).abs() < 1e-6, "{}", m50[0][1]);
        let m80 = p.transition(80);
        assert!((m80[1][2] - 0.060466).abs() < 1e-6, "{}", m80[1][2]);
        assert!((m80[0][2] - 0.25 * 0.060466).abs() < 1e-6);
        assert_eq!(m80[2][1], FRAIL_RECOVERY);
        assert_eq!(m80[1][0], 0.0);
    }

    #[test]
    fn state_hazards_average_to_the_table_rate() {
        let p = params();
        // Hand check: pi = (0.5, 0.3, 0.2), r = (1, 1.73, 2.79):
        // sum pi r = 0.5 + 0.519 + 0.558 = 1.577; q = 0.02 gives healthy
        // 0.02 / 1.577 = 0.0126823.
        let pi = [0.5, 0.3, 0.2];
        let h = p.state_hazards(0.02, &pi);
        assert!((h[0] - 0.02 / 1.577).abs() < 1e-12, "{h:?}");
        assert!((h[2] / h[0] - 2.79).abs() < 1e-12);
        let mean: f64 = (0..3).map(|s| pi[s] * h[s]).sum();
        assert!((mean - 0.02).abs() < 1e-15);
        // Near the end of the table a frail hazard would pass 1: capped, and the
        // average is still the table's.
        let h = p.state_hazards(0.8, &pi);
        assert_eq!(h[2], 1.0);
        let mean: f64 = (0..3).map(|s| pi[s] * h[s]).sum();
        assert!((mean - 0.8).abs() < 1e-9, "{h:?} {mean}");
    }

    #[test]
    fn prevalence_matches_the_calibration_targets() {
        let p = params();
        let data = Data::embedded();
        // Survivor-weighted averages over age bands, both sexes averaged, as in the
        // fit recorded in params.json (fitted 30.5% / 67.1% multimorbid at 45-64 /
        // 65+, frail 8.3% at 65-69 and 34.7% at 90+).
        let band = |lo: u32, hi: u32, frail_only: bool| -> f64 {
            let mut total = 0.0;
            for sex in [Sex::Male, Sex::Female] {
                let (mut w, mut v) = (0.0, 0.0);
                let mut alive = 1.0;
                for age in 18..120u32 {
                    if age >= lo && age <= hi {
                        let pi = p.prevalence(sex, age, data);
                        w += alive;
                        v += alive * if frail_only { pi[2] } else { pi[1] + pi[2] };
                    }
                    alive *= 1.0 - data.life_table.qx(sex, age as usize);
                }
                total += v / w;
            }
            total / 2.0
        };
        assert!((band(45, 64, false) - 0.305).abs() < 0.005);
        assert!((band(65, 119, false) - 0.671).abs() < 0.005);
        assert!((band(65, 69, true) - 0.083).abs() < 0.005);
        assert!((band(90, 119, true) - 0.347).abs() < 0.01);
    }

    fn model(age: i32, sex: Sex) -> MortalityModel {
        let start = YearMonth::ym(2026, 1);
        MortalityModel::new(
            YearMonth::ym(2026 - age, 1),
            sex,
            HealthStatus::Good,
            start,
            &Longevity::default(),
            Data::embedded(),
        )
    }

    #[test]
    fn chain_keeps_mortality_and_mean_costs() {
        let p = params();
        let m = model(60, Sex::Female);
        let chain = HealthChain::new(
            &p,
            Sex::Female,
            60 * 12,
            m.survival_table(),
            Data::embedded(),
        );
        let s = m.survival_table();
        for k in 0..chain.years() - 1 {
            let pi = chain.living(k);
            let h = chain.hazards(k);
            let q = 1.0 - s.get(12 * (k + 1)).copied().unwrap_or(0.0) / s[12 * k];
            let mean: f64 = (0..3).map(|i| pi[i] * h[i]).sum();
            assert!((mean - q).abs() < 1e-9, "year {k}: {mean} vs {q}");
            let oop: f64 = (0..3).map(|i| pi[i] * chain.oop_factor(k, i as u8)).sum();
            assert!((oop - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn sampled_states_reproduce_the_chain_and_precede_death() {
        // Simulated with the real death draws: the share frail among the living at
        // each age equals the chain's prediction, and the year of death is far more
        // often frail than the population of the same age.
        let p = params();
        let m = model(65, Sex::Male);
        let chain = HealthChain::new(&p, Sex::Male, 65 * 12, m.survival_table(), Data::embedded());
        let n = 40_000;
        let (mut alive20, mut frail20, mut deaths, mut frail_at_death) = (0.0, 0.0, 0.0, 0.0);
        let mut frail_living_expected = 0.0;
        let mut out = [0u8; MAX_YEARS];
        for path in 0..n {
            let mut lr = Rng::stream(5, path, Stream::Longevity(0));
            let d = m.draw_months_to_death(&mut lr);
            let mut hr = Rng::stream(5, path, Stream::HealthStates(0));
            let year = chain.sample(d, &mut hr, &mut out);
            if year >= 20 {
                alive20 += 1.0;
                frail20 += f64::from(u8::from(out[20] == 2));
            }
            if (15..=25).contains(&year) {
                deaths += 1.0;
                frail_at_death += f64::from(u8::from(out[year] == 2));
                frail_living_expected += chain.living(year)[2];
            }
        }
        let expected = chain.living(20)[2];
        let got = frail20 / alive20;
        assert!((got - expected).abs() < 0.015, "{got} vs {expected}");
        let at_death = frail_at_death / deaths;
        let living = frail_living_expected / deaths;
        assert!(at_death > 1.4 * living, "{at_death} vs {living}");
    }
}
