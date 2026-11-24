use super::super::*;
use super::*;
use crate::model::CareerRisk;
use crate::rng::Stream;

const START: YearMonth = YearMonth::ym(2026, 1);

fn data() -> &'static Data {
    Data::embedded()
}

fn params() -> CareerParams {
    CareerParams::from_data(data())
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

fn all_on() -> CareerRisk {
    CareerRisk {
        wage_shocks: WageShockShape::FatTailed,
        layoff_pay_cut: PayCutShape::TwoPart,
        job_search: JobSearchModel::Monthly,
        ..CareerRisk::default()
    }
}

fn job(level: RiskLevel, risk: CareerRisk) -> Job {
    Job {
        salary: 60_000.0,
        growth: WageGrowth::Flat { real_raise: 0.0 },
        risk: CareerRisk { level, ..risk },
        ..Job::default()
    }
}

fn model(level: RiskLevel, risk: CareerRisk, age: f64) -> JobModel {
    JobModel::new(&job(level, risk), age, true, data())
}

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
        Rng::stream(11, path, Stream::Career(0)),
        Rng::stream(11, path, Stream::EquityComp),
    )
}

// ---------------------------------------------------------------- parameters

#[test]
fn parameters_match_the_research_file() {
    let p = ShapeParams::from_data(data());
    assert_eq!(
        p,
        ShapeParams {
            large_move_probability: 0.15,
            large_to_small_sd_ratio: 4.0,
            large_move_mean_shift: -0.25,
            no_loss_probability: [0.45, 0.35],
            no_loss_older_multiplier: 0.8,
            no_loss_sd: 0.05,
            loss_max_pay_cut: 0.5,
            loss_max_multiple: 3.0,
            search_floor: 0.35,
            search_fade_months: 6.0,
        }
    );
    assert_eq!(params().shapes, p);
}

#[test]
fn defaults_compile_to_no_shapes() {
    let m = model(RiskLevel::Typical, CareerRisk::default(), 40.0);
    assert_eq!(m.shapes, Shapes::default());
    // With wage risk off nothing is built either, whatever is asked for.
    let none = model(RiskLevel::None, all_on(), 40.0);
    assert_eq!(none.shapes, Shapes::default());
    let off = JobModel::new(&job(RiskLevel::High, all_on()), 40.0, false, data());
    assert_eq!(off.shapes, Shapes::default());
    let on = model(RiskLevel::Typical, all_on(), 40.0);
    assert!(on.shapes.wage.is_some() && on.shapes.pay_cut.is_some() && on.shapes.search.is_some());
}

// ---------------------------------------------------------------- the wage mixture

#[test]
fn unit_mixture_has_mean_zero_and_variance_one() {
    let p = ShapeParams::from_data(data());
    let x = UnitMixture::new(
        p.large_move_probability,
        p.large_to_small_sd_ratio,
        p.large_move_mean_shift,
    );
    // 0.15 is 307 of 2048 to the nearest count.
    assert_eq!(x.p, 307.0 / 2048.0);
    close(x.moment(1), 0.0, 1e-15);
    close(x.moment(2), 1.0, 1e-14);
    // By hand with p = 0.15: m2 = -0.25, m1 = 0.15 * 0.25 / 0.85 = 0.0441;
    // between-part variance 0.85 * 0.0441^2 + 0.15 * 0.25^2 = 0.0110;
    // s1^2 = (1 - 0.0110) / (0.85 + 0.15 * 16) = 0.3043, s1 = 0.552, s2 = 2.21.
    close(x.mean[0], 0.0441, 2e-4);
    close(x.mean[1], -0.25, 1e-15);
    close(x.sd[0], 0.552, 1e-3);
    close(x.sd[1], 2.207, 2e-3);
    // Third moment: 0.85 * (3 * 0.0441 * 0.3043) + 0.15 * (-0.0156 - 0.75 * 4.869)
    // = 0.034 - 0.550 = -0.516. Fourth: 3 * (0.85 * 0.3043^2 + 0.15 * 4.869^2)
    // = 10.90, plus the mean terms 0.15 * (0.25^4 + 6 * 0.25^2 * 4.869) = 0.27 and
    // 0.85 * 6 * 0.0441^2 * 0.3043 = 0.003: 11.18.
    close(x.moment(3), -0.516, 5e-3);
    close(x.moment(4), 11.18, 0.03);
    // log E[exp(tX)] is t^2/2 to second order, then the third and fourth cumulants
    // (the third moment, and the fourth less 3).
    let t = 0.01;
    close(
        x.log_mgf(t),
        0.5 * t * t + x.moment(3) * t * t * t / 6.0 + (x.moment(4) - 3.0) * t * t * t * t / 24.0,
        1e-10,
    );
    // Degenerate inputs give the standard normal.
    for bad in [
        UnitMixture::new(0.0, 4.0, -0.25),
        UnitMixture::new(f64::NAN, 4.0, -0.25),
        UnitMixture::new(1.0, 4.0, -0.25),
    ] {
        assert_eq!(bad.sd, [1.0, 1.0]);
        close(bad.log_mgf(0.3), 0.045, 1e-15);
    }
    // A shift too large for unit variance is limited, not NaN.
    let wide = UnitMixture::new(0.15, f64::INFINITY, -50.0);
    assert!(wide.sd[0] > 0.0 && wide.sd[0].is_finite());
    close(wide.moment(2), 1.0, 1e-12);
}

