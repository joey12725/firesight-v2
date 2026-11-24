use super::super::*;
use super::*;
use crate::model::{CareerRisk, EquityGrant, ExitSize, ExitTiming, StockRuin, TailShape};
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

/// A listed employer with no market exposure, so the price moves only for reasons
/// of its own.
fn public(volatility: f64, tails: TailShape, ruin: StockRuin, layoff_link: bool) -> EquityCompany {
    EquityCompany::Public {
        volatility,
        beta: 0.0,
        tails,
        ruin,
        layoff_link,
    }
}

fn plain(volatility: f64) -> EquityCompany {
    public(volatility, TailShape::Normal, StockRuin::Never, false)
}

fn private(fail_prob: f64, exit_timing: ExitTiming, exit_size: ExitSize) -> EquityCompany {
    EquityCompany::Private {
        liquidity_hazard: 0.12,
        fail_prob,
        exit_volatility: 1.0,
        exit_timing,
        exit_size,
    }
}

/// A job with one grant of 48,000 vesting over 48 months and yearly refreshes.
fn job(company: EquityCompany, level: RiskLevel) -> JobModel {
    let j = Job {
        salary: 100_000.0,
        growth: WageGrowth::Flat { real_raise: 0.0 },
        risk: CareerRisk {
            level,
            ..CareerRisk::default()
        },
        equity: vec![EquityGrant {
            id: "g".into(),
            company,
            unvested_value: 48_000.0,
            vest_months_remaining: 48,
            refresh_annual_value: 12_000.0,
            ..EquityGrant::default()
        }],
        ..Job::default()
    };
    JobModel::new(&j, 35.0, true, data())
}

#[test]
fn every_shape_is_off_by_default() {
    let EquityCompany::Public {
        tails,
        ruin,
        layoff_link,
        ..
    } = EquityCompany::default()
    else {
        panic!("the default company is listed");
    };
    assert_eq!(
        (tails, ruin, layoff_link),
        (TailShape::Normal, StockRuin::Never, false)
    );
    for company in [
        EquityCompany::default(),
        private(0.5, ExitTiming::ConstantHazard, ExitSize::Classes),
    ] {
        let m = job(company, RiskLevel::Typical);
        let e = &m.equity;
        assert!(e.public.is_none() && e.link.is_none());
        assert!(e.exit_hazard.is_none() && e.pareto.is_none());
    }
    // The link needs the job's layoffs to be simulated.
    let linked = public(0.5, TailShape::Normal, StockRuin::Never, true);
    assert!(job(linked, RiskLevel::None).equity.link.is_none());
    assert!(job(linked, RiskLevel::Typical).equity.link.is_some());
}

// ------------------------------------------------------------------ fat tails

