//! Simulation-level tests of the opt-in health, longevity and care features, on
//! paired paths: each feature off reproduces the baseline exactly, and on it moves
//! the outcomes it should, by a plausible amount.

mod common;

use common::*;
use firesight_core::math;
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
    let v = x.iter().map(|a| math::powi(a - m, 2)).sum::<f64>() / x.len() as f64;
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
    let json = serde_json::to_value(implicit.assumptions.longevity).unwrap();
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

/// A widow of 78 with a paid-for home, $150,000 in cash and Social Security that
/// covers her budget and ordinary health costs: only a long care episode can
/// exhaust her savings.
fn widow_78(medicaid: bool) -> Profile {
    let mut p = retiree_70(Sex::Female);
    p.household.people[0] = person("ann", 78, Sex::Female);
    p.household.people[0].social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 3_000.0,
    };
    p.household.people[0].social_security.claim_age = 67.0;
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 150_000.0)];
    p.spending.items = vec![item("living", Category::Other, 2_400.0, true)];
    p.properties = vec![Property {
        id: "home".into(),
        value: 300_000.0,
        basis: Some(150_000.0),
        ..Property::default()
    }];
    p.retirement.home_equity = HomeEquity::Never;
    p.spending.healthcare.medicaid = medicaid;
    p
}

#[test]
fn medicaid_off_is_the_default_bit_for_bit() {
    let off = widow_78(false);
    let json = serde_json::to_value(&off).unwrap();
    assert!(json["spending"]["healthcare"].get("medicaid").is_none());
    let back: Profile = serde_json::from_value(json).unwrap();
    assert_eq!(
        run(&compile(&off), 6, 0..300).to_bytes(),
        run(&compile(&back), 6, 0..300).to_bytes()
    );
    // Without long-term care in the model there is nothing for Medicaid to pay.
    let mut no_ltc = widow_78(false);
    no_ltc.spending.healthcare.long_term_care = false;
    let mut no_ltc_on = no_ltc.clone();
    no_ltc_on.spending.healthcare.medicaid = true;
    assert_eq!(
        run(&compile(&no_ltc), 6, 0..300).to_bytes(),
        run(&compile(&no_ltc_on), 6, 0..300).to_bytes()
    );
}

#[test]
fn medicaid_pays_for_care_once_savings_are_spent_down() {
    let (off, on) = (widow_78(false), widow_78(true));
    let (plan_off, plan_on) = (compile(&off), compile(&on));
    let n = 3_000;
    let (a, b) = (run(&plan_off, 6, 0..n), run(&plan_on, 6, 0..n));
    let f = |acc: &Accumulator, s: PathScalar| -> Vec<f64> {
        column(acc, s).iter().map(|&v| f64::from(v)).collect()
    };
    let (dep_a, dep_b) = (
        f(&a, PathScalar::DepletionAge),
        f(&b, PathScalar::DepletionAge),
    );
    let medicaid = f(&b, PathScalar::MedicaidAge);
    assert!(f(&a, PathScalar::MedicaidAge).iter().all(|v| v.is_nan()));
    let count = |x: &[f64]| x.iter().filter(|v| !v.is_nan()).count();
    let (da, db, m) = (count(&dep_a), count(&dep_b), count(&medicaid));
    // Care is paid privately until savings are gone, so Medicaid pays only on
    // paths that would otherwise have run out, and on those the safety net
    // replaces depletion.
    assert!(m > n as usize / 20, "{m}");
    assert!(db < da / 2, "depleted {da} -> {db}");
    // Paths on which Medicaid never paid are the same path, bit for bit.
    let (ltc_a, ltc_b) = (
        f(&a, PathScalar::LongTermCareCost),
        f(&b, PathScalar::LongTermCareCost),
    );
    let mut before = 0;
    for k in 0..n as usize {
        if medicaid[k].is_nan() {
            assert_eq!(dep_a[k].to_bits(), dep_b[k].to_bits(), "path {k}");
            assert_eq!(ltc_a[k].to_bits(), ltc_b[k].to_bits(), "path {k}");
        } else {
            // The household pays less for care, and the path is one that ran out
            // without Medicaid (or, rarely, one where the last of the savings paid
            // the final month of care).
            assert!(ltc_b[k] < ltc_a[k], "path {k}");
            before += usize::from(!dep_a[k].is_nan() && medicaid[k] >= dep_a[k] - 1.0);
        }
    }
    assert!(before as f64 >= 0.95 * m as f64, "{before} of {m}");
    let s = plan_on.summarize(&b);
    assert!((s.headline.medicaid_rate.value - m as f64 / n as f64).abs() < 1e-4);
    assert_eq!(plan_off.summarize(&a).headline.medicaid_rate.value, 0.0);
}

