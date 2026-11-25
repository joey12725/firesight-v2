//! Determinism, chunking invariance and stream isolation.

mod common;

use common::*;
use firesight_core::data::Data;
use firesight_core::equity::{self, EquityParams};
use firesight_core::model::*;
use firesight_core::rng::{Rng, Stream};
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{Accumulator, PathScalar, TraceEventKind};

#[test]
fn the_same_seed_gives_the_same_bytes() {
    for example in Profile::examples(START) {
        let plan = compile(&example.profile);
        let a = run(&plan, 42, 0..300).to_bytes();
        let b = run(&plan, 42, 0..300).to_bytes();
        assert_eq!(a, b, "{}", example.slug);
        let other_seed = run(&plan, 43, 0..300).to_bytes();
        assert_ne!(a, other_seed, "{}", example.slug);
        // Recompiling the plan changes nothing either.
        let again = run(&compile(&example.profile), 42, 0..300).to_bytes();
        assert_eq!(a, again, "{}", example.slug);
    }
}

#[test]
fn one_range_equals_ten_ranges_merged_in_shuffled_order() {
    let plan = compile(&Profile::example());
    let whole = run(&plan, 9, 0..1000);
    let mut chunks: Vec<Accumulator> = (0..10)
        .map(|k| run(&plan, 9, k * 100..(k + 1) * 100))
        .collect();
    // Fisher-Yates with the engine's own generator.
    let mut rng = Rng::stream(1, 0, Stream::Events);
    for i in (1..chunks.len()).rev() {
        chunks.swap(i, rng.below(i + 1));
    }
    let mut merged = plan.new_accumulator();
    for chunk in &chunks {
        // Each chunk crosses a byte boundary first, as it does between workers.
        let decoded = Accumulator::from_bytes(&chunk.to_bytes()).unwrap();
        merged.merge(&decoded).unwrap();
    }
    assert_eq!(merged.to_bytes(), whole.to_bytes());
    assert_eq!(
        serde_json::to_string(&plan.summarize(&merged)).unwrap(),
        serde_json::to_string(&plan.summarize(&whole)).unwrap()
    );
}

#[test]
fn a_traced_path_matches_its_row_in_the_aggregate() {
    for example in Profile::examples(START) {
        let plan = compile(&example.profile);
        let acc = run(&plan, 3, 0..60);
        for path in [0u64, 17, 59] {
            let trace = sim::trace_path(&plan, 3, path);
            let row = acc.path_scalars(path).unwrap();
            for s in &trace.scalars {
                let a = row[s.scalar.index()];
                // The trace rounds: dollars to whole dollars, ages and ratios to a
                // few decimals.
                let same = (a.is_nan() && s.value.is_nan())
                    || (f64::from(a) - s.value).abs()
                        <= 0.51_f64
                            .min(0.006 + f64::from(a.abs()) * 1e-6)
                            .max(if a.abs() > 1000.0 { 0.51 } else { 0.006 });
                assert!(
                    same,
                    "{} path {path} {:?}: {a} vs {}",
                    example.slug, s.scalar, s.value
                );
            }
        }
    }
}

/// A two-earner household whose retirement date is fixed, so that a change in
/// spending cannot move anything that depends on when they stop working.
fn fixed_date_couple() -> Profile {
    let mut p = Profile::example();
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    p
}

#[test]
fn changing_spending_leaves_markets_careers_and_lifespans_untouched() {
    let base = fixed_date_couple();
    let mut frugal = base.clone();
    for item in &mut frugal.spending.items {
        item.amount *= 0.7;
    }
    frugal.spending.shocks = ShockLevel::Off;
    let (plan_a, plan_b) = (compile(&base), compile(&frugal));
    let mut differences = 0;
    for path in 0..25 {
        let a = sim::trace_path(&plan_a, 77, path);
        let b = sim::trace_path(&plan_b, 77, path);
        // Identical lifespans: the paths have the same length and death ages.
        assert_eq!(a.months, b.months, "path {path}");
        let scalar = |t: &firesight_core::stats::PathTrace, s: PathScalar| {
            t.scalars.iter().find(|x| x.scalar == s).unwrap().value
        };
        for s in [
            PathScalar::DeathAge0,
            PathScalar::DeathAge1,
            PathScalar::Inflation,
        ] {
            assert_eq!(
                scalar(&a, s).to_bits(),
                scalar(&b, s).to_bits(),
                "path {path} {s:?}"
            );
        }
        // Identical markets, month by month, for every asset class.
        for key in [
            "price_level",
            "return_us_stocks",
            "return_intl_stocks",
            "return_bonds",
            "return_cash",
        ] {
            assert_eq!(a.column(key), b.column(key), "path {path} {key}");
        }
        // Identical careers: the same layoffs in the same months, the same pay.
        let career = |t: &firesight_core::stats::PathTrace| -> Vec<(YearMonth, TraceEventKind)> {
            t.events
                .iter()
                .filter(|e| matches!(e.kind, TraceEventKind::LaidOff | TraceEventKind::Rehired))
                .map(|e| (e.month, e.kind))
                .collect()
        };
        assert_eq!(career(&a), career(&b), "path {path}");
        assert_eq!(
            scalar(&a, PathScalar::MonthsUnemployed).to_bits(),
            scalar(&b, PathScalar::MonthsUnemployed).to_bits()
        );
        assert_eq!(
            a.column("income_wages"),
            b.column("income_wages"),
            "path {path}"
        );
        // Identical health: the same long-term care bill.
        assert_eq!(
            scalar(&a, PathScalar::LongTermCareCost).to_bits(),
            scalar(&b, PathScalar::LongTermCareCost).to_bits()
        );
        // And the decision did change the outcome.
        if a.column("spend_essential") != b.column("spend_essential") {
            differences += 1;
        }
    }
    assert_eq!(differences, 25);
}

