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
    // employer cover for most of the kids' years to age 26 (24 years for k2). The
    // two adults already pay a single tier each, so the family tier adds
    // (7,471 - 2 x 1,765) / 12 = $328 a month: about $95k over 24 years, plus
    // expected out-of-pocket of $400 then $800 a year per child (about $27k), all
    // grown by 1.5% a year of excess medical inflation (about 1.2 times on average):
    // about $145k. Person a retires at 60, when k2 is 26, so there are no
    // marketplace years.
    let delta = h_on - h_off;
    assert!(
        delta > 115_000.0 && delta < 180_000.0,
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
    // The default is not written, so older profiles serialise as they always did.
    assert!(json["household"]["children"][0].get("arrival").is_none());
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

// ---------------------------------------------------------------------------
// F08: college cost uncertainty
// ---------------------------------------------------------------------------

/// One child aged 10 with a public in-state plan, paid in full by the parents.
fn college_household(cost: CollegeCost, college: College) -> Profile {
    let mut p = family();
    p.household.children = vec![Child {
        id: "kid".into(),
        birth: START - 10 * 12,
        college,
        college_cost: cost,
        ..Child::default()
    }];
    p
}

fn stochastic(attend: f64, growth: f64, vol: f64, aid: f64) -> CollegeCost {
    CollegeCost::Stochastic {
        attend_probability: attend,
        real_tuition_growth: growth,
        tuition_volatility: vol,
        aid_scale: aid,
    }
}

fn child_spending(p: &Profile, paths: u64) -> f64 {
    trace_total(&compile(p), 31, paths, "spend_children")
}

#[test]
fn college_cost_defaults_to_fixed() {
    let p = college_household(CollegeCost::Fixed, College::PublicInState);
    let json = serde_json::to_value(&p).unwrap();
    assert!(json["household"]["children"][0]
        .get("college_cost")
        .is_none());
    // An empty stochastic object takes the documented defaults.
    let c: CollegeCost = serde_json::from_str(r#"{"type": "stochastic"}"#).unwrap();
    assert_eq!(c, stochastic(0.95, 0.01, 0.012, 1.0));
}

#[test]
fn stochastic_duration_raises_cost_by_the_mean_extra_years() {
    // No aid, certain attendance, no price volatility: only the 4, 5 or 6 years
    // differ. Mean years are 0.68 * 4 + 0.25 * 5 + 0.07 * 6 = 4.39, so the college
    // bill is 4.39 / 4 = 1.0975 times the fixed one (a little more: the later years
    // are dearer in real terms).
    let paths = 300;
    let none = child_spending(&college_household(CollegeCost::Fixed, College::None), paths);
    let fixed = child_spending(
        &college_household(CollegeCost::Fixed, College::PublicInState),
        paths,
    );
    let stoch = child_spending(
        &college_household(stochastic(1.0, 0.01, 0.0, 0.0), College::PublicInState),
        paths,
    );
    let ratio = (stoch - none) / (fixed - none);
    assert!(fixed > none + 50_000.0, "college must cost something");
    assert!(
        ratio > 1.06 && ratio < 1.14,
        "ratio {ratio} (expected 1.10)"
    );
}

#[test]
fn not_attending_removes_the_college_bill_and_aid_lowers_it() {
    let paths = 300;
    let none = child_spending(&college_household(CollegeCost::Fixed, College::None), paths);
    let never = child_spending(
        &college_household(stochastic(0.0, 0.01, 0.012, 1.0), College::PublicInState),
        paths,
    );
    assert!((never - none).abs() < 1.0, "{never} vs {none}");
    let no_aid = child_spending(
        &college_household(stochastic(1.0, 0.01, 0.0, 0.0), College::PublicInState),
        paths,
    ) - none;
    let aid = child_spending(
        &college_household(stochastic(1.0, 0.01, 0.0, 1.0), College::PublicInState),
        paths,
    ) - none;
    // Household income is about $210,000, above the $200,000 at which the schedule
    // is flat at 0.21, so the bill falls by about 21% (the child's own aid shock has
    // mean zero; income falls on retirement, which comes after college here).
    let cut = 1.0 - aid / no_aid;
    assert!(
        cut > 0.17 && cut < 0.25,
        "aid cut the bill by {cut} (expected 0.21)"
    );
}

#[test]
fn assets_reduce_aid_as_the_federal_formula_counts_them() {
    // One earner on $60,000 with modest savings: the schedule gives 0.57 at $50,000
    // and 0.31 at $125,000, so about 0.5 at $60,000 plus 12% of a few tens of
    // thousands of cash and taxable savings. With $1,500,000 more in a taxable
    // account the assessed income is at least $60,000 + 0.12 x $1,500,000 =
    // $240,000, above the $200,000 at which the schedule is flat at 0.21.
    let household = |extra_taxable: f64, cost: CollegeCost| {
        let mut p = college_household(cost, College::PublicInState);
        p.incomes = vec![job("job_a", 0, 60_000.0)];
        p.spending.items = vec![item("living", Category::Other, 2_000.0, true)];
        p.accounts = vec![
            account("cash", 0, AccountKind::Cash, 20_000.0),
            account("k401", 0, AccountKind::Traditional, 200_000.0),
            account(
                "brokerage",
                0,
                AccountKind::Taxable,
                20_000.0 + extra_taxable,
            ),
        ];
        p
    };
    let paths = 200;
    let cut = |extra: f64| {
        let mut no_college = household(extra, CollegeCost::Fixed);
        no_college.household.children[0].college = College::None;
        let none = child_spending(&no_college, paths);
        let bill = |aid_scale: f64| {
            child_spending(
                &household(extra, stochastic(1.0, 0.01, 0.0, aid_scale)),
                paths,
            ) - none
        };
        1.0 - bill(1.0) / bill(0.0)
    };
    let (poor, rich) = (cut(0.0), cut(1_500_000.0));
    assert!(poor > 0.38 && poor < 0.54, "little savings: cut {poor}");
    assert!(rich > 0.19 && rich < 0.23, "large portfolio: cut {rich}");
}

#[test]
fn tuition_volatility_widens_the_spread_of_costs() {
    let sd = |vol: f64| {
        let plan = compile(&college_household(
            stochastic(1.0, 0.01, vol, 0.0),
            College::PublicInState,
        ));
        let acc = run(&plan, 41, 0..600);
        let v: Vec<f64> = acc
            .scalar_column(firesight_core::stats::PathScalar::LifetimeSpending)
            .iter()
            .map(|x| f64::from(*x))
            .collect();
        let m = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / v.len() as f64).sqrt()
    };
    let (calm, wild) = (sd(0.0), sd(0.03));
    // Same durations and the same markets in both; only the price path differs.
    assert!(wild > calm * 1.01, "spread {wild} vs {calm}");
}

#[test]
fn stochastic_college_conserves_money_and_shows_events() {
    let mut p = college_household(stochastic(0.9, 0.02, 0.02, 1.0), College::Private);
    p.household.children.push(Child {
        id: "sib".into(),
        birth: START - 6 * 12,
        college: College::PublicOutOfState,
        college_cost: stochastic(0.9, 0.01, 0.015, 0.5),
        ..Child::default()
    });
    for kind in MARKETS {
        let plan = compile(&with_market(p.clone(), kind));
        let acc = run_checked(&plan, 2, 0..100);
        for s in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(s.is_finite());
        }
    }
    let plan = compile(&p);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut seen = 0;
    for i in 0..10 {
        let t = sim::trace_path_in(&plan, 2, i, &mut ws);
        seen += t
            .events
            .iter()
            .filter(|e| e.label.contains("college"))
            .count();
    }
    assert!(
        seen >= 10,
        "{seen} college events in 10 paths of two children"
    );
}

