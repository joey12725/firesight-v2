//! What links lifespans: the partner's (`longevity.couple_dependence: linked`) and
//! the household's income (`longevity.income_gradient`).
//!
//! # Linked lifespans
//!
//! Two parts, both keeping each person's own survival curve **exactly**:
//!
//! * **Shared frailty** (long-term dependence): the two lifetimes are coupled by a
//!   Frank copula on their longevity uniforms, with the association fitted by
//!   Dufresne et al. (2018) to the joint-life data of Frees, Carriere and Valdez
//!   (1996). A copula leaves each margin as it was.
//! * **Bereavement** (short-term dependence): a mixture. With probability `w_j`
//!   the path is one where person `j` dies within [`BEREAVEMENT_MONTHS`] of the
//!   other, the pair drawn with mass `excess * p_i(m) * p_j(n)` (the independent
//!   chance of that pair raised by the relative risk of Moon et al. 2011 minus 1);
//!   otherwise the copula draws both from *residual* curves, the person's own
//!   density with the bereavement components' share taken out. The residual is
//!   never negative, so each person's margin is their own curve exactly (see
//!   [`CoupleModel::with`]). The raised risk is computed against the independent
//!   margins: the copula already makes close deaths a little more likely, so the
//!   total effect is somewhat above the relative risk.
//!
//! Draws: the first uniform of each person's `Longevity` stream (the one the
//! independent model uses) and two uniforms of the new `Stream::CoupleLongevity`.
//!
//! # Income gradient
//!
//! Mortality is multiplied by a factor that moves the period life expectancy at 40
//! by the gap between the household's income percentile and the average in Chetty
//! et al. (2016). It is an association: higher income goes with longer lives, for
//! many reasons.

use crate::data::Data;
use crate::math;
use crate::model::{IncomeKind, Longevity, Profile, Sex};

/// Months after a partner's death over which the survivor's mortality is raised
/// (`params.json` `health_mortality_care.couple_dependence.bereavement_months`).
pub const BEREAVEMENT_MONTHS: u32 = 6;

/// Draws one uniform-free month count from a survival curve: the number of
/// leading entries above `u` (as `MortalityModel::draw_months_to_death`).
fn months_from(curve: &[f64], u: f64) -> u32 {
    curve.partition_point(|&s| s > u) as u32
}

/// `v` given `u` under a Frank copula with association `alpha`, by inverting the
/// conditional distribution at `w` (Nelsen, An Introduction to Copulas, 4.2.5).
pub fn frank_conditional(alpha: f64, u: f64, w: f64) -> f64 {
    if alpha.abs() < 1e-9 {
        return w;
    }
    let a = math::exp(-alpha * u);
    let g = math::exp(-alpha) - 1.0;
    let x = w * g / (w + (1.0 - w) * a);
    (-math::ln_1p(x) / alpha).clamp(0.0, 1.0)
}

/// Probability mass of dying at the start of month `n` (`n >= 1`) from a survival
/// curve.
fn pmf(curve: &[f64]) -> Vec<f64> {
    let mut p = vec![0.0; curve.len()];
    for n in 1..curve.len() {
        p[n] = (curve[n - 1] - curve[n]).max(0.0);
    }
    p
}

/// Survival curve of a probability mass function over months.
fn curve_of(p: &[f64]) -> Vec<f64> {
    let total: f64 = p.iter().sum();
    let mut out = Vec::with_capacity(p.len());
    out.push(1.0);
    let mut gone = 0.0;
    for &x in &p[1..] {
        gone += x / total;
        out.push((1.0 - gone).clamp(0.0, 1.0));
    }
    if let Some(last) = out.last_mut() {
        *last = 0.0;
    }
    // Non-increasing despite rounding.
    for i in 1..out.len() {
        if out[i] > out[i - 1] {
            out[i] = out[i - 1];
        }
    }
    out
}

/// The compiled couple model.
#[derive(Clone, Debug, PartialEq)]
pub struct CoupleModel {
    alpha: f64,
    /// Probability of the path on which person `j` dies within the window after
    /// the other (`w[j]`).
    w: [f64; 2],
    /// Residual survival curves of the copula component.
    residual: [Vec<f64>; 2],
    /// For the component "`j` dies soon after the other": survival-style curve of
    /// the other's death month, indexed in the other's months.
    lead: [Vec<f64>; 2],
}