#[test]
fn wage_mixture_draws_match_its_moments() {
    let sigma = 0.1;
    let mix = WageMixture::new(sigma, 0.97, &ShapeParams::from_data(data()));
    let mut rng = Rng::stream(5, 0, Stream::Career(0));
    let n = 2_000_000;
    let (mut s1, mut s2, mut s3, mut s4) = (0.0, 0.0, 0.0, 0.0);
    let (mut large, mut quiet, mut big_down, mut big_up) = (0u32, 0u32, 0u32, 0u32);
    for _ in 0..n {
        let bits = rng.next_u64();
        let x = mix.draw(bits) / sigma;
        s1 += x;
        s2 += x * x;
        s3 += x * x * x;
        s4 += x * x * x * x;
        large += u32::from(bits & 2047 < 307);
        quiet += u32::from(x.abs() < 0.5);
        big_down += u32::from(x < -2.0);
        big_up += u32::from(x > 2.0);
    }
    let n = f64::from(n);
    // Standard errors: mean 0.0007, variance 0.0022, third 0.011, fourth 0.07.
    close(s1 / n, 0.0, 0.003);
    close(s2 / n, 1.0, 0.008);
    close(s3 / n, -0.516, 0.04);
    close(s4 / n, 11.18, 0.3);
    close(f64::from(large) / n, 307.0 / 2048.0, 0.001);
    // A normal puts 38.3% of years within half a standard deviation and 2.3% beyond
    // two on each side. The mixture: 0.85 * P(|N(0.044, 0.552)| < 0.5) + 0.15 * 0.18
    // = 0.54 + 0.03 = 0.57 within; beyond two on the downside almost only large
    // moves, 0.15 * P(N(-0.25, 2.207) < -2) = 0.15 * 0.214 = 0.032.
    close(f64::from(quiet) / n, 0.57, 0.01);
    close(f64::from(big_down) / n, 0.032, 0.002);
    assert!(big_down > big_up, "large falls should outnumber large rises");
}

#[test]
fn mixture_mean_correction_keeps_expected_pay_on_the_curve() {
    // Gaussian check of the table: with one part it is half the variance.
    let mut plain = ShapeParams::from_data(data());
    plain.large_move_probability = 0.0;
    let g = WageMixture::new(0.1, 0.97, &plain);
    let var = |n: i32| 0.01 * (1.0 - math::powi(0.97, 2 * n)) / (1.0 - 0.97 * 0.97);
    for n in [0, 1, 7, 40] {
        close(g.log_mean(n as u32), 0.5 * var(n), 1e-15);
    }
    // Beyond the table the last value is used.
    assert_eq!(g.log_mean(u32::MAX), g.log_mean(512));
    close(g.log_mean(512), 0.5 * 0.01 / (1.0 - 0.97 * 0.97), 1e-12);

    // The mixture through the job: mean pay follows the curve, the variance of the
    // persistent shock is the Gaussian one, the mean of the log is not minus half
    // the variance any more (negative skew makes the correction smaller).
    let risk = CareerRisk {
        wage_shocks: WageShockShape::FatTailed,
        ..CareerRisk::default()
    };
    let mut m = model(RiskLevel::High, risk, 30.0);
    m.hazard = [[0.0; 6]; 2];
    m.sigma_eps = 0.0;
    const PATHS: u64 = 200_000;
    let checkpoints = [1u32, 5, 20];
    let mut mean = [0.0; 3];
    let mut log_sq = [0.0; 3];
    for path in 0..PATHS {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        for t in 0..=240 {
            m.step(&mut s, &climate(t, 30.0), &mut career, &mut equity);
            for (i, y) in checkpoints.iter().enumerate() {
                if t == y * 12 {
                    mean[i] += s.real_salary;
                    log_sq[i] += s.persistent_shock * s.persistent_shock;
                    assert_eq!(s.persistent_shocks, *y);
                }
            }
        }
    }
    for (i, y) in checkpoints.iter().enumerate() {
        // High risk: sigma_eta 0.17. Standard error of the mean after 20 years is
        // about 0.15% of it.
        close_rel(mean[i] / PATHS as f64, 60_000.0, 0.006);
        let expected = 0.17 * 0.17 * (1.0 - math::powi(0.97, 2 * *y as i32)) / (1.0 - 0.97 * 0.97);
        close_rel(log_sq[i] / PATHS as f64, expected, 0.03);
    }
    assert_eq!(m.expected_real_salary(50.0), 60_000.0);
}

