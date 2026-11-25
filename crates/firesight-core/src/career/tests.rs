use super::*;
use crate::model::{Bonus, CareerRisk, Commission, EquityGrant};
use crate::rng::Stream;

const START: YearMonth = YearMonth::ym(2026, 1);

fn data() -> &'static Data {
    Data::embedded()
}

fn job(salary: f64, growth: WageGrowth, level: RiskLevel) -> Job {
    Job {
        salary,
        growth,
        risk: CareerRisk { level },
        ..Job::default()
    }
}

fn flat() -> WageGrowth {
    WageGrowth::Flat { real_raise: 0.0 }
}

/// Climate for month `t` since the start: flat prices, no recession, flat market.
fn climate(t: u32, age0: f64) -> CareerClimate {
    CareerClimate {
        now: START.add_months(t as i32),
        age: age0 + f64::from(t) / 12.0,
        price_level: 1.0,
        recession: 0,
        equity_return: 0.0,
        equity_return_12m: 0.0,
    }
}

fn rngs(path: u64) -> (Rng, Rng) {
    (
        Rng::stream(7, path, Stream::Career(0)),
        Rng::stream(7, path, Stream::EquityComp),
    )
}

/// Standard normal CDF (Abramowitz and Stegun 7.1.26, error below 1.5e-7).
fn phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * (x.abs() / std::f64::consts::SQRT_2));
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * math::exp(-x * x / 2.0);
    if x >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