/// `S(m) - S(m + window)`: probability of dying in months `m + 1 ..= m + window`.
fn window_mass(curve: &[f64], m: usize) -> f64 {
    let s = |k: usize| curve.get(k).copied().unwrap_or(0.0);
    (s(m) - s(m + BEREAVEMENT_MONTHS as usize)).max(0.0)
}

impl CoupleModel {
    /// Builds the model from both people's survival curves (`survival[m]` = alive
    /// `m` months after the start), with the association and bereavement risk of
    /// `params.json` `health_mortality_care.couple_dependence`.
    pub fn new(curves: [&[f64]; 2], data: &Data) -> CoupleModel {
        let c = &data.actuarial["health_mortality_care"]["couple_dependence"];
        let num = |k: &str| {
            c[k].as_f64()
                .unwrap_or_else(|| panic!("embedded data: couple_dependence.{k}"))
        };
        assert_eq!(num("bereavement_months") as u32, BEREAVEMENT_MONTHS);
        Self::with(
            num("frank_alpha"),
            num("bereavement_relative_risk") - 1.0,
            curves,
        )
    }

    /// The model with an explicit association and excess risk (for tests).
    ///
    /// The bereavement components have joint mass
    /// `E_j(m, n) = excess * p_i(m) * p_j(n)` for `1 <= n - m <= window` (`j` dies
    /// `n`, within the window after the other's death at `m`): the independent
    /// chance of that pair raised by the excess, at every age alike, which is what
    /// a relative risk means. Each person's residual density is then
    /// `p_j(n) (1 - excess (a_j(n) + b_j(n))) / (1 - w_0 - w_1)`, with `a_j(n)` the
    /// chance that the other died in the window before `n` and `b_j(n)` that they
    /// die in the window after it. Both are probabilities of at most six months of
    /// the other's life, so the residual is never negative and each margin is the
    /// person's own curve exactly.
    pub fn with(alpha: f64, excess: f64, curves: [&[f64]; 2]) -> CoupleModel {
        let excess = excess.max(0.0);
        let p = [pmf(curves[0]), pmf(curves[1])];
        let window = BEREAVEMENT_MONTHS as usize;
        // lead[j][m]: mass of the other's death at m with j following.
        let lead_mass = |j: usize| -> Vec<f64> {
            let i = 1 - j;
            (0..curves[i].len())
                .map(|m| excess * p[i][m] * window_mass(curves[j], m))
                .collect()
        };
        let lead_raw = [lead_mass(0), lead_mass(1)];
        let w = [
            lead_raw[0].iter().sum::<f64>(),
            lead_raw[1].iter().sum::<f64>(),
        ];
        let rest = 1.0 - w[0] - w[1];
        let residual = [0, 1].map(|j| {
            let i = 1 - j;
            let g: Vec<f64> = (0..curves[j].len())
                .map(|n| {
                    // a: the other died in n - window ..= n - 1.
                    let lo = n.saturating_sub(window);
                    let a: f64 = (lo..n).map(|m| p[i].get(m).copied().unwrap_or(0.0)).sum();
                    let b = window_mass(curves[i], n);
                    (p[j][n] * (1.0 - excess * (a + b))).max(0.0) / rest
                })
                .collect();
            curve_of(&g)
        });
        let lead = [0, 1].map(|j| {
            if w[j] > 0.0 {
                curve_of(&lead_raw[j])
            } else {
                vec![1.0, 0.0]
            }
        });
        CoupleModel {
            alpha,
            w,
            residual,
            lead,
        }
    }