#[test]
fn fat_tails_leave_layoffs_and_spells_where_they_were() {
    let plain = model(RiskLevel::High, CareerRisk::default(), 30.0);
    let fat = model(
        RiskLevel::High,
        CareerRisk {
            wage_shocks: WageShockShape::FatTailed,
            ..CareerRisk::default()
        },
        30.0,
    );
    let mut layoffs = 0;
    let mut differ = 0;
    for path in 0..300 {
        let (mut c1, mut e1) = rngs(path);
        let (mut c2, mut e2) = rngs(path);
        let (mut s1, mut s2) = (plain.initial_state(), fat.initial_state());
        for t in 0..360 {
            let mut c = climate(t, 30.0);
            c.recession = u8::from(t % 100 < 10);
            let a = plain.step(&mut s1, &c, &mut c1, &mut e1);
            let b = fat.step(&mut s2, &c, &mut c2, &mut e2);
            assert_eq!(a.laid_off, b.laid_off);
            assert_eq!(a.rehired, b.rehired);
            assert_eq!(s1.employment, s2.employment);
            assert_eq!(s1.pending_scar, s2.pending_scar);
            assert_eq!(s1.months_unemployed_total, s2.months_unemployed_total);
            layoffs += u32::from(a.laid_off);
            differ += u32::from(a.wages != b.wages);
        }
        assert_eq!(c1.next_u64(), c2.next_u64());
        assert_eq!(e1.next_u64(), e2.next_u64());
    }
    assert!(layoffs > 300 && differ > 50_000, "{layoffs} {differ}");
}

// ---------------------------------------------------------------- the two-part pay cut