// ---------------------------------------------------------------------------
// F09: adult children who need support
// ---------------------------------------------------------------------------

fn support_household(support: Option<AdultSupport>) -> Profile {
    let mut p = family();
    p.household.children = vec![Child {
        id: "kid".into(),
        birth: START - 10 * 12,
        adult_support: support,
        ..Child::default()
    }];
    p
}

#[test]
fn adult_support_is_off_by_default_and_has_documented_defaults() {
    assert!(Child::default().adult_support.is_none());
    let s: AdultSupport = serde_json::from_str("{}").unwrap();
    assert_eq!(
        s,
        AdultSupport {
            probability: 0.3,
            annual: 12_000.0,
            mean_years: 2.0
        }
    );
}

#[test]
fn adult_support_costs_probability_times_annual_times_years() {
    let paths = 600;
    let base = child_spending(&support_household(None), paths);
    // Never needed: the same, to the dollar.
    let never = child_spending(
        &support_household(Some(AdultSupport {
            probability: 0.0,
            ..AdultSupport::default()
        })),
        paths,
    );
    assert_eq!(base, never);
    // 0.5 * $12,000 a year * 2 years on average = $12,000 per path (the spell is
    // exponential with mean 24 months, rounded to whole months, at least one).
    let some = child_spending(
        &support_household(Some(AdultSupport {
            probability: 0.5,
            annual: 12_000.0,
            mean_years: 2.0,
        })),
        paths,
    );
    let extra = some - base;
    assert!(
        extra > 9_500.0 && extra < 14_500.0,
        "extra {extra} (expected 12,000)"
    );
}

