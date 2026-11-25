//! Unit tests of the household-dynamics mechanisms, with hand-computed values.

use super::divorce::{annual_divorce_hazard, old_age_taper};
use super::*;
use crate::accounts::Accounts;
use crate::model::{DivorceRisk, DivorceTerms, Example, EXAMPLE_START};

fn dual_income() -> Profile {
    Profile::examples(EXAMPLE_START)
        .into_iter()
        .find(|e: &Example| e.slug == "dual-income-family")
        .unwrap()
        .profile
}

#[test]
fn the_divorce_hazard_reproduces_the_nchs_cumulative_rates() {
    // A marriage that begins at the start: survival to the end of 5, 10 and 15
    // years must be 1 - 20%, 1 - 33% and 1 - 43% (Bramlett and Mosher 2001).
    let mut p = dual_income();
    p.household.married = Some(EXAMPLE_START);
    p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms::default());
    let h = MarriageHazard::compile(&p, EXAMPLE_START, 0, 0, 40 * 12).unwrap();
    for (years, kept) in [(5usize, 0.80), (10, 0.67), (15, 0.57)] {
        let s = h.survival[years * 12];
        assert!((s - kept).abs() < 1e-9, "{years} years: {s}");
    }
    // After 15 years: 1% a year while the first person is under 65.
    let s15 = h.survival[15 * 12];
    let s16 = h.survival[16 * 12];
    assert!((s16 / s15 - 0.99).abs() < 1e-12);
}

#[test]
fn the_late_hazard_depends_on_age() {
    assert_eq!(annual_divorce_hazard(20.0, 50.0), 0.010);
    assert_eq!(annual_divorce_hazard(30.0, 70.0), 0.006);
    // First band: constant hazard with (1 - h)^5 = 0.8.
    let h = annual_divorce_hazard(0.0, 30.0);
    assert!((crate::math::powi(1.0 - h, 5) - 0.8).abs() < 1e-12);
    // Second band: (1 - h)^5 = 0.67 / 0.8.
    let h = annual_divorce_hazard(7.0, 30.0);
    assert!((crate::math::powi(1.0 - h, 5) - 0.67 / 0.8).abs() < 1e-12);
}

#[test]
fn the_hazard_fades_to_nothing_by_80() {
    assert_eq!(old_age_taper(74.9), 1.0);
    assert_eq!(old_age_taper(75.0), 1.0);
    assert!((old_age_taper(77.5) - 0.6).abs() < 1e-12);
    assert_eq!(old_age_taper(80.0), 0.0);
    assert_eq!(old_age_taper(101.0), 0.0);
    // A couple of 70 married 40 years: 0.6% a year to 76, fading after, and no
    // divorce at all once the first person is 80.
    let mut p = dual_income();
    let age70 = EXAMPLE_START - 70 * 12;
    p.household.people[0].birth = age70;
    p.household.married = Some(EXAMPLE_START - 480);
    p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms::default());
    let h = MarriageHazard::compile(&p, EXAMPLE_START, -480, 0, 30 * 12).unwrap();
    assert!((h.survival[12] - 0.994).abs() < 1e-12);
    let at80 = h.survival[10 * 12];
    assert!(at80 < h.survival[9 * 12]);
    assert_eq!(h.survival[30 * 12], at80);
    // Ever divorcing from 70 on: about 5%, not the 14% of a flat 0.6% to 100.
    assert!((0.04..0.06).contains(&(1.0 - at80)), "{}", 1.0 - at80);
}

#[test]
fn a_marriage_already_long_starts_in_the_late_band() {
    let mut p = dual_income();
    // Married 20 years before the start: 1% a year from month 0.
    p.household.married = Some(EXAMPLE_START - 240);
    p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms::default());
    let h = MarriageHazard::compile(&p, EXAMPLE_START, -240, 0, 24).unwrap();
    assert!((h.survival[12] - 0.99).abs() < 1e-12);
}

