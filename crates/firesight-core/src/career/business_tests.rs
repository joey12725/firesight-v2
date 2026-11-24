use super::super::*;
use super::*;
use crate::model::{AfterFailure, BusinessFailure, SelfEmployment};
use crate::rng::Stream;

const START: YearMonth = YearMonth::ym(2026, 1);

fn data() -> &'static Data {
    Data::embedded()
}

fn params() -> BusinessParams {
    BusinessParams::from_data(data())
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn hazard(years_in_business: Option<f64>, then: AfterFailure) -> BusinessFailure {
    BusinessFailure::Hazard {
        young_annual_probability: 0.10,
        mature_annual_probability: 0.04,
        young_years: 5.0,
        years_in_business,
        recession_multiplier: 2.0,
        then,
    }
}

fn restart() -> AfterFailure {
    AfterFailure::Restart {
        gap_months: 12,
        income_fraction: 0.7,
    }
}

fn income(failure: BusinessFailure, lumpiness: f64) -> SelfEmployment {
    SelfEmployment {
        net_annual: 120_000.0,
        volatility: 0.0,
        failure,
        monthly_lumpiness: lumpiness,
        ..SelfEmployment::default()
    }
}

fn business(failure: BusinessFailure, lumpiness: f64, running: bool) -> Business {
    Business::new(&income(failure, lumpiness), running, &params()).expect("an option is on")
}

fn rng(path: u64) -> Rng {
    Rng::stream(11, path, Stream::Business(0))
}

/// The level paid in each of `months` months of one path, with the income active
/// throughout and the given recession flag.
fn levels(b: &Business, path: u64, months: usize, recession: bool) -> Vec<f64> {
    let mut s = b.initial_state();
    let mut r = rng(path);
    (0..months)
        .map(|_| {
            b.step(&mut s, true, recession, &mut r);
            s.level
        })
        .collect()
}

#[test]
fn both_options_are_off_by_default_and_draw_nothing() {
    let d = SelfEmployment::default();
    assert_eq!(d.failure, BusinessFailure::Never);
    assert_eq!(d.monthly_lumpiness, 0.0);
    assert!(Business::new(&d, true, &params()).is_none());
    let model = SelfEmploymentModel::new(&d, true, data());
    assert!(!model.has_business());
    let mut s = model.initial_state();
    assert_eq!(s.business, BusinessState::default());
    let (mut a, mut b) = (rng(0), rng(0));
    let month = model.business_step(&mut s, true, false, &mut a);
    assert_eq!(
        (month.lump, month.closed, month.reopened),
        (1.0, false, false)
    );
    assert_eq!(s.business, BusinessState::default());
    assert_eq!(a.next_u64(), b.next_u64(), "nothing was drawn");
    assert_eq!(model.expected_business_level(3), vec![1.0; 3]);

    // Career risk off silences both.
    let on = income(hazard(None, restart()), 0.8);
    assert!(SelfEmploymentModel::new(&on, true, data()).has_business());
    assert!(!SelfEmploymentModel::new(&on, false, data()).has_business());
}

/// With `then: stops` the share of businesses still open after `m` months is the
/// product of the monthly survival chances, which by construction is the entered
/// yearly figure every twelve months: 0.9 after one year, 0.9^5 after five, and
/// 0.9^5 * 0.96^5 after ten.
#[test]
fn closures_arrive_at_the_entered_yearly_chances() {
    let b = business(hazard(Some(0.0), AfterFailure::Stops), 0.0, true);
    let f = b.failure.unwrap();
    close(f.hazard[0][0], 1.0 - math::pow(0.9, 1.0 / 12.0), 1e-15);
    close(f.hazard[1][0], 1.0 - math::pow(0.96, 1.0 / 12.0), 1e-15);
    assert_eq!((f.young_months, f.age0_months, f.restart), (60, 0, None));

    let paths = 200_000u64;
    let mut open = [0u32; 3];
    for path in 0..paths {
        let l = levels(&b, path, 120, false);
        for (slot, m) in open.iter_mut().zip([11, 59, 119]) {
            *slot += u32::from(l[m] > 0.0);
        }
        // Closed for good: the level never comes back.
        assert!(l.windows(2).all(|w| w[1] <= w[0]));
    }
    let n = paths as f64;
    let expected = [
        0.9,
        math::powi(0.9, 5),
        math::powi(0.9, 5) * math::powi(0.96, 5),
    ];
    for (count, want) in open.iter().zip(expected) {
        // Three standard errors of a proportion near one half over 200,000 paths.
        close(f64::from(*count) / n, want, 0.0035);
    }
    // The exact calculation agrees with the hand figures.
    let exact = b.failure.unwrap().expected_level(120);
    close(exact[11], 0.9, 1e-12);
    close(exact[59], expected[1], 1e-12);
    close(exact[119], expected[2], 1e-12);
}

/// In a recession month the monthly chance is multiplied by the entered factor: a
/// year of recession months leaves (1 - 2q)^12 of young businesses open.
#[test]
fn recession_months_multiply_the_chance() {
    let b = business(hazard(Some(0.0), AfterFailure::Stops), 0.0, true);
    let q = 1.0 - math::pow(0.9, 1.0 / 12.0);
    close(b.failure.unwrap().hazard[0][1], 2.0 * q, 1e-15);
    let paths = 200_000u64;
    let open = (0..paths)
        .filter(|p| levels(&b, *p, 12, true)[11] > 0.0)
        .count();
    close(
        open as f64 / paths as f64,
        math::powi(1.0 - 2.0 * q, 12),
        0.003,
    );
    // A huge multiplier cannot push a probability above one.
    let wild = business(
        BusinessFailure::Hazard {
            young_annual_probability: 0.5,
            mature_annual_probability: 0.5,
            young_years: 5.0,
            years_in_business: None,
            recession_multiplier: 100.0,
            then: AfterFailure::Stops,
        },
        0.0,
        true,
    );
    assert_eq!(wild.failure.unwrap().hazard[0][1], 1.0);
}

/// A business whose age is not stated is established if the income is already
/// being received and new if it starts later.
#[test]
fn an_unstated_age_depends_on_whether_the_income_has_started() {
    let running = business(hazard(None, restart()), 0.0, true);
    let future = business(hazard(None, restart()), 0.0, false);
    assert_eq!(running.failure.unwrap().age0_months, 60);
    assert_eq!(running.initial_state().venture_months, 60);
    assert_eq!(future.failure.unwrap().age0_months, 0);
    let stated = business(hazard(Some(2.5), restart()), 0.0, true);
    assert_eq!(stated.failure.unwrap().age0_months, 30);
    // An established business closes at the mature rate from the first month.
    close(
        running.failure.unwrap().expected_level(1)[0],
        math::pow(0.96, 1.0 / 12.0),
        1e-15,
    );
    close(
        future.failure.unwrap().expected_level(1)[0],
        math::pow(0.9, 1.0 / 12.0),
        1e-15,
    );
}

/// After a closure the income is zero for exactly the gap, then pays the stated
/// share of the old profit, and the new venture is young again.
#[test]
fn a_restart_follows_the_gap_at_the_lower_level() {
    let b = business(hazard(Some(20.0), restart()), 0.0, true);
    let mut seen = 0;
    for path in 0..4_000 {
        let mut s = b.initial_state();
        let mut r = rng(path);
        let mut closed_at = None;
        for m in 0..240usize {
            let before = s;
            let month = b.step(&mut s, true, false, &mut r);
            match closed_at {
                None if month.closed => {
                    assert_eq!(before.level, 1.0);
                    assert_eq!((s.level, s.closed_months_left, s.closures), (0.0, 12, 1));
                    close(s.open_level, 0.7, 1e-15);
                    closed_at = Some(m);
                }
                None => assert_eq!(s.level, 1.0),
                Some(at) if m < at + 12 => {
                    assert_eq!(s.level, 0.0, "month {m} after a closure in {at}");
                    assert!(!month.reopened && !month.closed);
                }
                Some(at) if m == at + 12 => {
                    assert!(month.reopened);
                    close(s.level, 0.7, 1e-15);
                    assert_eq!(s.venture_months, 0, "a young venture again");
                    seen += 1;
                }
                Some(_) => break,
            }
        }
    }
    assert!(seen > 1_000, "only {seen} restarts seen");

    // A gap of zero months pays the lower level in the month of the closure.
    let at_once = business(
        hazard(
            Some(0.0),
            AfterFailure::Restart {
                gap_months: 0,
                income_fraction: 0.5,
            },
        ),
        0.0,
        true,
    );
    let mut closures = 0;
    for path in 0..2_000 {
        let mut s = at_once.initial_state();
        let mut r = rng(path);
        for _ in 0..120 {
            let month = at_once.step(&mut s, true, false, &mut r);
            assert_eq!(month.closed, month.reopened);
            assert!(s.level > 0.0 && s.closed_months_left == 0);
            closures += u32::from(month.closed);
        }
        close(s.level, math::powi(0.5, s.closures as i32), 1e-15);
    }
    assert!(closures > 500);
}

/// The simulated average level equals the exact calculation, month by month, and
/// the exact calculation equals a hand sum in its first months.
#[test]
fn the_expected_level_is_exact() {
    let b = business(hazard(Some(0.0), restart()), 0.0, true);
    let exact = b.failure.unwrap().expected_level(360);
    let q = 1.0 - math::pow(0.9, 1.0 / 12.0);
    // Before any restart can have happened (the first 12 months) the level is the
    // chance of never having closed.
    for (m, level) in exact.iter().take(12).enumerate() {
        close(*level, math::powi(1.0 - q, m as i32 + 1), 1e-14);
    }
    // In the 13th month those that closed in the first are back at 70%.
    close(exact[12], math::powi(1.0 - q, 13) + q * 0.7, 1e-14);

    let paths = 100_000u64;
    let mut sums = vec![0.0; 360];
    for path in 0..paths {
        for (sum, level) in sums.iter_mut().zip(levels(&b, path, 360, false)) {
            *sum += level;
        }
    }
    for m in [0, 11, 12, 59, 119, 239, 359] {
        // The level lies in [0, 1]: three standard errors are below 0.005.
        close(sums[m] / paths as f64, exact[m], 0.005);
    }
    // The risk lowers expected profit: by a tenth after a year and by almost a
    // quarter after ten.
    assert!(exact[11] > 0.89 && exact[11] < 0.91, "{}", exact[11]);
    assert!(exact[119] > 0.74 && exact[119] < 0.79, "{}", exact[119]);
    // The figures quoted in docs/methodology.md.
    let avg = |v: &[f64], years: usize| v[..12 * years].iter().sum::<f64>() / (12 * years) as f64;
    let established = business(hazard(None, restart()), 0.0, true)
        .failure
        .unwrap()
        .expected_level(360);
    let stops = business(hazard(None, AfterFailure::Stops), 0.0, true)
        .failure
        .unwrap()
        .expected_level(360);
    println!(
        "new, restart: level at 1y {:.3} 5y {:.3} 10y {:.3} 20y {:.3} 30y {:.3}; average over 10y {:.3} 20y {:.3} 30y {:.3}",
        exact[11], exact[59], exact[119], exact[239], exact[359],
        avg(&exact, 10), avg(&exact, 20), avg(&exact, 30)
    );
    println!(
        "established, restart: level at 10y {:.3} 20y {:.3} 30y {:.3}; average over 10y {:.3} 20y {:.3} 30y {:.3}",
        established[119], established[239], established[359],
        avg(&established, 10), avg(&established, 20), avg(&established, 30)
    );
    println!(
        "established, stops: level at 10y {:.3} 20y {:.3} 30y {:.3}; average over 10y {:.3} 20y {:.3} 30y {:.3}",
        stops[119], stops[239], stops[359],
        avg(&stops, 10), avg(&stops, 20), avg(&stops, 30)
    );
    close(avg(&established, 20), 0.846, 0.02);
    assert!(exact.iter().all(|x| (0.0..=1.0).contains(x)));
}

/// Exactly two words a month, whatever the state of the business and whether or
/// not the income is active; an inactive income cannot close.
#[test]
fn draws_are_fixed_and_an_inactive_income_cannot_close() {
    let b = business(hazard(Some(0.0), restart()), 0.6, true);
    for path in 0..200 {
        let mut s = b.initial_state();
        let (mut r, mut twin) = (rng(path), rng(path));
        for m in 0..360 {
            b.step(&mut s, m % 7 != 0, m % 5 == 0, &mut r);
            for _ in 0..WORDS_PER_MONTH {
                twin.next_u64();
            }
        }
        assert_eq!(r.next_u64(), twin.next_u64());

        let mut idle = b.initial_state();
        let mut r = rng(path);
        for _ in 0..360 {
            let month = b.step(&mut idle, false, true, &mut r);
            assert!(!month.closed && month.lump > 0.0);
        }
        assert_eq!(
            (idle.level, idle.closures, idle.venture_months),
            (1.0, 0, 0)
        );
    }
}

fn moments(table: &ShockTable, draws: u64) -> (f64, f64, f64) {
    let mut r = Rng::from_seed(9);
    let (mut sum, mut sum_sq, mut max) = (0.0, 0.0, 0.0f64);
    for _ in 0..draws {
        let x = table.sample(r.next_u64());
        assert!(x > 0.0 && x.is_finite());
        sum += x;
        sum_sq += x * x;
        max = max.max(x);
    }
    let mean = sum / draws as f64;
    (mean, (sum_sq / draws as f64 - mean * mean).sqrt(), max)
}

/// The monthly lump has mean exactly 1 and the entered coefficient of variation
/// (a little less where the limit of twelve average months binds), and a year's
/// total varies by the entered figure over the square root of twelve.
#[test]
fn the_lump_keeps_the_yearly_total_in_expectation() {
    assert!(lump_table(0.0, 12.0).is_none());
    assert!(lump_table(f64::NAN, 12.0).is_none());
    for (cv, sd_low, sd_high) in [(0.3, 0.297, 0.303), (1.0, 0.95, 1.0), (2.0, 1.5, 2.0)] {
        let table = lump_table(cv, 12.0).unwrap();
        close(table.mean(), 1.0, 1e-12);
        let (mean, sd, max) = moments(&table, 2_000_000);
        close(mean, 1.0, 4.0 * cv / 2_000_000f64.sqrt() + 1e-4);
        assert!(sd > sd_low && sd < sd_high, "cv {cv}: sd {sd}");
        // The limit is twelve times the mean before rescaling; the rescaling is
        // by less than 5%.
        assert!(max <= 12.6, "cv {cv}: largest month {max}");
    }
    // Twelve independent months: the year's total has the mean of twelve months
    // and a relative spread of cv / sqrt(12).
    let b = business(BusinessFailure::Never, 0.5, true);
    assert!(b.failure.is_none());
    let years = 200_000u64;
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    let mut r = rng(3);
    let mut s = b.initial_state();
    for _ in 0..years {
        let mut year = 0.0;
        for _ in 0..12 {
            year += b.step(&mut s, true, false, &mut r).lump;
        }
        sum += year / 12.0;
        sum_sq += (year / 12.0) * (year / 12.0);
    }
    let mean = sum / years as f64;
    close(mean, 1.0, 0.002);
    close(
        (sum_sq / years as f64 - mean * mean).sqrt(),
        0.5 / 12f64.sqrt(),
        0.002,
    );
    assert_eq!(s, BusinessState::default(), "lumpiness keeps no state");
}

/// Through the model: the yearly profit factor on the career stream is untouched,
/// and profit is the default's times the level times the lump.
#[test]
fn the_model_applies_level_and_lump_on_top_of_the_default_profit() {
    let plain = SelfEmployment {
        volatility: 0.3,
        ..income(BusinessFailure::Never, 0.0)
    };
    let risky = SelfEmployment {
        failure: hazard(Some(0.0), restart()),
        monthly_lumpiness: 0.7,
        ..plain
    };
    let (a, b) = (
        SelfEmploymentModel::new(&plain, true, data()),
        SelfEmploymentModel::new(&risky, true, data()),
    );
    let mut closures = 0;
    for path in 0..300 {
        let mut ca = Rng::stream(5, path, Stream::Career(0));
        let mut cb = Rng::stream(5, path, Stream::Career(0));
        let mut business = rng(path);
        let (mut sa, mut sb) = (a.initial_state(), b.initial_state());
        for t in 0..240u32 {
            let c = CareerClimate {
                now: START.add_months(t as i32),
                age: 40.0,
                price_level: 1.0,
                recession: 0,
                equity_return: 0.0,
                equity_return_12m: 0.0,
            };
            let pa = a.step(&mut sa, 0.0, &c, &mut ca);
            let pb = b.step(&mut sb, 0.0, &c, &mut cb);
            assert_eq!(pa.to_bits(), pb.to_bits(), "the yearly factor is unchanged");
            let month = b.business_step(&mut sb, true, false, &mut business);
            let paid = pb * sb.business.level * month.lump;
            assert!(paid >= 0.0 && paid.is_finite());
            closures += u32::from(month.closed);
        }
        assert_eq!(ca.next_u64(), cb.next_u64());
    }
    assert!(closures > 100, "{closures}");
}

/// Hostile settings are repaired, never a panic or a NaN.
#[test]
fn hostile_settings_stay_finite() {
    let bad = BusinessFailure::Hazard {
        young_annual_probability: f64::NAN,
        mature_annual_probability: 7.0,
        young_years: f64::INFINITY,
        years_in_business: Some(-3.0),
        recession_multiplier: f64::NAN,
        then: AfterFailure::Restart {
            gap_months: u32::MAX,
            income_fraction: f64::NAN,
        },
    };
    let b = business(bad, f64::INFINITY, true);
    let f = b.failure.unwrap();
    assert!(f.hazard.iter().flatten().all(|q| (0.0..=1.0).contains(q)));
    assert_eq!(f.restart, Some((1200, 0.0)));
    let mut s = b.initial_state();
    let mut r = rng(1);
    for _ in 0..600 {
        let month = b.step(&mut s, true, true, &mut r);
        assert!(month.lump.is_finite() && s.level.is_finite() && s.open_level.is_finite());
    }
    assert!(f.expected_level(50).iter().all(|x| x.is_finite()));
}
