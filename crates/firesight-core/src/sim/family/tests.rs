//! Unit tests of the household-dynamics mechanisms, with hand-computed values.

use super::divorce::annual_divorce_hazard;
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
    for ex in Profile::examples(EXAMPLE_START) {
        let f = FamilyPlan::compile(
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
    assert!(plan.family.active);
    let mut st = State::new(&plan);
    st.present = [true, true];
    st.acc = Accounts::default();
    st.acc.person[0].traditional[0] = 100_000.0;
    st.fam = FamilyState::default();
    st.fam.parents[0].death = 5;
    let cash0 = st.acc.cash;
    assert!(st.parents_month(&plan, 1.0, 5));
    // Half of $400k: $100k of cash and $100k in an inherited IRA.
    assert!((st.acc.cash - cash0 - 100_000.0).abs() < 1e-6);
    let trad: f64 = st.acc.person[0].traditional.iter().sum();
    assert!((trad - 200_000.0).abs() < 1e-6);
    let slot = st.fam.ira[0];
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
    assert!((st.fam.ira[0].share * trad - 90_000.0).abs() < 1e-6);
    assert_eq!(st.fam.ira[0].years_left, 9);
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
    st.fam = FamilyState::default();
    st.fam.parents[0].death = 3;
    st.fam.parents[0].care_total = 120_000.0;
    st.parents_month(&plan, 1.0, 3);
    assert!((st.acc.cash - 180_000.0).abs() < 1e-6);
    // Care costing more than the estate leaves nothing, never a debt.
    st.acc = Accounts::default();
    st.fam.parents[0] = super::parents::ParentPath {
        death: 4,
        care_total: 900_000.0,
        ..Default::default()
    };
    st.parents_month(&plan, 1.0, 4);
    assert_eq!(st.acc.cash, 0.0);
    // The parent of a partner who has left.
    st.fam.parents[0] = super::parents::ParentPath {
        death: 5,
        ..Default::default()
    };
    st.present[1] = false;
    st.fam.left[1] = true;
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
        st.fam = FamilyState::default();
        st.begin_parents(&plan, 1, path);
        years += f64::from(st.fam.parents[0].death) / 12.0;
        care += usize::from(st.fam.parents[0].care_total > 0.0);
    }
    let mean = years / n as f64;
    // SSA period life expectancy of a woman of 80 is about 9.7 years; the
    // default good health and mortality improvement add a little.
    assert!((9.0..12.5).contains(&mean), "{mean}");
    // About half of women who reach 80 use paid care before they die.
    let share = care as f64 / n as f64;
    assert!((0.35..0.75).contains(&share), "{share}");
}