#[test]
fn the_drawn_month_inverts_the_survival_curve() {
    let h = MarriageHazard {
        terms: DivorceTerms::default(),
        survival: vec![1.0, 0.9, 0.8, 0.7],
    };
    // P(month 0) = 0.1, P(month 1) = 0.1, P(month 2) = 0.1, none after: 0.7.
    assert_eq!(h.draw_month(0.95), 0);
    assert_eq!(h.draw_month(0.85), 1);
    assert_eq!(h.draw_month(0.75), 2);
    assert_eq!(h.draw_month(0.7), 2);
    assert_eq!(h.draw_month(0.5), i32::MAX);
}

#[test]
fn split_off_takes_the_same_share_of_value_and_basis() {
    let mut a = Accounts::default();
    a.taxable.value[0] = 100.0;
    a.taxable.basis[0] = 60.0;
    a.person[0].traditional[0] = 200.0;
    a.person[1].roth.value[1] = 40.0;
    a.person[1].roth.contribution_basis = 20.0;
    let out = a.split_off(0.25);
    assert!((out - 85.0).abs() < 1e-12);
    assert!((a.taxable.value[0] - 75.0).abs() < 1e-12);
    assert!((a.taxable.basis[0] - 45.0).abs() < 1e-12);
    assert!((a.person[0].traditional[0] - 150.0).abs() < 1e-12);
    assert!((a.person[1].roth.contribution_basis - 15.0).abs() < 1e-12);
}

#[test]
fn merge_person_keeps_tax_character() {
    let mut a = Accounts::default();
    a.person[0].traditional[0] = 10.0;
    a.person[1].traditional[0] = 5.0;
    a.person[1].roth.value[2] = 7.0;
    a.person[1].roth.contribution_basis = 3.0;
    a.person[1].hsa[0] = 2.0;
    let before = a.by_kind();
    a.merge_person(1, 0);
    assert_eq!(a.by_kind(), before);
    assert_eq!(a.person[0].traditional[0], 15.0);
    assert_eq!(a.person[0].roth.value[2], 7.0);
    assert_eq!(a.person[0].roth.contribution_basis, 3.0);
    assert_eq!(a.person[1].traditional[0], 0.0);
}

#[test]
fn a_profile_without_household_dynamics_is_inactive() {
    // The "full-life" example has parents and life insurance on purpose.
    for ex in Profile::examples(EXAMPLE_START)
        .into_iter()
        .filter(|e| e.slug != "full-life")
    {
        let f = DynamicsPlan::compile(
            &ex.profile,
            crate::data::Data::embedded(),
            EXAMPLE_START,
            600,
        );
        assert!(!f.active, "{}", ex.slug);
    }
}

fn with_parent(parent: crate::model::Parent) -> Profile {
    let mut p = dual_income();
    p.household.parents = vec![parent];
    p
}

#[test]
fn an_inheritance_arrives_as_cash_and_an_inherited_ira() {
    use crate::model::Parent;
    let profile = with_parent(Parent {
        id: "mom".into(),
        birth: EXAMPLE_START - 85 * 12,
        estate: 400_000.0,
        share: 0.5,
        inherited_ira_fraction: 0.5,
        care_erodes_estate: false,
        ..Parent::default()
    });
    let plan = Plan::compile(&profile, crate::data::Data::embedded()).unwrap();
    assert!(plan.dynamics.active);
    let mut st = State::new(&plan);
    st.present = [true, true];
    st.acc = Accounts::default();
    st.acc.person[0].traditional[0] = 100_000.0;
    st.dynamics = DynamicsState::default();
    st.dynamics.parents[0].death = 5;
    let cash0 = st.acc.cash;
    assert!(st.parents_month(&plan, 1.0, 5));
    // Half of $400k: $100k of cash and $100k in an inherited IRA.
    assert!((st.acc.cash - cash0 - 100_000.0).abs() < 1e-6);
    let trad: f64 = st.acc.person[0].traditional.iter().sum();
    assert!((trad - 200_000.0).abs() < 1e-6);
    let slot = st.dynamics.ira[0];
    assert_eq!(slot.years_left, 10);
    assert!((slot.share - 0.5).abs() < 1e-12);
    let first = slot.first_year;
    assert_eq!(first, (plan.start + 5).year() + 1);
    // Nothing is paid out before the December of the following year.
    let dec_before = YearMonth::ym(first - 1, 12) - plan.start;
    st.inherited_ira_month(&plan, 1.0, dec_before as usize);
    assert_eq!(st.ty.traditional_distributions, 0.0);
    // The first distribution: a tenth of the $100k, taxed, never penalised.
    let dec = YearMonth::ym(first, 12) - plan.start;
    st.inherited_ira_month(&plan, 1.0, dec as usize);
    assert!((st.ty.traditional_distributions - 10_000.0).abs() < 1e-6);
    assert_eq!(st.ty.early_distributions, 0.0);
    let trad: f64 = st.acc.person[0].traditional.iter().sum();
    assert!((trad - 190_000.0).abs() < 1e-6);
    // $90k is left of the inherited IRA: the next is a ninth of it.
    assert!((st.dynamics.ira[0].share * trad - 90_000.0).abs() < 1e-6);
    assert_eq!(st.dynamics.ira[0].years_left, 9);
    st.ty.traditional_distributions = 0.0;
    st.inherited_ira_month(&plan, 1.0, dec as usize + 12);
    assert!((st.ty.traditional_distributions - 10_000.0).abs() < 1e-6);
}

