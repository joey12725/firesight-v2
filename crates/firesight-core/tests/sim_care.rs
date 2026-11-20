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

/// A retired couple of 66 and 64 with a portfolio that supports them for about 30
/// years at their spending.
fn couple(dependence: CoupleDependence) -> Profile {
    let mut a = person("al", 66, Sex::Male);
    let mut b = person("bea", 64, Sex::Female);
    for (who, pia) in [(&mut a, 2_000.0), (&mut b, 1_400.0)] {
        who.social_security.basis = SocialSecurityBasis::KnownBenefit {
            monthly_at_fra: pia,
        };
        who.social_security.claim_age = 67.0;
    }
    let mut p = quiet(vec![a, b], 100.0);
    p.assumptions.longevity = Longevity::Stochastic {
        improvement: true,
        couple_dependence: dependence,
        income_gradient: false,
        income_percentile: None,
    };
    p.spending.items = vec![item("living", Category::Other, 6_000.0, true)];
    p.accounts = vec![account("brokerage", 0, AccountKind::Taxable, 900_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.spending_rule = SpendingRule::ConstantReal;
    p
}

fn stats(x: &[f64]) -> (f64, f64) {
    let m = mean(x);
    let v = x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / x.len() as f64;
    (m, v.sqrt())
}

#[test]
fn linked_lifespans_keep_each_life_and_link_the_two() {
    let (plan_i, plan_l) = (
        compile(&couple(CoupleDependence::Independent)),
        compile(&couple(CoupleDependence::Linked)),
    );
    let n = 4_000;
    let (a, b) = (run(&plan_i, 7, 0..n), run(&plan_l, 7, 0..n));
    let ages = |acc: &Accumulator, s: PathScalar| -> Vec<f64> {
        column(acc, s).iter().map(|&v| f64::from(v)).collect()
    };
    let (i0, i1) = (
        ages(&a, PathScalar::DeathAge0),
        ages(&a, PathScalar::DeathAge1),
    );
    let (l0, l1) = (
        ages(&b, PathScalar::DeathAge0),
        ages(&b, PathScalar::DeathAge1),
    );
    // Each person's life expectancy is kept (sampling noise is about 0.15 years).
    for (x, y) in [(&i0, &l0), (&i1, &l1)] {
        let (mx, sx) = stats(x);
        let (my, _) = stats(y);
        assert!(
            (mx - my).abs() < 4.0 * sx / (n as f64).sqrt(),
            "{mx} vs {my}"
        );
    }
    let corr = |x: &[f64], y: &[f64]| {
        let ((mx, sx), (my, sy)) = (stats(x), stats(y));
        x.iter()
            .zip(y)
            .map(|(a, b)| (a - mx) * (b - my))
            .sum::<f64>()
            / (x.len() as f64 * sx * sy)
    };
    let (ci, cl) = (corr(&i0, &i1), corr(&l0, &l1));
    assert!(ci.abs() < 0.05, "independent: {ci}");
    assert!(cl > 0.45 && cl < 0.8, "linked: {cl}");
    // The last survivor's life is shorter (lives end closer together), so the
    // couple needs its money for less long: depletion does not rise, and the
    // years spent widowed fall by roughly a third or more.
    let last = |x: &[f64], y: &[f64]| -> f64 {
        mean(&x.iter().zip(y).map(|(a, b)| a.max(*b)).collect::<Vec<_>>())
    };
    let widowed = |x: &[f64], y: &[f64]| -> f64 {
        // Ages differ by two years at the start.
        mean(
            &x.iter()
                .zip(y)
                .map(|(a, b)| ((a - 2.0) - b).abs())
                .collect::<Vec<_>>(),
        )
    };
    let (wi, wl) = (widowed(&i0, &i1), widowed(&l0, &l1));
    assert!(last(&l0, &l1) < last(&i0, &i1) - 0.5);
    assert!(
        wl < 0.75 * wi && wl > 0.2 * wi,
        "years widowed {wi} -> {wl}"
    );
    let depleted = |acc: &Accumulator| {
        column(acc, PathScalar::DepletionAge)
            .iter()
            .filter(|v| !v.is_nan())
            .count()
    };
    assert!(
        depleted(&b) <= depleted(&a),
        "{} -> {}",
        depleted(&a),
        depleted(&b)
    );
}

#[test]
fn linked_lifespans_are_bit_identical_when_off_and_traced_when_on() {
    let off = couple(CoupleDependence::Independent);
    let mut implicit = off.clone();
    implicit.assumptions.longevity = Longevity::default();
    let json = serde_json::to_value(&implicit.assumptions.longevity).unwrap();
    assert!(json.get("couple_dependence").is_none());
    assert_eq!(
        run(&compile(&off), 4, 0..300).to_bytes(),
        run(&compile(&implicit), 4, 0..300).to_bytes()
    );
    // A single person: linked does nothing.
    let mut single = retiree_70(Sex::Female);
    let base = run(&compile(&single), 4, 0..300).to_bytes();
    single.assumptions.longevity = Longevity::Stochastic {
        improvement: true,
        couple_dependence: CoupleDependence::Linked,
        income_gradient: false,
        income_percentile: None,
    };
    assert_eq!(run(&compile(&single), 4, 0..300).to_bytes(), base);
    // Some paths show a death soon after the partner's, with the event logged.
    let plan = compile(&couple(CoupleDependence::Linked));
    let logged = (0..400)
        .filter(|&path| {
            sim::trace_path(&plan, 4, path)
                .events
                .iter()
                .any(|e| e.kind == firesight_core::stats::TraceEventKind::Bereavement)
        })
        .count();
    // w is about 0.5% per partner of 66 and 64 (two components), minus paths cut
    // at the horizon: a handful in 400.
    assert!((1..=15).contains(&logged), "{logged}");
}

fn with_income_rank(mut p: Profile, gradient: bool, percentile: Option<f64>) -> Profile {
    p.assumptions.longevity = Longevity::Stochastic {
        improvement: true,
        couple_dependence: CoupleDependence::Independent,
        income_gradient: gradient,
        income_percentile: percentile,
    };
    p
}

#[test]
fn income_gradient_moves_life_expectancy_by_the_income_rank() {
    let base = retiree_70(Sex::Female);
    let n = 3_000;
    let death = |p: &Profile| -> f64 {
        let acc = run(&compile(p), 5, 0..n);
        mean(
            &column(&acc, PathScalar::DeathAge0)
                .iter()
                .map(|&v| f64::from(v))
                .collect::<Vec<_>>(),
        )
    };
    // Off, with or without a (then ignored) percentile, is the baseline bit for
    // bit.
    let off = run(&compile(&base), 5, 0..300).to_bytes();
    assert_eq!(
        run(
            &compile(&with_income_rank(base.clone(), false, Some(90.0))),
            5,
            0..300
        )
        .to_bytes(),
        off
    );
    let mid = death(&base);
    let top = death(&with_income_rank(base.clone(), true, Some(95.0)));
    let bottom = death(&with_income_rank(base.clone(), true, Some(5.0)));
    // Women at 40: the 95th percentile lives 3.5 years longer than the average of
    // the percentiles, the 5th 4.5 years shorter (Chetty et al. 2016). At 70 a
    // proportional shift of mortality moves remaining life by less: between one
    // and four years each way.
    assert!(top - mid > 1.0 && top - mid < 4.0, "{mid} -> {top}");
    assert!(
        mid - bottom > 1.0 && mid - bottom < 4.5,
        "{mid} -> {bottom}"
    );
    // Paired paths: a lower mortality only ever moves each death later.
    let (a, b) = (
        run(&compile(&base), 5, 0..500),
        run(
            &compile(&with_income_rank(base.clone(), true, Some(95.0))),
            5,
            0..500,
        ),
    );
    let (da, db) = (
        column(&a, PathScalar::DeathAge0),
        column(&b, PathScalar::DeathAge0),
    );
    assert!(da.iter().zip(&db).all(|(x, y)| y >= x));
}

#[test]
fn income_gradient_ranks_the_household_earnings() {
    // A worker of 45 earning $40k against one earning $400k: the second ranks near
    // the top and lives longer; the first below the middle.
    let mk = |salary: f64| {
        let mut p = quiet(vec![person("w", 45, Sex::Male)], 100.0);
        p.incomes = vec![job("pay", 0, salary)];
        with_income_rank(p, true, None)
    };
    let le = |p: &Profile| compile(p).life_expectancies()[0];
    let (low, high) = (le(&mk(40_000.0)), le(&mk(400_000.0)));
    let mut neutral = mk(40_000.0);
    neutral.assumptions.longevity = Longevity::default();
    let avg = le(&neutral);
    assert!(high > avg + 2.0 && low < avg - 0.5, "{low} {avg} {high}");
}
