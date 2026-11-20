//! Simulation-level tests of the opt-in health, longevity and care features, on
//! paired paths: each feature off reproduces the baseline exactly, and on it moves
//! the outcomes it should, by a plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{Accumulator, PathScalar};

/// A single retiree of 70 with modelled healthcare and long-term care, enough
/// savings that she never runs out, and no other randomness that matters here.
fn retiree_70(sex: Sex) -> Profile {
    let mut who = person("ann", 70, sex);
    who.social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 2_200.0,
    };
    who.social_security.claim_age = 67.0;
    let mut p = quiet(vec![who], 100.0);
    p.assumptions.longevity = Longevity::default();
    p.assumptions.health_risk = true;
    p.spending.healthcare = Healthcare::default();
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 3_000_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.spending_rule = SpendingRule::ConstantReal;
    p
}

fn column(acc: &Accumulator, s: PathScalar) -> Vec<f32> {
    acc.scalar_column(s)
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Health-care spending in today's dollars over a traced path, in total and over
/// its last 24 months.
fn health_spend(plan: &sim::Plan, path: u64) -> (f64, f64) {
    let trace = sim::trace_path(plan, 3, path);
    let col = trace.column("spend_healthcare").unwrap_or(&[]);
    let total: f64 = col.iter().sum();
    let last: f64 = col.iter().rev().take(24).sum();
    (total, last)
}

#[test]
fn health_states_keep_every_lifetime_and_the_mean_costs() {
    let off = retiree_70(Sex::Female);
    let mut on = off.clone();
    on.assumptions.health_model = HealthModelKind::States;
    let (plan_off, plan_on) = (compile(&off), compile(&on));
    let n = 3_000;
    let (a, b) = (run(&plan_off, 3, 0..n), run(&plan_on, 3, 0..n));
    // Deaths are not redrawn: every lifetime is the same, bit for bit.
    let (da, db) = (
        column(&a, PathScalar::DeathAge0),
        column(&b, PathScalar::DeathAge0),
    );
    assert!(da.iter().zip(&db).all(|(x, y)| x.to_bits() == y.to_bits()));
    // Care is concentrated on the frail but its population incidence is kept:
    // the share of lives with paid care and its mean cost move by little.
    let care = |acc: &Accumulator| -> (f64, f64) {
        let c: Vec<f64> = column(acc, PathScalar::LongTermCareCost)
            .iter()
            .map(|&v| f64::from(v))
            .collect();
        let users = c.iter().filter(|&&v| v > 0.0).count() as f64 / c.len() as f64;
        (users, mean(&c))
    };
    let ((ua, ca), (ub, cb)) = (care(&a), care(&b));
    assert!((ub - ua).abs() < 0.03, "care users {ua} -> {ub}");
    assert!((cb / ca - 1.0).abs() < 0.12, "care cost {ca} -> {cb}");
    assert_ne!(
        column(&a, PathScalar::LongTermCareCost),
        column(&b, PathScalar::LongTermCareCost),
        "the state at death must move who needs care"
    );
}

#[test]
fn health_states_put_the_expensive_years_before_death() {
    let off = retiree_70(Sex::Male);
    let mut on = off.clone();
    on.assumptions.health_model = HealthModelKind::States;
    let (plan_off, plan_on) = (compile(&off), compile(&on));
    let (mut tot_off, mut tot_on, mut last_off, mut last_on) = (0.0, 0.0, 0.0, 0.0);
    let mut months = 0.0;
    let paths = 300;
    for path in 0..paths {
        let (t0, l0) = health_spend(&plan_off, path);
        let (t1, l1) = health_spend(&plan_on, path);
        tot_off += t0;
        tot_on += t1;
        last_off += l0;
        last_on += l1;
        months += sim::trace_path(&plan_off, 3, path).months as f64;
    }
    // Premiums and the out-of-pocket shocks are the same on both sides of each
    // pair (same deaths, same draws), so the differences are the state
    // multiplier alone. Lifetime health spending is about the same (the
    // multipliers average to 1 over the living); the extra cost sits in the last
    // two years of life. Measured: +$26 a month in the last 24 months against
    // +$1 a month before them (premiums included, about a quarter of the
    // Medicare-phase cost is out of pocket).
    assert!(
        (tot_on / tot_off - 1.0).abs() < 0.02,
        "{tot_off} -> {tot_on}"
    );
    let late = (last_on - last_off) / (paths as f64 * 24.0);
    let early = ((tot_on - last_on) - (tot_off - last_off)) / (months - paths as f64 * 24.0);
    assert!(
        late > 10.0 && late < 200.0,
        "last two years: {late} a month"
    );
    assert!(late > 5.0 * early.abs(), "{late} vs {early} a month");
}

#[test]
fn health_states_off_is_the_default_bit_for_bit() {
    for example in Profile::examples(START) {
        let mut explicit = example.profile.clone();
        explicit.assumptions.health_model = HealthModelKind::Independent;
        let json = serde_json::to_value(&example.profile).unwrap();
        assert!(json["assumptions"].get("health_model").is_none());
        let back: Profile = serde_json::from_value(json).unwrap();
        assert_eq!(back.assumptions.health_model, HealthModelKind::Independent);
        let (a, b) = (compile(&example.profile), compile(&explicit));
        assert_eq!(
            run(&a, 9, 0..200).to_bytes(),
            run(&b, 9, 0..200).to_bytes(),
            "{}",
            example.slug
        );
    }
}