#[test]
fn two_part_pay_cut_keeps_the_mean_and_is_skewed() {
    let cut = PayCut::new(&ShapeParams::from_data(data()));
    // Chance of no loss, to the nearest 2048th: 0.45, 0.45 * 0.8, 0.35, 0.35 * 0.8.
    for (class, q) in [(0, 0.45), (1, 0.36), (2, 0.35), (3, 0.28)] {
        close(cut.no_loss_probability(class), q, 0.5 / 2048.0);
    }
    // The largest loss is a halving of pay (ln 2 = 0.693) unless three times the
    // mean loss of those who lose is more: 3 * 0.08 / 0.55 = 0.436 (no),
    // 3 * 0.325 / 0.72 = 1.354 (yes).
    close(cut.largest(0, 0.08), std::f64::consts::LN_2, 1e-12);
    close(cut.largest(1, 0.12), std::f64::consts::LN_2, 1e-12);
    close(cut.largest(0, 0.18), 3.0 * 0.18 / 0.55, 1e-3);
    close(cut.largest(2, 0.25), 3.0 * 0.25 / 0.65, 1e-3);
    close(cut.largest(3, 0.225), 3.0 * 0.225 / 0.72, 1e-3);
    close(cut.largest(3, 0.325), 1.354, 2e-3);
    let mut rng = Rng::stream(6, 0, Stream::Career(0));
    // (class, overall mean, b by hand): b = top / m - 1 with m = mean / (1 - q):
    // 0.693 / 0.1455 - 1 = 3.76; 0.693 / 0.1875 - 1 = 2.70; 0.693 / 0.2308 - 1 = 2.00;
    // and 2 exactly where the limit is three times m.
    for (class, mean, b_hand) in [
        (0usize, 0.08, 3.76),
        (1, 0.12, 2.70),
        (2, 0.15, 2.00),
        (3, 0.325, 2.00),
    ] {
        let q = cut.no_loss_probability(class);
        let n = 1_000_000;
        let (mut sum, mut loss_sum, mut loss_sq, mut loss_cu) = (0.0, 0.0, 0.0, 0.0);
        let (mut calm, mut calm_near, mut raises, mut max) = (0u32, 0u32, 0u32, 0.0f64);
        let mut below_mean = 0u32;
        let m = mean / (1.0 - q);
        let top = cut.largest(class, mean);
        let b = top / m - 1.0;
        close(b, b_hand, 0.02);
        for _ in 0..n {
            let bits = rng.next_u64();
            let l = cut.draw(class, mean, bits);
            sum += l;
            max = max.max(l);
            raises += u32::from(l < 0.0);
            if bits & 2047 < (q * 2048.0).round() as u64 {
                calm += 1;
                // About the old wage: a normal with sd 5%, which one word limits to
                // 8.3 standard deviations.
                assert!(l.abs() <= 0.05 * 8.3, "{l}");
                calm_near += u32::from(l.abs() < 0.1);
            } else {
                assert!((0.0..=top).contains(&l), "{l}");
                loss_sum += l;
                loss_sq += l * l;
                loss_cu += l * l * l;
                below_mean += u32::from(l < m);
            }
        }
        let nf = f64::from(n);
        let losers = nf - f64::from(calm);
        // The overall mean is the default's mean. The standard error is about
        // 0.0001 to 0.0003.
        close(sum / nf, mean, 0.0012);
        close(f64::from(calm) / nf, q, 0.002);
        // Within two standard deviations: 95.45% of the no-loss part.
        close(f64::from(calm_near) / f64::from(calm), 0.9545, 0.002);
        // Raises come only from the no-loss part, half of it.
        close(f64::from(raises) / nf, 0.5 * q, 0.002);
        // The loss part is top * Beta(1, b): mean top / (1 + b) = m, variance
        // top^2 b / ((1 + b)^2 (2 + b)), skewness
        // 2 (b - 1) sqrt(b + 2) / ((b + 3) sqrt(b)): 0.566 at b = 2, 1.01 at 3.76.
        let m1 = loss_sum / losers;
        let var = loss_sq / losers - m1 * m1;
        let third = loss_cu / losers - 3.0 * m1 * loss_sq / losers + 2.0 * m1 * m1 * m1;
        close_rel(m1, m, 0.004);
        close_rel(var, top * top * b / ((1.0 + b) * (1.0 + b) * (2.0 + b)), 0.01);
        let skew = 2.0 * (b - 1.0) * (b + 2.0).sqrt() / ((b + 3.0) * b.sqrt());
        assert!(skew > 0.56);
        close(third / (var * var.sqrt()), skew, 0.02);
        // Right-skewed: more than half of the losses are below the average loss,
        // 1 - (1 - 1 / (1 + b))^b of them: 0.556 at b = 2.
        close(
            f64::from(below_mean) / losers,
            1.0 - math::pow(1.0 - m / top, b),
            0.003,
        );
        assert!(f64::from(below_mean) / losers > 0.55);
        assert!(max > 0.97 * top);
    }
    // No mean, no loss: only the no-loss part moves pay.
    assert_eq!(cut.draw(0, 0.0, u64::MAX), 0.0);
    // The extreme words stay inside the range.
    assert_eq!(cut.draw(0, 0.08, 2047), 0.0);
    let most = cut.draw(0, 0.08, u64::MAX);
    assert!(most > 0.69 && most <= std::f64::consts::LN_2, "{most}");
    // An out-of-range class is treated as the last one.
    assert_eq!(cut.draw(9, 0.1, 12_345), cut.draw(3, 0.1, 12_345));
    // Out-of-range parameters are limited, not NaN.
    let odd = PayCut::new(&ShapeParams {
        no_loss_probability: [f64::NAN, 7.0],
        no_loss_sd: f64::NAN,
        loss_max_pay_cut: 5.0,
        loss_max_multiple: 0.0,
        ..ShapeParams::from_data(data())
    });
    for class in 0..4 {
        for bits in [0, 1, 2047, 1 << 40, u64::MAX] {
            assert!(odd.draw(class, 0.1, bits).is_finite());
        }
    }
}

