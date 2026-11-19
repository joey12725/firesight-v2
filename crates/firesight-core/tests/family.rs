//! The family features (children on the health plan, uncertain births, college cost
//! uncertainty, adult children who need support, parental leave): each is off by
//! default, and switched on moves outcomes in the expected direction by a plausible
//! amount on the same random numbers.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};

/// Two earners in their mid thirties with two young children, modelled healthcare
/// and US taxes; no shocks, fixed lifespan to 90, flat wages.
pub fn family() -> Profile {
    let mut a = person("a", 36, Sex::Female);
    let mut b = person("b", 34, Sex::Male);
    a.social_security.basis = SocialSecurityBasis::FromEarnings {
        career_start_age: 22.0,
    };
    b.social_security.basis = SocialSecurityBasis::FromEarnings {
        career_start_age: 22.0,
    };
    let mut p = quiet(vec![a, b], 90.0);
    p.name = "family".into();
    p.household.location.state = Some("TX".into());
    p.household.children = vec![
        Child {
            id: "k1".into(),
            birth: START - 5 * 12,
            ..Child::default()
        },
        Child {
            id: "k2".into(),
            birth: START - 2 * 12,
            ..Child::default()
        },
    ];
    p.incomes = vec![job("job_a", 0, 130_000.0), job("job_b", 1, 80_000.0)];
    p.spending.healthcare = Healthcare {
        mode: HealthcareMode::Modeled,
        long_term_care: false,
        ..Healthcare::default()
    };
    p.spending.items = vec![
        item("living", Category::Other, 5_000.0, true),
        item("fun", Category::Entertainment, 1_000.0, false),
    ];
    p.assumptions.taxes = TaxModel::default();
    p.accounts = vec![
        account("cash", 0, AccountKind::Cash, 30_000.0),
        account("k401", 0, AccountKind::Traditional, 200_000.0),
        account("brokerage", 0, AccountKind::Taxable, 100_000.0),
    ];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 60.0,
    };
    p
}

/// Sum over `paths` traced paths of an annual column (today's dollars), by key.
pub fn trace_total(plan: &sim::Plan, seed: u64, paths: u64, key: &str) -> f64 {
    let mut ws = Workspace::new(plan);
    let mut total = 0.0;
    for i in 0..paths {
        let trace = sim::trace_path_in(plan, seed, i, &mut ws);
        if let Some(c) = trace.annual.columns.iter().find(|c| c.key == key) {
            total += c.values.iter().sum::<f64>();
        }
    }
    total / paths as f64
}

// ---------------------------------------------------------------------------
// F06: children on the health plan
// ---------------------------------------------------------------------------

#[test]
fn children_health_cover_is_off_by_default_and_json_round_trips() {
    let p = family();
    assert!(!p.spending.healthcare.cover_children);
    assert_eq!(p.spending.healthcare.children_coverage_end_age, 26.0);
    // A profile written before the fields existed reads as off.
    let mut v = serde_json::to_value(&p).unwrap();
    let hc = v["spending"]["healthcare"].as_object_mut().unwrap();
    hc.remove("cover_children");
    hc.remove("children_coverage_end_age");
    let q: Profile = serde_json::from_value(v).unwrap();
    assert_eq!(p, q);
    let (pa, qa) = (compile(&p), compile(&q));
    let da = pa.summarize(&run(&pa, 3, 0..200)).digest();
    let db = qa.summarize(&run(&qa, 3, 0..200)).digest();
    assert_eq!(
        serde_json::to_string(&da).unwrap(),
        serde_json::to_string(&db).unwrap()
    );
}