/// Mean of a lognormal(ln median, sigma) clipped to `[lo, hi]`.
fn clipped_lognormal_mean(median: f64, sigma: f64, lo: f64, hi: f64) -> f64 {
    let mu = math::ln(median);
    let a = (math::ln(lo) - mu) / sigma;
    let b = (math::ln(hi) - mu) / sigma;
    lo * phi(a)
        + math::exp(mu + 0.5 * sigma * sigma) * (phi(b - sigma) - phi(a - sigma))
        + hi * (1.0 - phi(b))
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn close_rel(actual: f64, expected: f64, relative: f64) {
    close(actual, expected, expected.abs() * relative);
}

// ---------------------------------------------------------------- parameters

#[test]
fn parameters_match_the_research_file() {
    let p = CareerParams::from_data(data());
    assert_eq!(p.cgm_college, [0.3194, -0.0577, 0.0033]);
    assert_eq!(p.cgm_high_school, [0.1682, -0.0323, 0.002]);
    assert_eq!(p.rho, 0.97);
    assert_eq!(p.shock_sigma, [[0.05, 0.05], [0.1, 0.1], [0.17, 0.25]]);
    assert_eq!(p.base_hazard, 0.005);
    assert_eq!(p.hazard_age_multiplier, [2.0, 1.3, 1.0, 0.9, 0.9, 0.9]);
    assert_eq!(p.security_multiplier, [0.4, 1.0, 2.0]);
    assert_eq!(p.recession_hazard_multiplier, 1.6);
    assert_eq!(p.spell_median, [2.2, 4.0]);
    assert_eq!(p.spell_sigma, [1.0, 1.0]);
    assert_eq!(p.spell_age_multiplier, [0.85, 1.0, 1.4]);
    assert_eq!(p.scar_mean, [0.08, 0.15]);
    assert_eq!(p.scar_sd, 0.15);
    assert_eq!(p.scar_older_multiplier, 1.5);
    assert_eq!(p.ui_replacement_rate, 0.5);
    assert_eq!(p.ui_weekly_cap, 600.0);
    assert_eq!(p.ui_max_weeks, 26.0);
    assert_eq!(p.ui_recession_extension_weeks, 13.0);
    // Sorted by failure probability: late stage, series B-C, seed.
    assert_eq!(p.private_stages[0], [0.3, 0.45, 0.22, 0.03]);
    assert_eq!(p.private_stages[1], [0.5, 0.3, 0.16, 0.04]);
    assert_eq!(p.private_stages[2], [0.7, 0.2, 0.08, 0.02]);
    assert_eq!(
        p.private_multiple_ranges,
        [[0.1, 1.0], [1.0, 4.0], [4.0, 20.0]]
    );
    // US stocks: roughly 9-10% a year in logs with 18-19% volatility.
    assert!((0.07..0.12).contains(&p.equity_log_mean_12m));
    assert!((0.15..0.22).contains(&p.equity_log_sd_12m));
}

#[test]
fn lifecycle_curves_match_published_ratios() {
    let d = data();
    for profile in [
        LifecycleProfile::Flat,
        LifecycleProfile::Typical,
        LifecycleProfile::Steep,
    ] {
        assert_eq!(lifecycle_factor(profile, 40.0, d), 1.0);
    }
    let ratio = |p| lifecycle_factor(p, 55.0, d) / lifecycle_factor(p, 25.0, d);
    // params.json, implied_ratio_age55_to_age25: college 2.03; high school 1.36.
    // Hand computation for college: 0.3194*30 - 0.0577*240 + 0.0033*1507.5 = 0.70875.
    close(ratio(LifecycleProfile::Steep), math::exp(0.70875), 1e-9);
    close(ratio(LifecycleProfile::Steep), 2.03, 0.005);
    // Typical is the average of the two log profiles: sqrt(2.03 * 1.36) = 1.66.
    close(
        ratio(LifecycleProfile::Typical),
        math::exp(0.5 * (0.70875 + 0.309)),
        1e-9,
    );
    close(ratio(LifecycleProfile::Typical), 1.66, 0.005);
    assert_eq!(ratio(LifecycleProfile::Flat), 1.0);
    // Growth at 25 on the college profile (table: 9.28% in logs).
    // Hand computation of the log slope: 0.3194 - 2*0.0577*2.5 + 3*0.0033*6.25.
    let g25 = math::ln(
        lifecycle_factor(LifecycleProfile::Steep, 25.5, d)
            / lifecycle_factor(LifecycleProfile::Steep, 24.5, d),
    );
    close(g25, 0.0928, 0.0005);
    // The polynomial is held flat outside 20-65.
    assert_eq!(
        lifecycle_factor(LifecycleProfile::Steep, 80.0, d),
        lifecycle_factor(LifecycleProfile::Steep, 65.0, d)
    );
    assert_eq!(
        lifecycle_factor(LifecycleProfile::Steep, 10.0, d),
        lifecycle_factor(LifecycleProfile::Steep, 20.0, d)
    );
}

#[test]
fn layoff_hazards_follow_the_table() {
    let d = data();
    close(
        layoff_hazard(RiskLevel::Typical, 40.0, false, d),
        0.005,
        1e-15,
    );
    close(
        layoff_hazard(RiskLevel::Typical, 40.0, true, d),
        0.008,
        1e-15,
    );
    close(
        layoff_hazard(RiskLevel::Typical, 22.0, false, d),
        0.010,
        1e-15,
    );
    close(
        layoff_hazard(RiskLevel::Typical, 30.0, false, d),
        0.0065,
        1e-15,
    );
    close(
        layoff_hazard(RiskLevel::Typical, 60.0, true, d),
        0.0072,
        1e-15,
    );
    close(layoff_hazard(RiskLevel::Low, 40.0, false, d), 0.002, 1e-15);
    close(layoff_hazard(RiskLevel::High, 40.0, true, d), 0.016, 1e-15);
    assert_eq!(layoff_hazard(RiskLevel::None, 40.0, true, d), 0.0);
    // The research's check: 5.8% chance of at least one spell a year at 40.
    close(1.0 - math::powi(1.0 - 0.005, 12), 0.058, 0.0005);

    let m = JobModel::new(&job(1.0, flat(), RiskLevel::High), 30.0, true, d);
    close(
        m.layoff_probability(30.0, true),
        0.005 * 1.3 * 2.0 * 1.6,
        1e-15,
    );
    let off = JobModel::new(&job(1.0, flat(), RiskLevel::High), 30.0, false, d);
    assert_eq!(off.layoff_probability(30.0, true), 0.0);
}

// ---------------------------------------------------------------- wages

#[test]
fn risk_off_pay_follows_the_curve_exactly() {
    let d = data();
    let growth = WageGrowth::Lifecycle {
        profile: LifecycleProfile::Steep,
        extra: 0.01,
    };
    // Level `Typical` with career risk disabled, and level `None` with it enabled.
    for (level, enabled) in [(RiskLevel::Typical, false), (RiskLevel::None, true)] {
        let m = JobModel::new(&job(120_000.0, growth, level), 30.0, enabled, d);
        let mut s = m.initial_state();
        let (mut career, mut equity) = rngs(1);
        for t in 0..=360u32 {
            let mut c = climate(t, 30.0);
            c.price_level = 1.0 + 0.002 * f64::from(t);
            c.recession = u8::from(t % 50 < 10);
            let pay = m.step(&mut s, &c, &mut career, &mut equity);
            let years = f64::from(t) / 12.0;
            let expected = 120_000.0 * lifecycle_factor(LifecycleProfile::Steep, 30.0 + years, d)
                / lifecycle_factor(LifecycleProfile::Steep, 30.0, d)
                * math::pow(1.01, years);
            close_rel(s.real_salary, expected, 1e-12);
            close_rel(m.expected_real_salary(c.age), expected, 1e-12);
            close_rel(pay.wages, expected / 12.0 * c.price_level, 1e-12);
            assert_eq!(pay.gross(), pay.wages);
            assert!(!pay.laid_off && pay.unemployment_benefit == 0.0);
        }
        assert_eq!(s.months_unemployed_total, 0.0);
    }
}

#[test]
fn flat_raise_compounds() {
    let m = JobModel::new(
        &job(
            50_000.0,
            WageGrowth::Flat { real_raise: 0.02 },
            RiskLevel::None,
        ),
        45.0,
        true,
        data(),
    );
    close_rel(
        m.expected_real_salary(55.0),
        50_000.0 * math::powi(1.02, 10),
        1e-12,
    );
    close_rel(m.curve_factor(46.0), 1.02, 1e-12);
    // Before the start age the entered salary is returned.
    assert_eq!(m.expected_real_salary(30.0), 50_000.0);
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(2);
    let mut pay = JobMonth::default();
    for t in 0..=120 {
        pay = m.step(&mut s, &climate(t, 45.0), &mut career, &mut equity);
    }
    close_rel(pay.wages * 12.0, 50_000.0 * math::powi(1.02, 10), 1e-12);
}

/// Mean real salary over 100,000 paths, with Gaussian wage shocks on.
///
/// With layoffs removed the shocks are mean-preserving, so the mean follows the
/// deterministic curve. With layoffs on (typical risk, age 30 start, no recessions)
/// the mean base salary of everyone, employed or not, is 2.5% below the curve after
/// 5 years, 4% after 10 and 6.5% after 20, from re-employment scars that fade only
/// at the AR(1) rate. About 4.2 months of the 20 years are spent out of work, and
/// total pay received over the 20 years is 5.7% below the deterministic total.
#[test]
fn mean_wage_follows_the_curve_and_job_loss_lowers_it() {
    const PATHS: u64 = 100_000;
    let d = data();
    let growth = WageGrowth::Lifecycle {
        profile: LifecycleProfile::Typical,
        extra: 0.0,
    };
    let with_layoffs = JobModel::new(&job(80_000.0, growth, RiskLevel::Typical), 30.0, true, d);
    let mut no_layoffs = with_layoffs.clone();
    no_layoffs.hazard = [[0.0; 6]; 2];

    let checkpoints = [5u32, 10, 20];
    let run = |m: &JobModel, years: u32| {
        let mut sums = [0.0; 3];
        let mut paid = 0.0;
        let mut idle = 0.0;
        for path in 0..PATHS {
            let (mut career, mut equity) = rngs(path);
            let mut s = m.initial_state();
            for t in 0..=years * 12 {
                let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
                paid += pay.wages;
                for (sum, y) in sums.iter_mut().zip(checkpoints) {
                    if t == y * 12 {
                        *sum += s.real_salary;
                    }
                }
            }
            idle += s.months_unemployed_total;
        }
        (
            sums.map(|x| x / PATHS as f64),
            paid / PATHS as f64,
            idle / PATHS as f64,
        )
    };

    let (means, paid, idle) = run(&no_layoffs, 20);
    assert_eq!(idle, 0.0);
    let mut deterministic_paid = 0.0;
    for t in 0..=240u32 {
        deterministic_paid += no_layoffs.expected_real_salary(30.0 + f64::from(t) / 12.0) / 12.0;
    }
    for (mean, y) in means.iter().zip(checkpoints) {
        // Standard error after 20 years is about 0.12% of the mean.
        close_rel(
            *mean,
            no_layoffs.expected_real_salary(30.0 + f64::from(y)),
            0.005,
        );
    }
    close_rel(paid, deterministic_paid, 0.003);

    let (scarred, paid_with_loss, idle) = run(&with_layoffs, 20);
    let ratio = |i: usize, y: f64| scarred[i] / with_layoffs.expected_real_salary(30.0 + y);

    // Measured at this seed: 0.975, 0.960, 0.935.
    assert!((0.965..0.985).contains(&ratio(0, 5.0)), "{}", ratio(0, 5.0));
    assert!((0.95..0.97).contains(&ratio(1, 10.0)), "{}", ratio(1, 10.0));
    assert!((0.92..0.95).contains(&ratio(2, 20.0)), "{}", ratio(2, 20.0));
    // Hazard 0.65% then 0.5% a month times a mean spell of about 3.1 to 3.6 months:
    // about 4.2 months out of work in 20 years. Pay received is 94.3% of the
    // deterministic total.
    assert!((3.8..4.6).contains(&idle), "{idle}");
    let paid_ratio = paid_with_loss / deterministic_paid;
    assert!((0.93..0.955).contains(&paid_ratio), "{paid_ratio}");
}

#[test]
fn wage_shock_dispersion_matches_the_ar1() {
    // Low risk: sigma_eta = sigma_eps = 0.05, rho = 0.97. After 10 Januaries the log
    // salary has variance 0.05^2 * (1 - 0.97^20) / (1 - 0.97^2) + 0.05^2.
    let mut m = JobModel::new(&job(1.0, flat(), RiskLevel::Low), 40.0, true, data());
    m.hazard = [[0.0; 6]; 2];
    let expected_var = 0.0025 * (1.0 - math::powi(0.97, 20)) / (1.0 - 0.97 * 0.97) + 0.0025;
    let n = 20_000u64;
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    for path in 0..n {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        for t in 0..=120 {
            m.step(&mut s, &climate(t, 40.0), &mut career, &mut equity);
        }
        close(s.persistent_variance, expected_var - 0.0025, 1e-12);
        let l = math::ln(s.real_salary);
        sum += l;
        sum_sq += l * l;
    }
    let mean = sum / n as f64;
    let var = sum_sq / n as f64 - mean * mean;
    close_rel(var, expected_var, 0.04);
    // Mean of the log is minus half the variance (the mean-preserving correction).
    close(mean, -0.5 * expected_var, 0.004);
}

// ---------------------------------------------------------------- job loss

/// Runs one job for `months` at a fixed age and recession state and returns
/// (layoffs, months that started employed, months unemployed).
fn layoff_statistics(level: RiskLevel, age: f64, recession: u8, months: u32) -> (f64, f64, f64) {
    let m = JobModel::new(&job(60_000.0, flat(), level), age, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(u64::from(recession) + age as u64);
    let mut c = climate(1, age);
    c.recession = recession;
    let (mut layoffs, mut exposed) = (0.0, 0.0);
    for _ in 0..months {
        if s.employment == Employment::Employed {
            exposed += 1.0;
        }
        // Keep wages from drifting to zero under thousands of scars.
        s.persistent_shock = 0.0;
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        if pay.laid_off {
            layoffs += 1.0;
        }
    }
    (layoffs, exposed, s.months_unemployed_total)
}

#[test]
fn layoff_incidence_matches_the_hazards_by_age_and_cycle() {
    // (level, age, recession, expected monthly hazard)
    let cases = [
        (RiskLevel::Typical, 22.0, 0, 0.005 * 2.0),
        (RiskLevel::Typical, 40.0, 0, 0.005),
        (RiskLevel::Typical, 40.0, 1, 0.005 * 1.6),
        (RiskLevel::High, 30.0, 1, 0.005 * 1.3 * 2.0 * 1.6),
        (RiskLevel::Low, 60.0, 0, 0.005 * 0.9 * 0.4),
    ];
    for (level, age, recession, hazard) in cases {
        let (layoffs, exposed, _) = layoff_statistics(level, age, recession, 1_500_000);
        let rate = layoffs / exposed;
        // Binomial standard error is at most 2.3% of the rate here; allow 3 sigma.
        let se = (hazard * (1.0 - hazard) / exposed).sqrt();
        close(rate, hazard, 3.5 * se);
    }
}

#[test]
fn average_spell_length_matches_the_clipped_lognormal() {
    // Expansion, age 40: median 2.2 months, sigma 1 (unclipped mean 3.63).
    // Recession regime, age 60: median 4.0 * 1.4 = 5.6 months.
    // Expansion, age 25: median 2.2 * 0.85.
    let cases = [(40.0, 0, 2.2), (60.0, 1, 5.6), (25.0, 0, 1.87)];
    for (age, recession, median) in cases {
        let (layoffs, _, idle) = layoff_statistics(RiskLevel::High, age, recession, 1_500_000);
        let expected = clipped_lognormal_mean(median, 1.0, SPELL_MIN_MONTHS, SPELL_MAX_MONTHS);
        assert!(layoffs > 8_000.0);
        // Spell sd is about 1.2 times the mean; 3 standard errors is about 4%.
        close_rel(idle / layoffs, expected, 0.045);
    }
    // The research's headline numbers: 3.6 months in expansions, 6.6 in recessions.
    close(clipped_lognormal_mean(2.2, 1.0, 0.25, 36.0), 3.6, 0.05);
    close(clipped_lognormal_mean(4.0, 1.0, 0.25, 36.0), 6.4, 0.15);
}

#[test]
fn recession_regime_lasts_a_year_after_the_recession() {
    let mut m = JobModel::new(
        &job(60_000.0, flat(), RiskLevel::Typical),
        40.0,
        true,
        data(),
    );
    m.hazard = [[1.0; 6]; 2];
    // Layoff forced `gap` months after the last recession month.
    let ui_weeks = |gap: u32| {
        let mut m2 = m.clone();
        m2.hazard = [[0.0; 6]; 2];
        let mut s = m2.initial_state();
        let (mut career, mut equity) = rngs(3);
        let mut c = climate(1, 40.0);
        c.recession = 1;
        m2.step(&mut s, &c, &mut career, &mut equity);
        c.recession = 0;
        for _ in 1..gap {
            m2.step(&mut s, &c, &mut career, &mut equity);
        }
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        assert!(pay.laid_off);
        // Weeks granted = weeks left + weeks already paid this month.
        let idle = (s.months_unemployed_total).min(1.0);
        s.ui_weeks_left + (idle * WEEKS_PER_MONTH - UI_WAIT_WEEKS).max(0.0)
    };
    close(ui_weeks(12), 39.0, 1e-9);
    close(ui_weeks(13), 26.0, 1e-9);
}

#[test]
fn unemployment_insurance_and_scar_arithmetic() {
    let m = JobModel::new(
        &job(52_000.0, flat(), RiskLevel::Typical),
        40.0,
        true,
        data(),
    );
    let mut s = m.initial_state();
    // An eight-month spell with a 0.10 log scar, benefit 50% of $1,000 a week.
    s.employment = Employment::Unemployed { months: 0 };
    s.spell_months_left = 8.0;
    s.pending_scar = 0.10;
    s.ui_weekly_real = 500.0;
    s.ui_weeks_left = 26.0;
    s.ui_wait_weeks_left = 1.0;
    let (mut career, mut equity) = rngs(4);
    let mut c = climate(1, 40.0); // February onwards: no January shock in 8 months
    c.price_level = 1.2;
    let wpm = 52.0 / 12.0;
    let mut benefits = Vec::new();
    for i in 0..8 {
        c.now = START.add_months(1 + i);
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        benefits.push(pay.unemployment_benefit);
        assert_eq!(pay.wages, 0.0);
        assert_eq!(pay.rehired, i == 7);
    }
    // Month 1: 4.33 weeks less the waiting week. Months 2-6: 4.33 weeks each, which
    // brings the total to 25 weeks; month 7 pays the 26th week; month 8 nothing.
    close(benefits[0], (wpm - 1.0) * 500.0 * 1.2, 1e-9);
    for b in &benefits[1..6] {
        close(*b, wpm * 500.0 * 1.2, 1e-9);
    }
    close(benefits[6], 500.0 * 1.2, 1e-6);
    close(benefits[7], 0.0, 1e-9);
    close(benefits.iter().sum::<f64>(), 26.0 * 500.0 * 1.2, 1e-6);
    assert_eq!(s.employment, Employment::Employed);
    close(s.months_unemployed_total, 8.0, 1e-12);
    // Re-employed at exp(-0.10) of the old salary.
    close(s.persistent_shock, -0.10, 1e-12);
    close_rel(s.real_salary, 52_000.0 * math::exp(-0.10), 1e-12);
    c.now = START.add_months(9);
    let pay = m.step(&mut s, &c, &mut career, &mut equity);
    if !pay.laid_off {
        close_rel(pay.wages, 52_000.0 * math::exp(-0.10) / 12.0 * 1.2, 1e-12);
    }
}

#[test]
fn layoff_sets_benefit_spell_and_scar() {
    let d = data();
    let p = CareerParams::from_data(d);
    // $156,000: half of $3,000 a week exceeds the $600 cap. $41,600: $400 a week.
    for (salary, weekly) in [(156_000.0, 600.0), (41_600.0, 400.0)] {
        let mut m = JobModel::new(&job(salary, flat(), RiskLevel::Typical), 52.0, true, d);
        m.hazard = [[1.0; 6]; 2];
        let mut s = m.initial_state();
        // Reproduce the draws the step will make.
        let (mut career, mut equity) = rngs(5);
        let mut peek = career.clone();
        let _ = peek.uniform();
        let z_spell = peek.normal_one_draw();
        let z_scar = peek.normal_one_draw();
        let pay = m.step(&mut s, &climate(1, 52.0), &mut career, &mut equity);
        assert!(pay.laid_off);
        let spell = (2.2 * math::exp(z_spell)).clamp(0.25, 36.0);
        let idle = spell.min(1.0);
        close(s.spell_months_left + idle, spell, 1e-12);
        assert_eq!(s.ui_weekly_real, weekly);
        // Age 52: mean loss 0.08 * 1.5, plus 0.01 per month beyond six (max 0.10).
        let mean = 0.08 * 1.5 + (0.01 * (spell - 6.0).max(0.0)).min(0.10);
        let scar = mean + p.scar_sd * z_scar;
        if pay.rehired {
            close(s.persistent_shock, -scar, 1e-12);
        } else {
            close(s.pending_scar, scar, 1e-12);
        }
        close(
            pay.unemployment_benefit,
            weekly * (idle * WEEKS_PER_MONTH - 1.0).max(0.0),
            1e-9,
        );
        close(
            pay.wages,
            salary / 12.0 * (1.0 - idle) * s.shock_factor,
            1e-9,
        );
    }
}

#[test]
fn mean_scar_matches_the_research() {
    // Expansion, age 40: mean log loss 0.08 plus the long-spell add-on.
    let mut m = JobModel::new(
        &job(60_000.0, flat(), RiskLevel::Typical),
        40.0,
        true,
        data(),
    );
    m.hazard = [[1.0; 6]; 2];
    let n = 40_000u64;
    let mut sum = 0.0;
    let mut addon = 0.0;
    for path in 0..n {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        m.step(&mut s, &climate(1, 40.0), &mut career, &mut equity);
        let spell = s.spell_months_left + s.months_unemployed_total;
        addon += (0.01 * (spell - 6.0).max(0.0)).min(0.10);
        sum += s.pending_scar - s.persistent_shock;
    }
    let n = n as f64;
    close(sum / n, 0.08 + addon / n, 0.003);
    assert!((0.003..0.02).contains(&(addon / n)));
}

// ---------------------------------------------------------------- bonus, commission, breaks

#[test]
fn bonus_is_paid_in_its_month_and_moves_with_the_market() {
    let d = data();
    let p = CareerParams::from_data(d);
    let mut j = job(100_000.0, flat(), RiskLevel::None);
    j.bonus = Some(Bonus {
        target_fraction: 0.2,
        volatility: 0.0,
        month: 3,
    });
    // Guaranteed bonus: exactly 20% of salary, in March only.
    let m = JobModel::new(&j, 40.0, true, d);
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(6);
    for t in 0..24 {
        let mut c = climate(t, 40.0);
        c.price_level = 1.5;
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        if c.now.month() == 3 {
            close(pay.bonus, 0.2 * 100_000.0 * 1.5, 1e-9);
        } else {
            assert_eq!(pay.bonus, 0.0);
        }
    }

    // Volatile bonus: unit mean when the market signal is standard normal, sd 30% of
    // target, correlation 0.5 with the signal.
    j.bonus = Some(Bonus {
        target_fraction: 0.2,
        volatility: 0.3,
        month: 3,
    });
    let m = JobModel::new(&j, 40.0, true, d);
    let mut market = Rng::from_seed(99);
    let n = 200_000;
    let (mut sum, mut sum_sq, mut cross) = (0.0, 0.0, 0.0);
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(7);
    for _ in 0..n {
        let signal = market.normal();
        let mut c = climate(2, 40.0);
        c.equity_return_12m = math::exp_m1(p.equity_log_mean_12m + p.equity_log_sd_12m * signal);
        let factor = m.step(&mut s, &c, &mut career, &mut equity).bonus / 20_000.0;
        sum += factor;
        sum_sq += factor * factor;
        cross += math::ln(factor) * signal;
    }
    let n = f64::from(n);
    let mean = sum / n;
    let sd = (sum_sq / n - mean * mean).sqrt();
    close(mean, 1.0, 0.003);
    close(sd, 0.3, 0.006);
    let log_sd = math::ln_1p(0.09).sqrt();
    close(cross / n / log_sd, 0.5, 0.01);

    // With career risk switched off the bonus is the target.
    let off = JobModel::new(&j, 40.0, false, d);
    let mut s = off.initial_state();
    let mut c = climate(2, 40.0);
    c.equity_return_12m = -0.4;
    close(
        off.step(&mut s, &c, &mut career, &mut equity).bonus,
        20_000.0,
        1e-9,
    );
}

#[test]
fn commission_has_the_entered_annual_volatility() {
    let mut j = job(60_000.0, flat(), RiskLevel::None);
    j.commission = Some(Commission {
        expected_annual: 24_000.0,
        volatility: 0.4,
    });
    let m = JobModel::new(&j, 40.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(8);
    let years = 30_000;
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    for _ in 0..years {
        let mut annual = 0.0;
        for t in 1..=12 {
            let pay = m.step(&mut s, &climate(t, 40.0), &mut career, &mut equity);
            assert!(pay.commission >= 0.0);
            annual += pay.commission;
        }
        sum += annual;
        sum_sq += annual * annual;
    }
    let mean = sum / f64::from(years);
    let sd = (sum_sq / f64::from(years) - mean * mean).sqrt();
    close_rel(mean, 24_000.0, 0.01);
    close_rel(sd / mean, 0.4, 0.06);

    // No noise when career risk is off: exactly 2,000 a month, CPI-indexed.
    let off = JobModel::new(&j, 40.0, false, data());
    let mut s = off.initial_state();
    let mut c = climate(5, 40.0);
    c.price_level = 1.1;
    close(
        off.step(&mut s, &c, &mut career, &mut equity).commission,
        2_200.0,
        1e-9,
    );
}

#[test]
fn career_break_pays_a_fraction_then_resumes() {
    let m = JobModel::new(&job(120_000.0, flat(), RiskLevel::None), 35.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(9);
    m.start_break(&mut s, 3, 0.6);
    assert_eq!(s.employment, Employment::OnBreak { months_left: 3 });
    for t in 0..3 {
        let pay = m.step(&mut s, &climate(t, 35.0), &mut career, &mut equity);
        close(pay.wages, 6_000.0, 1e-9);
    }
    assert_eq!(s.employment, Employment::Employed);
    let pay = m.step(&mut s, &climate(3, 35.0), &mut career, &mut equity);
    close(pay.wages, 10_000.0, 1e-9);
    // Unpaid break; and a break cannot start when not employed.
    m.start_break(&mut s, 1, -2.0);
    assert_eq!(
        m.step(&mut s, &climate(4, 35.0), &mut career, &mut equity)
            .wages,
        0.0
    );
    m.end(&mut s);
    m.start_break(&mut s, 5, 1.0);
    assert_eq!(s.employment, Employment::Ended);
    m.start_break(&mut s, 0, 1.0);
    assert_eq!(s.employment, Employment::Ended);
}

#[test]
fn salary_events_and_job_lifecycle() {
    let growth = WageGrowth::Flat { real_raise: 0.03 };
    let m = JobModel::new(
        &job(100_000.0, growth, RiskLevel::Typical),
        30.0,
        true,
        data(),
    );
    let mut s = m.initial_state_not_started();
    let (mut career, mut equity) = rngs(10);
    // Not started: nothing is paid and the job cannot be lost.
    for t in 0..24 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        assert_eq!(pay, JobMonth::default());
    }
    assert_eq!(s.employment, Employment::NotStarted);
    // It starts two years in, at 32: the first pay is the salary entered, not the
    // salary grown from the plan's start, and shocks drawn before it began are gone.
    s.persistent_shock = 0.3;
    m.begin(&mut s, 32.0);
    assert_eq!(s.employment, Employment::Employed);
    assert_eq!(s.real_salary, m.salary);
    assert_eq!(s.persistent_shock, 0.0);
    close_rel(s.anchor * s.curve, m.salary, 1e-12);

    // Set salary: exact now, shocks wiped, then follows the curve from here.
    s.persistent_shock = -0.4;
    m.set_real_salary(&mut s, 150_000.0);
    assert_eq!(s.real_salary, 150_000.0);
    assert_eq!(s.persistent_shock, 0.0);
    let mut no_layoff = m.clone();
    no_layoff.hazard = [[0.0; 6]; 2];
    let pay = no_layoff.step(&mut s, &climate(24, 30.0), &mut career, &mut equity);
    // Month 24 is a January: shocks are redrawn, so compare through the anchor.
    close_rel(s.anchor * s.curve, 150_000.0, 1e-12);
    close_rel(pay.wages, s.real_salary / 12.0, 1e-12);
    close_rel(
        m.projected_real_salary(&s, 42.0),
        s.real_salary * math::powi(1.03, 10),
        1e-9,
    );

    // Scale keeps the shocks.
    let before = s;
    m.scale_real_salary(&mut s, 0.8);
    close_rel(s.real_salary, before.real_salary * 0.8, 1e-12);
    assert_eq!(s.persistent_shock, before.persistent_shock);

    // Ended: nothing more is paid.
    m.end(&mut s);
    for t in 25..40 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        assert_eq!(pay, JobMonth::default());
    }
    // Zero and negative salaries do not produce NaN.
    let z = JobModel::new(&job(0.0, growth, RiskLevel::High), 30.0, true, data());
    let mut s = z.initial_state();
    for t in 0..200 {
        let pay = z.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        assert_eq!(pay.wages, 0.0);
        assert!(pay.unemployment_benefit == 0.0);
    }
    // Ages far beyond the table are clamped, not out of bounds.
    let mut c = climate(0, 500.0);
    c.price_level = 1e9;
    let pay = z.step(&mut s, &c, &mut career, &mut equity);
    assert!(pay.gross().is_finite());
}

// ---------------------------------------------------------------- equity

fn grant(value: f64, months: u32, cliff: u32, company: EquityCompany) -> EquityGrant {
    EquityGrant {
        id: "g".into(),
        kind: EquityKind::Rsu,
        company,
        unvested_value: value,
        vest_months_remaining: months,
        cliff_months: cliff,
        refresh_annual_value: 0.0,
        sell_on_vest: true,
    }
}

/// A listed stock whose price never moves (no beta, no volatility).
fn still() -> EquityCompany {
    EquityCompany::Public {
        volatility: 0.0,
        beta: 0.0,
    }
}

fn equity_job(grants: Vec<EquityGrant>) -> Job {
    Job {
        equity: grants,
        ..job(100_000.0, flat(), RiskLevel::None)
    }
}

#[test]
fn rsu_vesting_arithmetic_with_cliff() {
    // $48,000 over 48 months with a 12-month cliff: nothing for 11 months, $12,000 in
    // the 12th, then $1,000 a month, $48,000 in total.
    let m = JobModel::new(
        &equity_job(vec![grant(48_000.0, 48, 12, still())]),
        30.0,
        true,
        data(),
    );
    let mut s = m.initial_state();
    close(m.unvested_value(&s), 48_000.0, 1e-9);
    let (mut career, mut equity) = rngs(11);
    let mut total = 0.0;
    for t in 0..60 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        let expected = match t + 1 {
            1..=11 => 0.0,
            12 => 12_000.0,
            13..=48 => 1_000.0,
            _ => 0.0,
        };
        close(pay.equity_income, expected, 1e-7);
        assert_eq!(pay.equity_retained, 0.0);
        close(pay.gross(), pay.wages + expected, 1e-7);
        total += pay.equity_income;
        close(m.unvested_value(&s), 48_000.0 - total, 1e-6);
    }
    close(total, 48_000.0, 1e-7);
    assert_eq!(s.unvested[0], 0.0);

    // No cliff: $1,000 from the first month. Kept shares are reported as retained.
    let mut g = grant(12_000.0, 12, 0, still());
    g.sell_on_vest = false;
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    for t in 0..12 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        close(pay.equity_income, 1_000.0, 1e-9);
        close(pay.equity_retained, 1_000.0, 1e-9);
    }
}

