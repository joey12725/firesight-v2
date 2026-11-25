//! Household dynamics on simulated paths: divorce, parents, caregiving and life
//! insurance. Each feature is checked twice: switched off it changes nothing, and
//! switched on it moves outcomes in the expected direction by a plausible amount,
//! on paired paths (the same seed and path indices).

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{PathScalar, PathTrace, Summary, TraceEventKind};

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

// ---------------------------------------------------------------- F47 parents

fn parent(id: &str, child: usize, age: i32, estate: f64) -> Parent {
    Parent {
        id: id.into(),
        name: id.into(),
        child,
        birth: START - age * 12,
        sex: Sex::Female,
        estate,
        share: 0.5,
        inherited_ira_fraction: 0.4,
        ..Parent::default()
    }
}

#[test]
fn an_inheritance_arrives_when_the_parent_dies() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    let mut mom = parent("mom", 0, 78, 600_000.0);
    mom.care_erodes_estate = false;
    p.household.parents = vec![mom];
    let a = traces(&base, 21, 30);
    let b = traces(&p, 21, 30);
    let mut seen = 0;
    for (x, y) in a.iter().zip(&b) {
        let Some(ev) = y
            .events
            .iter()
            .find(|e| e.label.starts_with("Inheritance from mom"))
        else {
            continue;
        };
        seen += 1;
        // Half of $600k, all of it with care not charged to the estate.
        assert_eq!(ev.amount, Some(300_000.0), "{}", ev.label);
        let at = (ev.month - START) as usize;
        let nx = column(x, "net_worth");
        let ny = column(y, "net_worth");
        for m in 0..at.min(nx.len()) {
            assert_eq!(nx[m], ny[m], "path {} month {m}", x.path_index);
        }
        // Ten December distributions of the inherited IRA (while someone is alive).
        let dists = y
            .events
            .iter()
            .filter(|e| e.label.starts_with("Inherited IRA distribution"))
            .count();
        if y.months as i32 > (ev.month - START) + 12 * 12 {
            assert_eq!(dists, 10, "path {}", y.path_index);
        }
    }
    assert!(
        seen > 25,
        "{seen} of 30 paths had the parent die within the plan"
    );
    let sa = summary(&base, 21, 400);
    let sb = summary(&p, 21, 400);
    assert!(sb.headline.success_rate.value >= sa.headline.success_rate.value);
    let gain = sb.headline.median_terminal_net_worth - sa.headline.median_terminal_net_worth;
    // $300k arriving about ten years in, invested for decades: more than its face
    // value at the end, but not wildly so.
    assert!((200_000.0..3_000_000.0).contains(&gain), "{gain}");
}

#[test]
fn a_parents_care_erodes_the_estate() {
    let mut p = example("dual-income-family");
    p.household.parents = vec![parent("mom", 0, 80, 200_000.0)];
    let ts = traces(&p, 5, 60);
    let mut eroded = 0;
    let mut full = 0;
    for t in &ts {
        if let Some(ev) = t.events.iter().find(|e| e.label.starts_with("Inheritance")) {
            let a = ev.amount.unwrap();
            assert!((0.0..=100_000.0).contains(&a));
            if ev.label.contains("of care") {
                eroded += 1;
                assert!(a < 100_000.0);
            } else {
                full += 1;
                assert_eq!(a, 100_000.0);
            }
        }
    }
    assert!(eroded > 10 && full > 10, "eroded {eroded} full {full}");
}

#[test]
fn parents_switched_off_change_nothing() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    p.household.parents = Vec::new();
    assert_eq!(json(&summary(&base, 3, 200)), json(&summary(&p, 3, 200)));
    // A parent with nothing to leave changes nothing either but the event log.
    p.household.parents = vec![parent("mom", 0, 70, 0.0)];
    let s = summary(&p, 3, 200);
    assert_eq!(
        s.headline.median_terminal_net_worth,
        summary(&base, 3, 200).headline.median_terminal_net_worth
    );
}

// ---------------------------------------------------------- F48 parent care