#[test]
fn care_erodes_the_estate_and_nothing_comes_after_a_divorce() {
    use crate::model::Parent;
    let profile = with_parent(Parent {
        id: "dad".into(),
        child: 1,
        birth: EXAMPLE_START - 85 * 12,
        estate: 300_000.0,
        share: 1.0,
        ..Parent::default()
    });
    let plan = Plan::compile(&profile, crate::data::Data::embedded()).unwrap();
    let mut st = State::new(&plan);
    st.present = [true, true];
    st.acc = Accounts::default();
    st.dynamics = DynamicsState::default();
    st.dynamics.parents[0].death = 3;
    st.dynamics.parents[0].care_total = 120_000.0;
    st.parents_month(&plan, 1.0, 3);
    assert!((st.acc.cash - 180_000.0).abs() < 1e-6);
    // Care costing more than the estate leaves nothing, never a debt.
    st.acc = Accounts::default();
    st.dynamics.parents[0] = super::parents::ParentPath {
        death: 4,
        care_total: 900_000.0,
        ..Default::default()
    };
    st.parents_month(&plan, 1.0, 4);
    assert_eq!(st.acc.cash, 0.0);
    // The parent of a partner who has left.
    st.dynamics.parents[0] = super::parents::ParentPath {
        death: 5,
        ..Default::default()
    };
    st.present[1] = false;
    st.dynamics.left[1] = true;
    assert!(!st.parents_month(&plan, 1.0, 5));
    assert_eq!(st.acc.cash, 0.0);
}

#[test]
fn parents_draw_from_the_life_table() {
    use crate::model::Parent;
    let profile = with_parent(Parent {
        id: "mom".into(),
        sex: crate::model::Sex::Female,
        birth: EXAMPLE_START - 80 * 12,
        estate: 1.0,
        ..Parent::default()
    });
    let plan = Plan::compile(&profile, crate::data::Data::embedded()).unwrap();
    let mut st = State::new(&plan);
    let n = 4000;
    let mut years = 0.0;
    let mut care = 0;
    for path in 0..n {
        st.dynamics = DynamicsState::default();
        st.begin_parents(&plan, 1, path);
        years += f64::from(st.dynamics.parents[0].death) / 12.0;
        care += usize::from(st.dynamics.parents[0].care_total > 0.0);
    }
    let mean = years / n as f64;
    // SSA period life expectancy of a woman of 80 is about 9.7 years; the
    // default good health and mortality improvement add a little.
    assert!((9.0..12.5).contains(&mean), "{mean}");
    // About half of women who reach 80 use paid care before they die.
    let share = care as f64 / n as f64;
    assert!((0.35..0.75).contains(&share), "{share}");
}