#[test]
fn retiring_earlier_does_not_shift_later_random_draws() {
    // Under `when_funded` the retirement month differs by path and by scenario, yet
    // markets, lifespans, health and every layoff before the earlier retirement
    // date are the same.
    let base = Profile::example();
    let mut eager = base.clone();
    if let RetirementTrigger::WhenFunded(rule) = &mut eager.retirement.trigger {
        rule.withdrawal_rate = 0.06;
    }
    let (plan_a, plan_b) = (compile(&base), compile(&eager));
    for path in 0..25 {
        let a = sim::trace_path(&plan_a, 5, path);
        let b = sim::trace_path(&plan_b, 5, path);
        assert_eq!(a.months, b.months);
        assert_eq!(a.column("return_us_stocks"), b.column("return_us_stocks"));
        assert_eq!(a.column("price_level"), b.column("price_level"));
        let retired = |t: &firesight_core::stats::PathTrace| {
            t.events
                .iter()
                .find(|e| e.kind == TraceEventKind::Retired)
                .map_or(YearMonth::ym(9999, 1), |e| e.month)
        };
        let cutoff = retired(&a).min(retired(&b));
        let layoffs = |t: &firesight_core::stats::PathTrace| -> Vec<YearMonth> {
            t.events
                .iter()
                .filter(|e| e.kind == TraceEventKind::LaidOff && e.month < cutoff)
                .map(|e| e.month)
                .collect()
        };
        assert_eq!(layoffs(&a), layoffs(&b), "path {path}");
        let ltc = |t: &firesight_core::stats::PathTrace| {
            t.events
                .iter()
                .filter(|e| e.kind == TraceEventKind::LongTermCare)
                .map(|e| e.month)
                .collect::<Vec<_>>()
        };
        assert_eq!(ltc(&a), ltc(&b), "path {path}");
    }
}

#[test]
fn adding_an_event_does_not_move_the_ones_before_it() {
    let base = Profile::example();
    let mut more = base.clone();
    more.events.push(LifeEvent {
        id: "sabbatical".into(),
        name: "Sabbatical".into(),
        when: When::date(START + 100),
        probability: 0.5,
        timing_uncertainty_years: 1.0,
        kind: EventKind::Expense {
            amount: 20_000.0,
            financed: None,
        },
    });
    let (plan_a, plan_b) = (compile(&base), compile(&more));
    for path in 0..20 {
        let a = sim::trace_path(&plan_a, 8, path);
        let b = sim::trace_path(&plan_b, 8, path);
        let home = |t: &firesight_core::stats::PathTrace| {
            t.events
                .iter()
                .find(|e| e.kind == TraceEventKind::BoughtHome)
                .map(|e| e.month)
        };
        assert_eq!(home(&a), home(&b), "path {path}");
        assert_eq!(a.column("return_us_stocks"), b.column("return_us_stocks"));
    }
}

/// A household that is retired in month 0 runs the funded test before the first
/// annual plan. Everything that test reads (the year's tax law, the filing status)
/// must come from this path, not from the path the workspace simulated before it:
/// the order in which paths run, and the workspace they run in, must not matter.
#[test]
fn a_household_retired_at_the_start_does_not_depend_on_path_order() {
    let mut p = Profile::example();
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    let plan = compile(&p);
    let n = 16u64;
    // Every path alone in a fresh workspace.
    let fresh: Vec<Vec<f32>> = (0..n)
        .map(|i| {
            let acc = run(&plan, 5, i..i + 1);
            acc.path_scalars(i).unwrap().to_vec()
        })
        .collect();
    // All paths in one workspace, forward and in reverse.
    for reverse in [false, true] {
        let mut ws = sim::Workspace::new(&plan);
        let mut acc = plan.new_accumulator();
        let order: Vec<u64> = if reverse {
            (0..n).rev().collect()
        } else {
            (0..n).collect()
        };
        for i in order {
            sim::run_path(&plan, 5, i, &mut ws, &mut acc);
        }
        for i in 0..n {
            let row = acc.path_scalars(i).unwrap();
            for (k, (a, b)) in row.iter().zip(&fresh[i as usize]).enumerate() {
                assert!(
                    a.to_bits() == b.to_bits(),
                    "path {i} scalar {k} (reverse {reverse}): {a} vs {b}"
                );
            }
        }
    }
    // Everyone starts from the same balance sheet, so the funded ratio on the
    // retirement date is one number.
    let acc = run(&plan, 5, 0..n);
    let ratios = acc.scalar_column(PathScalar::FundedRatio);
    assert!(ratios.iter().all(|r| r.to_bits() == ratios[0].to_bits()));
}

// ---------------------------------------------------------------------------
// Private-company equity: fates, draws and bit-identity
// ---------------------------------------------------------------------------

/// Every scalar of every path, as bits.
/// Scalars that existed before private-company equity: the columns up to
/// `equity_outcome`.
const OLD_SCALARS: usize = 36;

/// Every path's row of the scalars that existed before private-company equity (the
/// six after them describe the company and are not missing only when there is one).
fn scalar_bits(acc: &Accumulator) -> Vec<(u64, Vec<u32>)> {
    assert_eq!(PathScalar::EquityOutcome.index(), OLD_SCALARS);
    acc.rows()
        .map(|(i, row)| (i, row[..OLD_SCALARS].iter().map(|v| v.to_bits()).collect()))
        .collect()
}