#[test]
fn caring_for_a_parent_costs_money_and_hours() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    let mut dad = parent("dad", 1, 80, 0.0);
    dad.sex = Sex::Male;
    dad.care_support = Some(CareSupport {
        probability: 1.0,
        annual_cost: 12_000.0,
        hours: 0.5,
        caregiver: None,
    });
    p.household.parents = vec![dad];
    let a = traces(&base, 31, 60);
    let b = traces(&p, 31, 60);
    let mut spells = 0;
    for (x, y) in a.iter().zip(&b) {
        let Some(ev) = y
            .events
            .iter()
            .find(|e| e.kind == TraceEventKind::LongTermCare && e.label.starts_with("dad"))
        else {
            continue;
        };
        let at = (ev.month - START) as usize;
        let nx = column(x, "net_worth");
        let ny = column(y, "net_worth");
        for m in 0..at {
            assert_eq!(nx[m], ny[m], "path {} month {m}", x.path_index);
        }
        if !ev.label.contains("works 50% less") {
            continue; // the caregiver was not in a job then
        }
        spells += 1;
        // The first year of the spell (or all of it if shorter; it ends with the
        // death): the nurse earns about half, and the household pays $1,000 a
        // month toward the care.
        let death = y
            .events
            .iter()
            .find(|e| e.label.starts_with("Inheritance from dad"))
            .map_or(usize::MAX, |e| (e.month - START) as usize);
        let end = (at + 12)
            .min(death)
            .min(x.months as usize)
            .min(y.months as usize);
        let months = (end - at) as f64;
        let wx: f64 = column(x, "income_wages")[at..end].iter().sum();
        let wy: f64 = column(y, "income_wages")[at..end].iter().sum();
        let lost = wx - wy;
        // Daniel earns about $92k a year in today's dollars: half is about $3,800
        // a month, give or take raises, shocks and the other earner.
        assert!(
            (1_500.0 * months..7_000.0 * months).contains(&lost),
            "lost {lost} over {months} months"
        );
        let cx: f64 = column(x, "spend_long_term_care")[at..end].iter().sum();
        let cy: f64 = column(y, "spend_long_term_care")[at..end].iter().sum();
        // $1,000 a month in today's dollars at the start, rising with excess
        // medical inflation (1.5% a year by default) since then.
        let years = at as f64 / 12.0;
        let growth = (cy - cx) / (1_000.0 * months);
        let expected = firesight_core::math::powi(1.015, years.round() as i32);
        assert!(
            (growth / expected - 1.0).abs() < 0.05,
            "{growth} vs {expected}"
        );
    }
    assert!(spells > 5, "{spells}");
    let sa = summary(&base, 31, 400);
    let sb = summary(&p, 31, 400);
    assert!(sb.headline.median_terminal_net_worth < sa.headline.median_terminal_net_worth);
}

// ------------------------------------------------------- F49 spousal care

#[test]
fn spousal_care_lowers_the_care_bill() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    p.spending.healthcare.spousal_care = true;
    let n = 400;
    let plan_a = compile(&base);
    let plan_b = compile(&p);
    let a = run(&plan_a, 41, 0..n);
    let b = run(&plan_b, 41, 0..n);
    let ca = a.scalar_column(PathScalar::LongTermCareCost);
    let cb = b.scalar_column(PathScalar::LongTermCareCost);
    let (mut lower, mut higher) = (0, 0);
    let (mut sa, mut sb) = (0.0, 0.0);
    for (x, y) in ca.iter().zip(&cb) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        if x.is_finite() && y.is_finite() {
            sa += x;
            sb += y;
            if y < x - 1.0 {
                lower += 1;
            }
            if y > x + 1.0 {
                higher += 1;
            }
        }
    }
    // Home care is about a third of episodes; half of it is saved while the
    // partner is alive and at home: several percent of the lifetime care bill.
    let saved = 1.0 - sb / sa;
    assert!((0.02..0.3).contains(&saved), "saved {saved}");
    assert!(lower > 20, "{lower}");
    // Care can only cost more on a path where the household's money changes
    // something else (a later sale, a different plan): rare.
    assert!(higher * 10 < lower, "{higher} vs {lower}");
    // Off: the profile is the baseline.
    let mut off = base.clone();
    off.spending.healthcare.spousal_care = false;
    assert_eq!(json(&summary(&base, 3, 100)), json(&summary(&off, 3, 100)));
}