#[test]
fn the_households_share_of_care_comes_off_the_estates_bill() {
    use crate::model::{CareSupport, Parent};
    let mom = Parent {
        id: "mom".into(),
        sex: crate::model::Sex::Female,
        birth: EXAMPLE_START - 84 * 12,
        estate: 1.0,
        ..Parent::default()
    };
    let plain = Plan::compile(&with_parent(mom.clone()), crate::data::Data::embedded()).unwrap();
    let helped = Plan::compile(
        &with_parent(Parent {
            care_support: Some(CareSupport {
                probability: 1.0,
                annual_cost: 12_000.0,
                hours: 0.5,
                caregiver: None,
            }),
            ..mom.clone()
        }),
        crate::data::Data::embedded(),
    )
    .unwrap();
    let care = plain.dynamics.parent_care.as_ref().unwrap();
    let mut a = State::new(&plain);
    let mut b = State::new(&helped);
    let mut spells = 0;
    for path in 0..200 {
        a.dynamics = DynamicsState::default();
        b.dynamics = DynamicsState::default();
        a.begin_parents(&plain, 4, path);
        b.begin_parents(&helped, 4, path);
        let (x, y) = (a.dynamics.parents[0], b.dynamics.parents[0]);
        // The same life and the same care with or without the household's help.
        assert_eq!(
            (x.death, x.care_start, x.care_end),
            (y.death, y.care_start, y.care_end)
        );
        if x.care_start == u32::MAX {
            assert!(!y.helps);
            continue;
        }
        spells += 1;
        assert!(y.helps && !x.helps);
        // $1,000 a month (care costs far more), rising with medical costs.
        assert_eq!(y.household_monthly, 1_000.0);
        let mut growth = 0.0;
        for m in x.care_start..x.care_end {
            growth += care.excess_growth(m);
        }
        let diff = x.care_total - y.care_total;
        assert!(
            (diff - 1_000.0 * growth).abs() < 1e-6 * x.care_total,
            "{diff}"
        );
    }
    assert!(spells > 40, "{spells}");
}

#[test]
fn the_household_takes_part_with_the_stated_probability() {
    use crate::model::{CareSupport, Parent};
    let plan = Plan::compile(
        &with_parent(Parent {
            id: "dad".into(),
            birth: EXAMPLE_START - 84 * 12,
            care_support: Some(CareSupport {
                probability: 0.3,
                annual_cost: 5_000.0,
                ..CareSupport::default()
            }),
            ..Parent::default()
        }),
        crate::data::Data::embedded(),
    )
    .unwrap();
    let mut st = State::new(&plan);
    let (mut spells, mut helps) = (0, 0);
    for path in 0..4000 {
        st.dynamics = DynamicsState::default();
        st.begin_parents(&plan, 9, path);
        let p = st.dynamics.parents[0];
        if p.care_start != u32::MAX {
            spells += 1;
            helps += usize::from(p.helps);
        }
    }
    let share = helps as f64 / spells as f64;
    assert!((share - 0.3).abs() < 0.04, "{share} of {spells}");
}

fn spousal_plan() -> Plan {
    let mut p = dual_income();
    p.spending.healthcare.spousal_care = true;
    Plan::compile(&p, crate::data::Data::embedded()).unwrap()
}

fn home_care(st: &mut State, i: usize, from: u32, to: u32, care: crate::health::CareType) {
    st.health[i].ltc_start_month = from;
    st.health[i].ltc_end_month = to;
    st.health[i].ltc_care = care;
}

#[test]
fn a_partner_at_home_halves_the_home_care_bill() {
    use crate::health::CareType;
    let plan = spousal_plan();
    assert!(plan.dynamics.spousal_care && plan.dynamics.active);
    let mut st = State::new(&plan);
    st.present = [true, true];
    home_care(&mut st, 0, 10, 30, CareType::Home);
    assert_eq!(st.spousal_care_factor(0, 5), 1.0);
    assert_eq!(st.spousal_care_factor(0, 10), 0.5);
    assert_eq!(st.spousal_care_factor(0, 29), 0.5);
    assert_eq!(st.spousal_care_factor(0, 30), 1.0);
    assert_eq!(st.spousal_care_factor(1, 15), 1.0);
    // Not when the partner is in care too, in a facility, or gone.
    home_care(&mut st, 1, 20, 40, CareType::Home);
    assert_eq!(st.spousal_care_factor(0, 15), 0.5);
    assert_eq!(st.spousal_care_factor(0, 25), 1.0);
    home_care(&mut st, 1, u32::MAX, 0, CareType::None);
    st.in_facility[1] = true;
    assert_eq!(st.spousal_care_factor(0, 15), 1.0);
    st.in_facility[1] = false;
    st.present[1] = false;
    assert_eq!(st.spousal_care_factor(0, 15), 1.0);
    // Nor for a nursing home.
    st.present[1] = true;
    home_care(&mut st, 0, 10, 30, CareType::NursingHome);
    assert_eq!(st.spousal_care_factor(0, 15), 1.0);
}