/// The fat factor has mean 1 and the lognormal factor's standard deviation, far
/// more extreme months, and uses the same word: the equity stream is where the
/// default leaves it.
#[test]
fn fat_tails_keep_mean_and_volatility_and_add_extremes() {
    let thin = job(plain(0.5), RiskLevel::None);
    let fat = job(
        public(0.5, TailShape::Fat, StockRuin::Never, false),
        RiskLevel::None,
    );
    let table = fat.equity.public.as_ref().unwrap().fat.as_ref().unwrap();
    close(table.mean(), 1.0, 1e-12);
    // Monthly log sd of the company-specific part: 0.5 / sqrt(12).
    let s = 0.5 / 12f64.sqrt();
    let lognormal_sd = math::exp_m1(s * s).sqrt();
    close((table.second_moment() - 1.0).sqrt(), lognormal_sd, 1e-5);

    let months = 120u32;
    let paths = 20_000u64;
    let (mut sum_thin, mut sum_fat) = (0.0, 0.0);
    let (mut big_thin, mut big_fat, mut moves) = (0u64, 0u64, 0u64);
    for path in 0..paths {
        let (mut ca, mut ea) = rngs(path);
        let (mut cb, mut eb) = rngs(path);
        let (mut sa, mut sb) = (thin.initial_state(), fat.initial_state());
        for t in 0..months {
            let (pa, pb) = (sa.stock_price, sb.stock_price);
            thin.step(&mut sa, &climate(t, 35.0), &mut ca, &mut ea);
            fat.step(&mut sb, &climate(t, 35.0), &mut cb, &mut eb);
            // A month's move of more than four standard deviations.
            big_thin += u64::from(math::ln(sa.stock_price / pa).abs() > 4.0 * s);
            big_fat += u64::from(math::ln(sb.stock_price / pb).abs() > 4.0 * s);
            moves += 1;
            if t == 11 {
                sum_thin += sa.stock_price;
                sum_fat += sb.stock_price;
            }
        }
        assert_eq!(ea.next_u64(), eb.next_u64(), "same equity draws");
        assert_eq!(ca.next_u64(), cb.next_u64(), "same career draws");
        assert!(!sb.stock_ruined && sb.idio_12m == 0.0);
    }
    // After a year the price has mean 1 and sd about 0.53: three standard errors
    // over 20,000 paths are about 0.011.
    close(sum_thin / paths as f64, 1.0, 0.015);
    close(sum_fat / paths as f64, 1.0, 0.015);
    // A normal month is beyond four standard deviations 6 times in 100,000; a
    // unit-variance Student-t(4) about 5 times in 1,000 (the scale solved to keep
    // the volatility moves it a little).
    let (thin_rate, fat_rate) = (
        big_thin as f64 / moves as f64,
        big_fat as f64 / moves as f64,
    );
    assert!(thin_rate < 2e-4, "{thin_rate}");
    assert!(fat_rate > 2e-3 && fat_rate < 1e-2, "{fat_rate}");
}

// ----------------------------------------------------------------------- ruin

/// The preset hazards are read by the stock's own volatility exactly as the
/// single-stock sleeve reads them; a stated hazard is used as given.
#[test]
fn ruin_hazard_follows_the_volatility_presets() {
    let p = params().equity;
    assert_eq!(
        p.ruin_presets,
        [(0.25, 0.003), (0.35, 0.01), (0.50, 0.03)],
        "equity_compensation.single_public_stock.defaults"
    );
    let by = |vol: f64| ruin_annual(StockRuin::ByVolatility, vol, &p);
    close(by(0.25), 0.003, 1e-15);
    close(by(0.35), 0.01, 1e-15);
    close(by(0.50), 0.03, 1e-15);
    // Halfway between two presets, halfway between their hazards; flat beyond.
    close(by(0.30), 0.0065, 1e-15);
    close(by(0.125), 0.0015, 1e-15);
    close(by(0.9), 0.03, 1e-15);
    assert_eq!(by(0.0), 0.0);
    assert_eq!(ruin_annual(StockRuin::Never, 0.5, &p), 0.0);
    let stated = |x: f64| {
        ruin_annual(
            StockRuin::Stated {
                annual_probability: x,
            },
            0.5,
            &p,
        )
    };
    assert_eq!(stated(0.07), 0.07);
    assert_eq!(stated(f64::NAN), 0.0);
    assert_eq!(stated(5.0), 0.9);

    // A job reads the hazard from the company-specific part of its volatility.
    let m = job(
        public(0.35, TailShape::Normal, StockRuin::ByVolatility, false),
        RiskLevel::None,
    );
    let shape = m.equity.public.as_ref().unwrap();
    close(shape.ruin_monthly, 1.0 - math::pow(0.99, 1.0 / 12.0), 1e-12);
    close(
        shape.survivor_boost,
        1.0 / (1.0 - shape.ruin_monthly),
        1e-15,
    );
}