#[test]
fn vest_value_follows_the_share_price() {
    // Beta 1.5, no idiosyncratic risk: +10% market month then -20%.
    let company = EquityCompany::Public {
        volatility: 0.0,
        beta: 1.5,
    };
    let m = JobModel::new(
        &equity_job(vec![grant(24_000.0, 24, 0, company)]),
        30.0,
        true,
        data(),
    );
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(12);
    let mut c = climate(0, 30.0);
    c.equity_return = 0.10;
    close(
        m.step(&mut s, &c, &mut career, &mut equity).equity_income,
        1_000.0 * 1.15,
        1e-9,
    );
    c.equity_return = -0.20;
    close(
        m.step(&mut s, &c, &mut career, &mut equity).equity_income,
        1_000.0 * 1.15 * 0.70,
        1e-9,
    );
    close(s.stock_price, 1.15 * 0.70, 1e-12);
}

#[test]
fn public_stock_noise_is_mean_preserving() {
    let p = CareerParams::from_data(data());
    let company = EquityCompany::Public {
        volatility: 0.40,
        beta: 1.2,
    };
    let m = JobModel::new(
        &equity_job(vec![grant(1.0, 48, 0, company)]),
        30.0,
        true,
        data(),
    );
    let n = 40_000u64;
    let (mut sum, mut sum_l, mut sum_l2) = (0.0, 0.0, 0.0);
    for path in 0..n {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        for t in 0..12 {
            m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        }
        sum += s.stock_price;
        let l = math::ln(s.stock_price);
        sum_l += l;
        sum_l2 += l * l;
    }
    let n = n as f64;
    // Flat market: expected price 1; annual log sd is the idiosyncratic part,
    // sqrt(0.40^2 - (1.2 * sigma_market)^2).
    let idio = (0.16 - math::powi(1.2 * p.equity_log_sd_12m, 2)).sqrt();
    close(sum / n, 1.0, 0.006);
    let var = sum_l2 / n - (sum_l / n) * (sum_l / n);
    close_rel(var.sqrt(), idio, 0.02);
}