#[test]
fn adult_support_conserves_money_and_leaves_events() {
    let p = support_household(Some(AdultSupport {
        probability: 1.0,
        annual: 20_000.0,
        mean_years: 3.0,
    }));
    for kind in MARKETS {
        let plan = compile(&with_market(p.clone(), kind));
        let acc = run_checked(&plan, 3, 0..100);
        for s in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(s.is_finite());
        }
    }
    let plan = compile(&p);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let t = sim::trace_path_in(&plan, 3, 0, &mut ws);
    let started = t
        .events
        .iter()
        .find(|e| e.label.contains("needs support"))
        .expect("a support spell starts");
    // Child is 10 now: school ends at 22 (12 years), the spell starts within the
    // next six years.
    let months_in =
        |m: YearMonth| (m.year() - START.year()) * 12 + m.month() as i32 - START.month() as i32;
    let at = months_in(started.month);
    assert!((12 * 12..=18 * 12).contains(&at), "starts {at} months in");
    assert!(t
        .events
        .iter()
        .any(|e| e.label.contains("independent again")));
}

// ---------------------------------------------------------------------------
// F10: parental leave
// ---------------------------------------------------------------------------

fn leave_household(birth_months_in: i32, leave: Option<ParentalLeave>) -> Profile {
    let mut p = family();
    p.household.children = vec![Child {
        id: "baby".into(),
        birth: START + birth_months_in,
        parental_leave: leave,
        ..Child::default()
    }];
    for inc in &mut p.incomes {
        if let IncomeKind::Job(j) = &mut inc.kind {
            j.plan = Some(EmployerPlan::default());
        }
    }
    p
}

fn leave(person: usize, months: u32, pay_fraction: f64) -> Option<ParentalLeave> {
    Some(ParentalLeave {
        person,
        months,
        pay_fraction,
    })
}

fn wages(p: &Profile) -> f64 {
    trace_total(&compile(p), 5, 3, "income_wages")
}

fn employer_match(p: &Profile) -> f64 {
    trace_total(&compile(p), 5, 3, "save_employer")
}

#[test]
fn parental_leave_is_off_by_default_and_has_documented_defaults() {
    assert!(Child::default().parental_leave.is_none());
    let l: ParentalLeave = serde_json::from_str("{}").unwrap();
    assert_eq!(
        l,
        ParentalLeave {
            person: 0,
            months: 3,
            pay_fraction: 0.6
        }
    );
}

#[test]
fn parental_leave_cuts_wages_by_months_times_unpaid_share() {
    // Person 0 earns 130,000 a year (10,833 a month, nominal slightly more after
    // inflation). Unpaid, three months lose about 32,500; at 60% pay the loss is 40%
    // of that, exactly in proportion since the job has no risk.
    let base = wages(&leave_household(12, None));
    let unpaid = wages(&leave_household(12, leave(0, 3, 0.0)));
    let part = wages(&leave_household(12, leave(0, 3, 0.6)));
    let full = wages(&leave_household(12, leave(0, 3, 1.0)));
    let loss = base - unpaid;
    assert!(
        loss > 31_000.0 && loss < 38_000.0,
        "loss {loss} (expected 32,500 plus inflation)"
    );
    assert!(
        ((base - part) / loss - 0.4).abs() < 1e-9,
        "{}",
        (base - part) / loss
    );
    assert_eq!(full, base);
    // The second parent's leave takes their (80,000) pay.
    let second = base - wages(&leave_household(12, leave(1, 3, 0.0)));
    assert!(second > 19_000.0 && second < 24_000.0, "second {second}");
}