#[test]
fn two_part_pay_cut_in_the_job_keeps_the_mean_scar_and_the_spell() {
    let d = data();
    let two = CareerRisk {
        layoff_pay_cut: PayCutShape::TwoPart,
        ..CareerRisk::default()
    };
    // (age, recession, base mean): 0.08; 0.08 * 1.5; 0.15.
    for (age, recession, base) in [(40.0, 0, 0.08), (52.0, 0, 0.12), (40.0, 1, 0.15)] {
        let mut plain = JobModel::new(&job(RiskLevel::Typical, CareerRisk::default()), age, true, d);
        let mut shaped = JobModel::new(&job(RiskLevel::Typical, two), age, true, d);
        plain.hazard = [[1.0; 6]; 2];
        shaped.hazard = [[1.0; 6]; 2];
        let n = 200_000u64;
        let (mut sum, mut sum_plain, mut addon) = (0.0, 0.0, 0.0);
        let (mut near, mut near_plain, mut deep, mut deep_plain) = (0u32, 0u32, 0u32, 0u32);
        let (mut raises, mut raises_plain) = (0u32, 0u32);
        for path in 0..n {
            let mut c = climate(1, age);
            c.recession = recession;
            let (mut c1, mut e1) = rngs(path);
            let (mut c2, mut e2) = rngs(path);
            let (mut s1, mut s2) = (plain.initial_state(), shaped.initial_state());
            plain.step(&mut s1, &c, &mut c1, &mut e1);
            shaped.step(&mut s2, &c, &mut c2, &mut e2);
            // The spell is the default's, bit for bit.
            assert_eq!(s1.spell_months_left, s2.spell_months_left);
            assert_eq!(s1.employment, s2.employment);
            assert_eq!(s2.scar_class, 2 * recession + u8::from(age >= 50.0));
            let spell = s2.spell_months_left + s2.months_unemployed_total;
            addon += (0.01 * (spell - 6.0).max(0.0)).min(0.10);
            // The loss is pending while the spell runs and already applied when the
            // spell ended inside the month.
            let l = s2.pending_scar - s2.persistent_shock;
            let l_plain = s1.pending_scar - s1.persistent_shock;
            sum += l;
            sum_plain += l_plain;
            near += u32::from(l.abs() < 0.05);
            near_plain += u32::from(l_plain.abs() < 0.05);
            raises += u32::from(l < -0.1);
            raises_plain += u32::from(l_plain < -0.1);
            deep += u32::from(l > 0.5);
            deep_plain += u32::from(l_plain > 0.5);
        }
        let n = n as f64;
        close(sum / n, base + addon / n, 0.002);
        close(sum_plain / n, base + addon / n, 0.002);
        // Under one bell curve with sd 0.15 fewer than a quarter land within 5% of
        // the old wage; here the no-loss part alone puts 68% of 35% to 45% there and
        // the smallest losses add to it.
        assert!(f64::from(near_plain) / n < 0.25, "{near_plain}");
        assert!(f64::from(near) / n > 0.30, "{near}");
        // Raises of more than 10% in logs become rare: 2.3% of the no-loss part,
        // about 1% of layoffs against 5% to 12% under the bell curve.
        assert!(4 * raises < raises_plain, "{raises} against {raises_plain}");
        // More lose over 0.5 in logs (39% of pay): by hand 0.45% against 0.26% at
        // 40 in an expansion, 5% against 1% in a recession.
        assert!(
            f64::from(deep) > 1.3 * f64::from(deep_plain) && deep > 500,
            "{deep} against {deep_plain}"
        );
    }
}

// ---------------------------------------------------------------- the monthly search

#[test]
fn mean_months_is_exact_arithmetic() {
    // Certain in the first month: a uniform part of a month, at least a quarter:
    // 0.25 * 0.25 + (1 - 0.0625) / 2 = 0.53125.
    close(JobSearch::mean_months(&[1.0; 36]), 0.53125, 1e-15);
    // Never: the cap.
    close(JobSearch::mean_months(&[0.0; 36]), 36.0, 1e-12);
    // Certain in the second month: one whole month and half of the next.
    let mut second = [0.0; 36];
    second[1] = 1.0;
    close(JobSearch::mean_months(&second), 1.5, 1e-15);
    // A constant chance of one half from the second month on: 1 + sum over k of
    // 0.5^k * (0.5 * 0.5 + 0.5) = 1 + 0.75 * 2 = 2.5, less a cap term of 0.5^35.
    let mut half = [0.5; 36];
    half[0] = 0.0;
    close(JobSearch::mean_months(&half), 2.5, 1e-9);
}