#[test]
fn medicaid_recovers_its_costs_from_the_estate() {
    let plan = compile(&widow_78(true));
    // Full cost of care in today's dollars on the same paths without Medicaid
    // (`LongTermCareCost` counts care before any cut): what Medicaid paid, and so
    // what it can recover, is part of it.
    let full = column(
        &run(&compile(&widow_78(false)), 6, 0..300),
        PathScalar::LongTermCareCost,
    );
    let (mut traced, mut recovered) = (0, 0);
    let traces: Vec<_> = (0..300)
        .map(|path| sim::trace_path(&plan, 6, path))
        .collect();
    // Paths that reach the horizon end with her alive: nothing is recovered yet.
    let horizon = traces.iter().map(|t| t.months).max().unwrap();
    for (path, trace) in traces.iter().enumerate() {
        let events: Vec<_> = trace
            .events
            .iter()
            .filter(|e| e.kind == firesight_core::stats::TraceEventKind::Medicaid)
            .collect();
        if events.is_empty() || trace.months == horizon {
            continue;
        }
        traced += 1;
        // Medicaid starts paying, and after her death the state takes back what it
        // paid from the home: something, and no more than the care cost.
        let recovery = events
            .iter()
            .find(|e| e.label.contains("estate recovery"))
            .and_then(|e| e.amount);
        if let Some(x) = recovery {
            recovered += 1;
            let cap = f64::from(full[path]);
            assert!(x > 0.0 && x <= cap, "path {path}: {x} of {cap}");
        }
    }
    assert!(traced > 5, "{traced}");
    // She is single, so every Medicaid path that ends with her death ends with
    // recovery from the home.
    assert_eq!(recovered, traced);
}

#[test]
fn a_spouse_at_home_keeps_the_protected_savings() {
    // A couple of 80 and 78, $250,000 in cash, Social Security covering the budget.
    let mut a = person("al", 80, Sex::Male);
    let mut b = person("bea", 78, Sex::Female);
    for who in [&mut a, &mut b] {
        who.social_security.basis = SocialSecurityBasis::KnownBenefit {
            monthly_at_fra: 2_600.0,
        };
        who.social_security.claim_age = 67.0;
    }
    let mut p = quiet(vec![a, b], 100.0);
    p.assumptions.longevity = Longevity::default();
    p.assumptions.health_risk = true;
    p.spending.healthcare = Healthcare {
        medicaid: true,
        ..Healthcare::default()
    };
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 250_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.spending_rule = SpendingRule::ConstantReal;
    let plan = compile(&p);
    let medicaid = firesight_core::stats::TraceEventKind::Medicaid;
    let died = firesight_core::stats::TraceEventKind::Died;
    let mut checked = 0;
    for path in 0..400 {
        let trace = sim::trace_path(&plan, 8, path);
        let Some(first) = trace.events.iter().find(|e| e.kind == medicaid) else {
            continue;
        };
        // Skip paths where a partner had died or both were in care by then (no
        // spouse at home).
        let ltc = firesight_core::stats::TraceEventKind::LongTermCare;
        let in_care = trace
            .events
            .iter()
            .filter(|e| e.kind == ltc && e.month <= first.month)
            .count();
        if in_care > 1
            || trace
                .events
                .iter()
                .any(|e| e.kind == died && e.month <= first.month)
        {
            continue;
        }
        // Both alive and one in care: the couple still holds the protected
        // allowance (at least $32,532 in today's dollars, less one month's
        // spending) when Medicaid starts paying, not the $2,000 a single keeps.
        let k = (first.month - trace.start) as usize;
        let cash: f64 = [
            "balance_cash",
            "balance_taxable",
            "balance_traditional",
            "balance_roth",
            "balance_hsa",
        ]
        .iter()
        .filter_map(|key| trace.column(key))
        .map(|c| c[k])
        .sum();
        assert!(cash > 32_532.0 - 6_000.0, "path {path}: {cash}");
        checked += 1;
    }
    assert!(checked > 3, "{checked}");
}

