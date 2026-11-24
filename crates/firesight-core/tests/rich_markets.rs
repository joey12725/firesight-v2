//! The opt-in market options (long-run uncertainty, matched bootstrap joins, rare
//! disasters) inside the whole simulation: money is conserved, nothing is NaN, the
//! other subsystems' random numbers do not move, and the effects go the right way.
//! The distributions themselves are tested in `src/economy/rich_tests.rs`.

mod common;

use common::*;
use firesight_core::data::Data;
use firesight_core::economy;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, PathEnd, PathScalar, PathTrace, Recorder, TraceEventKind, YearRecord, YearSeries,
};

fn normal(sd: f64) -> Option<Uncertain> {
    Some(Uncertain::Normal { mean: 0.0, sd })
}

fn matched() -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
        joins: BootstrapJoins::Matched,
    }
}

/// Every option on at once, at settings stronger than the suggested ones.
fn all_on(mut p: Profile) -> Profile {
    let m = &mut p.assumptions.market;
    m.equity_premium_uncertainty = normal(0.015);
    m.inflation_uncertainty = Some(Uncertain::Triangular {
        low: -0.005,
        mode: 0.0,
        high: 0.015,
    });
    m.disasters = Disasters {
        annual_probability: 0.03,
        inflation_probability: 0.5,
        ..Disasters::default()
    };
    p
}

/// The four market kinds plus the bootstrap with matched joins.
fn kinds() -> Vec<MarketKind> {
    let mut kinds = MARKETS.to_vec();
    kinds.push(matched());
    kinds
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
        let dd = end.get(PathScalar::WorstDrawdown);
        assert!((0.0..=1.0).contains(&dd));
        self.inner.end_path(end);
    }
}