#[test]
fn search_chances_reproduce_the_default_mean_spell() {
    let p = params();
    let search = JobSearch::new(&p);
    for regime in 0..2 {
        for band in 0..3 {
            let chances = search.chances(regime, band);
            let target = clipped_lognormal_mean(
                p.spell_median[regime] * p.spell_age_multiplier[band],
                p.spell_sigma[regime],
            );
            close(JobSearch::mean_months(chances), target, 1e-9);
            // The chance falls with the length of the search, towards 35% of the
            // first month's: 0.35 + 0.65 * exp(-35 / 6) = 0.3519.
            for d in 1..36 {
                assert!(chances[d] < chances[d - 1]);
            }
            close(chances[35] / chances[0], 0.3519, 1e-4);
            close(chances[6] / chances[0], 0.35 + 0.65 * math::exp(-1.0), 1e-12);
            assert!(chances[0] > 0.05 && chances[0] < 0.6, "{}", chances[0]);
        }
    }
    // The research's headline means: 3.6 and 6.6 months at 35-54 (the second is
    // 6.4 after the 36-month cap).
    close(JobSearch::mean_months(search.chances(0, 1)), 3.6, 0.05);
    close(JobSearch::mean_months(search.chances(1, 1)), 6.4, 0.15);
    // A recession lowers the chance at every length of search, and so does age.
    for d in 0..36 {
        assert!(search.chances(1, 1)[d] < search.chances(0, 1)[d]);
        assert!(search.chances(0, 2)[d] < search.chances(0, 1)[d]);
        assert!(search.chances(0, 0)[d] > search.chances(0, 1)[d]);
    }
    assert_eq!(clipped_lognormal_mean(f64::NAN, 1.0), 0.25);
    assert_eq!(clipped_lognormal_mean(4.0, 0.0), 4.0);
}

#[test]
fn search_step_respects_the_floor_and_the_cap() {
    let search = JobSearch::new(&params());
    let f = search.chances(0, 1)[0];
    // The lowest uniform ends the search at once: a quarter of a month in the
    // first month, nothing in a later one.
    assert_eq!(search.step(0, 1, 0, 0), Some(0.25));
    assert_eq!(search.step(0, 1, 3, 0), Some(0.0));
    // The highest never ends it, except in the 36th month.
    assert_eq!(search.step(0, 1, 0, u64::MAX), None);
    assert_eq!(search.step(0, 1, 34, u64::MAX), None);
    assert_eq!(search.step(0, 1, 35, u64::MAX), Some(1.0));
    assert_eq!(search.step(0, 1, 400, u64::MAX), Some(1.0));
    // A uniform just below the chance ends it at the end of the month.
    let bits = ((f * 0.999 * (1u64 << 53) as f64) as u64) << 11;
    let part = search.step(0, 1, 0, bits).expect("ends");
    close(part, 0.999, 1e-6);
    // The low bits (used to pick the pay-cut part) do not matter.
    assert_eq!(search.step(0, 1, 0, bits | 2047), Some(part));
}

/// Forces layoffs at a fixed age and regime and returns (spells, mean months, share
/// of spells longer than six months, share longer than twelve).
fn spell_statistics(risk: CareerRisk, age: f64, recession: u8, spells: u32) -> [f64; 4] {
    let mut m = model(RiskLevel::Typical, risk, age);
    m.hazard = [[1.0; 6]; 2];
    let mut s = m.initial_state();
    let (mut career, mut equity) = rngs(u64::from(recession) + age as u64);
    let mut c = climate(1, age);
    c.recession = recession;
    let (mut count, mut total, mut over6, mut over12) = (0.0, 0.0, 0.0, 0.0);
    let mut before = 0.0;
    while count < f64::from(spells) {
        s.persistent_shock = 0.0;
        let pay = m.step(&mut s, &c, &mut career, &mut equity);
        if pay.rehired {
            let spell = s.months_unemployed_total - before;
            before = s.months_unemployed_total;
            assert!((0.25..=36.0 + 1e-9).contains(&spell), "{spell}");
            count += 1.0;
            total += spell;
            over6 += f64::from(u8::from(spell > 6.0));
            over12 += f64::from(u8::from(spell > 12.0));
        }
    }
    [count, total / count, over6 / count, over12 / count]
}