/// A company with no holdings that cannot touch the job: it has a fate and a random
/// stream, and nothing else.
fn empty_company(job: &str) -> PrivateCompany {
    PrivateCompany {
        id: "shell".into(),
        name: "Shell".into(),
        job: Some(job.into()),
        preferred_price: 10.0,
        company_failure_ends_job: false,
        ..PrivateCompany::default()
    }
}

#[test]
fn an_empty_private_equity_list_changes_nothing() {
    let old_demos: Vec<_> = Profile::examples(START)
        .into_iter()
        .filter(|e| e.profile.private_equity.is_empty())
        .collect();
    assert_eq!(old_demos.len(), 3);
    for example in old_demos {
        // The key written out and read back, as a saved plan has it.
        let mut doc = serde_json::to_value(&example.profile).unwrap();
        assert_eq!(doc["private_equity"], serde_json::json!([]));
        let read: Profile = serde_json::from_value(doc.clone()).unwrap();
        // And a plan saved before the key existed.
        doc.as_object_mut().unwrap().remove("private_equity");
        let old: Profile = serde_json::from_value(doc).unwrap();
        let expected = run(&compile(&example.profile), 4, 0..200).to_bytes();
        assert_eq!(run(&compile(&read), 4, 0..200).to_bytes(), expected);
        assert_eq!(run(&compile(&old), 4, 0..200).to_bytes(), expected);
    }
}

#[test]
fn adding_a_company_leaves_every_other_stream_in_place() {
    // Under a flat tax there is no minimum tax to switch on, so a company with no
    // holdings must change nothing at all: every scalar of every path.
    let mut base = Profile::examples(START).remove(0).profile;
    base.assumptions.taxes.kind = TaxKind::Flat { rate: 0.25 };
    let job = base.incomes[0].id.clone();
    let mut with = base.clone();
    with.private_equity.push(empty_company(&job));
    let a = run(&compile(&base), 6, 0..400);
    let b = run(&compile(&with), 6, 0..400);
    assert_eq!(scalar_bits(&a), scalar_bits(&b));
}

#[test]
fn adding_a_company_switches_the_amt_on_and_nothing_else() {
    let base = Profile::examples(START).remove(0).profile;
    let job = base.incomes[0].id.clone();
    let mut with = base.clone();
    with.private_equity.push(empty_company(&job));
    let (plan_a, plan_b) = (compile(&base), compile(&with));
    let (mut same, mut moved) = (0, 0);
    for path in 0..120 {
        let a = sim::trace_path(&plan_a, 6, path);
        let b = sim::trace_path(&plan_b, 6, path);
        let value = |t: &firesight_core::stats::PathTrace, s: PathScalar| {
            t.scalars
                .iter()
                .find(|x| x.scalar == s)
                .unwrap()
                .value
                .to_bits()
        };
        // Lifespans, prices, careers and markets never move.
        assert_eq!(a.months, b.months, "path {path}");
        for s in [
            PathScalar::DeathAge0,
            PathScalar::Inflation,
            PathScalar::MonthsUnemployed,
            PathScalar::LongTermCareCost,
        ] {
            assert_eq!(value(&a, s), value(&b, s), "path {path} {s:?}");
        }
        for key in ["price_level", "return_us_stocks", "income_wages"] {
            assert_eq!(a.column(key), b.column(key), "path {path} {key}");
        }
        // The first calendar year is identical: nothing in it owes minimum tax.
        // (The return now reports a tentative minimum tax; it is below the
        // regular tax.)
        for key in ["total", "agi", "state", "federal_ordinary"] {
            assert_eq!(
                a.tax_returns[0].result[key], b.tax_returns[0].result[key],
                "path {path} {key}"
            );
        }
        assert_eq!(b.tax_returns[0].result["amt"].as_f64(), Some(0.0));
        let amt_some_year = b
            .tax_returns
            .iter()
            .any(|r| r.result["amt"].as_f64().unwrap_or(0.0) > 0.0);
        let scalars_equal = a.scalars[..OLD_SCALARS]
            .iter()
            .zip(&b.scalars[..OLD_SCALARS])
            .all(|(x, y)| x.scalar == y.scalar && x.value.to_bits() == y.value.to_bits());
        if scalars_equal && a.columns == b.columns {
            same += 1;
        } else {
            moved += 1;
            // A path that moved owes minimum tax in some year (under 2026 law a
            // large long-term gain alone can), and is identical until that year.
            let first = (0..a.months as usize)
                .find(|&t| {
                    a.columns
                        .iter()
                        .zip(&b.columns)
                        .any(|(x, y)| x.key != y.key || x.values[t] != y.values[t])
                })
                .unwrap_or(a.months as usize) as i32;
            let first_amt_year = b
                .tax_returns
                .iter()
                .find(|r| r.result["amt"].as_f64().unwrap_or(0.0) > 0.0)
                .map_or(i32::MAX, |r| r.year);
            let year_of_first = (START + first).year();
            assert!(
                amt_some_year && first_amt_year <= year_of_first,
                "path {path} first differs in month {first} ({year_of_first}); first minimum tax in {first_amt_year}"
            );
        }
    }
    // For the record: how far the switch reaches on this household.
    println!("adding an empty company: {same} of 120 paths identical, {moved} moved");
    assert!(same > moved, "{same} paths the same, {moved} moved");
}

/// The demo's fates as the preview would draw them: kind and month before the IPO
/// month is placed in a window.
fn drawn_fate(company: &PrivateCompany, seed: u64, path: u64, index: usize) -> equity::Fate {
    let params = EquityParams::from_data(Data::embedded());
    let table = equity::resolved_table(company, &params);
    let mut rng = Rng::stream(seed, path, Stream::PrivateCompany(index));
    equity::draw_fate(&table, &params, &mut rng)
}