#[test]
fn refresh_grants_vest_over_four_years() {
    // No initial grant; $48,000 refresh each year: $1,000 a month per grant in
    // flight, so $1,000, $2,000, $3,000 then a steady $4,000 a month.
    let mut g = grant(0.0, 0, 0, still());
    g.refresh_annual_value = 48_000.0;
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(13);
    for t in 0..120u32 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        // Step t is month t+1; the grant made at month 12k vests in months 12k+1..12k+48.
        let month = t + 1;
        let in_flight = ((month - 1) / 12).min(4);
        close(pay.equity_income, 1_000.0 * f64::from(in_flight), 1e-7);
    }
    // 4 grants in flight just after a refresh: 48 + 36 + 24 + 12 months left.
    close(m.unvested_value(&s), 120_000.0, 1e-6);

    // Refresh grants are sized in today's dollars at the current share price: with
    // prices doubled a refresh is worth $96,000 nominal.
    let mut s = m.initial_state();
    for t in 0..13u32 {
        let mut c = climate(t, 30.0);
        c.price_level = 2.0;
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        close(pay.equity_income, if t == 12 { 2_000.0 } else { 0.0 }, 1e-7);
    }
}

#[test]
fn unvested_equity_is_forfeited_on_layoff_and_retirement() {
    let mut g = grant(48_000.0, 48, 0, still());
    g.refresh_annual_value = 48_000.0;
    let j = Job {
        equity: vec![g],
        ..job(100_000.0, flat(), RiskLevel::Typical)
    };
    let m = JobModel::new(&j, 30.0, true, data());
    let mut calm = m.clone();
    calm.hazard = [[0.0; 6]; 2];
    let mut certain = m.clone();
    certain.hazard = [[1.0; 6]; 2];

    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(14);
    for t in 0..20 {
        calm.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
    }
    assert!(calm.unvested_value(&s) > 60_000.0);
    let pay = certain.step(&mut s, &climate(20, 30.0), &mut career, &mut equity);
    assert!(pay.laid_off);
    assert_eq!(pay.equity_income, 0.0);
    assert_eq!(calm.unvested_value(&s), 0.0);
    // After re-employment nothing vests until the next refresh grant.
    let mut months_with_equity = 0;
    for t in 21..140 {
        let pay = calm.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        if pay.equity_income > 0.0 {
            months_with_equity += 1;
            assert!(s.employment == Employment::Employed);
        }
    }
    assert!(months_with_equity > 50);

    let mut s = m.initial_state();
    calm.step(&mut s, &climate(0, 30.0), &mut career, &mut equity);
    m.end(&mut s);
    assert_eq!(m.unvested_value(&s), 0.0);
    let pay = calm.step(&mut s, &climate(1, 30.0), &mut career, &mut equity);
    assert_eq!(pay.gross(), 0.0);
}

