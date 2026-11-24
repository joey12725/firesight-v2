//! The opt-in risks of self-employment income (`failure`, `monthly_lumpiness`) and
//! the opt-in shapes of employer stock (`tails`, `ruin`, `layoff_link`,
//! `exit_timing`, `exit_size`) inside the whole simulation: money is conserved,
//! nothing is NaN, the other subsystems' random numbers do not move, and the
//! effects go the right way. The mechanisms themselves are tested in
//! `src/career/business_tests.rs` and `src/career/equity_tests.rs`.

mod common;

use common::*;
use firesight_core::career::{SelfEmploymentModel, BUSINESS_WORDS_PER_MONTH, EMPLOYER_LINK_MONTHS};
use firesight_core::data::Data;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, PathEnd, PathScalar, PathTrace, Recorder, TraceEventKind, YearRecord, YearSeries,
};

fn restart() -> AfterFailure {
    AfterFailure::Restart {
        gap_months: 12,
        income_fraction: 0.7,
    }
}

fn shop(failure: BusinessFailure, lumpiness: f64) -> Income {
    Income {
        id: "shop".into(),
        name: "Shop".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 60_000.0,
            volatility: 0.25,
            failure,
            monthly_lumpiness: lumpiness,
            ..SelfEmployment::default()
        }),
        ..Income::default()
    }
}

fn equity_job(id: &str, salary: f64, level: RiskLevel, company: EquityCompany) -> Income {
    Income {
        id: id.into(),
        name: id.into(),
        owner: 0,
        kind: IncomeKind::Job(Job {
            salary,
            growth: WageGrowth::Flat { real_raise: 0.0 },
            risk: CareerRisk {
                level,
                ..CareerRisk::default()
            },
            equity: vec![EquityGrant {
                id: "grant".into(),
                company,
                unvested_value: 80_000.0,
                vest_months_remaining: 48,
                refresh_annual_value: 20_000.0,
                sell_on_vest: false,
                ..EquityGrant::default()
            }],
            ..Job::default()
        }),
        ..Income::default()
    }
}

fn public(tails: TailShape, ruin: StockRuin, layoff_link: bool) -> EquityCompany {
    EquityCompany::Public {
        volatility: 0.6,
        beta: 1.2,
        tails,
        ruin,
        layoff_link,
    }
}

fn private(exit_timing: ExitTiming, exit_size: ExitSize) -> EquityCompany {
    EquityCompany::Private {
        liquidity_hazard: 0.12,
        fail_prob: 0.5,
        exit_volatility: 1.0,
        exit_timing,
        exit_size,
    }
}

const RUIN: StockRuin = StockRuin::Stated {
    annual_probability: 0.04,
};

/// Which options are switched on.
#[derive(Clone, Copy, Default)]
struct On {
    failure: bool,
    lumpiness: bool,
    fat: bool,
    ruin: bool,
    link: bool,
    timing: bool,
    pareto: bool,
}

impl On {
    const ALL: On = On {
        failure: true,
        lumpiness: true,
        fat: true,
        ruin: true,
        link: true,
        timing: true,
        pareto: true,
    };
}