fn kind_key(kind: OutcomeKind) -> &'static str {
    match kind {
        OutcomeKind::Fails => "company_failed",
        OutcomeKind::Acquired => "company_acquired",
        OutcomeKind::Ipo => "company_ipo",
        OutcomeKind::StillPrivate => "company_failed",
    }
}

#[test]
fn the_simulation_realises_the_fate_drawn_from_the_company_stream() {
    // Four words at path start on the company's own stream, read the same way the
    // preview reads them: the event the life shows is the fate drawn, in its month
    // (an IPO within the window search of it), whenever the owner lives that long.
    let profile = startup_employee();
    let plan = compile(&profile);
    let mut ws = Workspace::new(&plan);
    let mut seen = [0u32; 3];
    for path in 0..600 {
        let log = equity_log(&plan, 21, path, &mut ws);
        let fate = drawn_fate(&profile.private_equity[0], 21, path, 0);
        match log.terminal() {
            Some((month, kind, label)) => {
                assert_eq!(*kind, kind_key(fate.kind), "path {path}: {label}");
                if fate.kind == OutcomeKind::Ipo {
                    assert!((month - fate.month).abs() <= 60, "path {path}");
                    seen[2] += 1;
                } else {
                    assert_eq!(*month, fate.month, "path {path}: {label}");
                    seen[usize::from(fate.kind == OutcomeKind::Acquired)] += 1;
                }
                if fate.kind == OutcomeKind::StillPrivate {
                    assert!(label.contains("written off"), "{label}");
                }
            }
            // No event: the life ended before the month drawn.
            None => assert!(
                fate.month >= log.months as i32,
                "path {path}: fate {fate:?} never happened in {} months",
                log.months
            ),
        }
    }
    assert!(seen.iter().all(|n| *n > 30), "{seen:?}");
}

/// What must not move when only a decision changes: the company's event, its month,
/// the IPO price, and every tender month.
fn fate_fingerprint(log: &EquityLog) -> Vec<(i32, &'static str, String)> {
    log.events
        .iter()
        .filter_map(|(month, kind, label)| match *kind {
            "company_failed" | "company_ipo" => Some((*month, *kind, label.clone())),
            // The cash paid depends on what was held; the price does not.
            "company_acquired" | "tender_offer" => Some((
                *month,
                *kind,
                label.split(':').next().unwrap_or_default().to_string(),
            )),
            _ => None,
        })
        .map(|(month, kind, label)| {
            // "(... paid for its shares is lost)" depends on what was exercised.
            (month, kind, label.split(" (").next().unwrap().to_string())
        })
        .collect()
}

#[test]
fn equity_policy_changes_keep_every_fate() {
    let base = startup_employee();
    let vary = |f: &dyn Fn(&mut Profile)| {
        let mut p = base.clone();
        f(&mut p);
        p
    };
    let variants: Vec<(&str, Profile)> = vec![
        (
            "exercise now",
            vary(&|p| p.private_equity[0].policy.exercise = ExerciseTiming::VestedNow),
        ),
        (
            "exercise as they vest",
            vary(&|p| p.private_equity[0].policy.exercise = ExerciseTiming::AsTheyVest),
        ),
        (
            "sell all in tenders",
            vary(&|p| p.private_equity[0].policy.tender_sell_fraction = 1.0),
        ),
        (
            "hold after the lockup",
            vary(&|p| p.private_equity[0].policy.sell_down = SellDown::Hold),
        ),
        (
            "forfeit at the deadline",
            vary(&|p| p.private_equity[0].policy.at_deadline = DeadlinePolicy::Forfeit),
        ),
        (
            "leave after a year",
            vary(&|p| p.private_equity[0].policy.leave_after_months = Some(12)),
        ),
        (
            "sell nothing in tenders",
            vary(&|p| p.private_equity[0].policy.tender_sell_fraction = 0.0),
        ),
        (
            "retire five years earlier",
            vary(&|p| {
                if let RetirementTrigger::WhenFunded(rule) = &mut p.retirement.trigger {
                    rule.max_age = 62.0;
                }
            }),
        ),
    ];
    let plan = compile(&base);
    let reference = run(&plan, 13, 0..300);
    let mut ws = Workspace::new(&plan);
    let logs: Vec<EquityLog> = (0..40)
        .map(|path| equity_log(&plan, 13, path, &mut ws))
        .collect();
    assert!(logs.iter().any(|l| !l.months_of("tender_offer").is_empty()));
    for (name, profile) in variants {
        let plan = compile(&profile);
        let acc = run(&plan, 13, 0..300);
        // The outcome and its age are the fate: no decision moves them.
        for s in [
            PathScalar::DeathAge0,
            PathScalar::Inflation,
            PathScalar::EquityOutcome,
            PathScalar::EquityEventAge,
        ] {
            let (a, b) = (reference.scalar_column(s), acc.scalar_column(s));
            assert!(
                a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
                "{name}: {s:?} moved"
            );
        }
        let mut ws = Workspace::new(&plan);
        for (path, expected) in logs.iter().enumerate() {
            let log = equity_log(&plan, 13, path as u64, &mut ws);
            assert_eq!(
                fate_fingerprint(&log),
                fate_fingerprint(expected),
                "{name}: path {path}"
            );
        }
    }
}