    /// The two death months (`months_to_death` of each person) from the first
    /// uniform of each person's longevity stream, `u`, and two uniforms of the
    /// couple stream: `pick` chooses the component, `delay` the months between the
    /// deaths on a bereavement path.
    ///
    /// Also returns which person, if any, was drawn as dying in the bereavement
    /// window after the other.
    pub fn draw(
        &self,
        curves: [&[f64]; 2],
        u: [f64; 2],
        pick: f64,
        delay: f64,
    ) -> ([u32; 2], Option<usize>) {
        let follower = if pick < self.w[1] {
            Some(1)
        } else if pick < self.w[1] + self.w[0] {
            Some(0)
        } else {
            None
        };
        let Some(j) = follower else {
            let v = frank_conditional(self.alpha, u[0], u[1]);
            return (
                [
                    months_from(&self.residual[0], u[0]),
                    months_from(&self.residual[1], v),
                ],
                None,
            );
        };
        let i = 1 - j;
        let m = months_from(&self.lead[j], u[i]);
        // The follower's month within the window, in proportion to their own
        // chance of dying in each month of it.
        let total = window_mass(curves[j], m as usize);
        let s = |k: usize| curves[j].get(k).copied().unwrap_or(0.0);
        let target = delay * total;
        let mut acc = 0.0;
        let mut n = m + BEREAVEMENT_MONTHS;
        for k in 1..=BEREAVEMENT_MONTHS {
            let at = (m + k) as usize;
            acc += (s(at - 1) - s(at)).max(0.0);
            if target < acc {
                n = m + k;
                break;
            }
        }
        let n = n.min((curves[j].len() - 1) as u32);
        let mut out = [0; 2];
        out[i] = m;
        out[j] = n;
        (out, Some(j))
    }

    /// Probability of each bereavement component (`[j]`: person `j` dies soon after
    /// the other).
    pub fn bereavement_weights(&self) -> [f64; 2] {
        self.w
    }
}

/// Period life expectancy at exact age 40 on the life table of `sex` with
/// mortality multiplied by `m`.
fn e40(sex: Sex, m: f64, data: &Data) -> f64 {
    let mut alive = 1.0;
    let mut sum = 0.5;
    for age in 40..=data.life_table.max_age() {
        alive *= 1.0 - (data.life_table.qx(sex, age) * m).min(1.0);
        sum += alive;
    }
    sum
}