#[test]
fn options_pay_the_spread() {
    // 12,000 shares-worth vesting over 12 months, strike 40% of today's price.
    let mut g = grant(
        12_000.0,
        12,
        0,
        EquityCompany::Public {
            volatility: 0.0,
            beta: 1.0,
        },
    );
    g.kind = EquityKind::Option {
        strike_fraction: 0.4,
    };
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    close(m.unvested_value(&s), 12_000.0 * 0.6, 1e-9);
    let (mut career, mut equity) = rngs(15);
    let mut c = climate(0, 30.0);
    // Price 1.0: spread 0.6 a share.
    close(
        m.step(&mut s, &c, &mut career, &mut equity).equity_income,
        600.0,
        1e-9,
    );
    // Price rises 50% to 1.5: spread 1.1.
    c.equity_return = 0.5;
    close(
        m.step(&mut s, &c, &mut career, &mut equity).equity_income,
        1_100.0,
        1e-9,
    );
    // Price falls 80% to 0.3: under water, worth nothing.
    c.equity_return = -0.8;
    assert_eq!(
        m.step(&mut s, &c, &mut career, &mut equity).equity_income,
        0.0
    );
}

fn private(liquidity_hazard: f64, fail_prob: f64) -> EquityCompany {
    EquityCompany::Private {
        liquidity_hazard,
        fail_prob,
        exit_volatility: 1.0,
    }
}

