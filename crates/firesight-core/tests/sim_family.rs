//! Household dynamics on simulated paths: divorce, parents, caregiving and life
//! insurance. Each feature is checked twice: switched off it changes nothing, and
//! switched on it moves outcomes in the expected direction by a plausible amount,
//! on paired paths (the same seed and path indices).

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{PathTrace, Summary, TraceEventKind};

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .profile
}

fn json(s: &Summary) -> String {
    serde_json::to_string(s).unwrap()
}

fn traces(profile: &Profile, seed: u64, paths: u64) -> Vec<PathTrace> {
    let plan = compile(profile);
    let mut ws = Workspace::new(&plan);
    (0..paths)
        .map(|i| sim::trace_path_in(&plan, seed, i, &mut ws))
        .collect()
}

fn column(t: &PathTrace, key: &str) -> Vec<f64> {
    t.column(key)
        .map_or(vec![0.0; t.months as usize], |c| c.to_vec())
}

// ---------------------------------------------------------------- F46 divorce

fn divorce_event(at: i32, terms: DivorceTerms) -> LifeEvent {
    LifeEvent {
        id: "divorce".into(),
        name: "Divorce".into(),
        when: When::date(START + at),
        kind: EventKind::Divorce(terms),
        ..LifeEvent::default()
    }
}

#[test]
fn a_marriage_date_with_divorce_risk_off_changes_nothing() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    p.household.married = Some(START - 60);
    p.household.divorce_risk = DivorceRisk::Off;
    assert_eq!(json(&summary(&base, 3, 300)), json(&summary(&p, 3, 300)));
}

#[test]
fn a_divorce_event_splits_the_household() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    p.household.married = Some(START - 60);
    let at = 72;
    p.events.push(divorce_event(at, DivorceTerms::default()));
    let a = traces(&base, 7, 30);
    let b = traces(&p, 7, 30);
    let mut ratios = Vec::new();
    for (x, y) in a.iter().zip(&b) {
        let nx = column(x, "net_worth");
        let ny = column(y, "net_worth");
        let ev = y
            .events
            .iter()
            .find(|e| e.kind == TraceEventKind::LifeEvent && e.label.starts_with("Divorce"));
        let alive = x.months as i32 > at;
        if !alive {
            continue;
        }
        let ev = ev.expect("the divorce is in the event log");
        if ev.label.contains("no partner") {
            // Someone died before the divorce date: nothing happens (the event
            // month still re-plans the year, as every event does).
            assert!(x.events.iter().any(|e| e.kind == TraceEventKind::Died));
            assert_eq!(x.months, y.months);
            continue;
        }
        assert!(ev.label.contains("years of marriage"), "{}", ev.label);
        // Identical up to the month before the divorce.
        for m in 0..at as usize {
            assert_eq!(nx[m], ny[m], "path {} month {m}", x.path_index);
        }
        // The partner's wages stop.
        let wx = column(x, "income_wages");
        let wy = column(y, "income_wages");
        let after: f64 = wy[at as usize + 1..at as usize + 13].iter().sum();
        let before: f64 = wx[at as usize + 1..at as usize + 13].iter().sum();
        assert!(after < 0.85 * before, "wages {after} vs {before}");
        if nx[at as usize - 1] > 50_000.0 {
            ratios.push(ny[at as usize] / nx[at as usize]);
        }
    }
    // With an even split of net worth, the household keeps about half (less the
    // legal costs and a month of the partner's pay that is now theirs).
    assert!(ratios.len() > 10, "{}", ratios.len());
    for r in &ratios {
        assert!(
            (0.35..0.62).contains(r),
            "kept {r} of net worth: {ratios:?}"
        );
    }
    // Over the whole plan: the household is worse off.
    let sa = summary(&base, 7, 400);
    let sb = summary(&p, 7, 400);
    assert!(
        sb.headline.median_terminal_net_worth < 0.8 * sa.headline.median_terminal_net_worth,
        "{} vs {}",
        sb.headline.median_terminal_net_worth,
        sa.headline.median_terminal_net_worth
    );
}

#[test]
fn a_divorce_with_a_home_sale_and_support_conserves_money() {
    let mut p = example("dual-income-family");
    p.household.married = Some(START - 60);
    p.events.push(divorce_event(
        60,
        DivorceTerms {
            keeps_home: false,
            support: Support {
                annual: 12_000.0,
                years: 5.0,
                direction: SupportDirection::Pays,
            },
            ..DivorceTerms::default()
        },
    ));
    let plan = compile(&p);
    let acc = run_checked(&plan, 5, 0..200);
    assert_eq!(acc.paths, 200);
    // The sale and the support show in a trace.
    let t = sim::trace_path(&plan, 5, 0);
    assert!(t
        .events
        .iter()
        .any(|e| e.kind == TraceEventKind::SoldHome && e.label.starts_with("Divorce")));
    let ess = column(&t, "spend_essential");
    assert!(ess[61] > 0.0);
}

#[test]
fn the_typical_divorce_risk_matches_the_published_rates() {
    // A couple married at the start: 33% of first marriages end within ten years
    // (less the few in which someone dies first).
    let mut p = example("dual-income-family");
    p.household.married = Some(START);
    p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms::default());
    let n = 400;
    let ts = traces(&p, 11, n);
    let within = ts
        .iter()
        .filter(|t| {
            t.events.iter().any(|e| {
                e.kind == TraceEventKind::LifeEvent
                    && e.label.starts_with("Divorce")
                    && e.month < START + 120
            })
        })
        .count() as f64
        / n as f64;
    // Binomial standard error at 400 paths is 2.4 points.
    assert!((0.26..0.40).contains(&within), "{within}");
    // Paired: the market and careers are untouched, so the months before each
    // path's divorce are identical to the baseline.
    let base = traces(&example("dual-income-family"), 11, 20);
    for (x, y) in base.iter().zip(&ts) {
        let first = y
            .events
            .iter()
            .find(|e| e.label.starts_with("Divorce"))
            .map_or(x.months.min(y.months) as i32, |e| e.month - START);
        let nx = column(x, "net_worth");
        let ny = column(y, "net_worth");
        for m in 0..first as usize {
            assert_eq!(nx[m], ny[m], "path {} month {m}", x.path_index);
        }
    }
}

#[test]
fn a_divorce_after_ten_years_keeps_the_spousal_record() {
    // A one-earner couple: after a long marriage the person who stays draws a
    // benefit on the former partner's record; after a short one they do not.
    let mut base = example("dual-income-family");
    base.incomes.retain(|i| i.owner == 1);
    let ss = |married_before: i32| {
        let mut p = base.clone();
        p.household.married = Some(START - married_before);
        p.events.push(divorce_event(12, DivorceTerms::default()));
        traces(&p, 13, 40)
            .iter()
            .map(|t| column(t, "income_social_security").iter().sum::<f64>())
            .sum::<f64>()
    };
    let long = ss(120);
    let short = ss(12);
    assert!(long > 1.05 * short, "long {long} short {short}");
}