/// With a stock that does not otherwise move, the price is zero on the stated
/// share of paths and `1 / (survival)` on the rest, so its mean is 1: the hazard
/// takes nothing from the expected return.
#[test]
fn ruin_arrives_at_the_stated_rate_and_keeps_the_expected_price() {
    let ruin = StockRuin::Stated {
        annual_probability: 0.12,
    };
    let m = job(public(0.0, TailShape::Normal, ruin, false), RiskLevel::None);
    let paths = 100_000u64;
    let (mut ruined, mut sum, mut vested_after) = (0u64, 0.0, 0.0);
    for path in 0..paths {
        let (mut career, mut equity) = rngs(path);
        let mut s = m.initial_state();
        for t in 0..12 {
            let month = m.step(&mut s, &climate(t, 35.0), &mut career, &mut equity);
            if s.stock_ruined {
                vested_after += month.equity_income;
                assert_eq!(m.unvested_value(&s), 0.0);
            }
        }
        if s.stock_ruined {
            ruined += 1;
            assert_eq!(s.stock_price, 0.0);
        } else {
            close(s.stock_price, 1.0 / 0.88, 1e-9);
        }
        sum += s.stock_price;
    }
    let n = paths as f64;
    // Three standard errors of a 12% proportion over 100,000 paths.
    close(ruined as f64 / n, 0.12, 0.0031);
    close(sum / n, 1.0, 0.0036);
    assert_eq!(vested_after, 0.0, "worthless shares pay nothing");
}

// ----------------------------------------------------------------- layoff link

/// The link's rule: three times the ordinary chance while the trailing return is
/// below a 40% fall, a certain layoff at ruin.
#[test]
fn the_link_triples_the_chance_after_a_crash_and_ends_the_job_at_ruin() {
    let p = params().equity;
    assert_eq!(p.link_hazard_multiplier, 3.0);
    assert_eq!(p.link_crash_return, -0.4);
    assert_eq!(LINK_MONTHS, 12);
    let ruin = StockRuin::Stated {
        annual_probability: 0.05,
    };
    let m = job(
        public(0.5, TailShape::Normal, ruin, true),
        RiskLevel::Typical,
    );
    let link = m.equity.link.unwrap();
    close(link.log_threshold, math::ln(0.6), 1e-15);
    let h = 0.004;
    let mut s = m.initial_state();
    // No crash: the ordinary chance.
    s.idio_12m = math::ln(0.61);
    assert!(link.laid_off(&s, h, 0.0039, 0.5));
    assert!(!link.laid_off(&s, h, 0.0041, 0.5));
    // Crashed: three times it.
    s.idio_12m = math::ln(0.59);
    assert!(link.laid_off(&s, h, 0.0119, 0.5));
    assert!(!link.laid_off(&s, h, 0.0121, 0.5));
    // Never above certainty: three times 0.9 is a certain layoff, not more.
    assert!(link.laid_off(&s, 0.9, 0.999_999, 0.5));
    // Ruin this month (the liquidity word is below the monthly chance), or a
    // stock already worthless: laid off whatever the layoff word.
    let q = m.equity.public.as_ref().unwrap().ruin_monthly;
    assert!(link.laid_off(&s, h, 0.99, q * 0.99));
    assert!(!link.laid_off(&s, h, 0.99, q * 1.01));
    s.stock_ruined = true;
    assert!(link.laid_off(&s, h, 0.99, 0.99));
}