#[test]
fn monthly_search_matches_the_default_mean_spell_in_a_steady_economy() {
    let monthly = CareerRisk {
        job_search: JobSearchModel::Monthly,
        ..CareerRisk::default()
    };
    // (age, recession, median of the default lognormal)
    for (age, recession, median) in [(40.0, 0, 2.2), (60.0, 1, 5.6), (25.0, 0, 1.87)] {
        let expected = clipped_lognormal_mean(median, 1.0);
        let [_, mean, over6, over12] = spell_statistics(monthly, age, recession, 150_000);
        let [_, mean0, over6_0, over12_0] =
            spell_statistics(CareerRisk::default(), age, recession, 150_000);
        // Spell sd is about 1.2 times the mean: three standard errors are 1%.
        close_rel(mean, expected, 0.012);
        close_rel(mean0, expected, 0.012);
        // The shape differs: fewer middling spells and more very long ones for the
        // same mean, so the share over six months is a few points below the
        // default's (41% against 47% from age 55 in a recession).
        close(over6, over6_0, 0.07);
        close(over12, over12_0, 0.04);
    }
}

#[test]
fn a_recession_that_arrives_during_a_search_lengthens_it() {
    let monthly = CareerRisk {
        job_search: JobSearchModel::Monthly,
        ..CareerRisk::default()
    };
    // Laid off in an expansion; a recession starts the next month and lasts.
    let mean_spell = |risk: CareerRisk, recession_after: bool| {
        let mut forced = model(RiskLevel::Typical, risk, 40.0);
        forced.hazard = [[1.0; 6]; 2];
        let mut calm = forced.clone();
        calm.hazard = [[0.0; 6]; 2];
        let n = 60_000u64;
        let mut total = 0.0;
        let mut weeks = 0.0;
        for path in 0..n {
            let (mut career, mut equity) = rngs(path);
            let mut s = forced.initial_state();
            let mut c = climate(1, 40.0);
            let pay = forced.step(&mut s, &c, &mut career, &mut equity);
            assert!(pay.laid_off);
            let granted = s.ui_weeks_left
                + (s.months_unemployed_total * WEEKS_PER_MONTH - UI_WAIT_WEEKS).max(0.0);
            // Benefit weeks are fixed on the day of the layoff: an expansion.
            close(granted, 26.0, 1e-9);
            let mut paid_weeks = pay.unemployment_benefit / s.ui_weekly_real;
            let mut t = 1;
            while s.employment != Employment::Employed {
                t += 1;
                c = climate(t, 40.0);
                c.recession = u8::from(recession_after);
                let pay = calm.step(&mut s, &c, &mut career, &mut equity);
                paid_weeks += pay.unemployment_benefit / s.ui_weekly_real;
                assert!(t < 40);
            }
            assert!(paid_weeks <= 26.0 + 1e-9);
            weeks += paid_weeks;
            total += s.months_unemployed_total;
        }
        (total / n as f64, weeks / n as f64)
    };
    let (steady, _) = mean_spell(monthly, false);
    let (hit, weeks_hit) = mean_spell(monthly, true);
    let (fixed, _) = mean_spell(CareerRisk::default(), false);
    let (fixed_hit, _) = mean_spell(CareerRisk::default(), true);
    // The default fixes the spell at the layoff: what happens next cannot matter.
    assert_eq!(fixed, fixed_hit);
    close_rel(steady, fixed, 0.03);
    // 3.6 months if nothing changes; with the recession regime from the second
    // month on it is between that and the recession mean of 6.4.
    assert!(hit > steady + 1.0 && hit < 6.4, "{hit} against {steady}");
    assert!(weeks_hit > 8.0);
}