#[test]
fn a_working_partner_cuts_back_with_the_stated_probabilities() {
    use crate::career::Employment;
    use crate::health::CareType;
    let plan = spousal_plan();
    let run = |u: f64| {
        let mut st = State::new(&plan);
        st.present = [true, true];
        for (rt, jp) in st.jobs.iter_mut().zip(&plan.jobs) {
            jp.model.begin(&mut rt.st, 40.0);
            rt.begun = true;
        }
        home_care(&mut st, 0, 12, 36, CareType::Home);
        st.dynamics.care_u = [1.0, u];
        let replan = st.spousal_care_month(&plan, 12);
        let job = plan.jobs.iter().position(|j| j.owner == 1).unwrap();
        (replan, st.jobs[job].st.employment, st.jobs[job].ended)
    };
    // u below 0.15: half time for the 24 months of the episode.
    let (r, e, ended) = run(0.1);
    assert!(r && !ended);
    assert_eq!(e, Employment::OnBreak { months_left: 24 });
    // Between 0.15 and 0.26: stops working.
    let (r, _, ended) = run(0.2);
    assert!(r && ended);
    // Otherwise: works on.
    let (r, e, ended) = run(0.5);
    assert!(!r && !ended);
    assert_eq!(e, Employment::Employed);
}

fn insured_plan(kind: crate::model::PolicyKind) -> Plan {
    let mut p = dual_income();
    p.household.people[0].life_insurance = vec![crate::model::LifePolicy {
        id: "term".into(),
        name: "Term life".into(),
        kind,
        death_benefit: 500_000.0,
        annual_premium: 1_200.0,
    }];
    Plan::compile(&p, crate::data::Data::embedded()).unwrap()
}

fn insured_state(plan: &Plan) -> State {
    let mut st = State::new(plan);
    st.present = [true, true];
    st.alive = [true, true];
    st.months = 600;
    st.death_month = [500, 550];
    st.dynamics = DynamicsState::default();
    st.dynamics.in_force[0] = true;
    st
}

#[test]
fn premiums_are_paid_until_the_term_ends() {
    use crate::model::PolicyKind;
    let plan = insured_plan(PolicyKind::Term {
        years_remaining: 10.0,
    });
    assert!(plan.dynamics.active);
    let p = &plan.dynamics.policies[0];
    assert_eq!((p.insured, p.end_month), (0, 120));
    assert_eq!(p.premium, 100.0);
    let mut st = insured_state(&plan);
    let cash = st.acc.cash;
    assert!(!st.insurance_month(&plan, 1.0, 0));
    assert!((cash - st.acc.cash - 100.0).abs() < 1e-9);
    assert_eq!(st.planned, 100.0);
    assert_eq!(st.premium_rate(&plan, 0), 100.0);
    // Month 120: the term has run out; nothing more is paid.
    let cash = st.acc.cash;
    st.insurance_month(&plan, 1.0, 120);
    assert!(!st.dynamics.in_force[0]);
    assert_eq!(st.acc.cash, cash);
    assert_eq!(st.premium_rate(&plan, 121), 0.0);
    // A death after the term pays nothing.
    st.alive[0] = false;
    st.insurance_month(&plan, 1.0, 130);
    assert_eq!(st.acc.cash, cash);
}

