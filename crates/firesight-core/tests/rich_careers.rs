//! The opt-in shapes of a job's random parts (`risk.wage_shocks`,
//! `risk.layoff_pay_cut`, `risk.job_search`) inside the whole simulation: money is
//! conserved, nothing is NaN, the other subsystems' random numbers do not move, and
//! the effects go the right way. The distributions themselves are tested in
//! `src/career/shape_tests.rs`.

mod common;

use common::*;
use firesight_core::math;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, PathEnd, PathScalar, PathTrace, Recorder, TraceEventKind, YearRecord, YearSeries,
};

/// Applies `change` to the risk settings of every job.
fn with_risk(mut p: Profile, change: impl Fn(&mut CareerRisk)) -> Profile {
    for income in &mut p.incomes {
        if let IncomeKind::Job(job) = &mut income.kind {
            change(&mut job.risk);
        }
    }
    p
}

fn fat(r: &mut CareerRisk) {
    r.wage_shocks = WageShockShape::FatTailed;
}

fn two_part(r: &mut CareerRisk) {
    r.layoff_pay_cut = PayCutShape::TwoPart;
}

fn monthly(r: &mut CareerRisk) {
    r.job_search = JobSearchModel::Monthly;
}

fn all_on(r: &mut CareerRisk) {
    fat(r);
    two_part(r);
    monthly(r);
}

/// Forwards to an accumulator after checking that every number is finite and of a
/// sane size.
struct Finite {
    inner: Accumulator,
}

impl Recorder for Finite {
    fn begin_path(&mut self, path_index: u64) {
        self.inner.begin_path(path_index);
    }
    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        for (s, v) in YearSeries::ALL.iter().zip(record.values) {
            assert!(v.is_finite(), "year {year_index} {}: {v}", s.key());
            assert!(v.abs() < 1e13, "year {year_index} {} = {v}", s.key());
            assert!(
                v >= -1.0 || *s == YearSeries::InvestmentReturn,
                "year {year_index} {} is negative: {v}",
                s.key()
            );
        }
        self.inner.record_year(year_index, record);
    }
    fn end_path(&mut self, end: &PathEnd) {
        for s in PathScalar::ALL {
            assert!(!end.get(*s).is_infinite(), "scalar {}", s.key());
        }
        self.inner.end_path(end);
    }
}

fn valid(profile: &Profile) {
    let report = firesight_core::validate::validate(profile);
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
}

#[test]
fn every_shape_on_conserves_money_and_stays_finite_under_every_market_kind() {
    for example in Profile::examples(START) {
        for kind in MARKETS {
            // The riskiest level, so that layoffs and large shocks are common.
            let profile = with_risk(with_market(example.profile.clone(), kind), |r| {
                all_on(r);
                r.level = RiskLevel::High;
            });
            valid(&profile);
            let plan = compile(&profile);
            // Check mode: the conservation identities are asserted inside the loop.
            let acc = run_checked(&plan, 31, 0..150);
            assert_eq!(plan.summarize(&acc).paths, 150);
            let mut ws = Workspace::new(&plan);
            let mut rec = Finite {
                inner: plan.new_accumulator(),
            };
            for i in 0..400 {
                sim::run_path(&plan, 17, i, &mut ws, &mut rec);
            }
            let summary = plan.summarize(&rec.inner);
            let rate = summary.headline.success_rate.value;
            assert!((0.0..=1.0).contains(&rate), "{} {kind:?}", example.slug);
            assert!(summary.headline.median_terminal_net_worth.is_finite());
        }
    }
}

fn scalar(t: &PathTrace, s: PathScalar) -> f64 {
    t.scalars.iter().find(|x| x.scalar == s).unwrap().value
}

fn events_of(t: &PathTrace, kind: TraceEventKind) -> Vec<YearMonth> {
    t.events
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.month)
        .collect()
}