#[test]
fn parental_leave_lowers_the_match_and_ignores_born_children() {
    // The employer match is 50% of the first 6% of pay: 3% of the 32,500 of pay lost
    // in three unpaid months is 975 (a little more in nominal dollars). The employee
    // deferral also follows pay, but the planner redirects other savings to offset
    // it, so the match is the clean measure.
    let base = employer_match(&leave_household(12, None));
    let unpaid = employer_match(&leave_household(12, leave(0, 3, 0.0)));
    let drop = base - unpaid;
    assert!(drop > 900.0 && drop < 1_200.0, "match fell by {drop}");
    // A child already born has no leave to take.
    let born = leave_household(-12, leave(0, 3, 0.0));
    assert_eq!(wages(&born), wages(&leave_household(-12, None)));
}

#[test]
fn parental_leave_follows_realised_births_and_twins_take_one_leave() {
    let mut p = leave_household(12, leave(0, 3, 0.0));
    // A child who never arrives: no leave.
    p.household.children[0].arrival = Arrival::Uncertain {
        probability: 0.0,
        delay_mean_months: 4.0,
        twins_probability: 0.0,
    };
    let mut never = p.clone();
    never.household.children[0].parental_leave = None;
    assert_eq!(wages(&p), wages(&never));
    // Twins are one leave, not two.
    let mut twins = leave_household(12, leave(0, 3, 0.0));
    twins.household.children[0].arrival = Arrival::Uncertain {
        probability: 1.0,
        delay_mean_months: 0.0,
        twins_probability: 1.0,
    };
    let single = wages(&leave_household(12, leave(0, 3, 0.0)));
    assert_eq!(wages(&twins), single);
}

#[test]
fn both_parents_can_take_leave_at_the_same_birth() {
    // Each parent's unpaid months come off their own pay: the loss with both leaves
    // is the sum of the two separate losses (flat wages, no job risk), about
    // 3 x 130,000 / 12 + 2 x 80,000 / 12 = 32,500 + 13,333 before inflation.
    let base = wages(&leave_household(12, None));
    let first = base - wages(&leave_household(12, leave(0, 3, 0.0)));
    let mut second_only = leave_household(12, None);
    second_only.household.children[0].second_parental_leave = leave(1, 2, 0.0);
    let second = base - wages(&second_only);
    assert!(second > 13_000.0 && second < 15_500.0, "second {second}");
    let mut both = leave_household(12, leave(0, 3, 0.0));
    both.household.children[0].second_parental_leave = leave(1, 2, 0.0);
    let loss = base - wages(&both);
    assert!(
        (loss - (first + second)).abs() < 1e-6,
        "{loss} vs {first} + {second}"
    );
    assert!(firesight_core::validate::validate(&both).errors.is_empty());
    // The trace names each parent.
    let plan = compile(&both);
    let t = sim::trace_path_in(&plan, 3, 0, &mut Workspace::new(&plan));
    let leaves: Vec<&str> = t
        .events
        .iter()
        .filter(|e| e.label.contains("Parental leave"))
        .map(|e| e.label.as_str())
        .collect();
    assert_eq!(leaves.len(), 2, "{leaves:?}");
    assert!(leaves[0].contains("person 1 (3 months)"), "{leaves:?}");
    assert!(leaves[1].contains("person 2 (2 months)"), "{leaves:?}");
    // The same person twice is an error: the second leave would be ignored.
    both.household.children[0].second_parental_leave = leave(0, 2, 0.0);
    let r = firesight_core::validate::validate(&both);
    assert!(r.errors.iter().any(|e| e.code == "leave_same_person"));
}