#[test]
fn counting_equity_at_zero_keeps_the_fate_and_removes_the_money() {
    let base = startup_employee();
    let mut zero = base.clone();
    zero.private_equity[0].included = false;
    let (plan_a, plan_b) = (compile(&base), compile(&zero));
    let (mut ws_a, mut ws_b) = (Workspace::new(&plan_a), Workspace::new(&plan_b));
    let (mut failures, mut ended_job) = (0u32, 0u32);
    for path in 0..150 {
        let a = equity_log(&plan_a, 17, path, &mut ws_a);
        let b = equity_log(&plan_b, 17, path, &mut ws_b);
        // No money, no paper value, and no event except a failure.
        assert_eq!((b.income, b.exercise, b.paper_months), (0.0, 0.0, 0));
        assert!(
            b.events.iter().all(|e| !matches!(
                e.1,
                "options_exercised"
                    | "tender_offer"
                    | "company_acquired"
                    | "company_ipo"
                    | "equity_forfeited"
                    | "lockup_sale"
            )),
            "path {path}: {:?}",
            b.events
        );
        // The same company fails in the same month, and takes the job with it.
        let fails = |l: &EquityLog| -> Vec<i32> {
            l.events
                .iter()
                .filter(|e| e.1 == "company_failed" && e.2.contains("shuts down"))
                .map(|e| e.0)
                .collect()
        };
        assert_eq!(fails(&a), fails(&b), "path {path}");
        // Until either household retires the job has the same history: the same
        // layoffs, the forced one included.
        let retired = |l: &EquityLog| l.months_of("retired").first().copied().unwrap_or(i32::MAX);
        let cutoff = retired(&a).min(retired(&b));
        let layoffs = |l: &EquityLog| -> Vec<i32> {
            l.months_of("laid_off")
                .into_iter()
                .filter(|m| *m < cutoff)
                .collect()
        };
        assert_eq!(layoffs(&a), layoffs(&b), "path {path}");
        for month in fails(&b) {
            failures += 1;
            ended_job += u32::from(b.months_of("laid_off").contains(&month));
        }
        // The same outcome at the same age, and no money in any of the four
        // amounts: zero, not "no company".
        for s in [
            PathScalar::DeathAge0,
            PathScalar::EquityOutcome,
            PathScalar::EquityEventAge,
        ] {
            assert_eq!(
                a.scalars[s.index()].to_bits(),
                b.scalars[s.index()].to_bits(),
                "path {path}: {s:?}"
            );
        }
        assert!(b.scalars[PathScalar::EquityOutcome.index()].is_finite());
        for s in [
            PathScalar::EquityGrossProceeds,
            PathScalar::EquityExerciseCost,
            PathScalar::EquityTenderProceeds,
            PathScalar::EquityNetProceeds,
        ] {
            assert_eq!(b.scalars[s.index()], 0.0, "path {path}: {s:?}");
        }
    }
    assert!(failures > 30, "{failures} failures in 150 paths");
    assert!(
        ended_job * 2 > failures,
        "{ended_job} of {failures} failures ended the job"
    );
}

#[test]
fn a_second_company_does_not_move_the_first() {
    let base = startup_employee();
    let mut two = base.clone();
    let mut other = base.private_equity[0].clone();
    other.id = "second".into();
    other.name = "Second Co".into();
    other.job = None;
    other.stage = CompanyStage::Seed;
    other.holdings = vec![EquityHolding {
        id: "angel".into(),
        kind: HoldingKind::Shares {
            basis_per_share: 2.0,
            months_held: 30,
            from_iso: false,
            fmv_at_exercise: 0.0,
        },
        shares: 5_000.0,
        ..EquityHolding::default()
    }];
    two.private_equity.push(other);
    let (plan_a, plan_b) = (compile(&base), compile(&two));
    let (mut ws_a, mut ws_b) = (Workspace::new(&plan_a), Workspace::new(&plan_b));
    let mut second_events = 0;
    for path in 0..80 {
        let a = equity_log(&plan_a, 19, path, &mut ws_a);
        let b = equity_log(&plan_b, 19, path, &mut ws_b);
        let first = |l: &EquityLog| -> Vec<(i32, &'static str, String)> {
            fate_fingerprint(l)
                .into_iter()
                .filter(|e| e.2.contains("Northwind"))
                .collect()
        };
        assert_eq!(first(&a), first(&b), "path {path}");
        second_events += b
            .events
            .iter()
            .filter(|e| e.2.contains("Second Co"))
            .count();
        // The second company's fate is the one its own stream draws.
        let fate = drawn_fate(&two.private_equity[1], 19, path, 1);
        if let Some(e) = b
            .events
            .iter()
            .find(|e| e.2.contains("Second Co") && e.1 != "tender_offer" && e.1 != "lockup_sale")
        {
            assert_eq!(e.1, kind_key(fate.kind), "path {path}: {}", e.2);
        }
    }
    assert!(second_events > 40, "{second_events}");
}