/// Switching a shape on changes pay and nothing else random: lifespans, long-term
/// care, prices and every asset's returns are the same bits. The wage and pay-cut
/// shapes also leave every layoff, every return to work and the months out of work
/// where they were; the monthly search moves only the returns to work (and so the
/// layoffs that can follow them).
#[test]
fn each_shape_moves_only_pay() {
    // A fixed retirement date, so that nothing depends on when the household can
    // afford to stop working.
    let mut base = with_risk(Profile::example(), |r| r.level = RiskLevel::High);
    base.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    type Change = (&'static str, bool, fn(&mut CareerRisk));
    let changes: [Change; 4] = [
        ("wage shocks", true, fat),
        ("pay cut", true, two_part),
        ("job search", false, monthly),
        ("all", false, all_on),
    ];
    let plan_base = compile(&base);
    for (name, spells_fixed, change) in changes {
        let p = with_risk(base.clone(), change);
        valid(&p);
        let plan = compile(&p);
        let (mut changed, mut layoffs) = (0, 0);
        for path in 0..16 {
            let a = sim::trace_path(&plan_base, 77, path);
            let b = sim::trace_path(&plan, 77, path);
            assert_eq!(a.months, b.months, "{name} path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::LongTermCareCost,
                PathScalar::Inflation,
                PathScalar::RetirementAge,
            ] {
                assert_eq!(
                    scalar(&a, s).to_bits(),
                    scalar(&b, s).to_bits(),
                    "{name} path {path} {s:?}"
                );
            }
            for key in [
                "price_level",
                "return_us_stocks",
                "return_intl_stocks",
                "return_bonds",
                "return_cash",
                "return_real_estate",
                "return_gold",
                "return_crypto",
                "return_single_stock",
            ] {
                assert!(a.column(key) == b.column(key), "{name} path {path} {key}");
            }
            let laid_off = events_of(&a, TraceEventKind::LaidOff);
            layoffs += laid_off.len();
            if spells_fixed {
                assert_eq!(laid_off, events_of(&b, TraceEventKind::LaidOff));
                assert_eq!(
                    events_of(&a, TraceEventKind::Rehired),
                    events_of(&b, TraceEventKind::Rehired),
                    "{name} path {path}"
                );
                assert_eq!(
                    scalar(&a, PathScalar::MonthsUnemployed).to_bits(),
                    scalar(&b, PathScalar::MonthsUnemployed).to_bits()
                );
            } else {
                // The first layoff of the household cannot have moved: nothing
                // differs before it.
                assert_eq!(
                    laid_off.first(),
                    events_of(&b, TraceEventKind::LaidOff).first(),
                    "{name} path {path}"
                );
            }
            changed += usize::from(a.column("income_wages") != b.column("income_wages"));
        }
        assert!(layoffs >= 16, "{name}: only {layoffs} layoffs to compare");
        // Wage shocks change every path; the others change every path with a layoff.
        assert!(changed >= 12, "{name}: changed on {changed} of 16 paths");
    }
}

/// With career risk switched off, or the job's risk level at `none`, the shapes
/// have nothing to act on: results are the same bits as without them.
#[test]
fn shapes_do_nothing_when_career_risk_is_off() {
    let mut off = Profile::example();
    off.assumptions.career_risk = false;
    let none = with_risk(Profile::example(), |r| r.level = RiskLevel::None);
    for base in [off, none] {
        let shaped = with_risk(base.clone(), all_on);
        valid(&shaped);
        let (plan_a, plan_b) = (compile(&base), compile(&shaped));
        let a = run(&plan_a, 5, 0..60);
        let b = run(&plan_b, 5, 0..60);
        assert_eq!(a.to_bytes(), b.to_bytes());
    }
}

#[test]
fn new_fields_round_trip_through_json() {
    let p = with_risk(Profile::example(), all_on);
    let back = Profile::from_json(&p.to_json_pretty()).unwrap();
    assert_eq!(p, back);
    let text = serde_json::json!({
        "incomes": [{
            "id": "job",
            "kind": {
                "type": "job",
                "salary": 90000.0,
                "risk": {
                    "level": "high",
                    "wage_shocks": "fat_tailed",
                    "layoff_pay_cut": "two_part",
                    "job_search": "monthly"
                }
            }
        }]
    })
    .to_string();
    let p = Profile::from_json(&text).unwrap();
    let IncomeKind::Job(job) = &p.incomes[0].kind else {
        panic!("not a job");
    };
    assert_eq!(
        job.risk,
        CareerRisk {
            level: RiskLevel::High,
            wage_shocks: WageShockShape::FatTailed,
            layoff_pay_cut: PayCutShape::TwoPart,
            job_search: JobSearchModel::Monthly,
            ..CareerRisk::default()
        }
    );
    // Unknown names are refused.
    for bad in [
        r#"{"incomes": [{"kind": {"type": "job", "risk": {"wage_shocks": "wild"}}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "risk": {"layoff_pay_cut": "lognormal"}}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "risk": {"job_search": "weekly"}}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "risk": {"search": "monthly"}}}]}"#,
    ] {
        assert!(Profile::from_json(bad).is_err(), "{bad}");
    }
    // The defaults are the model as it was, and a job that does not mention the
    // new fields gets them.
    let d = CareerRisk::default();
    assert_eq!(d.wage_shocks, WageShockShape::Normal);
    assert_eq!(d.layoff_pay_cut, PayCutShape::Normal);
    assert_eq!(d.job_search, JobSearchModel::DrawnAtLayoff);
    let old = r#"{"incomes": [{"kind": {"type": "job", "risk": {"level": "low"}}}]}"#;
    let p = Profile::from_json(old).unwrap();
    let IncomeKind::Job(job) = &p.incomes[0].kind else {
        panic!("not a job");
    };
    assert_eq!(
        job.risk,
        CareerRisk {
            level: RiskLevel::Low,
            ..CareerRisk::default()
        }
    );
    // No example household switches a shape on.
    for example in Profile::examples(START) {
        for income in &example.profile.incomes {
            if let IncomeKind::Job(job) = &income.kind {
                let d = CareerRisk::default();
                assert_eq!(
                    (
                        job.risk.wage_shocks,
                        job.risk.layoff_pay_cut,
                        job.risk.job_search
                    ),
                    (d.wage_shocks, d.layoff_pay_cut, d.job_search),
                    "{}",
                    example.slug
                );
            }
        }
    }
}

/// One worker, 30 years old, in a typical job paying $80,000 with no real raises,
/// who stops at 60 whatever happens. No tax and a fixed lifespan, so pay is the only
/// thing the shapes can move.
fn worker(change: impl Fn(&mut CareerRisk)) -> Profile {
    let mut p = quiet(vec![person("worker", 30, Sex::Unspecified)], 90.0);
    p.assumptions.career_risk = true;
    p.incomes = vec![job("job", 0, 80_000.0)];
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("savings", 0, AccountKind::Taxable, 50_000.0)];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 60.0,
    };
    with_risk(p, |r| {
        r.level = RiskLevel::Typical;
        change(r);
    })
}