#[test]
fn private_equity_pays_only_at_the_liquidity_event() {
    // Fully vested after 12 months; never fails. Find the event month on each path.
    let mut g = grant(12_000.0, 12, 0, private(0.5, 0.0));
    g.sell_on_vest = false;
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut exits_in_first_year = 0;
    let paths = 20_000u64;
    for path in 0..paths {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        assert_eq!(s.liquidity, Liquidity::Pending);
        let mut received = 0.0;
        let mut multiple = 0.0;
        for t in 0..240u32 {
            let mut c = climate(t, 30.0);
            // A recession in months 3-8 blocks liquidity events.
            c.recession = u8::from((3..9).contains(&t));
            let before = s.liquidity;
            let pay = m.step(&mut s, &c, &mut career, &mut equity);
            if before == Liquidity::Pending && s.liquidity == Liquidity::Exited {
                assert_eq!(c.recession, 0);
                multiple = s.stock_price;
                // Everything vested so far (t + 1 months, capped at 12) pays now.
                let vested = 1_000.0 * f64::from((t + 1).min(12));
                close_rel(pay.equity_income, vested * multiple, 1e-9);
                if t < 12 {
                    exits_in_first_year += 1;
                }
            } else if s.liquidity == Liquidity::Pending {
                assert_eq!(pay.equity_income, 0.0);
            }
            assert_eq!(pay.equity_retained, pay.equity_income);
            received += pay.equity_income;
        }
        if s.liquidity == Liquidity::Exited {
            assert!((0.1..=20.0).contains(&multiple));
            assert!(received > 0.0);
        }
    }
    // 50% a year, six of the first twelve months blocked: 1 - 0.5^(6/12) = 29.3%.
    close(
        f64::from(exits_in_first_year) / paths as f64,
        1.0 - 0.5f64.sqrt(),
        0.012,
    );
}