/// A household with a business and with stock from a listed and from a private
/// employer added to it, and the chosen options switched on for those and for
/// every self-employment income and grant it already had.
fn enrich(mut p: Profile, on: On) -> Profile {
    p.incomes.push(shop(BusinessFailure::Never, 0.0));
    p.incomes.push(equity_job(
        "listed",
        90_000.0,
        RiskLevel::High,
        public(TailShape::Normal, StockRuin::Never, false),
    ));
    p.incomes.push(equity_job(
        "startup",
        70_000.0,
        RiskLevel::Typical,
        private(ExitTiming::ConstantHazard, ExitSize::Classes),
    ));
    for income in &mut p.incomes {
        match &mut income.kind {
            IncomeKind::SelfEmployment(se) => {
                if on.failure {
                    se.failure = BusinessFailure::hazard();
                }
                if on.lumpiness {
                    se.monthly_lumpiness = 1.0;
                }
            }
            IncomeKind::Job(job) => {
                for grant in &mut job.equity {
                    match &mut grant.company {
                        EquityCompany::Public {
                            tails,
                            ruin,
                            layoff_link,
                            ..
                        } => {
                            if on.fat {
                                *tails = TailShape::Fat;
                            }
                            if on.ruin {
                                *ruin = RUIN;
                            }
                            *layoff_link = on.link;
                        }
                        EquityCompany::Private {
                            exit_timing,
                            exit_size,
                            ..
                        } => {
                            if on.timing {
                                *exit_timing = ExitTiming::lognormal();
                            }
                            if on.pareto {
                                *exit_size = ExitSize::ParetoTail;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    p
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
fn every_option_on_conserves_money_and_stays_finite_under_every_market_kind() {
    for example in Profile::examples(START) {
        for kind in MARKETS {
            let profile = enrich(with_market(example.profile.clone(), kind), On::ALL);
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

/// Months of the events of one kind whose label does (or does not) mention the
/// business: closures and restarts are logged with the layoff and rehire kinds.
fn events_of(t: &PathTrace, kind: TraceEventKind, business: bool) -> Vec<YearMonth> {
    t.events
        .iter()
        .filter(|e| {
            let about_business =
                e.label.starts_with("Business") || e.label.starts_with("Self-employment");
            e.kind == kind && about_business == business
        })
        .map(|e| e.month)
        .collect()
}

/// Switching an option on changes the income it belongs to and nothing else
/// random: lifespans, long-term care, prices and every asset's returns are the same
/// bits. The business options leave every pay cheque where it was; the stock
/// options leave business profit and (without the link) every layoff where it was.
#[test]
fn each_option_moves_only_its_own_income() {
    // A fixed retirement date, so that nothing depends on when the household can
    // afford to stop working.
    let fixed = |on: On| {
        let mut p = enrich(Profile::example(), on);
        p.retirement.trigger = RetirementTrigger::AtAge {
            person: 0,
            age: 58.0,
        };
        valid(&p);
        compile(&p)
    };
    let base = fixed(On::default());
    // (name, options, pay unchanged, layoffs unchanged, business profit unchanged)
    let cases: [(&str, On, bool, bool, bool); 7] = [
        (
            "failure",
            On {
                failure: true,
                ..On::default()
            },
            true,
            true,
            false,
        ),
        (
            "lumpiness",
            On {
                lumpiness: true,
                ..On::default()
            },
            true,
            true,
            false,
        ),
        (
            "fat tails",
            On {
                fat: true,
                ..On::default()
            },
            false,
            true,
            true,
        ),
        (
            "ruin",
            On {
                ruin: true,
                ..On::default()
            },
            false,
            true,
            true,
        ),
        (
            "layoff link",
            On {
                ruin: true,
                link: true,
                ..On::default()
            },
            false,
            false,
            true,
        ),
        (
            "private exit",
            On {
                timing: true,
                pareto: true,
                ..On::default()
            },
            false,
            true,
            true,
        ),
        ("all", On::ALL, false, false, false),
    ];
    for (name, on, pay_fixed, layoffs_fixed, profit_fixed) in cases {
        let plan = fixed(on);
        let mut changed = 0;
        for path in 0..16 {
            let a = sim::trace_path(&base, 77, path);
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
            let wages_same = a.column("income_wages") == b.column("income_wages");
            let profit_same =
                a.column("income_self_employment") == b.column("income_self_employment");
            if pay_fixed {
                assert!(wages_same, "{name} path {path}: pay moved");
            }
            if profit_fixed {
                assert!(profit_same, "{name} path {path}: business profit moved");
            }
            if layoffs_fixed {
                for kind in [TraceEventKind::LaidOff, TraceEventKind::Rehired] {
                    assert_eq!(
                        events_of(&a, kind, false),
                        events_of(&b, kind, false),
                        "{name} path {path} {kind:?}"
                    );
                }
                assert_eq!(
                    scalar(&a, PathScalar::MonthsUnemployed).to_bits(),
                    scalar(&b, PathScalar::MonthsUnemployed).to_bits()
                );
            }
            changed += usize::from(!wages_same || !profit_same);
        }
        // A closure or a ruin does not happen on every path; the rest do.
        assert!(changed >= 4, "{name}: changed on {changed} of 16 paths");
    }
}

/// With career risk switched off the business options have nothing to act on:
/// results are the same bits as without them. Employer stock is market risk and is
/// simulated either way, but the link needs the job's layoffs and is silent.
#[test]
fn career_risk_off_silences_the_business_options_and_the_link() {
    let off = |on: On| {
        let mut p = enrich(Profile::example(), on);
        p.assumptions.career_risk = false;
        valid(&p);
        compile(&p)
    };
    let base = run(&off(On::default()), 5, 0..60);
    for on in [
        On {
            failure: true,
            lumpiness: true,
            ..On::default()
        },
        On {
            link: true,
            ..On::default()
        },
    ] {
        assert_eq!(base.to_bytes(), run(&off(on), 5, 0..60).to_bytes());
    }
    let stock = run(
        &off(On {
            fat: true,
            ruin: true,
            ..On::default()
        }),
        5,
        0..60,
    );
    assert_ne!(base.to_bytes(), stock.to_bytes());
}

#[test]
fn new_fields_round_trip_through_json() {
    let p = enrich(Profile::example(), On::ALL);
    let back = Profile::from_json(&p.to_json_pretty()).unwrap();
    assert_eq!(p, back);
    let text = serde_json::json!({
        "incomes": [
            {"id": "shop", "kind": {
                "type": "self_employment",
                "net_annual": 50000.0,
                "failure": {"type": "hazard", "then": {"type": "stops"}},
                "monthly_lumpiness": 0.5
            }},
            {"id": "job", "kind": {"type": "job", "equity": [
                {"id": "a", "company": {
                    "type": "public", "tails": "fat",
                    "ruin": {"type": "by_volatility"}, "layoff_link": true
                }},
                {"id": "b", "company": {
                    "type": "private",
                    "exit_timing": {"type": "lognormal", "median_years": 7.0},
                    "exit_size": "pareto_tail"
                }}
            ]}}
        ]
    })
    .to_string();
    let p = Profile::from_json(&text).unwrap();
    let IncomeKind::SelfEmployment(se) = &p.incomes[0].kind else {
        panic!("not self-employment");
    };
    // Fields left out of a variant take their defaults.
    assert_eq!(
        se.failure,
        BusinessFailure::Hazard {
            young_annual_probability: 0.10,
            mature_annual_probability: 0.04,
            young_years: 5.0,
            years_in_business: None,
            recession_multiplier: 2.0,
            then: AfterFailure::Stops,
        }
    );
    assert_eq!(se.monthly_lumpiness, 0.5);
    assert_eq!(
        BusinessFailure::hazard(),
        BusinessFailure::Hazard {
            young_annual_probability: 0.10,
            mature_annual_probability: 0.04,
            young_years: 5.0,
            years_in_business: None,
            recession_multiplier: 2.0,
            then: restart(),
        }
    );
    let IncomeKind::Job(job) = &p.incomes[1].kind else {
        panic!("not a job");
    };
    assert_eq!(
        job.equity[0].company,
        EquityCompany::Public {
            volatility: 0.35,
            beta: 1.2,
            tails: TailShape::Fat,
            ruin: StockRuin::ByVolatility,
            layoff_link: true,
        }
    );
    assert_eq!(
        job.equity[1].company,
        EquityCompany::Private {
            liquidity_hazard: 0.12,
            fail_prob: 0.5,
            exit_volatility: 1.0,
            exit_timing: ExitTiming::Lognormal {
                median_years: 7.0,
                sigma: 0.5,
            },
            exit_size: ExitSize::ParetoTail,
        }
    );
    // Unknown names are refused.
    for bad in [
        r#"{"incomes": [{"kind": {"type": "self_employment", "failure": {"type": "often"}}}]}"#,
        r#"{"incomes": [{"kind": {"type": "self_employment", "failure": {"type": "hazard", "chance": 0.1}}}]}"#,
        r#"{"incomes": [{"kind": {"type": "self_employment", "lumpiness": 0.5}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "equity": [{"company": {"type": "public", "tails": "wild"}}]}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "equity": [{"company": {"type": "public", "ruin": "never"}}]}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "equity": [{"company": {"type": "private", "exit_size": "huge"}}]}}]}"#,
        r#"{"incomes": [{"kind": {"type": "job", "equity": [{"company": {"type": "private", "exit_timing": {"type": "lognormal", "mean": 5}}}]}}]}"#,
    ] {
        assert!(Profile::from_json(bad).is_err(), "{bad}");
    }
    // A profile written before the fields existed gets the model as it was.
    let old = r#"{"incomes": [
        {"kind": {"type": "self_employment", "net_annual": 1000.0}},
        {"kind": {"type": "job", "equity": [
            {"company": {"type": "public", "volatility": 0.4, "beta": 1.0}},
            {"id": "p", "company": {"type": "private", "liquidity_hazard": 0.2, "fail_prob": 0.4, "exit_volatility": 1.0}}
        ]}}
    ]}"#;
    let p = Profile::from_json(old).unwrap();
    let off = |income: &Income| match &income.kind {
        IncomeKind::SelfEmployment(se) => {
            se.failure == BusinessFailure::Never && se.monthly_lumpiness == 0.0
        }
        IncomeKind::Job(job) => job.equity.iter().all(|g| match g.company {
            EquityCompany::Public {
                tails,
                ruin,
                layoff_link,
                ..
            } => tails == TailShape::Normal && ruin == StockRuin::Never && !layoff_link,
            EquityCompany::Private {
                exit_timing,
                exit_size,
                ..
            } => exit_timing == ExitTiming::ConstantHazard && exit_size == ExitSize::Classes,
        }),
        _ => true,
    };
    assert!(p.incomes.iter().all(off));
    // No example household switches an option on.
    for example in Profile::examples(START) {
        assert!(example.profile.incomes.iter().all(off), "{}", example.slug);
    }
}

/// The defaults in the code are the numbers in `data/actuarial/params.json`.
#[test]
fn defaults_match_the_data_file() {
    let section = &Data::embedded().actuarial["business_and_equity"];
    let num = |path: &[&str]| {
        let mut v = section;
        for key in path {
            v = &v[*key];
        }
        v.as_f64()
            .unwrap_or_else(|| panic!("missing {}", path.join(".")))
    };
    let f = "business_failure";
    assert_eq!(
        BusinessFailure::hazard(),
        BusinessFailure::Hazard {
            young_annual_probability: num(&[f, "young_annual_probability"]),
            mature_annual_probability: num(&[f, "mature_annual_probability"]),
            young_years: num(&[f, "young_years"]),
            years_in_business: None,
            recession_multiplier: num(&[f, "recession_multiplier"]),
            then: AfterFailure::Restart {
                gap_months: num(&[f, "restart_gap_months"]) as u32,
                income_fraction: num(&[f, "restart_income_fraction"]),
            },
        }
    );
    assert_eq!(
        StockRuin::DEFAULT_ANNUAL_PROBABILITY,
        num(&["employer_stock_ruin", "stated_default_annual_probability"])
    );
    assert_eq!(
        ExitTiming::lognormal(),
        ExitTiming::Lognormal {
            median_years: num(&["private_exit_timing", "default_median_years"]),
            sigma: num(&["private_exit_timing", "default_sigma"]),
        }
    );
    assert_eq!(
        EMPLOYER_LINK_MONTHS as f64,
        num(&["employer_stock_layoff_link", "window_months"])
    );
    assert_eq!(BUSINESS_WORDS_PER_MONTH, 2);
    // The stated defaults are the ones the equity section already had.
    let equity = &Data::embedded().actuarial["equity_compensation"];
    assert_eq!(
        equity["private_company"]["model"]["stage_table"]["series_b_to_c"]["median_T_years"]
            .as_f64(),
        Some(ExitTiming::DEFAULT_MEDIAN_YEARS)
    );
    assert_eq!(
        equity["single_public_stock"]["defaults"]["typical_tech_or_growth"]["ruin_hazard"].as_f64(),
        Some(StockRuin::DEFAULT_ANNUAL_PROBABILITY)
    );
}

// ---------------------------------------------------------------- behaviour

/// One self-employed person, 35 years old, earning $96,000 a year with no yearly
/// volatility, who stops at 65 whatever happens. No tax and a fixed lifespan, so
/// the business options are the only thing that can move profit.
fn freelancer(failure: BusinessFailure, lumpiness: f64) -> Profile {
    let mut p = quiet(vec![person("owner", 35, Sex::Unspecified)], 90.0);
    p.assumptions.career_risk = true;
    p.incomes = vec![Income {
        id: "practice".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 96_000.0,
            volatility: 0.0,
            failure,
            monthly_lumpiness: lumpiness,
            ..SelfEmployment::default()
        }),
        ..Income::default()
    }];
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.accounts = vec![account("savings", 0, AccountKind::Taxable, 100_000.0)];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 65.0,
    };
    p
}

/// What one working life in business looked like.
struct Business {
    /// Profit received over the 30 years as a fraction of 30 years of the entered
    /// profit, in today's dollars.
    lifetime: f64,
    /// Months with no profit.
    idle: usize,
    closures: usize,
    /// Real profit in each of the 360 months as a fraction of the entered month.
    months: Vec<f64>,
    terminal: f64,
}

fn businesses(profile: &Profile, seed: u64, paths: u64) -> Vec<Business> {
    valid(profile);
    let plan = compile(profile);
    (0..paths)
        .map(|path| {
            let t = sim::trace_path(&plan, seed, path);
            // Trace columns are in whole today's dollars.
            let zeros = vec![0.0; t.months as usize];
            let profit = t.column("income_self_employment").unwrap_or(&zeros);
            let months: Vec<f64> = (0..360).map(|m| profit[m] / 8_000.0).collect();
            Business {
                lifetime: months.iter().sum::<f64>() / 360.0,
                idle: months.iter().filter(|x| **x == 0.0).count(),
                closures: events_of(&t, TraceEventKind::LaidOff, true).len(),
                months,
                terminal: scalar(&t, PathScalar::TerminalNetWorth),
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
    mean(v.map(|b| if b { 1.0 } else { 0.0 }))
}

fn quantile(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[test]
fn a_business_that_can_close_earns_less_and_lumps_keep_the_total() {
    let paths = 3_000;
    let steady = businesses(&freelancer(BusinessFailure::Never, 0.0), 9, 200);
    for b in &steady {
        assert!((b.lifetime - 1.0).abs() < 1e-9 && b.idle == 0 && b.closures == 0);
    }

    // Closures: an established business, restarting after a year at 70%.
    let risky = businesses(&freelancer(BusinessFailure::hazard(), 0.0), 9, paths);
    let exact = SelfEmploymentModel::new(
        &SelfEmployment {
            failure: BusinessFailure::hazard(),
            ..SelfEmployment::default()
        },
        true,
        Data::embedded(),
    )
    .expected_business_level(360);
    let expected = exact.iter().sum::<f64>() / 360.0;
    // 0.79 with no recessions; the simulated economy has some, which double the
    // chance while they last, so the simulated mean is a little lower.
    assert!((expected - 0.792).abs() < 0.002, "{expected}");
    let lifetime = mean(risky.iter().map(|b| b.lifetime));
    assert!(
        lifetime < expected + 0.01 && lifetime > expected - 0.06,
        "lifetime {lifetime}, expected without recessions {expected}"
    );
    // 4% a year for 30 years: 71% of businesses close at least once, a few more
    // with recessions.
    let closed = share(risky.iter().map(|b| b.closures > 0));
    assert!(closed > 0.68 && closed < 0.82, "{closed}");
    for b in &risky {
        // Each closure costs twelve months, unless the plan ends first.
        assert!(b.idle <= 12 * b.closures, "{} {}", b.idle, b.closures);
        assert!(b.months.iter().all(|x| (0.0..=1.0 + 1e-9).contains(x)));
        if b.closures == 0 {
            assert!((b.lifetime - 1.0).abs() < 1e-9);
        }
    }
    let median = |v: &[Business]| quantile(v.iter().map(|b| b.terminal).collect(), 0.5);
    let steady_many = businesses(&freelancer(BusinessFailure::Never, 0.0), 9, paths);
    assert!(median(&risky) < median(&steady_many));
    // "Stops" is worse than "restart".
    let BusinessFailure::Hazard {
        young_annual_probability,
        mature_annual_probability,
        young_years,
        years_in_business,
        recession_multiplier,
        ..
    } = BusinessFailure::hazard()
    else {
        unreachable!()
    };
    let stops = businesses(
        &freelancer(
            BusinessFailure::Hazard {
                young_annual_probability,
                mature_annual_probability,
                young_years,
                years_in_business,
                recession_multiplier,
                then: AfterFailure::Stops,
            },
            0.0,
        ),
        9,
        paths,
    );
    assert!(mean(stops.iter().map(|b| b.lifetime)) < lifetime - 0.1);
    assert!(stops.iter().all(|b| b.closures <= 1));

    // Lumpiness: the same lifetime profit on average, months all over the place.
    let lumpy = businesses(&freelancer(BusinessFailure::Never, 1.0), 9, paths);
    let lifetime = mean(lumpy.iter().map(|b| b.lifetime));
    // A lifetime is 360 months with a relative spread of about 1: three standard
    // errors over 3,000 paths are below 0.003.
    assert!((lifetime - 1.0).abs() < 0.004, "{lifetime}");
    let all: Vec<f64> = lumpy.iter().flat_map(|b| b.months.clone()).collect();
    let m = mean(all.iter().copied());
    let sd = mean(all.iter().map(|x| (x - m) * (x - m))).sqrt();
    assert!(sd > 0.93 && sd < 1.0, "monthly spread {sd}");
    assert!(share(all.iter().map(|x| *x < 0.25)) > 0.08);
    assert!(lumpy.iter().all(|b| b.closures == 0 && b.idle == 0));
}

/// One employee, 30 years old, on $100,000 with stock from the employer, who stops
/// at 60 whatever happens.
fn employee(level: RiskLevel, company: EquityCompany) -> Profile {
    let mut p = quiet(vec![person("worker", 30, Sex::Unspecified)], 90.0);
    p.assumptions.career_risk = true;
    p.incomes = vec![equity_job("job", 100_000.0, level, company)];
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.accounts = vec![account("savings", 0, AccountKind::Taxable, 50_000.0)];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 60.0,
    };
    p
}

/// Per path: layoffs, and real pay and equity in each of the 360 working months.
fn employees(profile: &Profile, seed: u64, paths: u64) -> Vec<(usize, Vec<f64>)> {
    valid(profile);
    let plan = compile(profile);
    (0..paths)
        .map(|path| {
            let t = sim::trace_path(&plan, seed, path);
            // Trace columns are in whole today's dollars.
            let zeros = vec![0.0; t.months as usize];
            let wages = t.column("income_wages").unwrap_or(&zeros);
            (
                events_of(&t, TraceEventKind::LaidOff, false).len(),
                wages[..360].to_vec(),
            )
        })
        .collect()
}

#[test]
fn a_collapsing_employer_costs_the_job_and_fat_tails_do_not() {
    let paths = 2_000;
    let run = |company| employees(&employee(RiskLevel::Typical, company), 12, paths);
    let base = run(public(TailShape::Normal, StockRuin::Never, false));
    let fat = run(public(TailShape::Fat, StockRuin::Never, false));
    let ruin = run(public(TailShape::Normal, RUIN, false));
    let linked = run(public(TailShape::Normal, RUIN, true));
    let layoffs = |v: &[(usize, Vec<f64>)]| v.iter().map(|p| p.0).sum::<usize>();
    let income = |v: &[(usize, Vec<f64>)]| mean(v.iter().map(|p| p.1.iter().sum::<f64>()));
    // Tails and ruin alone change the shares, not the job.
    for (a, b) in base.iter().zip(&fat).chain(base.iter().zip(&ruin)) {
        assert_eq!(a.0, b.0);
    }
    assert_eq!(layoffs(&base), layoffs(&fat));
    // The link: 4% a year of certain job loss plus tripled chances after a crash.
    assert!(
        layoffs(&linked) as f64 > 1.5 * layoffs(&base) as f64,
        "{} against {}",
        layoffs(&linked),
        layoffs(&base)
    );
    assert!(income(&linked) < 0.97 * income(&ruin));
    // Fat tails keep the mean: lifetime pay and equity within 3% of the default.
    let ratio = income(&fat) / income(&base);
    assert!((ratio - 1.0).abs() < 0.03, "fat: {ratio}");
    // Ruin keeps the expected price, but an employer whose shares are worthless
    // makes no further grants: lifetime equity is lower.
    assert!(income(&ruin) < 0.99 * income(&base));
}

#[test]
fn private_outcomes_come_later_and_a_few_are_far_larger() {
    let paths = 6_000;
    let run = |company| employees(&employee(RiskLevel::None, company), 21, paths);
    let base = run(private(ExitTiming::ConstantHazard, ExitSize::Classes));
    let late = run(private(ExitTiming::lognormal(), ExitSize::Classes));
    let tail = run(private(ExitTiming::ConstantHazard, ExitSize::ParetoTail));
    // With no wage risk, pay is exactly the salary: anything above it is the
    // payout of the company's outcome (or shares vesting after it).
    let paid_by = |v: &[(usize, Vec<f64>)], months: usize| {
        share(
            v.iter()
                .map(|p| p.1[..months].iter().any(|w| *w > 100_000.0 / 12.0 * 1.001)),
        )
    };
    // Half of outcomes are failures, which pay nothing: 6% of paths are paid in
    // the first year with a 12% hazard, almost none with the lognormal wait.
    let (early_base, early_late) = (paid_by(&base, 12), paid_by(&late, 12));
    assert!(early_base > 0.04 && early_base < 0.08, "{early_base}");
    assert!(early_late < 0.004, "{early_late}");
    // By ten years the lognormal has caught up and passed (92% against 72% of
    // companies have had their outcome).
    assert!(paid_by(&late, 120) > paid_by(&base, 120));

    // The Pareto tail: the same paths are paid in the same months; only the
    // largest payouts differ, and some are beyond anything the classes allow.
    let biggest = |p: &(usize, Vec<f64>)| p.1.iter().copied().fold(0.0, f64::max);
    let (mut same, mut larger) = (0, 0);
    for (a, b) in base.iter().zip(&tail) {
        if a.1 == b.1 {
            same += 1;
        } else {
            larger += 1;
            for (x, y) in a.1.iter().zip(&b.1) {
                assert_eq!(*x > 100_000.0 / 12.0 * 1.001, *y > 100_000.0 / 12.0 * 1.001);
            }
        }
    }
    // A large outcome is 4% of outcomes at this failure rate.
    assert!(
        same > 9 * paths as usize / 10 && larger > 100,
        "{same} {larger}"
    );
    let top = |v: &[(usize, Vec<f64>)]| v.iter().map(biggest).fold(0.0, f64::max);
    assert!(
        top(&tail) > 1.5 * top(&base),
        "{} {}",
        top(&tail),
        top(&base)
    );
}

/// Prints the figures quoted in docs/methodology.md:
/// `cargo test -p firesight-core --release --test rich_business report -- --ignored --nocapture`.
#[test]
#[ignore = "report: prints business and employer-stock statistics and success rates"]
fn report_measured_effects() {
    let paths = 20_000;
    println!("freelancer, 30 years, $96,000, {paths} paths");
    let stops = match BusinessFailure::hazard() {
        BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            years_in_business,
            recession_multiplier,
            ..
        } => BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            years_in_business,
            recession_multiplier,
            then: AfterFailure::Stops,
        },
        BusinessFailure::Never => unreachable!(),
    };
    let young = match BusinessFailure::hazard() {
        BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            recession_multiplier,
            then,
            ..
        } => BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            years_in_business: Some(0.0),
            recession_multiplier,
            then,
        },
        BusinessFailure::Never => unreachable!(),
    };
    for (label, failure, lump) in [
        ("default", BusinessFailure::Never, 0.0),
        ("failure, established", BusinessFailure::hazard(), 0.0),
        ("failure, new business", young, 0.0),
        ("failure, stops", stops, 0.0),
        ("lumpiness 0.5", BusinessFailure::Never, 0.5),
        ("lumpiness 1.0", BusinessFailure::Never, 1.0),
    ] {
        let b = businesses(&freelancer(failure, lump), 2026, paths);
        let lifetime: Vec<f64> = b.iter().map(|b| b.lifetime).collect();
        let all: Vec<f64> = b.iter().flat_map(|b| b.months.clone()).collect();
        let m = mean(all.iter().copied());
        let sd = mean(all.iter().map(|x| (x - m) * (x - m))).sqrt();
        let years: Vec<f64> = b
            .iter()
            .flat_map(|b| {
                b.months
                    .chunks(12)
                    .map(|y| y.iter().sum::<f64>() / 12.0)
                    .collect::<Vec<_>>()
            })
            .collect();
        let ym = mean(years.iter().copied());
        let ysd = mean(years.iter().map(|x| (x - ym) * (x - ym))).sqrt();
        println!(
            "  {label:22} lifetime profit / entered: mean {:.3} p5 {:.3} p50 {:.3} p95 {:.3}; closed at least once {:.1}%, twice or more {:.1}%; months with no profit: mean {:.1}; monthly sd {:.3}, yearly sd {:.3}; months under 25% of average {:.1}%; median terminal net worth {:.0}",
            mean(lifetime.iter().copied()),
            quantile(lifetime.clone(), 0.05),
            quantile(lifetime.clone(), 0.5),
            quantile(lifetime.clone(), 0.95),
            100.0 * share(b.iter().map(|b| b.closures > 0)),
            100.0 * share(b.iter().map(|b| b.closures > 1)),
            mean(b.iter().map(|b| b.idle as f64)),
            sd,
            ysd,
            100.0 * share(all.iter().map(|x| *x < 0.25)),
            quantile(b.iter().map(|b| b.terminal).collect(), 0.5),
        );
    }
    println!("employee, 30 years, $100,000 typical risk, $80,000 grant and $20,000 a year of refreshes, stock volatility 60% beta 1.2");
    for (label, company) in [
        (
            "default",
            public(TailShape::Normal, StockRuin::Never, false),
        ),
        ("fat tails", public(TailShape::Fat, StockRuin::Never, false)),
        (
            "ruin by volatility",
            public(TailShape::Normal, StockRuin::ByVolatility, false),
        ),
        (
            "ruin and link",
            public(TailShape::Normal, StockRuin::ByVolatility, true),
        ),
        (
            "all three",
            public(TailShape::Fat, StockRuin::ByVolatility, true),
        ),
    ] {
        let e = employees(&employee(RiskLevel::Typical, company), 2026, paths);
        let total: Vec<f64> = e.iter().map(|p| p.1.iter().sum::<f64>() / 3.0e6).collect();
        println!(
            "  {label:22} layoffs per career {:.3}; lifetime pay and equity / 30 salaries: mean {:.3} p5 {:.3} p50 {:.3} p95 {:.3}",
            mean(e.iter().map(|p| p.0 as f64)),
            mean(total.iter().copied()),
            quantile(total.clone(), 0.05),
            quantile(total.clone(), 0.5),
            quantile(total.clone(), 0.95),
        );
    }
    println!("employee of a private company, no wage risk, $80,000 grant, fail 50%");
    for (label, company) in [
        (
            "default",
            private(ExitTiming::ConstantHazard, ExitSize::Classes),
        ),
        (
            "lognormal wait",
            private(ExitTiming::lognormal(), ExitSize::Classes),
        ),
        (
            "pareto tail",
            private(ExitTiming::ConstantHazard, ExitSize::ParetoTail),
        ),
        (
            "both",
            private(ExitTiming::lognormal(), ExitSize::ParetoTail),
        ),
    ] {
        let e = employees(&employee(RiskLevel::None, company), 2026, paths);
        let extra: Vec<f64> = e.iter().map(|p| p.1.iter().sum::<f64>() - 3.0e6).collect();
        let paid_by = |months: usize| {
            100.0
                * share(
                    e.iter()
                        .map(|p| p.1[..months].iter().any(|w| *w > 100_000.0 / 12.0 * 1.001)),
                )
        };
        println!(
            "  {label:22} equity received over 30 years: mean {:.0} p50 {:.0} p95 {:.0} p99 {:.0} max {:.0}; paid within 1y {:.1}% 2y {:.1}% 5y {:.1}% 10y {:.1}% 30y {:.1}%",
            mean(extra.iter().copied()),
            quantile(extra.clone(), 0.5),
            quantile(extra.clone(), 0.95),
            quantile(extra.clone(), 0.99),
            quantile(extra.clone(), 1.0),
            paid_by(12),
            paid_by(24),
            paid_by(60),
            paid_by(120),
            paid_by(360),
        );
    }
    // The demo households that have the income in question, 10,000 paths at the
    // seed of the golden files.
    let set = |p: &Profile, on: On| {
        let mut p = p.clone();
        let before = p.incomes.len();
        p = enrich(p, on);
        p.incomes.truncate(before);
        p
    };
    for example in Profile::examples(START) {
        for (label, on) in [
            ("default", On::default()),
            (
                "failure",
                On {
                    failure: true,
                    ..On::default()
                },
            ),
            (
                "lumpiness",
                On {
                    lumpiness: true,
                    ..On::default()
                },
            ),
            (
                "fat tails",
                On {
                    fat: true,
                    ..On::default()
                },
            ),
            (
                "ruin 4%",
                On {
                    ruin: true,
                    ..On::default()
                },
            ),
            (
                "ruin 4% and link",
                On {
                    ruin: true,
                    link: true,
                    ..On::default()
                },
            ),
            ("all", On::ALL),
        ] {
            let s = summary(&set(&example.profile, on), 2026, 10_000);
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