/// What one working life looked like.
struct Career {
    /// Pay received over the 30 years as a fraction of 30 years of the entered
    /// salary, in today's dollars.
    lifetime: f64,
    /// Months out of work.
    idle: f64,
    /// Lengths of the completed spells out of work, in whole calendar months
    /// between the layoff and the return.
    spells: Vec<i32>,
    /// Year-on-year log changes of March pay, for years worked in full.
    changes: Vec<f64>,
    /// Log change of pay from the month before each layoff to the month after the
    /// return to work, when no January fell in between.
    cuts: Vec<f64>,
}

fn careers(change: fn(&mut CareerRisk), seed: u64, paths: u64) -> Vec<Career> {
    let profile = worker(change);
    valid(&profile);
    let plan = compile(&profile);
    (0..paths)
        .map(|path| {
            let t = sim::trace_path(&plan, seed, path);
            let wages = t.column("income_wages").expect("the worker is paid");
            let working = &wages[..360.min(wages.len())];
            let month_of = |ym: YearMonth| (ym - t.start) as usize;
            let laid_off = events_of(&t, TraceEventKind::LaidOff);
            let rehired = events_of(&t, TraceEventKind::Rehired);
            let mut spells = Vec::new();
            let mut cuts = Vec::new();
            for (out, back) in laid_off.iter().zip(&rehired) {
                spells.push(*back - *out);
                let (before, after) = (month_of(*out), month_of(*back) + 1);
                // The same pay year on both sides, so that the yearly shocks cancel.
                let same_year = (before..=after).all(|m| (t.start + m as i32).month() != 1);
                if before >= 1
                    && after < working.len()
                    && same_year
                    && working[before - 1] > 0.0
                    && working[after] > 0.0
                {
                    cuts.push(math::ln(working[after] / working[before - 1]));
                }
            }
            // March of each year (the plan starts in November, so month 4), for
            // years with no month out of work in the twelve before.
            let mut changes = Vec::new();
            let mut m = 16;
            while m < working.len() {
                let full_year = working[m - 12..=m].iter().all(|w| *w > 0.0)
                    && !laid_off
                        .iter()
                        .chain(&rehired)
                        .any(|e| (m - 12..=m).contains(&month_of(*e)));
                if full_year {
                    changes.push(math::ln(working[m] / working[m - 12]));
                }
                m += 12;
            }
            Career {
                lifetime: working.iter().sum::<f64>() / (30.0 * 80_000.0),
                idle: scalar(&t, PathScalar::MonthsUnemployed),
                spells,
                changes,
                cuts,
            }
        })
        .collect()
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let (mut n, mut sum) = (0.0, 0.0);
    for x in v {
        n += 1.0;
        sum += x;
    }
    sum / n
}