#[test]
fn private_outcome_frequencies_match_the_stage_table() {
    // (fail_prob, expected unconditional shares of modest / good / large)
    let cases = [
        (0.5, [0.30, 0.16, 0.04]),                   // series B to C row
        (0.3, [0.45, 0.22, 0.03]),                   // late stage row
        (0.7, [0.20, 0.08, 0.02]),                   // seed row
        (0.0, [0.45 / 0.7, 0.22 / 0.7, 0.03 / 0.7]), // late-stage mix, no failure
    ];
    for (fail_prob, expected) in cases {
        let g = grant(1_000.0, 1, 0, private(1.0, fail_prob));
        let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
        let n = 200_000u64;
        let mut counts = [0.0; 4];
        let mut total_multiple = 0.0;
        let (mut career, mut equity) = rngs(16);
        for _ in 0..n {
            let mut s = m.initial_state();
            // Hazard 1.0: the event happens in the first month.
            let pay = m.step(&mut s, &climate(0, 30.0), &mut career, &mut equity);
            let multiple = s.stock_price;
            total_multiple += multiple;
            match s.liquidity {
                Liquidity::Failed => {
                    assert_eq!(pay.equity_income, 0.0);
                    assert_eq!(multiple, 0.0);
                    counts[0] += 1.0;
                }
                Liquidity::Exited => {
                    close_rel(pay.equity_income, 1_000.0 * multiple, 1e-9);
                    let class = if multiple < 1.0 {
                        1
                    } else if multiple < 4.0 {
                        2
                    } else {
                        3
                    };
                    counts[class] += 1.0;
                }
                other => panic!("no event: {other:?}"),
            }
        }
        let n = n as f64;
        close(counts[0] / n, fail_prob, 0.004);
        for class in 0..3 {
            close(counts[class + 1] / n, expected[class], 0.004);
        }
        // Log-uniform multiples: mean (hi - lo) / ln(hi / lo) inside each class.
        let mean = expected[0] * 0.9 / math::ln(10.0)
            + expected[1] * 3.0 / math::ln(4.0)
            + expected[2] * 16.0 / math::ln(5.0);
        close_rel(total_multiple / n, mean, 0.02);
        if fail_prob == 0.5 {
            // The research's implied expected multiple for series B to C.
            close(mean, 0.86, 0.005);
        }
    }
    // Between rows the class mix is interpolated.
    let p = CareerParams::from_data(data());
    let (cumulative, _) = private_classes(&p, 0.4);
    close(cumulative[0], 0.5 * (0.45 / 0.7 + 0.30 / 0.5), 1e-12);
}

#[test]
fn failed_company_wipes_out_equity() {
    let mut g = grant(48_000.0, 48, 0, private(1.0, 1.0));
    g.refresh_annual_value = 10_000.0;
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(17);
    for t in 0..60 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        assert_eq!(pay.equity_income, 0.0);
        assert!(pay.wages > 0.0);
    }
    assert_eq!(s.liquidity, Liquidity::Failed);
    assert_eq!(m.unvested_value(&s), 0.0);
}