#[test]
fn a_death_within_the_term_pays_the_survivor() {
    use crate::model::PolicyKind;
    let plan = insured_plan(PolicyKind::Term {
        years_remaining: 20.0,
    });
    let mut st = insured_state(&plan);
    st.alive[0] = false;
    st.present[0] = false;
    let cash = st.acc.cash;
    assert!(st.insurance_month(&plan, 1.3, 60));
    // Nominal $500,000, not indexed, and no premium in the month of death.
    assert!((st.acc.cash - cash - 500_000.0).abs() < 1e-6);
    assert_eq!(
        st.led[crate::stats::YearSeries::IncomeOther.index()],
        500_000.0
    );
    assert!(!st.dynamics.in_force[0]);
    // Paid once.
    assert!(!st.insurance_month(&plan, 1.3, 61));
    assert!((st.acc.cash - cash - 500_000.0).abs() < 1e-6);
}

#[test]
fn the_last_death_pays_the_estate_in_the_final_month() {
    use crate::model::PolicyKind;
    let plan = insured_plan(PolicyKind::Permanent);
    assert_eq!(plan.dynamics.policies[0].end_month, i32::MAX);
    let mut st = insured_state(&plan);
    // Person 2 died earlier; person 1 dies at month 400, which ends the path.
    st.alive[1] = false;
    st.present[1] = false;
    st.death_month = [400, 300];
    st.months = 400;
    let cash = st.acc.cash;
    st.insurance_month(&plan, 1.0, 398);
    assert!((cash - st.acc.cash - 100.0).abs() < 1e-9);
    let cash = st.acc.cash;
    st.insurance_month(&plan, 1.0, 399);
    assert!((st.acc.cash - cash - 500_000.0).abs() < 1e-6);
    assert!(!st.dynamics.in_force[0]);
    // A path cut by the horizon (death after it) pays nothing.
    let mut st = insured_state(&plan);
    st.death_month = [700, 300];
    st.months = 600;
    let cash = st.acc.cash;
    st.insurance_month(&plan, 1.0, 599);
    assert!((cash - st.acc.cash - 100.0).abs() < 1e-9);
}

#[test]
fn a_policy_lapses_when_money_runs_out_or_the_insured_leaves() {
    use crate::model::PolicyKind;
    let plan = insured_plan(PolicyKind::Permanent);
    let mut st = insured_state(&plan);
    st.depleted_now = true;
    let cash = st.acc.cash;
    st.insurance_month(&plan, 1.0, 50);
    assert!(!st.dynamics.in_force[0]);
    assert_eq!(st.acc.cash, cash);
    // Lapsed: a later death pays nothing.
    st.alive[0] = false;
    st.insurance_month(&plan, 1.0, 60);
    assert_eq!(st.acc.cash, cash);

    let mut st = insured_state(&plan);
    st.dynamics.left[0] = true;
    st.insurance_month(&plan, 1.0, 50);
    assert!(!st.dynamics.in_force[0]);
    assert_eq!(st.acc.cash, cash);
}

#[test]
fn nothing_comes_from_the_parent_of_a_partner_who_has_not_joined() {
    use crate::model::{CareSupport, Parent};
    let profile = with_parent(Parent {
        id: "dad".into(),
        child: 1,
        birth: EXAMPLE_START - 85 * 12,
        estate: 300_000.0,
        share: 1.0,
        care_support: Some(CareSupport {
            annual_cost: 12_000.0,
            ..CareSupport::default()
        }),
        ..Parent::default()
    });
    let plan = Plan::compile(&profile, crate::data::Data::embedded()).unwrap();
    let mut st = State::new(&plan);
    st.present = [true, false];
    st.alive = [true, true];
    st.acc = Accounts::default();
    st.dynamics = DynamicsState::default();
    st.dynamics.parents[0] = super::parents::ParentPath {
        death: 6,
        care_start: 2,
        care_end: 6,
        care_monthly: 5_000.0,
        household_monthly: 1_000.0,
        helps: true,
        ..Default::default()
    };
    // The care spell: the household does not pay while the child is not in it.
    assert!(!st.parents_month(&plan, 1.0, 2));
    assert_eq!(st.acc.cash, 0.0);
    // The death: the estate goes to the child's own household.
    assert!(!st.parents_month(&plan, 1.0, 6));
    assert_eq!(st.acc.cash, 0.0);
    // Once the child has died, their share passes to the partner who stays.
    st.dynamics.parents[0] = super::parents::ParentPath {
        death: 7,
        ..Default::default()
    };
    st.alive = [true, false];
    assert!(st.parents_month(&plan, 1.0, 7));
    assert!((st.acc.cash - 300_000.0).abs() < 1e-6);
}