// ------------------------------------------------------- F50 life insurance

fn term_policy(benefit: f64, premium: f64, years: f64) -> LifePolicy {
    LifePolicy {
        id: "term".into(),
        name: "Term life".into(),
        kind: PolicyKind::Term {
            years_remaining: years,
        },
        death_benefit: benefit,
        annual_premium: premium,
    }
}

#[test]
fn life_insurance_pays_on_early_deaths_and_costs_premiums_otherwise() {
    let base = example("dual-income-family");
    let mut p = base.clone();
    // About the market price for a healthy 40-year-old: $1M for 20 years.
    p.household.people[0].life_insurance = vec![term_policy(1_000_000.0, 900.0, 20.0)];
    let age0 = f64::from(p.household.people[0].birth.age_months_at(START)) / 12.0;
    let n = 600;
    let a = run(&compile(&base), 51, 0..n);
    let b = run(&compile(&p), 51, 0..n);
    assert_eq!(a.path_indices(), b.path_indices());
    let d0 = b.scalar_column(PathScalar::DeathAge0);
    let d1 = b.scalar_column(PathScalar::DeathAge1);
    let wa = a.scalar_column(PathScalar::TerminalNetWorth);
    let wb = b.scalar_column(PathScalar::TerminalNetWorth);
    let (mut paid, mut unpaid) = (Vec::new(), Vec::new());
    for i in 0..wa.len() {
        let (x, y) = (f64::from(wa[i]), f64::from(wb[i]));
        let died = f64::from(d0[i]);
        if died < age0 + 19.9 && f64::from(d1[i]) > died + 1.0 {
            paid.push(y - x);
        } else if died > age0 + 20.1 {
            unpaid.push(y - x);
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    // A death within the term while the partner lives: about 5% of paths at
    // these ages. $1M nominal paid then, invested for decades: more than its
    // real value at the end, but not wildly so.
    assert!(paid.len() > 8, "{}", paid.len());
    let gain = median(&mut paid);
    assert!((300_000.0..6_000_000.0).contains(&gain), "{gain}");
    // Otherwise: twenty years of $900 premiums, compounded, are lost.
    let loss = -median(&mut unpaid);
    assert!((3_000.0..150_000.0).contains(&loss), "{loss}");
    // Traces log the payout, in real dollars.
    let ts = traces(&p, 51, 120);
    let ev = ts
        .iter()
        .flat_map(|t| &t.events)
        .find(|e| e.label.starts_with("Life insurance pays out"));
    if let Some(ev) = ev {
        let real = ev.amount.unwrap();
        assert!(real <= 1_000_000.0 && real > 300_000.0, "{real}");
    }
    // Off: an empty list is the baseline.
    let mut off = base.clone();
    off.household.people[0].life_insurance = Vec::new();
    assert_eq!(json(&summary(&base, 3, 100)), json(&summary(&off, 3, 100)));
}

#[test]
fn a_permanent_policy_counts_toward_the_bequest() {
    let mut base = example("dual-income-family");
    base.retirement.bequest_target = 2_000_000.0;
    let mut p = base.clone();
    for person in &mut p.household.people {
        person.life_insurance = vec![LifePolicy {
            id: "whole".into(),
            kind: PolicyKind::Permanent,
            death_benefit: 750_000.0,
            annual_premium: 6_000.0,
            ..LifePolicy::default()
        }];
    }
    let sa = summary(&base, 9, 300);
    let sb = summary(&p, 9, 300);
    // Both lives are insured, so the last death always pays $750k (nominal) to
    // the estate unless the household ran out of money: more paths leave the
    // bequest than without, at the cost of $12k a year of premiums.
    assert!(
        sb.headline.success_rate.value > sa.headline.success_rate.value,
        "{} vs {}",
        sb.headline.success_rate.value,
        sa.headline.success_rate.value
    );
    // On a path that ends with the last death (not at the horizon) the estate is
    // paid unless the policy lapsed; on one cut by the horizon it never is.
    let horizon = compile(&p).horizon_months();
    let ts = traces(&p, 9, 20);
    let (mut estate, mut cut) = (0, 0);
    for t in &ts {
        let has = |s: &str| t.events.iter().any(|e| e.label.starts_with(s));
        let lapsed = has("Life insurance whole: lapses");
        let paid = has("Life insurance pays the estate");
        if t.months < horizon && !lapsed {
            assert!(paid, "path {}", t.path_index);
            estate += 1;
        } else {
            assert!(!paid, "path {}", t.path_index);
            cut += 1;
        }
        // The first death paid the survivor unless the policy had lapsed.
        assert!(
            has("Life insurance pays out") || lapsed,
            "path {}",
            t.path_index
        );
    }
    assert!(estate > 5 && cut > 2, "{estate} {cut}");
}

// ------------------------------------------------------- stream discipline

/// Months and labels of the household-dynamics events on a trace.
fn family_events(t: &PathTrace) -> Vec<(i32, String)> {
    t.events
        .iter()
        .filter(|e| {
            e.label.starts_with("Divorce")
                || e.label.starts_with("Inheritance from")
                || e.label.contains("dies; the estate")
                || e.label.contains("needs care for")
                || (e.label.starts_with("Person") && e.label.contains("cares for person"))
        })
        .map(|e| {
            // What happens, without what the household does about it (whether the
            // caregiver was in a job then depends on its decisions).
            let what = e.label.split([':', ';', ',']).next().unwrap();
            (e.month - START, what.to_string())
        })
        .collect()
}

#[test]
fn the_features_draw_only_on_their_own_streams() {
    // Every feature on, without the scheduled divorce event (its probability is
    // drawn on the shared events stream, like any event's).
    let mut on = household_dynamics();
    on.events
        .retain(|e| !matches!(e.kind, EventKind::Divorce(_)));
    let base = example("dual-income-family");
    let a = traces(&base, 61, 40);
    let b = traces(&on, 61, 40);
    let (mut divorces, mut inheritances) = (0, 0);
    for (x, y) in a.iter().zip(&b) {
        // Switching the features on leaves the market, inflation and the people's
        // lifetimes as they were.
        for key in ["price_level", "return_us_stocks"] {
            let (cx, cy) = (column(x, key), column(y, key));
            let n = cx.len().min(cy.len());
            assert_eq!(cx[..n], cy[..n], "path {} {key}", x.path_index);
        }
        let died = |t: &PathTrace| {
            t.events
                .iter()
                .filter(|e| e.kind == TraceEventKind::Died)
                .map(|e| (e.month, e.label.clone()))
                .collect::<Vec<_>>()
        };
        // The partner's death is not reported after a divorce (they have left);
        // the first person's is the same whenever it is in both.
        let (dx, dy) = (died(x), died(y));
        for d in &dy {
            assert!(dx.contains(d), "path {} {d:?}", x.path_index);
        }
        let ev = family_events(y);
        divorces += ev.iter().filter(|e| e.1.starts_with("Divorce")).count();
        inheritances += ev.iter().filter(|e| e.1.starts_with("Inheritance")).count();
    }
    assert!(
        divorces > 5 && inheritances > 20,
        "{divorces} {inheritances}"
    );
    // A household decision (how withdrawals are ordered, how much is saved) does
    // not move a divorce, a parent's death or a care spell.
    let mut other = on.clone();
    other.retirement.withdrawal_order = WithdrawalOrder::Proportional;
    other.retirement.bequest_target = 500_000.0;
    let c = traces(&other, 61, 40);
    let mut differ = 0;
    for (y, z) in b.iter().zip(&c) {
        let (ey, ez) = (family_events(y), family_events(z));
        // Events can only be cut off by a path that ends sooner (a depletion does
        // not end a path, so the lists are equal unless the paths differ in length).
        if y.months == z.months {
            assert_eq!(ey, ez, "path {}", y.path_index);
        }
        if y.events.len() != z.events.len() {
            differ += 1;
        }
    }
    // The decision did change something on some paths, so the check is not empty.
    assert!(differ > 0);
}