/// On a path: the stock price is the same bits with the link on or off, a ruin
/// lays the person off in the same month, a rehire starts a new employer's stock,
/// and neither stream's draws move.
#[test]
fn the_link_moves_layoffs_and_nothing_else() {
    let ruin = StockRuin::Stated {
        annual_probability: 0.04,
    };
    let off = job(
        public(0.8, TailShape::Fat, ruin, false),
        RiskLevel::Typical,
    );
    let on = job(public(0.8, TailShape::Fat, ruin, true), RiskLevel::Typical);
    let (mut layoffs_off, mut layoffs_on, mut ruin_layoffs, mut restarts) = (0, 0, 0, 0);
    let (mut crashed_months, mut months_at_risk) = (0u64, 0u64);
    for path in 0..3_000 {
        let (mut ca, mut ea) = rngs(path);
        let (mut cb, mut eb) = rngs(path);
        let (mut sa, mut sb) = (off.initial_state(), on.initial_state());
        let mut diverged = false;
        for t in 0..240u32 {
            let c = climate(t, 35.0);
            let was_ruined = sb.stock_ruined;
            let a = off.step(&mut sa, &c, &mut ca, &mut ea);
            let b = on.step(&mut sb, &c, &mut cb, &mut eb);
            layoffs_off += u32::from(a.laid_off);
            layoffs_on += u32::from(b.laid_off);
            if !diverged {
                // Until the first layoff that differs, the price is the same bits.
                assert_eq!(sa.stock_price.to_bits(), sb.stock_price.to_bits());
            }
            diverged |= a.laid_off != b.laid_off;
            if sb.stock_ruined && !was_ruined {
                // Ruined this month while employed: laid off this month (someone
                // who returned to work this very month is laid off the next).
                assert!(b.laid_off || b.rehired || sb.employment != Employment::Employed);
                ruin_layoffs += u32::from(b.laid_off);
            }
            if b.rehired && was_ruined {
                assert!(!sb.stock_ruined || sb.stock_price == 0.0);
                restarts += 1;
            }
            months_at_risk += u64::from(sb.employment == Employment::Employed);
            crashed_months += u64::from(
                sb.employment == Employment::Employed
                    && sb.idio_12m < on.equity.link.unwrap().log_threshold,
            );
            assert!(sb.stock_price.is_finite() && sb.idio_12m.is_finite());
        }
        assert_eq!(ca.next_u64(), cb.next_u64(), "same career draws");
        assert_eq!(ea.next_u64(), eb.next_u64(), "same equity draws");
    }
    assert!(ruin_layoffs > 100, "{ruin_layoffs}");
    assert!(restarts > 100, "{restarts}");
    assert!(crashed_months * 20 > months_at_risk, "the stock crashes often");
    assert!(
        layoffs_on > layoffs_off + 500,
        "layoffs {layoffs_off} without the link, {layoffs_on} with it"
    );
}

/// A stock that never moves never crashes: with the link on and no ruin, every
/// month is the same bits as without it.
#[test]
fn the_link_is_silent_when_the_stock_is_quiet() {
    let off = job(plain(0.0), RiskLevel::High);
    let on = job(
        public(0.0, TailShape::Normal, StockRuin::Never, true),
        RiskLevel::High,
    );
    for path in 0..300 {
        let (mut ca, mut ea) = rngs(path);
        let (mut cb, mut eb) = rngs(path);
        let (mut sa, mut sb) = (off.initial_state(), on.initial_state());
        for t in 0..240u32 {
            let a = off.step(&mut sa, &climate(t, 35.0), &mut ca, &mut ea);
            let b = on.step(&mut sb, &climate(t, 35.0), &mut cb, &mut eb);
            assert_eq!(a, b);
        }
        assert_eq!(sa.real_salary.to_bits(), sb.real_salary.to_bits());
    }
}

// ---------------------------------------------------------------- exit timing