#[test]
fn paper_value_is_in_no_balance_and_no_test() {
    // A company that never has an event, with nothing exercised, tendered or owned
    // outright: no cash ever moves, so counting it or not must give the same life,
    // bit for bit, while the paper value is reported throughout.
    let mut base = startup_with(certain(OutcomeKind::StillPrivate, 0.0, 0.0, 1.0));
    {
        let co = &mut base.private_equity[0];
        co.holdings.retain(|h| !h.kind.is_shares());
        co.policy.exercise = ExerciseTiming::AtLiquidity;
        co.policy.at_deadline = DeadlinePolicy::Forfeit;
    }
    base.assumptions.career_risk = true;
    let mut zero = base.clone();
    zero.private_equity[0].included = false;
    let (plan_a, plan_b) = (compile(&base), compile(&zero));
    let a = run(&plan_a, 23, 0..300);
    let b = run(&plan_b, 23, 0..300);
    assert_eq!(scalar_bits(&a), scalar_bits(&b));
    for path in 0..6 {
        let (ta, tb) = (
            sim::trace_path(&plan_a, 23, path),
            sim::trace_path(&plan_b, 23, path),
        );
        for key in ["net_worth", "investable", "balance_cash", "tax_federal"] {
            assert_eq!(ta.column(key), tb.column(key), "path {path} {key}");
        }
        assert!(tb.column("equity_paper_value").is_none());
        let paper = ta.column("equity_paper_value").expect("paper value");
        // Vested holdings are worth something on paper every month until the
        // person leaves (the options lapse then) or thirty years have passed.
        let left = event_months(&ta, "equity_forfeited")
            .into_iter()
            .chain(event_months(&ta, "company_failed"))
            .min()
            .unwrap_or(360)
            .min(ta.months as i32);
        assert!(left > 0);
        assert!(
            paper[..left as usize].iter().all(|v| *v > 0.0),
            "path {path}: paper value missing before month {left}"
        );
        assert_eq!(
            column(&ta, "income_equity").iter().sum::<f64>()
                + column(&ta, "equity_exercise").iter().sum::<f64>(),
            0.0
        );
        // The annual record carries the year-end stock, not a sum of months.
        let annual = ta
            .annual
            .columns
            .iter()
            .find(|c| c.key == "equity_paper_value")
            .expect("annual paper value");
        if ta.months >= 14 {
            assert_eq!(annual.values[0], paper[1], "path {path}");
        }
    }
}

#[test]
fn deadline_exercises_do_not_depend_on_the_fate() {
    // Leaving after a year starts a three-month window. Whether the household
    // exercises at that deadline must not depend on what the company will do later:
    // the decision compares the strike with the last round's price, never with the
    // hidden path. Lives whose event comes before the deadline are left out.
    let mut profile = startup_employee();
    {
        let co = &mut profile.private_equity[0];
        co.tenders.annual_probability = Some(0.0);
        co.policy.leave_after_months = Some(12);
    }
    profile.assumptions.career_risk = false;
    let plan = compile(&profile);
    let mut ws = Workspace::new(&plan);
    // [exercised, lives] for fates that pay nothing and fates that pay.
    let mut groups = [[0.0f64; 2]; 2];
    for path in 0..1_500 {
        let fate = drawn_fate(&profile.private_equity[0], 29, path, 0);
        if fate.month <= 15 + 60 * i32::from(fate.kind == OutcomeKind::Ipo) {
            continue;
        }
        let log = equity_log(&plan, 29, path, &mut ws);
        if log.months <= 16 {
            continue;
        }
        let good = matches!(fate.kind, OutcomeKind::Ipo)
            || (fate.kind == OutcomeKind::Acquired && fate.x >= 1.0);
        let g = &mut groups[usize::from(good)];
        g[0] += f64::from(u8::from(log.months_of("options_exercised").contains(&15)));
        g[1] += 1.0;
    }
    let (bad, good) = (groups[0], groups[1]);
    assert!(bad[1] > 200.0 && good[1] > 100.0, "{groups:?}");
    let (p_bad, p_good) = (bad[0] / bad[1], good[0] / good[1]);
    let pooled = (bad[0] + good[0]) / (bad[1] + good[1]);
    let se = (pooled * (1.0 - pooled) * (1.0 / bad[1] + 1.0 / good[1])).sqrt();
    assert!(
        (p_bad - p_good).abs() <= 4.0 * se + 1e-12,
        "exercised at the deadline: {p_bad:.3} of {} lives heading for nothing, {p_good:.3} of {} heading for a payout (4 SE = {:.3})",
        bad[1],
        good[1],
        4.0 * se
    );
    assert!(pooled > 0.9, "the default policy exercises: {pooled}");
}

#[test]
fn tenders_and_ipo_dates_are_realised_as_stated() {
    // A unicorn that lists for certain, with the preset's timing: the month it
    // lists in the simulation is the month drawn, moved only into an open window.
    let mut profile = startup_with(Vec::new());
    profile.assumptions.longevity = Longevity::FixedAge { age: 50.0 };
    {
        let co = &mut profile.private_equity[0];
        co.stage = CompanyStage::Unicorn;
        co.outcomes = vec![OutcomeRow {
            id: "ipo".into(),
            kind: OutcomeKind::Ipo,
            probability: 1.0,
            years_from: 0.5,
            years_to: 8.0,
            multiple_low: 0.8,
            multiple_high: 3.0,
        }];
    }
    let plan = compile(&profile);
    let mut ws = Workspace::new(&plan);
    let n = 3_000;
    let (mut sum, mut sum_sq, mut drawn_sum, mut listed) = (0.0, 0.0, 0.0, 0);
    for path in 0..n {
        let log = equity_log(&plan, 31, path, &mut ws);
        let fate = drawn_fate(&profile.private_equity[0], 31, path, 0);
        let month = log.months_of("company_ipo");
        assert_eq!(month.len(), 1, "path {path}");
        let moved = f64::from(month[0] - fate.month);
        sum += moved;
        sum_sq += moved * moved;
        drawn_sum += f64::from(fate.month);
        listed += 1;
    }
    let mean = sum / f64::from(listed);
    let se = ((sum_sq / f64::from(listed) - mean * mean) / f64::from(listed)).sqrt();
    assert!(
        mean.abs() <= 2.0 + 4.0 * se,
        "IPOs list {mean:.2} months from the month drawn on average (mean drawn month {:.1}, SE {se:.2})",
        drawn_sum / f64::from(listed)
    );

    // A company that stays private at its last round's price and lists there in
    // thirty years, 15% a year: the share of company-years with a tender in the
    // first ten years is close to it in every market model (a little under,
    // because of the twelve-month gap). (With one future, and that one not heading
    // below the last round, the chance entered is the chance in that future.)
    let mut flat = startup_with(certain(OutcomeKind::Ipo, 30.0, 30.0, 1.0));
    flat.assumptions.longevity = Longevity::FixedAge { age: 50.0 };
    flat.private_equity[0].tenders.annual_probability = Some(0.15);
    for kind in MARKETS {
        let plan = compile(&with_market(flat.clone(), kind));
        let mut ws = Workspace::new(&plan);
        let (mut years_with, mut years) = (0u32, 0u32);
        for path in 0..800 {
            let log = equity_log(&plan, 37, path, &mut ws);
            let months = log.months_of("tender_offer");
            for pair in months.windows(2) {
                assert!(pair[1] - pair[0] >= 12, "{kind:?} path {path}: {months:?}");
            }
            for year in 0..10 {
                years += 1;
                years_with += u32::from(
                    months
                        .iter()
                        .any(|m| (12 * year..12 * year + 12).contains(m)),
                );
            }
        }
        let share = f64::from(years_with) / f64::from(years);
        assert!(
            (0.115..=0.165).contains(&share),
            "{kind:?}: {share:.3} of company-years had a tender"
        );
    }
}