#[test]
fn private_options_pay_the_pooled_spread_at_exit() {
    // At-the-money options on a private company: only the gain above the strike pays.
    let mut g = grant(10_000.0, 1, 0, private(0.0, 0.0));
    g.kind = EquityKind::Option {
        strike_fraction: 1.0,
    };
    let j = equity_job(vec![g]);
    let m = JobModel::new(&j, 30.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(18);
    let pay = m.step(&mut s, &climate(0, 30.0), &mut career, &mut equity);
    assert_eq!(pay.equity_income, 0.0);
    close(s.illiquid_vested[0], 10_000.0, 1e-9);
    close(s.illiquid_strike_cost[0], 10_000.0, 1e-9);
    // The job ends; the vested options are kept and pay out when the company exits.
    m.end(&mut s);
    let mut exit = m.clone();
    if let Company::Private { monthly_hazard, .. } = &mut exit.company {
        *monthly_hazard = 1.0;
    }
    let pay = exit.step(&mut s, &climate(1, 30.0), &mut career, &mut equity);
    assert_eq!(s.liquidity, Liquidity::Exited);
    close(
        pay.equity_income,
        (10_000.0 * s.stock_price - 10_000.0).max(0.0),
        1e-9,
    );
    assert_eq!(pay.wages, 0.0);
}

#[test]
fn extra_grants_are_counted_as_dropped() {
    let grants = vec![grant(1_000.0, 10, 0, still()); MAX_GRANTS + 2];
    let m = JobModel::new(&equity_job(grants), 30.0, true, data());
    assert_eq!(m.grants_dropped(), 2);
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(19);
    let pay = m.step(&mut s, &climate(0, 30.0), &mut career, &mut equity);
    close(pay.equity_income, 100.0 * MAX_GRANTS as f64, 1e-9);
}

// ---------------------------------------------------------------- determinism

fn busy_job() -> Job {
    let mut g = grant(40_000.0, 48, 12, private(0.2, 0.5));
    g.refresh_annual_value = 20_000.0;
    Job {
        bonus: Some(Bonus::default()),
        commission: Some(Commission {
            expected_annual: 10_000.0,
            volatility: 0.4,
        }),
        equity: vec![g],
        ..job(90_000.0, WageGrowth::default(), RiskLevel::High)
    }
}

#[test]
fn same_seed_same_path() {
    let m = JobModel::new(&busy_job(), 28.0, true, data());
    let run = || {
        let (mut career, mut equity) = rngs(20);
        let mut s = m.initial_state();
        let mut out = Vec::new();
        for t in 0..480 {
            let mut c = climate(t, 28.0);
            c.recession = u8::from(t % 90 < 12);
            c.equity_return = 0.01 * f64::from(t % 7) - 0.02;
            c.equity_return_12m = 0.05 * f64::from(t % 5) - 0.1;
            out.push(m.step(&mut s, &c, &mut career, &mut equity));
        }
        (out, s)
    };
    let (a, sa) = run();
    let (b, sb) = run();
    assert_eq!(a, b);
    assert_eq!(sa, sb);
    assert!(a.iter().any(|p| p.laid_off));
    assert!(a.iter().all(|p| p.gross().is_finite() && p.gross() >= 0.0));
}

#[test]
fn draw_counts_do_not_depend_on_state() {
    let d = data();
    let risky = JobModel::new(&busy_job(), 28.0, true, d);
    let quiet = JobModel::new(&job(50_000.0, flat(), RiskLevel::None), 28.0, false, d);
    let (career0, equity0) = rngs(21);
    let mut streams: Vec<(Rng, Rng)> = Vec::new();
    // 0: risky job running; 1: risky job ended at once; 2: risky job on a break and
    // never started; 3: plain job with risk off; 4: risky job with a layoff forced
    // (by an employer's failure) every fifty months.
    for variant in 0..5 {
        let (mut career, mut equity) = (career0.clone(), equity0.clone());
        let m = if variant == 3 { &quiet } else { &risky };
        let mut s = if variant == 2 {
            m.initial_state_not_started()
        } else {
            m.initial_state()
        };
        if variant == 1 {
            m.end(&mut s);
        }
        for t in 0..300 {
            let mut c = climate(t, 28.0);
            c.recession = u8::from(t % 40 < 10);
            if variant == 2 && t == 100 {
                m.begin(&mut s, 30.0);
                m.start_break(&mut s, 30, 0.5);
            }
            if variant == 4 && t % 50 == 7 {
                s.force_layoff = true;
            }
            m.step(&mut s, &c, &mut career, &mut equity);
        }
        streams.push((career, equity));
    }
    let (mut career, mut equity) = streams.remove(0);
    let expected = (
        career.next_u64(),
        career.normal(),
        equity.next_u64(),
        equity.normal(),
    );
    for (mut career, mut equity) in streams {
        let got = (
            career.next_u64(),
            career.normal(),
            equity.next_u64(),
            equity.normal(),
        );
        assert_eq!(got, expected);
    }
}

#[test]
fn a_forced_layoff_behaves_like_any_layoff() {
    let d = data();
    // Risk off: the hazard is zero, so only the flag can end the job.
    let m = JobModel::new(&job(104_000.0, flat(), RiskLevel::None), 40.0, false, d);
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(31);
    for t in 0..6 {
        let pay = m.step(&mut s, &climate(t, 40.0), &mut career, &mut equity);
        assert!(!pay.laid_off);
    }
    // The spell and scar come from the same words an ordinary layoff would use.
    let mut peek = career.clone();
    let _ = peek.uniform();
    let z_spell = peek.normal_one_draw();
    s.force_layoff = true;
    let pay = m.step(&mut s, &climate(6, 40.0), &mut career, &mut equity);
    assert!(pay.laid_off && !s.force_layoff);
    let p = CareerParams::from_data(d);
    let spell = (p.spell_median[0]
        * p.spell_age_multiplier[spell_age_band(40.5)]
        * math::exp(p.spell_sigma[0] * z_spell))
    .clamp(SPELL_MIN_MONTHS, SPELL_MAX_MONTHS);
    close(s.spell_months_left + spell.min(1.0), spell, 1e-12);
    assert_eq!(
        s.ui_weekly_real,
        (p.ui_replacement_rate * 104_000.0 / WEEKS_PER_YEAR).min(p.ui_weekly_cap)
    );
    // Re-hired when the spell is over, with the scar, and not laid off again.
    let mut rehired = false;
    for t in 7..60 {
        let pay = m.step(&mut s, &climate(t, 40.0), &mut career, &mut equity);
        assert!(!pay.laid_off);
        rehired |= pay.rehired;
    }
    assert!(rehired);
    assert_eq!(s.employment, Employment::Employed);
    assert_eq!(s.pending_scar, 0.0);
    close(s.months_unemployed_total, spell, 1e-9);

    // The flag means nothing on a job that is already lost or has ended.
    let mut s = m.initial_state();
    m.end(&mut s);
    s.force_layoff = true;
    let pay = m.step(&mut s, &climate(0, 40.0), &mut career, &mut equity);
    assert!(!pay.laid_off && !s.force_layoff);
    assert_eq!(s.employment, Employment::Ended);
}

#[test]
fn a_forced_layoff_waits_for_a_career_break_to_end() {
    let m = JobModel::new(
        &job(120_000.0, flat(), RiskLevel::None),
        35.0,
        false,
        data(),
    );
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(32);
    m.start_break(&mut s, 3, 0.5);
    s.force_layoff = true;
    for t in 0..3 {
        let pay = m.step(&mut s, &climate(t, 35.0), &mut career, &mut equity);
        assert!(!pay.laid_off && s.force_layoff, "month {t}");
        close(pay.wages, 5_000.0, 1e-9);
    }
    // The break is over: the layoff happens in the first month back.
    let pay = m.step(&mut s, &climate(3, 35.0), &mut career, &mut equity);
    assert!(pay.laid_off && !s.force_layoff);
    assert!(matches!(
        s.employment,
        Employment::Unemployed { .. } | Employment::Employed
    ));
    assert!(s.months_unemployed_total > 0.0);
}

#[test]
fn the_old_private_grant_reports_its_payout_and_its_failure() {
    // A recession holds the exit back for six months; then it is certain, and the
    // company never fails.
    let g = grant(12_000.0, 12, 0, private(1.0, 0.0));
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(33);
    let (mut payout, mut income) = (0.0, 0.0);
    for t in 0..24 {
        let mut c = climate(t, 30.0);
        c.recession = u8::from(t < 6);
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        assert!(!pay.company_failed);
        assert!(pay.private_payout <= pay.equity_income);
        if pay.private_payout > 0.0 {
            assert_eq!(s.liquidity, Liquidity::Exited);
        }
        payout += pay.private_payout;
        income += pay.equity_income;
    }
    // One payout at the event; what vests afterwards is ordinary vesting income.
    assert!(payout > 0.0 && income > payout);

    let g = grant(12_000.0, 12, 0, private(1.0, 1.0));
    let m = JobModel::new(&equity_job(vec![g]), 30.0, true, data());
    let mut s = m.initial_state();
    let mut failures = 0;
    for t in 0..24 {
        let pay = m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
        failures += u32::from(pay.company_failed);
        assert_eq!(pay.private_payout, 0.0);
    }
    assert_eq!(failures, 1);
}

// ---------------------------------------------------------------- self-employment

#[test]
fn self_employment_income() {
    let d = data();
    let p = CareerParams::from_data(d);
    let se = SelfEmployment {
        net_annual: 96_000.0,
        volatility: 0.25,
        real_growth: 0.02,
        ..SelfEmployment::default()
    };
    // Risk off: 8,000 a month growing 2% a year in real terms, times the price level.
    let off = SelfEmploymentModel::new(&se, false, d);
    let mut s = off.initial_state();
    let mut career = Rng::stream(1, 1, Stream::Career(0));
    for t in 0..120u32 {
        let mut c = climate(t, 40.0);
        c.price_level = 1.3;
        let years = f64::from(t) / 12.0;
        let income = off.step(&mut s, years, &c, &mut career);
        close_rel(income, 8_000.0 * math::pow(1.02, years) * 1.3, 1e-12);
    }
    close_rel(
        off.expected_real_annual(10.0),
        96_000.0 * math::powi(1.02, 10),
        1e-12,
    );

    // Risk on: same draw count, unit-mean factor, the entered volatility in the long
    // run, positive co-movement with the market.
    let on = SelfEmploymentModel::new(&se, true, d);
    let mut twin = Rng::stream(1, 1, Stream::Career(0));
    let mut s2 = on.initial_state();
    for t in 0..120u32 {
        on.step(&mut s2, 0.0, &climate(t, 40.0), &mut twin);
    }
    assert_eq!(career.next_u64(), twin.next_u64());

    let flat_growth = SelfEmployment {
        real_growth: 0.0,
        ..se
    };
    let on = SelfEmploymentModel::new(&flat_growth, true, d);
    let mut market = Rng::from_seed(5);
    let paths = 60_000u64;
    let (mut sum, mut sum_sq, mut cross) = (0.0, 0.0, 0.0);
    for path in 0..paths {
        let mut career = Rng::stream(3, path, Stream::Career(1));
        let mut s = on.initial_state();
        let mut income = 0.0;
        let mut signal = 0.0;
        // Eight Januaries: the shock variance is within 0.01% of stationary.
        for t in 0..=96u32 {
            let mut c = climate(t, 40.0);
            if c.now.month() == 1 {
                signal = market.normal();
                c.equity_return_12m =
                    math::exp_m1(p.equity_log_mean_12m + p.equity_log_sd_12m * signal);
            }
            income = on.step(&mut s, 0.0, &c, &mut career);
            assert!(income > 0.0);
        }
        let factor = income / 8_000.0;
        sum += factor;
        sum_sq += factor * factor;
        cross += math::ln(factor) * signal;
    }
    let n = paths as f64;
    let mean = sum / n;
    close(mean, 1.0, 0.004);
    close((sum_sq / n - mean * mean).sqrt(), 0.25, 0.006);
    // The last innovation carries sqrt(1 - 0.5^2) of the log sd, half of it market.
    let log_sd = math::ln_1p(0.0625).sqrt();
    close(cross / n, 0.5 * log_sd * 0.75f64.sqrt(), 0.004);

    // Zero volatility or zero income.
    let none = SelfEmploymentModel::new(&SelfEmployment::default(), true, d);
    let mut s = none.initial_state();
    assert_eq!(none.step(&mut s, 3.0, &climate(12, 40.0), &mut career), 0.0);
}