#[test]
fn inherited_iras_keep_their_value_when_accounts_are_merged() {
    use super::parents::InheritedIra;
    let profile = with_parent(crate::model::Parent {
        id: "mom".into(),
        birth: EXAMPLE_START - 80 * 12,
        estate: 100_000.0,
        ..crate::model::Parent::default()
    });
    let plan = Plan::compile(&profile, crate::data::Data::embedded()).unwrap();
    let mut st = State::new(&plan);
    st.present = [true, true];
    st.alive = [true, true];
    st.acc = Accounts::default();
    st.dynamics = DynamicsState::default();
    st.acc.person[0].traditional[0] = 100.0;
    st.acc.person[1].traditional[0] = 300.0;
    let slot = |heir, share| InheritedIra {
        heir,
        share,
        years_left: 5,
        first_year: 2030,
        value: 0.0,
    };
    st.dynamics.ira[0] = slot(0, 0.5);
    st.dynamics.ira[1] = slot(1, 1.0 / 3.0);
    // A divorce's rollover: person 0 now holds both, each still worth $50 and $100.
    st.merge_person_keeping_inherited(1, 0);
    assert_eq!(st.acc.person[0].traditional[0], 400.0);
    assert_eq!(st.dynamics.ira[0].heir, 0);
    assert_eq!(st.dynamics.ira[1].heir, 0);
    assert!((st.dynamics.ira[0].share * 400.0 - 50.0).abs() < 1e-9);
    assert!((st.dynamics.ira[1].share * 400.0 - 100.0).abs() < 1e-9);

    // A death: person 1's accounts pass to person 0 before the month's step, which
    // then re-bases both inherited IRAs on what they were worth a month ago.
    st.acc.person[0].traditional[0] = 100.0;
    st.acc.person[1].traditional[0] = 300.0;
    st.dynamics.ira[0] = slot(0, 0.5);
    st.dynamics.ira[1] = slot(1, 1.0 / 3.0);
    st.parents_month(&plan, 1.0, 0);
    assert!((st.dynamics.ira[0].value - 50.0).abs() < 1e-9);
    assert!((st.dynamics.ira[1].value - 100.0).abs() < 1e-9);
    st.acc.person[0].traditional[0] = 400.0;
    st.acc.person[1].traditional[0] = 0.0;
    st.present[1] = false;
    st.alive[1] = false;
    st.parents_month(&plan, 1.0, 1);
    assert_eq!(st.dynamics.ira[1].heir, 0);
    assert!((st.dynamics.ira[0].share * 400.0 - 50.0).abs() < 1e-9);
    assert!((st.dynamics.ira[1].share * 400.0 - 100.0).abs() < 1e-9);
}

#[test]
fn commitments_raise_the_funded_test_requirement() {
    use crate::model::PolicyKind;
    // Term: 60 premiums of $100 still to pay at month 60, at a price level of 2.
    let plan = insured_plan(PolicyKind::Term {
        years_remaining: 10.0,
    });
    let mut st = insured_state(&plan);
    assert_eq!(st.dynamics_requirement(&plan, 60, 2.0), (0.0, 3_000.0));
    st.dynamics.in_force[0] = false;
    assert_eq!(st.dynamics_requirement(&plan, 60, 2.0), (0.0, 0.0));
    // Support still owed: 24 months of $500 nominal.
    st.dynamics.support_monthly = 500.0;
    st.dynamics.support_until = 84;
    assert_eq!(st.dynamics_requirement(&plan, 60, 1.0), (0.0, 12_000.0));
    // Received support is not a cost.
    st.dynamics.support_monthly = -500.0;
    assert_eq!(st.dynamics_requirement(&plan, 60, 1.0), (0.0, 0.0));
    // Permanent: $1,200 a year for life, in today's dollars.
    let plan = insured_plan(PolicyKind::Permanent);
    let st = insured_state(&plan);
    assert_eq!(st.dynamics_requirement(&plan, 60, 2.0), (600.0, 0.0));
}