#[test]
fn adult_support_for_a_child_past_the_window_warns() {
    let mut p = support_household(Some(AdultSupport {
        probability: 1.0,
        ..AdultSupport::default()
    }));
    let r = firesight_core::validate::validate(&p);
    assert!(!r.warnings.iter().any(|w| w.code == "support_window_passed"));
    // A ten-year-old with certain support: 12,000 x 2 years on average.
    let mut none = p.clone();
    none.household.children[0].adult_support = None;
    let fresh = child_spending(&p, 300) - child_spending(&none, 300);
    assert!(fresh > 20_000.0 && fresh < 28_000.0, "fresh {fresh}");
    // A 31-year-old left school nine years ago: the spell started three to nine years
    // ago, and only what is left of it is charged. With an exponential length of mean
    // 24 months that is 24 x mean(exp(-s / 24)) for s from 36 to 108 months,
    // 8 x (exp(-1.5) - exp(-4.5)) = 1.7 months: about $1,700.
    p.household.children[0].birth = START - 31 * 12;
    none.household.children[0].birth = START - 31 * 12;
    let r = firesight_core::validate::validate(&p);
    assert!(r.warnings.iter().any(|w| w.code == "support_window_passed"));
    let late = child_spending(&p, 300) - child_spending(&none, 300);
    assert!((0.0..4_000.0).contains(&late), "late {late}");
}

#[test]
fn parental_leave_conserves_money_and_leaves_an_event() {
    let p = leave_household(12, leave(0, 6, 0.5));
    for kind in MARKETS {
        let plan = compile(&with_market(p.clone(), kind));
        let acc = run_checked(&plan, 3, 0..60);
        for s in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(s.is_finite());
        }
    }
    let plan = compile(&p);
    let mut ws = Workspace::new(&plan);
    let t = sim::trace_path_in(&plan, 3, 0, &mut ws);
    assert!(t.events.iter().any(|e| e.label.contains("Parental leave")));
}

#[test]
fn parental_leave_validation() {
    let mut p = leave_household(12, leave(5, 3, 0.6));
    let r = firesight_core::validate::validate(&p);
    assert!(r.errors.iter().any(|e| e.code == "leave_person"));
    p.household.children[0].parental_leave = leave(0, 3, 1.5);
    let r = firesight_core::validate::validate(&p);
    assert!(!r.errors.is_empty());
    p.household.children[0].parental_leave = leave(0, 18, 0.6);
    let r = firesight_core::validate::validate(&p);
    assert!(r.warnings.iter().any(|w| w.code == "leave_long"));
}

// ---------------------------------------------------------------------------
// Edges, the features together, and stream discipline
// ---------------------------------------------------------------------------

#[test]
fn a_couple_with_children_pays_the_family_tier_not_a_single_tier_on_top() {
    // Month by month while both adults are on the employer plan, the household's
    // extra health spending for the children is the family tier less the two single
    // tiers the adults already pay, plus the children's out-of-pocket means:
    // (7,471 - 2 x 1,765 + 2 x 400) / 12 = $395 a month in today's dollars before
    // excess medical inflation (1.5% a year: about 2% over the first 30 months).
    let off = family();
    let mut on = family();
    on.spending.healthcare.cover_children = true;
    let month = |p: &Profile| {
        let plan = compile(p);
        let mut ws = Workspace::new(&plan);
        let t = sim::trace_path_in(&plan, 9, 0, &mut ws);
        t.column("spend_healthcare").unwrap()[12..24]
            .iter()
            .sum::<f64>()
            / 12.0
    };
    let extra = month(&on) - month(&off);
    assert!(extra > 395.0 && extra < 420.0, "extra {extra} a month");
}

#[test]
fn a_custom_early_retirement_amount_already_covers_the_children() {
    // The household's own figure for early-retirement cover is the whole household's:
    // switching children's cover on adds nothing while the adult is on it.
    let mut p = quiet(vec![person("a", 45, Sex::Female)], 70.0);
    p.spending.healthcare = Healthcare {
        mode: HealthcareMode::Modeled,
        early_retirement: EarlyRetirementCoverage::Custom { annual: 9_000.0 },
        long_term_care: false,
        ..Healthcare::default()
    };
    p.household.children = vec![Child {
        id: "k".into(),
        birth: START - 5 * 12,
        costs: ChildCosts::InBudget,
        ..Child::default()
    }];
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 2_000_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    let mut on = p.clone();
    on.spending.healthcare.cover_children = true;
    let health = |p: &Profile| {
        let plan = compile(p);
        let mut ws = Workspace::new(&plan);
        let t = sim::trace_path_in(&plan, 1, 0, &mut ws);
        t.column("spend_healthcare").unwrap()[..120].to_vec()
    };
    let (a, b) = (health(&p), health(&on));
    assert!(
        a.iter().sum::<f64>() > 50_000.0,
        "the custom amount is paid"
    );
    assert_eq!(a, b);
}