fn insured(mut p: Profile, policy: LtcInsurance) -> Profile {
    p.household.people[0].ltc_insurance = Some(policy);
    p
}

fn policy_3y() -> LtcInsurance {
    LtcInsurance {
        annual_premium: 3_000.0,
        daily_benefit: 200.0,
        benefit_years: 3.0,
        elimination_days: 90.0,
        inflation_rider: InflationRider::None,
        premium_increase_risk: true,
    }
}

#[test]
fn ltc_insurance_off_is_the_default_bit_for_bit() {
    let base = retiree_70(Sex::Female);
    let json = serde_json::to_value(&base).unwrap();
    assert!(json["household"]["people"][0]
        .get("ltc_insurance")
        .is_none());
    // With long-term care off a policy has nothing to pay and charges nothing.
    let mut no_ltc = base.clone();
    no_ltc.spending.healthcare.long_term_care = false;
    assert_eq!(
        run(&compile(&no_ltc), 2, 0..300).to_bytes(),
        run(&compile(&insured(no_ltc.clone(), policy_3y())), 2, 0..300).to_bytes()
    );
}

#[test]
fn ltc_insurance_pays_for_care_and_costs_premiums() {
    let base = retiree_70(Sex::Female);
    let (plan_a, plan_b) = (compile(&base), compile(&insured(base.clone(), policy_3y())));
    let n = 3_000;
    let (a, b) = (run(&plan_a, 2, 0..n), run(&plan_b, 2, 0..n));
    let f = |acc: &Accumulator, s: PathScalar| -> Vec<f64> {
        column(acc, s).iter().map(|&v| f64::from(v)).collect()
    };
    // Same lives: the policy's draws come from their own stream.
    assert_eq!(
        column(&a, PathScalar::DeathAge0),
        column(&b, PathScalar::DeathAge0)
    );
    let (ca, cb) = (
        f(&a, PathScalar::LongTermCareCost),
        f(&b, PathScalar::LongTermCareCost),
    );
    let mut longer = 0;
    for k in 0..n as usize {
        // Benefits only ever lower what the household pays for care.
        assert!(cb[k] <= ca[k] + 1e-3, "path {k}: {} -> {}", ca[k], cb[k]);
        if ca[k] > 0.0 && cb[k] < ca[k] {
            longer += 1;
        }
    }
    assert!(longer > n as usize / 5, "{longer}");
    // A pool of 3 x 365 x $200 = $219,000 nominal against care costing about
    // $5,000 to $11,000 a month: most of a typical episode, little of a long one.
    let (ma, mb) = (mean(&ca), mean(&cb));
    assert!(mb < 0.8 * ma && mb > 0.2 * ma, "care paid {ma} -> {mb}");
    // Premiums: $3,000 a year from 70 until a claim or death, more after rate
    // increases; lifetime spending rises by roughly that.
    let (sa, sb) = (
        mean(&f(&a, PathScalar::LifetimeSpending)),
        mean(&f(&b, PathScalar::LifetimeSpending)),
    );
    let years = mean(&f(&a, PathScalar::DeathAge0)) - 70.0;
    let premiums = sb - sa + (ma - mb);
    assert!(
        premiums > 0.7 * 3_000.0 * years && premiums < 2.0 * 3_000.0 * years,
        "premiums {premiums} over {years} years"
    );
}