fn share(v: impl Iterator<Item = bool>) -> f64 {
    mean(v.map(|b| f64::from(u8::from(b))))
}

fn quantile(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p) as usize]
}

fn kurtosis(v: &[f64]) -> f64 {
    let m = mean(v.iter().copied());
    let m2 = mean(v.iter().map(|x| (x - m) * (x - m)));
    mean(v.iter().map(|x| math::powi(x - m, 4))) / (m2 * m2)
}

fn skewness(v: &[f64]) -> f64 {
    let m = mean(v.iter().copied());
    let m2 = mean(v.iter().map(|x| (x - m) * (x - m)));
    mean(v.iter().map(|x| math::powi(x - m, 3))) / (m2 * m2.sqrt())
}

#[test]
fn effects_go_the_right_way_for_a_worker() {
    let paths = 1_500;
    let base = careers(|_| {}, 2026, paths);
    let lifetime = |c: &[Career]| mean(c.iter().map(|c| c.lifetime));

    // Fat-tailed wage shocks: the same layoffs and spells, the same average pay,
    // and yearly changes with more quiet years, fatter tails and a lean downwards.
    let shaped = careers(fat, 2026, paths);
    for (a, b) in base.iter().zip(&shaped) {
        assert_eq!(a.idle.to_bits(), b.idle.to_bits());
        assert_eq!(a.spells, b.spells);
    }
    let rel = lifetime(&shaped) / lifetime(&base);
    assert!((rel - 1.0).abs() < 0.02, "lifetime pay ratio {rel}");
    let all = |c: &[Career]| -> Vec<f64> { c.iter().flat_map(|c| c.changes.clone()).collect() };
    let (plain, fat_changes) = (all(&base), all(&shaped));
    assert!(plain.len() > 20_000);
    // The observed change also holds the transitory shock, which stays normal, so
    // the kurtosis of 11 of the persistent shock shows as about 4.
    let (k0, k1) = (kurtosis(&plain), kurtosis(&fat_changes));
    assert!(k0 < 3.3, "normal shocks: kurtosis {k0}");
    assert!(k1 > k0 + 0.5, "kurtosis {k1} against {k0}");
    let (s0, s1) = (skewness(&plain), skewness(&fat_changes));
    assert!(s0.abs() < 0.04, "normal shocks: skewness {s0}");
    assert!(s1 < -0.06, "skewness {s1}");
    let quiet = |v: &[f64]| share(v.iter().map(|x| x.abs() < 0.05));
    assert!(quiet(&fat_changes) > quiet(&plain));

    // Two-part pay cut: the same layoffs and spells; the same average log change
    // across a layoff; far more returns at about the old pay and fewer raises.
    let shaped = careers(two_part, 2026, paths);
    for (a, b) in base.iter().zip(&shaped) {
        assert_eq!(a.idle.to_bits(), b.idle.to_bits());
        assert_eq!(a.spells, b.spells);
    }
    let cuts = |c: &[Career]| -> Vec<f64> { c.iter().flat_map(|c| c.cuts.clone()).collect() };
    let (plain, two) = (cuts(&base), cuts(&shaped));
    assert!(plain.len() > 1_000 && plain.len() == two.len());
    let (m0, m1) = (mean(plain.iter().copied()), mean(two.iter().copied()));
    assert!(m0 < -0.05 && (m1 - m0).abs() < 0.015, "{m1} against {m0}");
    let near = |v: &[f64]| share(v.iter().map(|x| x.abs() < 0.05));
    assert!(near(&two) > near(&plain) + 0.1);
    let raise = |v: &[f64]| share(v.iter().map(|x| *x > 0.1));
    assert!(raise(&two) < 0.5 * raise(&plain));
    let rel = lifetime(&shaped) / lifetime(&base);
    assert!((rel - 1.0).abs() < 0.02, "lifetime pay ratio {rel}");

    // Monthly search: about the same time out of work on average.
    let shaped = careers(monthly, 2026, paths);
    let idle = |c: &[Career]| mean(c.iter().map(|c| c.idle));
    let rel = idle(&shaped) / idle(&base);
    assert!((rel - 1.0).abs() < 0.12, "months out of work ratio {rel}");
    assert!(base
        .iter()
        .zip(&shaped)
        .any(|(a, b)| a.idle.to_bits() != b.idle.to_bits()));
    let rel = lifetime(&shaped) / lifetime(&base);
    assert!((rel - 1.0).abs() < 0.02, "lifetime pay ratio {rel}");
}

