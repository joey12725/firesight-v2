//! Determinism, chunking invariance and stream isolation.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::rng::{Rng, Stream};
use firesight_core::sim;
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