/// Paths for the frequency tests: the number the design asks for in a release
/// build, fewer in a debug build, where the tolerance (in standard errors) widens
/// by itself.
fn frequency_paths() -> u64 {
    if cfg!(debug_assertions) {
        3_000
    } else {
        20_000
    }
}

#[test]
fn a_default_policy_never_loses_money_silently() {
    // A company that never sells, lists or fails. Under the default policy the
    // options are exercised when they would lapse (ten years from the grant, or on
    // leaving), and the shares are written off after thirty years. A life that
    // ends with less than it put in must show both: the exercise, and afterwards
    // the event that made the money worthless.
    let mut profile = startup_employee();
    profile.private_equity[0].outcomes = certain(OutcomeKind::StillPrivate, 0.0, 0.0, 1.0);
    let plan = compile(&profile);
    let mut ws = Workspace::new(&plan);
    let (mut losses, mut written_off, mut ended_first) = (0u32, 0u32, 0u32);
    for path in 0..200 {
        let log = equity_log(&plan, 41, path, &mut ws);
        let net = log.scalars[PathScalar::EquityNetProceeds.index()];
        let cost = log.scalars[PathScalar::EquityExerciseCost.index()];
        assert_eq!(
            log.scalars[PathScalar::EquityOutcome.index()],
            firesight_core::stats::EquityOutcome::StillPrivate.index() as f32,
            "path {path}"
        );
        assert!(log.scalars[PathScalar::EquityEventAge.index()].is_nan());
        if net >= -1.0 {
            continue;
        }
        losses += 1;
        let exercised = log.months_of("options_exercised");
        assert!(
            !exercised.is_empty() && cost > 0.0,
            "path {path}: lost {net:.0} without an exercise: {:?}",
            log.events
        );
        let first = exercised[0];
        let later = |kind: &str| log.months_of(kind).into_iter().any(|m| m >= first);
        if later("company_failed") {
            written_off += 1;
            let label = &log
                .events
                .iter()
                .find(|e| e.1 == "company_failed")
                .unwrap()
                .2;
            assert!(
                label.contains("written off") && label.contains("is lost"),
                "{label}"
            );
        } else if log.months <= 360 {
            // The owner died holding the shares, before the write-off.
            ended_first += 1;
        } else {
            assert!(
                later("equity_forfeited"),
                "path {path}: lost {net:.0}, nothing accounts for it: {:?}",
                log.events
            );
        }
    }
    assert!(losses > 40, "{losses} lives lost money in 200");
    assert!(
        written_off * 10 >= losses * 8,
        "{written_off} of {losses} losses end in the write-off ({ended_first} lives ended first)"
    );
}

#[test]
fn the_preview_and_the_simulation_agree_on_nothing() {
    // The preview's holder stays until the outcome and sells everything then.
    // With the household made to do the same (no layoffs, no early retirement, a
    // long fixed life, no tender offers, nothing exercised early), the share of
    // lives in which the equity pays nothing is the preview's.
    let mut profile = startup_employee();
    profile.assumptions.career_risk = false;
    profile.assumptions.health_risk = false;
    profile.assumptions.longevity = Longevity::FixedAge { age: 95.0 };
    profile.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 67.0,
    };
    {
        let co = &mut profile.private_equity[0];
        co.tenders.annual_probability = Some(0.0);
        co.policy.at_deadline = DeadlinePolicy::Forfeit;
    }
    let params = EquityParams::from_data(Data::embedded());
    let preview = equity::preview(&profile.private_equity[0], &params, 50_000);
    let paths = frequency_paths();
    let plan = compile(&profile);
    let summary = plan.summarize(&run(&plan, 43, 0..paths));
    let e = summary.equity.as_ref().expect("an equity block");
    assert_eq!(e.paths, paths);
    let (p, q) = (preview.nothing_share, e.nothing_rate.value);
    let se = (p * (1.0 - p) * (1.0 / paths as f64 + 1.0 / 50_000.0)).sqrt();
    assert!(
        (p - q).abs() < 4.0 * se + 0.02,
        "nothing: preview {p:.4}, simulation {q:.4} over {paths} paths (4 SE = {:.4})",
        4.0 * se
    );
    // With nothing exercised and no tender offers, no cost and no loss.
    assert_eq!(e.exercised_rate.value, 0.0);
    assert_eq!(e.tender_rate.value, 0.0);
    assert_eq!(e.loss_rate.value, 0.0);
    // Before tax the two agree on the size of the payout too: the preview lets the
    // grants lapse at their ten-year term as the simulation does (it used to pay
    // them in full), and deflates at the simulation's median inflation.
    for (name, a, b) in [
        ("p75", preview.quantiles[3], e.gross_proceeds.p75),
        ("p90", preview.quantiles[4], e.gross_proceeds.p90),
    ] {
        assert!(
            (a / b - 1.0).abs() < 0.12,
            "{name} before tax: {a:.0} in the preview, {b:.0} in the simulation"
        );
    }
    // The preview is before tax and the simulation after it: among lives that
    // are paid, the median after tax is below the preview's, by the tax.
    let (gross, net) = (preview.if_paid[1], e.net_proceeds_if_paid.p50);
    assert!(
        net < 0.8 * gross && net > 0.4 * gross,
        "median when paid: {gross:.0} before tax in the preview, {net:.0} after tax in the simulation"
    );
}