/// The table is the discrete hazard of the lognormal: multiplying the monthly
/// survival chances gives the lognormal's survival function.
#[test]
fn the_exit_hazard_is_the_lognormal_s() {
    let table = exit_hazard_table(5.0, 0.5);
    assert_eq!(table.len(), 1200);
    let mut survival = 1.0;
    let mut by_month = Vec::new();
    for h in &table {
        assert!((0.0..=1.0).contains(h));
        survival *= 1.0 - h;
        by_month.push(survival);
    }
    // Half have had their outcome at the median of five years.
    close(by_month[59], 0.5, 1e-12);
    // P(T > 1 year) = Phi(ln(5) / 0.5) = Phi(3.2189): 0.9994.
    close(by_month[11], math::normal_cdf(math::ln(5.0) / 0.5), 1e-12);
    close(by_month[11], 0.9994, 1e-4);
    // P(T > 10 years) = 1 - Phi(ln(2) / 0.5) = 1 - Phi(1.3863): 0.083.
    close(by_month[119], math::normal_cdf(-math::ln(2.0) / 0.5), 1e-12);
    close(by_month[119], 0.083, 5e-4);
    // The hazard rises from almost nothing, peaks, and falls: unlike a constant.
    let peak = (0..table.len())
        .max_by(|a, b| table[*a].total_cmp(&table[*b]))
        .unwrap();
    assert!(table[0] < 1e-9 && table[11] < 1e-3);
    assert!((60..150).contains(&peak), "peak in month {peak}");
    assert!(table[peak] > 2.0 * table[30] && table[peak] > 1.2 * table[600]);
    // Degenerate inputs give a usable table.
    for (median, sigma) in [(f64::NAN, 0.5), (0.0, 0.0), (1e9, 1e-9), (40.0, 2.0)] {
        assert!(exit_hazard_table(median, sigma)
            .iter()
            .all(|h| (0.0..=1.0).contains(h)));
    }
}

/// Month of the outcome on one path, if it came within `months`.
fn exit_month(m: &JobModel, path: u64, months: u32, recession_until: u32) -> Option<(u32, f64)> {
    let (mut career, mut equity) = rngs(path);
    let mut s = m.initial_state();
    for t in 0..months {
        let mut c = climate(t, 35.0);
        c.recession = u8::from(t < recession_until);
        m.step(&mut s, &c, &mut career, &mut equity);
        if s.liquidity != Liquidity::Pending {
            return Some((t, s.stock_price));
        }
    }
    None
}

/// On paths: with the lognormal wait almost no company has its outcome in the
/// first year (the constant hazard gives 12%), half have had it by the median,
/// and recession months stop the clock.
#[test]
fn outcomes_follow_the_lognormal_wait() {
    let constant = job(
        private(0.0, ExitTiming::ConstantHazard, ExitSize::Classes),
        RiskLevel::None,
    );
    let lognormal = job(
        private(0.0, ExitTiming::lognormal(), ExitSize::Classes),
        RiskLevel::None,
    );
    let paths = 40_000u64;
    let share = |m: &JobModel, within: u32| {
        (0..paths)
            .filter(|p| exit_month(m, *p, within, 0).is_some())
            .count() as f64
            / paths as f64
    };
    close(share(&constant, 12), 0.12, 0.005);
    close(share(&lognormal, 12), 0.000_643, 0.0005);
    close(share(&lognormal, 60), 0.5, 0.0076);
    close(share(&lognormal, 120), 1.0 - 0.0829, 0.0042);
    // Two years of recession at the start: no outcome during them, and the clock
    // starts when they end.
    for path in 0..2_000 {
        if let Some((month, _)) = exit_month(&lognormal, path, 400, 24) {
            assert!(month >= 24, "no outcome in a recession month");
        }
    }
    let first_year_after_recession = (0..paths)
        .filter(|p| exit_month(&lognormal, *p, 36, 24).is_some())
        .count() as f64
        / paths as f64;
    close(first_year_after_recession, 0.000_643, 0.0005);
}

// ------------------------------------------------------------------ exit size