/// The shipped two-earner family with every family feature switched on (`leave`:
/// with parental leave too). Its second child is planned for 14 months in.
fn everything_on(leave: bool) -> Profile {
    let mut p = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .expect("the dual-income example")
        .profile;
    p.spending.healthcare.cover_children = true;
    for (i, c) in p.household.children.iter_mut().enumerate() {
        c.college_cost = stochastic(0.95, 0.01, 0.012, 1.0);
        c.adult_support = Some(AdultSupport::default());
        if i == 1 {
            c.arrival = Arrival::Uncertain {
                probability: 0.85,
                delay_mean_months: 4.0,
                twins_probability: 0.2,
            };
            if leave {
                c.parental_leave = Some(ParentalLeave::default());
                c.second_parental_leave = Some(ParentalLeave {
                    person: 1,
                    months: 1,
                    pay_fraction: 1.0,
                });
            }
        }
    }
    p
}

fn fixed_retirement(mut p: Profile) -> Profile {
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    p
}

#[test]
fn the_features_together_conserve_money_and_stay_finite() {
    for kind in MARKETS {
        let plan = compile(&with_market(everything_on(true), kind));
        let acc = run_checked(&plan, 12, 0..150);
        for &s in firesight_core::stats::PathScalar::ALL {
            for v in acc.scalar_column(s) {
                assert!(!v.is_infinite(), "{kind:?} {s:?}");
            }
        }
        for v in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
            assert!(v.is_finite(), "{kind:?}");
        }
    }
    // A single parent, already retired, with a planned child and everything on.
    let mut p = quiet(vec![person("a", 40, Sex::Female)], 85.0);
    p.spending.healthcare = Healthcare {
        mode: HealthcareMode::Modeled,
        cover_children: true,
        ..Healthcare::default()
    };
    p.household.children = vec![Child {
        id: "k".into(),
        birth: START + 10,
        college: College::Private,
        arrival: Arrival::Uncertain {
            probability: 0.8,
            delay_mean_months: 6.0,
            twins_probability: 0.5,
        },
        college_cost: stochastic(0.9, 0.01, 0.02, 1.0),
        adult_support: Some(AdultSupport::default()),
        parental_leave: Some(ParentalLeave::default()),
        ..Child::default()
    }];
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 3_000_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    let plan = compile(&p);
    let acc = run_checked(&plan, 13, 0..150);
    for v in acc.scalar_column(firesight_core::stats::PathScalar::LifetimeSpending) {
        assert!(v.is_finite());
    }
}

#[test]
fn family_features_leave_markets_careers_health_and_lifespans_untouched() {
    use firesight_core::stats::{PathScalar, PathTrace, TraceEventKind};
    let base = fixed_retirement(
        Profile::examples(START)
            .into_iter()
            .find(|e| e.slug == "dual-income-family")
            .unwrap()
            .profile,
    );
    let scalar = |t: &PathTrace, s: PathScalar| {
        t.scalars
            .iter()
            .find(|x| x.scalar == s)
            .unwrap()
            .value
            .to_bits()
    };
    let events = |t: &PathTrace, kinds: &[TraceEventKind]| -> Vec<(YearMonth, TraceEventKind)> {
        t.events
            .iter()
            .filter(|e| kinds.contains(&e.kind))
            .map(|e| (e.month, e.kind))
            .collect()
    };
    let plan_base = compile(&base);
    for leave in [false, true] {
        let plan_on = compile(&fixed_retirement(everything_on(leave)));
        let mut moved = false;
        for path in 0..25 {
            let a = sim::trace_path(&plan_base, 77, path);
            let b = sim::trace_path(&plan_on, 77, path);
            assert_eq!(a.months, b.months, "path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::Inflation,
            ] {
                assert_eq!(scalar(&a, s), scalar(&b, s), "path {path} {s:?}");
            }
            for key in [
                "price_level",
                "return_us_stocks",
                "return_intl_stocks",
                "return_bonds",
                "return_cash",
            ] {
                assert_eq!(a.column(key), b.column(key), "path {path} {key}");
            }
            // Health draws: long-term care begins in the same months.
            let care = [TraceEventKind::LongTermCare];
            assert_eq!(events(&a, &care), events(&b, &care), "path {path}");
            if !leave {
                // Careers: the same layoffs in the same months. (A job cannot be lost
                // during a leave, so with leave on a layoff may legitimately go.)
                let career = [TraceEventKind::LaidOff, TraceEventKind::Rehired];
                assert_eq!(events(&a, &career), events(&b, &career), "path {path}");
                assert_eq!(
                    scalar(&a, PathScalar::MonthsUnemployed),
                    scalar(&b, PathScalar::MonthsUnemployed)
                );
            }
            moved |= a.column("spend_children") != b.column("spend_children");
        }
        assert!(moved, "the features must change something");
    }
}