#[test]
fn covering_children_raises_health_spending_by_a_plausible_amount() {
    let off = family();
    let mut on = family();
    on.spending.healthcare.cover_children = true;
    let (poff, pon) = (compile(&off), compile(&on));
    let paths = 60;
    let h_off = trace_total(&poff, 9, paths, "spend_healthcare");
    let h_on = trace_total(&pon, 9, paths, "spend_healthcare");
    let k_off = trace_total(&poff, 9, paths, "spend_children");
    let k_on = trace_total(&pon, 9, paths, "spend_children");
    // Hand calculation of the gross health increase, today's dollars (the USDA
    // health share removed from child costs is checked below). A parent has
    // employer cover for most of the kids' years to age 26 (24 years for k2), so
    // the family tier adds $475 a month: about $137k over 24 years, plus expected
    // out-of-pocket of $400 then $800 a year per child (about $27k), all grown by
    // 1.5% a year of excess medical inflation (about 1.2 times on average), and
    // ACA child premiums in any years after the parents retire. About $180k.
    let delta = h_on - h_off;
    assert!(
        delta > 100_000.0 && delta < 260_000.0,
        "health delta {delta}"
    );
    // The USDA health share comes out of the child line, so children spending falls.
    assert!(k_on < k_off, "children {k_on} vs {k_off}");
    assert!(
        k_off - k_on > 50_000.0 && k_off - k_on < 80_000.0,
        "removed USDA health share {}",
        k_off - k_on
    );
}

#[test]
fn aca_children_follow_the_three_child_rule_and_the_credit() {
    // Early retiree on the marketplace with four children under 21: only the three
    // oldest are charged a premium.
    let mut p = quiet(vec![person("a", 45, Sex::Female)], 70.0);
    p.spending.healthcare = Healthcare {
        mode: HealthcareMode::Modeled,
        long_term_care: false,
        cover_children: true,
        ..Healthcare::default()
    };
    p.household.children = (0..4)
        .map(|i| Child {
            id: format!("k{i}"),
            birth: START - (3 + 3 * i) * 12,
            costs: ChildCosts::InBudget,
            ..Child::default()
        })
        .collect();
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 2_000_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    let plan = compile(&p);
    let mut four = p.clone();
    four.household.children.pop();
    let plan3 = compile(&four);
    // The fourth child is the youngest (3 years old). Until the oldest turns 21, nine
    // years on, the rule leaves it with only its out-of-pocket cost; a full premium
    // would be $330 a month (0.765 of the age-21 benchmark, before the credit).
    // Compare the first 60 months: out-of-pocket is $400 a year (about $2,100 with
    // medical inflation), a premium would add about $20,000.
    let first5 = |plan: &sim::Plan| {
        let mut ws = Workspace::new(plan);
        let t = sim::trace_path_in(plan, 1, 0, &mut ws);
        t.column("spend_healthcare").unwrap()[..60]
            .iter()
            .sum::<f64>()
    };
    let extra = first5(&plan) - first5(&plan3);
    assert!(extra > 1_000.0 && extra < 6_000.0, "extra {extra}");
}

#[test]
fn children_health_cover_conserves_money_and_is_finite() {
    let mut p = family();
    p.spending.healthcare.cover_children = true;
    // Retire early so the children spend years on the marketplace plan.
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 45.0,
    };
    for kind in MARKETS {
        let plan = compile(&with_market(p.clone(), kind));
        let acc = run_checked(&plan, 4, 0..120);
        for s in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(s.is_finite(), "{kind:?}");
        }
    }
    let plan = compile(&p);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let trace = sim::trace_path_in(&plan, 4, 0, &mut ws);
    assert!(
        trace
            .events
            .iter()
            .any(|e| e.kind.key() == "child" && e.label.contains("health plan")),
        "{:?}",
        trace.events
    );
}

// ---------------------------------------------------------------------------
// F07: uncertain arrival of planned children
// ---------------------------------------------------------------------------

/// One planned child, no others.
fn planned_child_household(arrival: Arrival) -> Profile {
    let mut p = family();
    p.household.children = vec![Child {
        id: "planned".into(),
        name: "Planned".into(),
        birth: START + 12,
        arrival,
        ..Child::default()
    }];
    p
}

fn mean_lifetime_spending(plan: &sim::Plan, paths: u64) -> f64 {
    let acc = run(plan, 21, 0..paths);
    let col = acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending);
    col.iter().map(|v| f64::from(*v)).sum::<f64>() / col.len() as f64
}