#[test]
fn every_option_on_conserves_money_and_stays_finite_under_every_market_kind() {
    for example in Profile::examples(START) {
        for kind in kinds() {
            let profile = all_on(with_market(example.profile.clone(), kind));
            let report = firesight_core::validate::validate(&profile);
            assert!(report.errors.is_empty(), "{:#?}", report.errors);
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

/// The hardest settings the validator accepts, each kind: the engine still neither
/// panics nor produces a non-finite number.
#[test]
fn the_most_extreme_valid_settings_stay_finite() {
    for kind in kinds() {
        let mut profile = with_market(Profile::example(), kind);
        let m = &mut profile.assumptions.market;
        m.equity_premium_uncertainty = normal(0.05);
        m.inflation_uncertainty = normal(0.025);
        m.disasters = Disasters {
            annual_probability: 0.1,
            depth: Uncertain::point(0.95),
            years: Uncertain::Uniform {
                low: 1.0,
                high: 40.0,
            },
            inflation_probability: 1.0,
            inflation: Uncertain::point(0.25),
        };
        let report = firesight_core::validate::validate(&profile);
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let plan = compile(&profile);
        run_checked(&plan, 5, 0..60);
        let mut ws = Workspace::new(&plan);
        let mut rec = Finite {
            inner: plan.new_accumulator(),
        };
        for i in 0..300 {
            sim::run_path(&plan, 6, i, &mut ws, &mut rec);
        }
    }
}

fn scalar(t: &PathTrace, s: PathScalar) -> f64 {
    t.scalars.iter().find(|x| x.scalar == s).unwrap().value
}

fn career_events(t: &PathTrace) -> Vec<(YearMonth, TraceEventKind)> {
    t.events
        .iter()
        .filter(|e| matches!(e.kind, TraceEventKind::LaidOff | TraceEventKind::Rehired))
        .map(|e| (e.month, e.kind))
        .collect()
}

/// Switching an option on changes the economy and nothing else random: lifespans
/// and long-term care are the same bits, and whatever part of the economy the
/// option does not touch is the same bits too.
#[test]
fn options_leave_the_other_random_streams_alone() {
    // A fixed retirement date, so that nothing depends on when the household can
    // afford to stop working.
    let mut base = Profile::example();
    base.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    let with = |f: &dyn Fn(&mut MarketModel)| {
        let mut p = base.clone();
        f(&mut p.assumptions.market);
        compile(&p)
    };
    let plan_base = compile(&base);
    let equity = with(&|m| m.equity_premium_uncertainty = normal(0.015));
    let inflation = with(&|m| m.inflation_uncertainty = normal(0.006));
    let disasters = with(&|m| {
        m.disasters = Disasters {
            annual_probability: 0.05,
            inflation_probability: 0.0,
            ..Disasters::default()
        }
    });
    let joined = with(&|m| m.kind = matched());
    let mut changed = [0; 4];
    for path in 0..20 {
        let a = sim::trace_path(&plan_base, 77, path);
        for (k, plan) in [&equity, &inflation, &disasters, &joined]
            .iter()
            .enumerate()
        {
            let b = sim::trace_path(plan, 77, path);
            assert_eq!(a.months, b.months, "option {k} path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::LongTermCareCost,
            ] {
                // The long-term care bill is in today's dollars of a nominal cost
                // that follows the price level, so compare it only where prices
                // are untouched.
                if s == PathScalar::LongTermCareCost && k != 0 {
                    continue;
                }
                assert_eq!(
                    scalar(&a, s).to_bits(),
                    scalar(&b, s).to_bits(),
                    "option {k} path {path} {s:?}"
                );
            }
            match k {
                // Equity uncertainty: inflation, bonds, cash and the recession
                // flag are untouched, so the same layoffs fall in the same months.
                // (Pay is not compared: bonuses follow the stock market.)
                0 => {
                    for key in ["price_level", "return_bonds", "return_cash"] {
                        assert!(a.column(key) == b.column(key), "path {path} {key}");
                    }
                    assert_eq!(career_events(&a), career_events(&b), "path {path}");
                }
                // Inflation uncertainty: the same layoffs in the same months.
                1 => assert_eq!(career_events(&a), career_events(&b), "path {path}"),
                // Disasters without inflation bursts: prices, bonds and cash are
                // untouched (layoffs may differ: a disaster is a recession).
                2 => {
                    for key in ["price_level", "return_bonds", "return_cash"] {
                        assert!(a.column(key) == b.column(key), "path {path} {key}");
                    }
                }
                _ => {}
            }
            if a.column("return_us_stocks") != b.column("return_us_stocks") {
                changed[k] += 1;
            }
        }
    }
    // Long-run shifts and matched joins change every path; a 5% hazard reaches most.
    assert_eq!((changed[0], changed[3]), (20, 20));
    assert_eq!(changed[1], 20);
    assert!(changed[2] >= 10, "{changed:?}");
}

/// A 30-year retiree drawing 4% of a 60/40 portfolio: the classic case, where the
/// direction of each effect is known.
fn retiree_stats(f: &dyn Fn(&mut MarketModel), paths: u64) -> (f64, Vec<f64>) {
    let mut p = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    f(&mut p.assumptions.market);
    let plan = compile(&p);
    let acc = run(&plan, 2026, 0..paths);
    let summary = plan.summarize(&acc);
    let mut worth: Vec<f64> = acc
        .scalar_column(PathScalar::TerminalNetWorth)
        .into_iter()
        .map(f64::from)
        .collect();
    worth.sort_by(f64::total_cmp);
    (summary.headline.success_rate.value, worth)
}

#[test]
fn effects_go_the_right_way_for_a_retiree() {
    let paths = 3000;
    let q = |v: &[f64], p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let (base_rate, base_worth) = retiree_stats(&|_| {}, paths);
    // Disasters are extra downside: fewer plans survive, and the bad outcomes get
    // worse while the best ones barely move.
    let (rate, worth) = retiree_stats(&|m| m.disasters.annual_probability = 0.02, paths);
    assert!(rate < base_rate - 0.02, "disasters: {rate} vs {base_rate}");
    assert!(q(&worth, 0.25) < q(&base_worth, 0.25));
    assert!(q(&worth, 0.5) < q(&base_worth, 0.5));
    // More hazard, more damage.
    let (worse, _) = retiree_stats(&|m| m.disasters.annual_probability = 0.05, paths);
    assert!(worse < rate, "{worse} vs {rate}");
    // Long-run uncertainty widens the outcomes in both directions and, for a plan
    // that mostly succeeds, costs some success: the paths pushed below the line
    // outnumber those rescued.
    let (rate, worth) = retiree_stats(&|m| m.equity_premium_uncertainty = normal(0.02), paths);
    assert!(rate < base_rate, "uncertainty: {rate} vs {base_rate}");
    assert!(q(&worth, 0.9) > q(&base_worth, 0.9));
    assert!(q(&worth, 0.25) < q(&base_worth, 0.25));
    // Uncertain inflation alone leaves real returns alone: a retiree with constant
    // real spending and no nominal income or debt is hardly affected.
    let (rate, _) = retiree_stats(&|m| m.inflation_uncertainty = normal(0.01), paths);
    assert!(
        (rate - base_rate).abs() < 0.02,
        "inflation: {rate} vs {base_rate}"
    );
}

/// The defaults and constants written in `data/actuarial/params.json`
/// (`market_risk`) are the ones the code uses.
#[test]
fn defaults_match_the_data_file() {
    let section = &Data::embedded().actuarial["market_risk"];
    let d = &section["rare_disasters"];
    let num = |v: &serde_json::Value| v.as_f64().unwrap();
    let pert = |v: &serde_json::Value| Uncertain::Pert {
        low: num(&v["low"]),
        mode: num(&v["mode"]),
        high: num(&v["high"]),
    };
    let defaults = Disasters::default();
    assert_eq!(defaults.annual_probability, 0.0);
    assert_eq!(defaults.depth, pert(&d["depth_pert"]));
    assert_eq!(defaults.years, pert(&d["years_pert"]));
    assert_eq!(defaults.inflation, pert(&d["inflation_pert"]));
    assert_eq!(
        defaults.inflation_probability,
        num(&d["inflation_probability"])
    );
    assert_eq!(
        economy::CASH_PASS_THROUGH,
        num(&d["cash_inflation_pass_through"])
    );
    assert_eq!(0.0, num(&d["bond_inflation_pass_through"]));
    assert_eq!(
        economy::DISASTER_RECESSION_MONTHS as f64,
        num(&d["recession_months_max"])
    );
    assert_eq!(
        economy::MAX_EPISODES as f64,
        num(&d["max_episodes_per_path"])
    );
    assert_eq!(economy::MAX_DISASTER_DEPTH, num(&d["max_depth"]));
    assert_eq!(economy::MAX_BURST_INFLATION, num(&d["max_burst_inflation"]));
    assert_eq!(
        economy::MAX_BURST_PRICE_FACTOR,
        num(&d["max_burst_price_factor"])
    );
    let j = &section["bootstrap_joins"];
    assert_eq!(
        economy::JOIN_TRAILING_MONTHS as f64,
        num(&j["trailing_months"])
    );
    assert_eq!(economy::JOIN_MIN_CELL as f64, num(&j["min_cell_rows"]));
    // The default market model has every option off.
    let m = MarketModel::default();
    assert!(m.equity_premium_uncertainty.is_none() && m.inflation_uncertainty.is_none());
    assert_eq!(m.kind, MARKETS[0]);
}

/// Prints the figures quoted in docs/methodology.md:
/// `cargo test -p firesight-core --release --test rich_markets report -- --ignored --nocapture`.
#[test]
#[ignore = "report: prints success rates of the demo households and a retiree with each option on"]
fn report_success_rates() {
    type Change = (&'static str, fn(&mut MarketModel));
    let changes: [Change; 7] = [
        ("default", |_| {}),
        ("equity uncertainty sd 0.01", |m| {
            m.equity_premium_uncertainty = normal(0.01)
        }),
        ("equity uncertainty sd 0.015", |m| {
            m.equity_premium_uncertainty = normal(0.015)
        }),
        ("inflation uncertainty sd 0.005", |m| {
            m.inflation_uncertainty = normal(0.005)
        }),
        ("matched joins", |m| m.kind = matched()),
        ("disasters 0.01", |m| m.disasters.annual_probability = 0.01),
        ("disasters 0.02", |m| m.disasters.annual_probability = 0.02),
    ];
    let paths = 10_000;
    let mut households: Vec<(String, Profile)> = Profile::examples(START)
        .into_iter()
        .map(|e| (e.slug.to_string(), e.profile))
        .collect();
    let mut classic = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    classic.assumptions.market = MarketModel::default();
    households.push(("retiree 4% 60/40 30y".into(), classic));
    for (name, profile) in households {
        for (label, change) in changes {
            let mut p = profile.clone();
            change(&mut p.assumptions.market);
            let s = summary(&p, 2026, paths);
            println!(
                "{name:24} {label:32} success {:5.1}%  median terminal net worth {:>12.0}",
                s.headline.success_rate.value * 100.0,
                s.headline.median_terminal_net_worth,
            );
        }
    }
}