#[test]
fn an_unrelated_decision_leaves_the_family_draws_untouched() {
    // Spending less changes income, taxes and aid, but not which children arrive
    // when, whether they attend and for how long, or when they need support.
    let base = fixed_retirement(everything_on(true));
    let mut frugal = base.clone();
    for item in &mut frugal.spending.items {
        item.amount *= 0.7;
    }
    frugal.spending.shocks = ShockLevel::Off;
    let family_events = |t: &firesight_core::stats::PathTrace| -> Vec<(YearMonth, String)> {
        t.events
            .iter()
            .filter(|e| {
                [
                    "born",
                    "does not arrive",
                    "college",
                    "needs support",
                    "independent again",
                ]
                .iter()
                .any(|k| e.label.contains(k))
            })
            // The aid percentage depends on income; the draws do not.
            .map(|e| (e.month, e.label.split(", aid").next().unwrap().to_string()))
            .collect()
    };
    let (plan_a, plan_b) = (compile(&base), compile(&frugal));
    let (mut seen, mut differences) = (0, 0);
    for path in 0..25 {
        let a = sim::trace_path(&plan_a, 77, path);
        let b = sim::trace_path(&plan_b, 77, path);
        let (ea, eb) = (family_events(&a), family_events(&b));
        assert_eq!(ea, eb, "path {path}");
        seen += ea.len();
        if a.column("spend_essential") != b.column("spend_essential") {
            differences += 1;
        }
    }
    // Two children: a birth or a non-arrival, and a college start or a miss, each.
    assert!(seen >= 25 * 3, "{seen} family events on 25 paths");
    assert!(differences > 20, "the decision must change the paths");
}

#[test]
fn a_planned_child_who_never_arrives_does_not_hold_back_a_funded_retirement() {
    // The shipped household retires when funded. Once the planned month has passed
    // without a birth, the funded test stops setting money aside for that child:
    // from then on the household is the one that never planned a second child.
    let example = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .unwrap()
        .profile;
    let mut one = example.clone();
    one.household.children.truncate(1);
    let mut never = example.clone();
    never.household.children[1].arrival = Arrival::Uncertain {
        probability: 0.0,
        delay_mean_months: 4.0,
        twins_probability: 0.0,
    };
    let (po, pn, pe) = (compile(&one), compile(&never), compile(&example));
    let (ao, an, ae) = (
        run(&po, 5, 0..150),
        run(&pn, 5, 0..150),
        run(&pe, 5, 0..150),
    );
    use firesight_core::stats::PathScalar::*;
    for s in [RetirementAge, LifetimeSpending, LifetimeTax] {
        assert_eq!(ao.scalar_column(s), an.scalar_column(s), "{s:?}");
    }
    // And the test can fail: the child who does arrive moves the retirement date.
    assert_ne!(
        ao.scalar_column(RetirementAge),
        ae.scalar_column(RetirementAge)
    );
}

#[test]
fn stochastic_college_with_a_reduced_share_warns_about_counting_aid_twice() {
    let mut p = college_household(stochastic(0.95, 0.01, 0.012, 1.0), College::PublicInState);
    p.household.children[0].college_share = 0.75;
    let r = firesight_core::validate::validate(&p);
    assert!(r.warnings.iter().any(|w| w.code == "college_aid_twice"));
    p.household.children[0].college_cost = stochastic(0.95, 0.01, 0.012, 0.0);
    let r = firesight_core::validate::validate(&p);
    assert!(!r.warnings.iter().any(|w| w.code == "college_aid_twice"));
}