/// The Pareto multiple: from 4 to 200, with the mean of the log-uniform class it
/// replaces.
#[test]
fn the_pareto_tail_keeps_the_class_mean() {
    let p = params();
    assert_eq!(p.private_multiple_ranges[2], [4.0, 20.0]);
    assert_eq!(p.equity.pareto_cap_multiple, 200.0);
    let pareto = Pareto::matching(4.0, 20.0, 200.0).unwrap();
    let target = 16.0 / math::ln(5.0);
    close(target, 9.941_3, 1e-4);
    close(Pareto::mean_of(4.0, 200.0, pareto.alpha), target, 1e-10);
    // The index the data file quotes (to two decimals).
    close(pareto.alpha, 1.55, 0.01);
    assert_eq!(pareto.quantile(0.0), 4.0);
    close(pareto.cap(), 200.0, 1e-9);
    // The closed-form mean against a midpoint sum of the quantile function.
    let cells = 2_000_000;
    let mut sum = 0.0;
    let mut last = 0.0;
    for i in 0..cells {
        let x = pareto.quantile((i as f64 + 0.5) / cells as f64);
        assert!(x >= last && (4.0..=200.0).contains(&x));
        last = x;
        sum += x;
    }
    close(sum / cells as f64, target, 2e-3);
    // Its shape against the log-uniform: a lower median (6.3 against 8.9) and one
    // outcome in twelve above the old ceiling of 20.
    let a = pareto.alpha;
    let span = 1.0 - math::pow(0.02, a);
    close(
        pareto.quantile(0.5),
        4.0 * math::pow(1.0 - 0.5 * span, -1.0 / a),
        1e-12,
    );
    assert!(pareto.quantile(0.5) > 6.0 && pareto.quantile(0.5) < 6.5);
    let above_20 = (math::pow(0.2, a) - math::pow(0.02, a)) / span;
    assert!(above_20 > 0.075 && above_20 < 0.09, "{above_20}");
    // The mean formula at an index of exactly one (its removable singularity).
    close(
        Pareto::mean_of(4.0, 200.0, 1.0),
        Pareto::mean_of(4.0, 200.0, 1.0 + 1e-6),
        1e-4,
    );
    // No room for a tail: no Pareto.
    assert!(Pareto::matching(4.0, 20.0, 20.0).is_none());
    assert!(Pareto::matching(0.0, 20.0, 200.0).is_none());
    assert!(Pareto::matching(4.0, f64::NAN, 200.0).is_none());
}

/// On paths: the same companies have the same kind of outcome in the same month;
/// only the large outcomes change size, some now far above 20 times, and the
/// average multiple is the same.
#[test]
fn the_pareto_tail_changes_only_large_outcomes() {
    let classes = job(
        private(0.3, ExitTiming::ConstantHazard, ExitSize::Classes),
        RiskLevel::None,
    );
    let tail = job(
        private(0.3, ExitTiming::ConstantHazard, ExitSize::ParetoTail),
        RiskLevel::None,
    );
    let paths = 150_000u64;
    let (mut n_large, mut sum_a, mut sum_b, mut above_20, mut max) = (0u64, 0.0, 0.0, 0u64, 0.0f64);
    for path in 0..paths {
        let a = exit_month(&classes, path, 360, 0);
        let b = exit_month(&tail, path, 360, 0);
        let (Some((ta, ma)), Some((tb, mb))) = (a, b) else {
            assert_eq!(a.is_none(), b.is_none());
            continue;
        };
        assert_eq!(ta, tb);
        if ma < 4.0 {
            // Failed, modest or good: untouched.
            assert_eq!(ma.to_bits(), mb.to_bits());
        } else {
            assert!((4.0..=200.0).contains(&mb));
            n_large += 1;
            sum_a += ma;
            sum_b += mb;
            above_20 += u64::from(mb > 20.0);
            max = max.max(mb);
        }
    }
    let n = n_large as f64;
    assert!(n_large > 3_000, "{n_large} large outcomes");
    close(sum_a / n, 16.0 / math::ln(5.0), 0.25);
    // The Pareto's standard deviation is about 14: three standard errors over
    // 3,000 or more outcomes are below 0.8.
    close(sum_b / n, 16.0 / math::ln(5.0), 0.8);
    assert!(max > 100.0, "largest multiple {max}");
    let share = above_20 as f64 / n;
    assert!(share > 0.06 && share < 0.1, "{share}");
}