/// Prints the figures quoted in docs/methodology.md:
/// `cargo test -p firesight-core --release --test rich_careers report -- --ignored --nocapture`.
#[test]
#[ignore = "report: prints career statistics and success rates with each shape on"]
fn report_measured_effects() {
    type Change = (&'static str, fn(&mut CareerRisk));
    let changes: [Change; 5] = [
        ("default", |_| {}),
        ("fat-tailed shocks", fat),
        ("two-part pay cut", two_part),
        ("monthly search", monthly),
        ("all three", all_on),
    ];
    let paths = 20_000;
    for (label, change) in changes {
        let c = careers(change, 2026, paths);
        let lifetime: Vec<f64> = c.iter().map(|c| c.lifetime).collect();
        let changes: Vec<f64> = c.iter().flat_map(|c| c.changes.clone()).collect();
        let cuts: Vec<f64> = c.iter().flat_map(|c| c.cuts.clone()).collect();
        let spells: Vec<f64> = c
            .iter()
            .flat_map(|c| c.spells.iter().map(|s| f64::from(*s)))
            .collect();
        println!("{label}");
        println!(
            "  lifetime pay / curve: mean {:.3}  p5 {:.3}  p50 {:.3}  p95 {:.3}",
            mean(lifetime.iter().copied()),
            quantile(lifetime.clone(), 0.05),
            quantile(lifetime.clone(), 0.5),
            quantile(lifetime.clone(), 0.95),
        );
        println!(
            "  yearly change: within 5% {:.1}%  beyond 30% {:.2}%  down 30% {:.2}%  up 30% {:.2}%  kurtosis {:.1}  skewness {:.2}",
            100.0 * share(changes.iter().map(|x| x.abs() < 0.05)),
            100.0 * share(changes.iter().map(|x| x.abs() > 0.3)),
            100.0 * share(changes.iter().map(|x| *x < -0.3)),
            100.0 * share(changes.iter().map(|x| *x > 0.3)),
            kurtosis(&changes),
            skewness(&changes),
        );
        println!(
            "  change across a layoff: mean {:.3}  within 5% {:.1}%  raise over 10% {:.1}%  cut over 30% {:.1}%  cut over 40% {:.1}%",
            mean(cuts.iter().copied()),
            100.0 * share(cuts.iter().map(|x| x.abs() < 0.05)),
            100.0 * share(cuts.iter().map(|x| *x > 0.1)),
            100.0 * share(cuts.iter().map(|x| *x < math::ln(0.7))),
            100.0 * share(cuts.iter().map(|x| *x < math::ln(0.6))),
        );
        println!(
            "  months out of work per career: mean {:.2}  p95 {:.1};  spells: mean {:.2} calendar months, over 6 {:.1}%  over 12 {:.1}%  over 24 {:.1}%",
            mean(c.iter().map(|c| c.idle)),
            quantile(c.iter().map(|c| c.idle).collect(), 0.95),
            mean(spells.iter().copied()),
            100.0 * share(spells.iter().map(|s| *s > 6.0)),
            100.0 * share(spells.iter().map(|s| *s > 12.0)),
            100.0 * share(spells.iter().map(|s| *s > 24.0)),
        );
    }
    // The demo households, 10,000 paths at the seed of the golden files.
    for example in Profile::examples(START) {
        for (label, change) in changes {
            let s = summary(&with_risk(example.profile.clone(), change), 2026, 10_000);
            println!(
                "{:22} {:18} success {:5.1}%  median terminal net worth {:>10.0}",
                example.slug,
                label,
                s.headline.success_rate.value * 100.0,
                s.headline.median_terminal_net_worth,
            );
        }
    }
}