#[test]
fn demo_outcome_shares_match_the_lifetime_odds() {
    // The demo as shipped. The table gives the odds for ten years; a company
    // still private faces them again twice, so over a life each outcome has its
    // ten-year probability times (1 - q^3) / (1 - q), with q the chance of staying
    // private. Lives that end before the event can only lower a share.
    let profile = startup_employee();
    let company = &profile.private_equity[0];
    let params = EquityParams::from_data(Data::embedded());
    let p = params.stage_probabilities(company.stage, company.outlook);
    let table = equity::resolved_table(company, &params);
    let q = table.still_private_probability();
    let factor = equity::lifetime_factor(q, params.rounds);
    let paths = frequency_paths();
    let plan = compile(&profile);
    let acc = run(&plan, 47, 0..paths);
    let summary = plan.summarize(&acc);
    let e = summary.equity.as_ref().expect("an equity block");
    assert_eq!(e.company, "Northwind Robotics");
    assert_eq!(e.paths, paths);
    assert_eq!(e.outcomes.iter().map(|o| o.count).sum::<u64>(), paths);
    use firesight_core::stats::EquityOutcome;
    // (class, ten-year probability): the order of `stage_probabilities` is fails,
    // acquired, ipo, still private.
    for (outcome, ten_year) in [
        (
            EquityOutcome::Failed,
            table.probability_of(OutcomeKind::Fails),
        ),
        (
            EquityOutcome::Acquired,
            table.probability_of(OutcomeKind::Acquired),
        ),
        (EquityOutcome::Ipo, table.probability_of(OutcomeKind::Ipo)),
    ] {
        let expected = ten_year * factor;
        let share = e.outcomes[outcome.index()].share.value;
        let se = (expected * (1.0 - expected) / paths as f64).sqrt();
        assert!(
            share <= expected + 4.0 * se && share >= expected - 4.0 * se - 0.02,
            "{}: {share:.4} of {paths} lives against lifetime odds of {expected:.4} (4 SE = {:.4})",
            outcome.key(),
            4.0 * se
        );
    }
    // The preset's probabilities are the table's, and they are the documented
    // Series C odds to within a point (0.422, 0.380, 0.169 over a life).
    assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    for (kind, documented) in [
        (OutcomeKind::Fails, 0.422),
        (OutcomeKind::Acquired, 0.380),
        (OutcomeKind::Ipo, 0.169),
    ] {
        let lifetime = table.probability_of(kind) * factor;
        assert!(
            (lifetime - documented).abs() < 0.01,
            "{kind:?}: {lifetime:.4} against {documented}"
        );
    }
    // "Still private" is the written-off share q^3 plus the lives that end first.
    let still = e.outcomes[EquityOutcome::StillPrivate.index()].share.value;
    let written_off = firesight_core::math::powi(q, params.rounds as i32);
    assert!(
        still >= written_off - 0.01 && still <= written_off + 0.06,
        "still private {still:.4} against a write-off share of {written_off:.4}"
    );

    // The digest carries the same shares, and the headline by outcome is ordered
    // as one would expect: a listing does better than a failure.
    let d = summary.digest();
    let de = d.equity.as_ref().unwrap();
    assert!((de.failed + de.still_private + de.acquired + de.ipo - 1.0).abs() < 0.002);
    assert!(
        de.nothing.p > 0.45 && de.nothing.p < 0.80,
        "{}",
        de.nothing.p
    );
    // The median life is paid nothing. Its net figure is a little above zero, not
    // zero: the shares already held are a capital loss when the company fails, and
    // the tax that saves counts as tax the equity caused, with a minus sign.
    assert_eq!(e.gross_proceeds.p50, 0.0);
    assert!(
        (0.0..=5_000.0).contains(&de.net_proceeds.p50),
        "{}",
        de.net_proceeds.p50
    );
    assert!(de.success_by_outcome[3] > de.success_by_outcome[0]);
    assert!(serde_json::to_string(&d).unwrap().len() < 2_400);
    // Every path's row agrees with the block built from it.
    let ipo_rows = acc
        .rows()
        .filter(|(_, r)| r[PathScalar::EquityOutcome.index()] == EquityOutcome::Ipo.index() as f32)
        .count() as u64;
    assert_eq!(e.by_outcome[3].paths, ipo_rows);
    // An event has an age exactly when the company failed, sold or listed.
    for (_, r) in acc.rows() {
        let private =
            r[PathScalar::EquityOutcome.index()] == EquityOutcome::StillPrivate.index() as f32;
        assert_eq!(r[PathScalar::EquityEventAge.index()].is_nan(), private);
    }
}