#[test]
fn monthly_search_draws_the_scar_when_the_search_ends() {
    let d = data();
    let risk = CareerRisk {
        job_search: JobSearchModel::Monthly,
        ..CareerRisk::default()
    };
    let p = params();
    let mut forced = JobModel::new(&job(RiskLevel::Typical, risk), 52.0, true, d);
    forced.hazard = [[1.0; 6]; 2];
    let mut calm = forced.clone();
    calm.hazard = [[0.0; 6]; 2];
    let mut long_spells = 0;
    for path in 0..4_000 {
        let (mut career, mut equity) = rngs(path);
        let mut s = forced.initial_state();
        let mut t = 1;
        let mut model = &forced;
        loop {
            // Reproduce the scar word of this month: the third career word.
            let mut peek = career.clone();
            peek.next_u64();
            peek.next_u64();
            let z_scar = peek.normal_one_draw();
            let before = s.persistent_shock;
            let pay = model.step(&mut s, &climate(t, 52.0), &mut career, &mut equity);
            model = &calm;
            if pay.rehired {
                let spell = s.months_unemployed_total;
                long_spells += u32::from(spell > 6.0);
                // Age 52 in an expansion: 0.08 * 1.5 plus 0.01 a month beyond six.
                let mean = 0.12 + (0.01 * (spell - 6.0).max(0.0)).min(0.10);
                close(before - s.persistent_shock, mean + p.scar_sd * z_scar, 1e-12);
                assert_eq!(s.pending_scar, 0.0);
                assert_eq!(s.scar_class, 1);
                break;
            }
            assert_eq!(s.pending_scar, 0.0);
            assert_eq!(s.scar_base_mean, 0.12);
            assert!(matches!(s.employment, Employment::Unemployed { .. }));
            t += 1;
            // No January in the spell: the persistent shock moves only by the scar.
            if climate(t, 52.0).now.month() == 1 {
                break;
            }
        }
    }
    assert!(long_spells > 300, "{long_spells}");
}

// ---------------------------------------------------------------- contract

#[test]
fn shapes_do_not_change_the_draw_counts() {
    let plain = model(RiskLevel::High, CareerRisk::default(), 28.0);
    let shaped = model(RiskLevel::High, all_on(), 28.0);
    let (career0, equity0) = rngs(21);
    let mut ends = Vec::new();
    for m in [&plain, &shaped] {
        let (mut career, mut equity) = (career0.clone(), equity0.clone());
        let mut s = m.initial_state();
        let mut layoffs = 0;
        for t in 0..600 {
            let mut c = climate(t, 28.0);
            c.recession = u8::from(t % 40 < 10);
            let pay = m.step(&mut s, &c, &mut career, &mut equity);
            layoffs += u32::from(pay.laid_off);
            assert!(pay.gross().is_finite() && pay.gross() >= 0.0);
            assert!(s.real_salary.is_finite() && s.real_salary > 0.0);
        }
        assert!(layoffs > 0);
        ends.push((career.next_u64(), equity.next_u64(), s));
    }
    assert_eq!(ends[0].0, ends[1].0);
    assert_eq!(ends[0].1, ends[1].1);
    assert_ne!(ends[0].2, ends[1].2);
}

#[test]
fn the_first_layoff_falls_in_the_same_month_with_every_shape_on() {
    let plain = model(RiskLevel::High, CareerRisk::default(), 35.0);
    let shaped = model(RiskLevel::High, all_on(), 35.0);
    let mut seen = 0;
    for path in 0..500 {
        let first = |m: &JobModel| {
            let (mut career, mut equity) = rngs(path);
            let mut s = m.initial_state();
            (0..240).find(|t| {
                m.step(&mut s, &climate(*t, 35.0), &mut career, &mut equity)
                    .laid_off
            })
        };
        let a = first(&plain);
        assert_eq!(a, first(&shaped));
        seen += u32::from(a.is_some());
    }
    assert!(seen > 300);
}

#[test]
fn all_shapes_together_stay_bounded_over_long_careers() {
    // The lowest pay a path can reach: each layoff costs at most three times the
    // mean loss of those who lose.
    let m = model(RiskLevel::High, all_on(), 22.0);
    let mut lowest = f64::INFINITY;
    let mut highest = 0.0f64;
    for path in 0..3_000 {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        for t in 0..(45 * 12) {
            let mut c = climate(t, 22.0);
            c.recession = u8::from(t % 60 < 14);
            let pay = m.step(&mut s, &c, &mut career, &mut equity);
            assert!(pay.wages.is_finite() && pay.wages >= 0.0);
            assert!(pay.unemployment_benefit.is_finite() && pay.unemployment_benefit >= 0.0);
        }
        lowest = lowest.min(s.real_salary);
        highest = highest.max(s.real_salary);
        assert!(s.months_unemployed_total <= 45.0 * 12.0);
    }
    assert!(lowest > 100.0 && highest < 1e8, "{lowest} {highest}");
}