/// The mortality multiplier that moves the period life expectancy at 40 of `sex`
/// by `gap` years (positive: longer lives).
pub fn multiplier_for_gap(sex: Sex, gap: f64, data: &Data) -> f64 {
    let target = e40(sex, 1.0, data) + gap;
    let (mut lo, mut hi) = (-4.0f64, 4.0f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if e40(sex, math::exp(mid), data) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    math::exp(0.5 * (lo + hi))
}

/// Chetty et al. (2016) table for one sex: (household income in 2012 dollars,
/// life expectancy at 40) by percentile 1..=100.
fn chetty(sex_key: &str, data: &Data) -> (Vec<f64>, Vec<f64>) {
    let t = &data.actuarial["health_mortality_care"]["income_gradient"][sex_key];
    let col = |k: &str| -> Vec<f64> {
        t[k].as_array()
            .unwrap_or_else(|| panic!("embedded data: income_gradient.{sex_key}.{k}"))
            .iter()
            .map(|v| v.as_f64().expect("embedded data: income_gradient number"))
            .collect()
    };
    (col("hh_income_2012"), col("le40_race_adjusted"))
}

/// Percentile (1 to 100) of a household income in today's dollars in the
/// Chetty et al. distribution of one sex, interpolated between percentile means.
pub fn income_percentile(income_today: f64, sex_key: &str, data: &Data) -> f64 {
    let g = &data.actuarial["health_mortality_care"]["income_gradient"];
    let cpi = g["cpi_2012_to_2026_09"]
        .as_f64()
        .expect("embedded data: income_gradient.cpi_2012_to_2026_09");
    let x = income_today / cpi;
    let (inc, _) = chetty(sex_key, data);
    if x <= inc[0] {
        return 1.0;
    }
    for i in 1..inc.len() {
        if x <= inc[i] {
            return i as f64 + (x - inc[i - 1]) / (inc[i] - inc[i - 1]);
        }
    }
    100.0
}

/// The life-expectancy gap at 40 of a percentile against the mean over
/// percentiles, for one sex (interpolated).
pub fn le40_gap(percentile: f64, sex_key: &str, data: &Data) -> f64 {
    let (_, le) = chetty(sex_key, data);
    let mean = le.iter().sum::<f64>() / le.len() as f64;
    let p = percentile.clamp(1.0, 100.0) - 1.0;
    let i = (p.floor() as usize).min(le.len() - 2);
    let f = p - i as f64;
    le[i] * (1.0 - f) + le[i + 1] * f - mean
}

/// Household income in today's dollars that the gradient ranks: the gross pay of
/// the jobs and the net self-employment income running at the start (the paper
/// ranks pre-tax household earnings; for people past 61 it uses the income they
/// had at 61 to 63, which a profile of retirees does not hold). `None` when no
/// earnings run at the start.
pub fn household_income(profile: &Profile) -> Option<f64> {
    let ctx = profile.when_context()?;
    let mut total = 0.0;
    for income in &profile.incomes {
        let started = match &income.start {
            None => true,
            Some(w) => w.resolve(&ctx).is_some_and(|at| at <= ctx.start),
        };
        let ended = income
            .end
            .as_ref()
            .and_then(|w| w.resolve(&ctx))
            .is_some_and(|at| at <= ctx.start);
        if !started || ended {
            continue;
        }
        total += match &income.kind {
            IncomeKind::Job(j) => {
                j.salary * (1.0 + j.bonus.as_ref().map_or(0.0, |b| b.target_fraction))
                    + j.commission.as_ref().map_or(0.0, |c| c.expected_annual)
            }
            IncomeKind::SelfEmployment(s) => s.net_annual.max(0.0),
            _ => 0.0,
        };
    }
    (total > 0.0).then_some(total)
}

/// Mortality multiplier of person `i` from the income gradient; exactly 1 when it
/// is off.
pub fn income_multiplier(profile: &Profile, i: usize, data: &Data) -> f64 {
    let Longevity::Stochastic {
        income_gradient: true,
        income_percentile: fixed,
        ..
    } = profile.assumptions.longevity
    else {
        return 1.0;
    };
    let sex = profile.household.people[i].sex;
    let one = |sex: Sex, key: &str| -> f64 {
        let p = match (fixed, household_income(profile)) {
            (Some(p), _) => p,
            (None, Some(x)) => income_percentile(x, key, data),
            (None, None) => 50.0,
        };
        multiplier_for_gap(sex, le40_gap(p, key, data), data)
    };
    match sex {
        Sex::Female => one(Sex::Female, "female"),
        Sex::Male => one(Sex::Male, "male"),
        Sex::Unspecified => 0.5 * (one(Sex::Female, "female") + one(Sex::Male, "male")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::MortalityModel;
    use crate::model::{HealthStatus, YearMonth};
    use crate::rng::{Rng, Stream};

    fn curve(age: i32, sex: Sex) -> Vec<f64> {
        MortalityModel::new(
            YearMonth::ym(2026 - age, 1),
            sex,
            HealthStatus::Good,
            YearMonth::ym(2026, 1),
            &Longevity::default(),
            Data::embedded(),
        )
        .survival_table()
        .to_vec()
    }

    #[test]
    fn income_gradient_parts_match_hand_values() {
        let d = Data::embedded();
        // Chetty et al. table: men p1 72.74, p100 87.34, mean over percentiles
        // 81.2613; women p1 78.78, p100 88.87, mean 84.9234.
        assert!((le40_gap(100.0, "male", d) - (87.34 - 81.2613)).abs() < 1e-9);
        assert!((le40_gap(1.0, "male", d) - (72.74 - 81.2613)).abs() < 1e-9);
        assert!((le40_gap(1.0, "female", d) - (78.78 - 84.9234)).abs() < 1e-9);
        // Out of range is clamped.
        assert_eq!(le40_gap(250.0, "male", d), le40_gap(100.0, "male", d));
        // Percentile of an income: the 50th percentile's mean (62,567 in 2012
        // dollars for men) at today's prices is exactly 50.
        let cpi = 1.4676;
        assert!((income_percentile(62_567.0 * cpi, "male", d) - 50.0).abs() < 1e-9);
        assert_eq!(income_percentile(100.0, "male", d), 1.0);
        assert_eq!(income_percentile(1e9, "male", d), 100.0);
        // The multiplier moves period e40 by the gap: 1 for no gap, below 1 for a
        // longer life, and the target is hit.
        for sex in [Sex::Male, Sex::Female] {
            assert!((multiplier_for_gap(sex, 0.0, d) - 1.0).abs() < 1e-9);
            let m = multiplier_for_gap(sex, 3.0, d);
            assert!(m < 1.0 && m > 0.5, "{m}");
            assert!((e40(sex, m, d) - e40(sex, 1.0, d) - 3.0).abs() < 1e-6);
            let m = multiplier_for_gap(sex, -8.0, d);
            assert!(m > 1.5, "{m}");
            assert!((e40(sex, m, d) - e40(sex, 1.0, d) + 8.0).abs() < 1e-6);
        }
    }

    #[test]
    fn income_multiplier_is_exactly_one_when_off_and_ranks_earnings() {
        let d = Data::embedded();
        let mut p = Profile::example();
        assert_eq!(income_multiplier(&p, 0, d), 1.0);
        let earnings = household_income(&p);
        p.assumptions.longevity = Longevity::Stochastic {
            improvement: true,
            couple_dependence: crate::model::CoupleDependence::Independent,
            income_gradient: true,
            income_percentile: None,
        };
        let ranked = income_multiplier(&p, 0, d);
        let key = match p.household.people[0].sex {
            Sex::Female => "female",
            _ => "male",
        };
        if let Some(x) = earnings {
            let pct = income_percentile(x, key, d);
            let want = multiplier_for_gap(p.household.people[0].sex, le40_gap(pct, key, d), d);
            if p.household.people[0].sex != Sex::Unspecified {
                assert_eq!(ranked, want);
            }
        }
        // A fixed percentile overrides the ranking: top decile lives longer.
        if let Longevity::Stochastic {
            income_percentile, ..
        } = &mut p.assumptions.longevity
        {
            *income_percentile = Some(95.0);
        }
        let top = income_multiplier(&p, 0, d);
        if let Longevity::Stochastic {
            income_percentile, ..
        } = &mut p.assumptions.longevity
        {
            *income_percentile = Some(5.0);
        }
        let bottom = income_multiplier(&p, 0, d);
        assert!(top < 1.0 && bottom > 1.0, "{top} {bottom}");
    }

    #[test]
    fn frank_conditional_inverts_the_copula() {
        // Independence at alpha 0; the symmetric point maps to itself.
        assert_eq!(frank_conditional(0.0, 0.3, 0.7), 0.7);
        assert!((frank_conditional(2.0, 0.5, 0.5) - 0.5).abs() < 1e-12);
        // Hand value: alpha 2, u 0.2, w 0.5: a = e^-0.4 = 0.670320, g = e^-2 - 1 =
        // -0.864665, x = 0.5 g / (0.5 + 0.5 a) = -0.517665, v = -ln(1 + x) / 2 =
        // 0.364557.
        let v = frank_conditional(2.0, 0.2, 0.5);
        assert!((v - 0.364557).abs() < 1e-5, "{v}");
        // Check against the copula: dC/du at (u, v) is w. C(u, v) = -ln(1 + (e^-au
        // - 1)(e^-av - 1)/(e^-a - 1)) / a.
        let c = |u: f64, v: f64| {
            let a = 2.0f64;
            -math::ln(
                1.0 + (math::exp(-a * u) - 1.0) * (math::exp(-a * v) - 1.0) / (math::exp(-a) - 1.0),
            ) / a
        };
        let h = 1e-6;
        let d = (c(0.2 + h, v) - c(0.2 - h, v)) / (2.0 * h);
        assert!((d - 0.5).abs() < 1e-6, "{d}");
    }

    #[test]
    fn kendall_tau_of_draws_matches_the_fitted_association() {
        // Frank tau = 1 - 4/a (1 - D1(a)), D1(a) = (1/a) int_0^a t / (e^t - 1) dt.
        let a = 7.065f64;
        let n_int = 100_000;
        let mut d1 = 0.0;
        for k in 0..n_int {
            let t = (k as f64 + 0.5) * a / n_int as f64;
            d1 += t / (math::exp(t) - 1.0) * a / n_int as f64;
        }
        d1 /= a;
        let tau = 1.0 - 4.0 / a * (1.0 - d1);
        assert!((tau - 0.56).abs() < 0.01, "{tau}");
        let mut rng = Rng::stream(3, 0, Stream::CoupleLongevity);
        let pairs: Vec<(f64, f64)> = (0..2_000)
            .map(|_| {
                let u = rng.uniform();
                (u, frank_conditional(a, u, rng.uniform()))
            })
            .collect();
        let mut concordant = 0.0;
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                let s = (pairs[i].0 - pairs[j].0) * (pairs[i].1 - pairs[j].1);
                concordant += s.signum();
            }
        }
        let n = pairs.len() as f64;
        let sample = concordant / (n * (n - 1.0) / 2.0);
        assert!((sample - tau).abs() < 0.03, "{sample} vs {tau}");
    }

    /// The mixture's margin of person `j`, exactly, from the compiled parts.
    fn margin(m: &CoupleModel, curves: [&[f64]; 2], j: usize) -> Vec<f64> {
        let i = 1 - j;
        let rest = 1.0 - m.w[0] - m.w[1];
        let g = pmf(&m.residual[j]);
        let mut out: Vec<f64> = g.iter().map(|x| rest * x).collect();
        let p = [pmf(curves[0]), pmf(curves[1])];
        // j follows: the joint mass excess p_i(m) p_j(n), n in the window after m;
        // its total over m at n is j's share.
        let excess = 0.41;
        let window = BEREAVEMENT_MONTHS as usize;
        for (mo, &pi) in p[i].iter().enumerate() {
            for k in 1..=window {
                if let Some(&pj) = p[j].get(mo + k) {
                    out[mo + k] += excess * pi * pj;
                }
            }
        }
        // j leads: j dies at n, the other in the window after.
        for (n, &pj) in p[j].iter().enumerate() {
            out[n] += excess * pj * window_mass(curves[i], n);
        }
        out
    }

    #[test]
    fn the_lead_and_delay_draws_reproduce_the_joint_mass() {
        // Hand-sized curves: person 0 dies in month 1, 2 or 3 with 0.2 / 0.5 / 0.3;
        // person 1 in month 1..=4 with 0.25 each. Window 6 covers everything after.
        let c0 = [1.0, 0.8, 0.3, 0.0];
        let c1 = [1.0, 0.75, 0.5, 0.25, 0.0];
        let m = CoupleModel::with(0.0, 0.41, [&c0, &c1]);
        // w[1] = 0.41 * sum_m p0(m) S1(m) = 0.41 (0.2 x 0.75 + 0.5 x 0.5 + 0.3 x 0.25)
        //      = 0.41 x 0.475 = 0.19475.
        assert!((m.w[1] - 0.19475).abs() < 1e-12, "{:?}", m.w);
        // w[0] = 0.41 * sum_m p1(m) S0(m) = 0.41 x 0.25 (0.8 + 0.3) = 0.11275.
        assert!((m.w[0] - 0.11275).abs() < 1e-12);
        // Person 1 follows a month-2 death of person 0: months 3 and 4 equally.
        let lead_u = 0.5; // inside the month-2 band of the lead curve
        let ([d0, d1], f) = m.draw([&c0, &c1], [lead_u, 0.0], 0.0, 0.2);
        assert_eq!((d0, d1, f), (2, 3, Some(1)));
        let ([_, d1], _) = m.draw([&c0, &c1], [lead_u, 0.0], 0.0, 0.7);
        assert_eq!(d1, 4);
    }

    #[test]
    fn each_persons_own_curve_is_kept_exactly() {
        for (a, b) in [
            ((65, Sex::Male), (63, Sex::Female)),
            ((80, Sex::Female), (52, Sex::Male)),
        ] {
            let (c0, c1) = (curve(a.0, a.1), curve(b.0, b.1));
            let curves = [c0.as_slice(), c1.as_slice()];
            let m = CoupleModel::new(curves, Data::embedded());
            assert!(m.w[0] > 0.0 && m.w[1] > 0.0);
            for j in 0..2 {
                let want = pmf(curves[j]);
                let got = margin(&m, curves, j);
                let worst = want
                    .iter()
                    .zip(&got)
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0, f64::max);
                assert!(worst < 1e-12, "{a:?} {b:?} person {j}: {worst}");
            }
        }
    }

    fn simulate(m: &CoupleModel, curves: [&[f64]; 2], n: u64) -> (f64, f64, f64, f64) {
        let (mut s0, mut s1, mut close, mut s01) = (0.0, 0.0, 0.0, 0.0);
        for path in 0..n {
            let u = [0, 1].map(|i| Rng::stream(9, path, Stream::Longevity(i)).uniform());
            let mut r = Rng::stream(9, path, Stream::CoupleLongevity);
            let ([d0, d1], _) = m.draw(curves, u, r.uniform(), r.uniform());
            let (x, y) = (f64::from(d0), f64::from(d1));
            s0 += x;
            s1 += y;
            s01 += x * y;
            let gap = d0.abs_diff(d1);
            close += f64::from(u8::from((1..=BEREAVEMENT_MONTHS).contains(&gap)));
        }
        let n = n as f64;
        (s0 / n, s1 / n, close / n, s01 / n - s0 * s1 / (n * n))
    }

    #[test]
    fn bereavement_raises_deaths_soon_after_the_partners_by_the_relative_risk() {
        let (c0, c1) = (curve(70, Sex::Male), curve(68, Sex::Female));
        let curves = [c0.as_slice(), c1.as_slice()];
        // Without the copula, only the bereavement part: the chance that the second
        // death falls within six months of the first rises by about 41%.
        let plain = CoupleModel::with(0.0, 0.0, curves);
        let bereaved = CoupleModel::with(0.0, 0.41, curves);
        let n = 200_000;
        let (m0, m1, base, cov0) = simulate(&plain, curves, n);
        let (b0, b1, raised, _) = simulate(&bereaved, curves, n);
        let ratio = raised / base;
        assert!((ratio - 1.41).abs() < 0.06, "{ratio}: {base} -> {raised}");
        // Months of life unchanged (each margin is exact; sampling noise only).
        assert!((b0 / m0 - 1.0).abs() < 0.01 && (b1 / m1 - 1.0).abs() < 0.01);
        assert!(cov0.abs() / (m0 * m1) < 0.01);
    }

    #[test]
    fn the_fitted_model_links_lifespans_strongly() {
        let (c0, c1) = (curve(65, Sex::Male), curve(63, Sex::Female));
        let curves = [c0.as_slice(), c1.as_slice()];
        let m = CoupleModel::new(curves, Data::embedded());
        let n = 100_000;
        let (m0, m1, _, cov) = simulate(&m, curves, n);
        let indep = CoupleModel::with(0.0, 0.0, curves);
        let (i0, i1, _, _) = simulate(&indep, curves, n);
        assert!((m0 / i0 - 1.0).abs() < 0.01 && (m1 / i1 - 1.0).abs() < 0.01);
        // Correlation of the months of life: positive and sizeable (a Frank copula
        // with tau 0.56 gives a Pearson correlation of about 0.7 on uniforms, a little
        // less on lifetimes).
        let mut v0 = 0.0;
        let mut v1 = 0.0;
        for path in 0..20_000 {
            let u = [0, 1].map(|i| Rng::stream(9, path, Stream::Longevity(i)).uniform());
            let mut r = Rng::stream(9, path, Stream::CoupleLongevity);
            let ([d0, d1], _) = m.draw(curves, u, r.uniform(), r.uniform());
            v0 += math::powi(f64::from(d0) - m0, 2);
            v1 += math::powi(f64::from(d1) - m1, 2);
        }
        let corr = cov / ((v0 / 20_000.0).sqrt() * (v1 / 20_000.0).sqrt());
        assert!(corr > 0.5 && corr < 0.8, "{corr}");
    }
}