#[test]
fn ltc_premiums_rise_and_unaffordable_policies_lapse() {
    let plan = compile(&insured(retiree_70(Sex::Female), policy_3y()));
    let kind = firesight_core::stats::TraceEventKind::LtcInsurance;
    let (mut raised, mut paying) = (0, 0);
    for path in 0..300 {
        let trace = sim::trace_path(&plan, 2, path);
        let labels: Vec<&str> = trace
            .events
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.label.as_str())
            .collect();
        raised += usize::from(labels.iter().any(|l| l.contains("raised")));
        paying += usize::from(labels.iter().any(|l| l.contains("starts paying")));
        // She has $3 million: never unaffordable.
        assert!(!labels.iter().any(|l| l.contains("lapses")), "path {path}");
    }
    // 8% a year: over roughly 17 years most paths see an increase; about half the
    // women of 70 use paid care, most for longer than 90 days.
    assert!(raised > 150 && raised < 300, "{raised}");
    assert!(paying > 60 && paying < 200, "{paying}");

    // A widow with $20,000 and $1,500 a month of Social Security paying $5,000 a
    // year: more than 10% of her income plus 4% of her savings, so the policy
    // lapses at its first anniversary and never pays.
    let mut poor = retiree_70(Sex::Female);
    poor.household.people[0].social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 1_500.0,
    };
    poor.accounts = vec![account("cash", 0, AccountKind::Cash, 20_000.0)];
    poor.spending.items = vec![item("living", Category::Other, 1_000.0, true)];
    let plan = compile(&insured(
        poor,
        LtcInsurance {
            annual_premium: 5_000.0,
            ..policy_3y()
        },
    ));
    let mut lapsed = 0;
    let ltc = firesight_core::stats::TraceEventKind::LongTermCare;
    for path in 0..100 {
        let trace = sim::trace_path(&plan, 2, path);
        let events: Vec<_> = trace.events.iter().filter(|e| e.kind == kind).collect();
        let care_by_12 = trace
            .events
            .iter()
            .any(|e| e.kind == ltc && e.month - trace.start <= 12);
        if let Some(e) = events.iter().find(|e| e.label.contains("lapses")) {
            assert_eq!(e.month - trace.start, 12, "path {path}");
            // A lapsed policy never pays.
            assert!(
                !events.iter().any(|e| e.label.contains("starts paying")),
                "path {path}"
            );
            lapsed += 1;
        } else {
            // Only a policy already claimed on (premiums waived) or a life over
            // before the anniversary escapes the lapse.
            assert!(care_by_12 || trace.months <= 12, "path {path}");
        }
    }
    assert!(lapsed > 85, "{lapsed}");

    // A retiree with $3,000 a month of Social Security and $100,000: her
    // resources are 36,000 + 4,000 a year, so a $2,500 premium (6%) is
    // affordable and never lapses (her benefit counts as income, not only wages).
    let mut modest = retiree_70(Sex::Female);
    modest.household.people[0].social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 3_000.0,
    };
    modest.accounts = vec![account("cash", 0, AccountKind::Cash, 100_000.0)];
    modest.spending.items = vec![item("living", Category::Other, 1_500.0, true)];
    let plan = compile(&insured(
        modest,
        LtcInsurance {
            annual_premium: 2_500.0,
            premium_increase_risk: false,
            ..policy_3y()
        },
    ));
    for path in 0..60 {
        let trace = sim::trace_path(&plan, 2, path);
        let lapse = trace
            .events
            .iter()
            .find(|e| e.kind == kind && e.label.contains("lapses"));
        // Only running short of essentials (a long care episode after the
        // benefits) could make it lapse, and never in the first years.
        if let Some(e) = lapse {
            assert!(e.month - trace.start > 36, "path {path}");
        }
    }
}