#[test]
fn certain_is_the_default_and_probability_one_on_time_is_the_same() {
    let certain = planned_child_household(Arrival::Certain);
    let json = serde_json::to_value(&certain).unwrap();
    assert!(json["household"]["children"][0]["arrival"]["type"] == "certain");
    let on_time = planned_child_household(Arrival::Uncertain {
        probability: 1.0,
        delay_mean_months: 0.0,
        twins_probability: 0.0,
    });
    let (pc, pu) = (compile(&certain), compile(&on_time));
    let (ac, au) = (run(&pc, 5, 0..200), run(&pu, 5, 0..200));
    let sc = serde_json::to_string(&pc.summarize(&ac).digest()).unwrap();
    let su = serde_json::to_string(&pu.summarize(&au).digest()).unwrap();
    assert_eq!(sc, su);
}

#[test]
fn a_child_who_never_arrives_costs_nothing_and_changes_nothing_else() {
    let none = {
        let mut p = family();
        p.household.children.clear();
        p
    };
    let never = planned_child_household(Arrival::Uncertain {
        probability: 0.0,
        delay_mean_months: 6.0,
        twins_probability: 0.5,
    });
    let (pn, pv) = (compile(&none), compile(&never));
    let (an, av) = (run(&pn, 5, 0..200), run(&pv, 5, 0..200));
    use firesight_core::stats::PathScalar::*;
    for s in [LifetimeSpending, LifetimeTax] {
        assert_eq!(an.scalar_column(s), av.scalar_column(s), "{s:?}");
    }
}

#[test]
fn uncertain_arrival_scales_child_costs_by_probability_and_twins() {
    let none = {
        let mut p = family();
        p.household.children.clear();
        p
    };
    let certain = planned_child_household(Arrival::Certain);
    let uncertain = planned_child_household(Arrival::Uncertain {
        probability: 0.6,
        delay_mean_months: 0.0,
        twins_probability: 0.1,
    });
    let (pn, pc, pu) = (compile(&none), compile(&certain), compile(&uncertain));
    let paths = 1500;
    let (sn, sc, su) = (
        mean_lifetime_spending(&pn, paths),
        mean_lifetime_spending(&pc, paths),
        mean_lifetime_spending(&pu, paths),
    );
    // The household has one child to 22, so the extra spending of the certain child
    // is sc - sn. With probability 0.6 and 10% twins (two children each cost 1.0 of
    // the USDA two-child rate against 1.27 for an only child: a twin birth costs
    // 2/1.27 = 1.57 as much), the expected extra is 0.6 * (0.9 + 0.1 * 1.57) = 0.634
    // of it.
    let ratio = (su - sn) / (sc - sn);
    assert!(sc > sn, "the child must cost something");
    assert!(
        ratio > 0.55 && ratio < 0.72,
        "ratio {ratio} (expected 0.63)"
    );
}

#[test]
fn delayed_births_leave_trace_events() {
    let p = planned_child_household(Arrival::Uncertain {
        probability: 1.0,
        delay_mean_months: 12.0,
        twins_probability: 0.5,
    });
    let plan = compile(&p);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let (mut late, mut twins) = (0, 0);
    for i in 0..30 {
        let trace = sim::trace_path_in(&plan, 8, i, &mut ws);
        let born = trace
            .events
            .iter()
            .find(|e| e.kind.key() == "child" && e.label.contains("born"))
            .unwrap_or_else(|| panic!("no birth event on path {i}: {:?}", trace.events));
        // The planned month is START + 12; delays are never negative.
        assert!(born.month >= START + 12, "{born:?}");
        if born.month > START + 12 {
            late += 1;
        }
        if born.label.contains("twins") {
            twins += 1;
        }
    }
    assert!(late >= 20, "{late} of 30 late with a 12-month mean delay");
    assert!(
        (5..=25).contains(&twins),
        "{twins} twin births of 30 at 50%"
    );
}

#[test]
fn uncertain_births_conserve_money_and_are_finite() {
    let mut p = planned_child_household(Arrival::Uncertain {
        probability: 0.7,
        delay_mean_months: 8.0,
        twins_probability: 0.3,
    });
    p.household.children.push(Child {
        id: "second".into(),
        birth: START + 40,
        arrival: Arrival::Uncertain {
            probability: 0.5,
            delay_mean_months: 12.0,
            twins_probability: 0.1,
        },
        college: College::PublicInState,
        ..Child::default()
    });
    for kind in MARKETS {
        let plan = compile(&with_market(p.clone(), kind));
        let acc = run_checked(&plan, 6, 0..120);
        for s in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(s.is_finite());
        }
    }
}