/// Events of the given kinds on a traced path, as (month, label).
fn events_of(
    plan: &sim::Plan,
    path: u64,
    kinds: &[firesight_core::stats::TraceEventKind],
) -> Vec<(YearMonth, String)> {
    sim::trace_path(plan, 8, path)
        .events
        .into_iter()
        .filter(|e| kinds.contains(&e.kind))
        .map(|e| (e.month, e.label))
        .collect()
}

#[test]
fn the_features_draw_only_from_their_own_streams() {
    use firesight_core::stats::TraceEventKind as K;
    // Every feature on except the two that change lifetimes by design.
    let mut on = couple(CoupleDependence::Independent);
    on.accounts = vec![account("brokerage", 0, AccountKind::Taxable, 5_000_000.0)];
    on.spending.healthcare = Healthcare::default();
    on.assumptions.health_risk = true;
    let off = on.clone();
    on.spending.healthcare.medicaid = true;
    for person in &mut on.household.people {
        person.ltc_insurance = Some(policy_3y());
    }
    let mut states = on.clone();
    states.assumptions.health_model = HealthModelKind::States;
    let (plan_off, plan_on, plan_states) = (compile(&off), compile(&on), compile(&states));
    let mut care = 0;
    for path in 0..40 {
        let (a, b, c) = (
            sim::trace_path(&plan_off, 8, path),
            sim::trace_path(&plan_on, 8, path),
            sim::trace_path(&plan_states, 8, path),
        );
        // Same lives, same markets and prices: switching the features on touches
        // none of those draws.
        assert_eq!(a.months, b.months, "path {path}");
        assert_eq!(a.months, c.months, "path {path}");
        let prices = a.column("price_level");
        assert_eq!(prices, b.column("price_level"), "path {path}");
        assert_eq!(prices, c.column("price_level"), "path {path}");
        // Medicaid and insurance leave the care episodes (the health stream) as
        // they were. Health states reweight an episode's probability by the state
        // at death, by design, so they are not compared here.
        let starts = |t: &firesight_core::stats::PathTrace| -> Vec<YearMonth> {
            t.events
                .iter()
                .filter(|e| e.kind == K::LongTermCare)
                .map(|e| e.month)
                .collect()
        };
        assert_eq!(starts(&a), starts(&b), "path {path}");
        care += usize::from(!starts(&a).is_empty());
    }
    assert!(care > 5, "{care}");

    // A household decision (spending half as much again) leaves the features'
    // own draws where they were: the health-state path and the premium increases
    // (no policy lapses with $5 million).
    let (on, plan_on) = (states, plan_states);
    let mut richer = on.clone();
    richer.spending.items = vec![item("living", Category::Other, 9_000.0, true)];
    let plan_richer = compile(&richer);
    let kinds = [K::HealthState, K::LtcInsurance, K::LongTermCare];
    let mut raised = 0;
    for path in 0..40 {
        let (a, b) = (
            events_of(&plan_on, path, &kinds),
            events_of(&plan_richer, path, &kinds),
        );
        assert_eq!(a, b, "path {path}");
        raised += usize::from(a.iter().any(|(_, l)| l.contains("raised")));
    }
    assert!(raised > 5, "{raised}");

    // Linked lifespans: the couple stream is drawn whatever the household does.
    let mut linked = on.clone();
    linked.assumptions.longevity = Longevity::Stochastic {
        improvement: true,
        couple_dependence: CoupleDependence::Linked,
        income_gradient: false,
        income_percentile: None,
    };
    let mut linked_richer = linked.clone();
    linked_richer.spending.items = richer.spending.items.clone();
    let (x, y) = (
        run(&compile(&linked), 8, 0..300),
        run(&compile(&linked_richer), 8, 0..300),
    );
    for s in [PathScalar::DeathAge0, PathScalar::DeathAge1] {
        assert_eq!(column(&x, s), column(&y, s));
    }
}
