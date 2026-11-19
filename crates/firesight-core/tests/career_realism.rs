//! Career and income realism: five opt-in extensions (`career_realism`). Each section
//! checks one feature on paired paths (same seed, same path index, so the same
//! markets, careers and lifespans) against the baseline with the feature off, and
//! the last tests run every feature at once under the monthly conservation checks
//! and the finite-value checks.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;

const SEED: u64 = 77;

/// One worker aged `age`, `salary` a year on the typical lifecycle curve, a large cash
/// buffer so nothing else matters, retiring at `retire_at`. Career risk is on at the
/// job's `level`.
fn worker(age: i32, salary: f64, level: RiskLevel, retire_at: f64) -> Profile {
    let mut who = person("w", age, Sex::Unspecified);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = quiet(vec![who], 95.0);
    p.assumptions.career_risk = true;
    let mut income = job("job", 0, salary);
    if let IncomeKind::Job(j) = &mut income.kind {
        j.growth = WageGrowth::Lifecycle {
            profile: LifecycleProfile::Typical,
            extra: 0.0,
        };
        j.risk.level = level;
    }
    p.incomes = vec![income];
    p.spending.items = vec![item("living", Category::Other, 2_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 50_000.0)];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: retire_at,
    };
    p
}

fn job_risk(p: &mut Profile) -> &mut CareerRisk {
    match &mut p.incomes[0].kind {
        IncomeKind::Job(j) => &mut j.risk,
        _ => unreachable!(),
    }
}

/// Wages by calendar year (today's dollars, whole dollars) of one traced path.
fn wages(profile: &Profile, path: u64) -> Vec<f64> {
    let plan = compile(profile);
    let trace = sim::trace_path(&plan, SEED, path);
    trace
        .column("income_wages")
        .map(<[f64]>::to_vec)
        .unwrap_or_default()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn sd(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

fn count_events(profile: &Profile, path: u64, text: &str) -> usize {
    let plan = compile(profile);
    sim::trace_path(&plan, SEED, path)
        .events
        .iter()
        .filter(|e| e.label.contains(text))
        .count()
}

// ---------------------------------------------------------------------------
// F16: job changes with pay jumps
// ---------------------------------------------------------------------------

#[test]
fn f16_default_and_explicit_off_are_identical() {
    let base = worker(30, 80_000.0, RiskLevel::Typical, 50.0);
    let mut off = base.clone();
    job_risk(&mut off).job_mobility = JobMobility::Off;
    for path in 0..10 {
        assert_eq!(wages(&base, path), wages(&off, path));
    }
    // A profile JSON written before the field existed still loads and is the same.
    // The field is not written while it is off, so existing profiles serialise as
    // they always did.
    let json = serde_json::to_string(&base).unwrap();
    assert!(!json.contains("job_mobility"));
    let back: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(back, base);
}

#[test]
fn f16_mobility_needs_career_risk_and_a_risk_level() {
    // With the risk level at none, or career risk switched off, the setting is inert.
    let mut none = worker(30, 80_000.0, RiskLevel::None, 50.0);
    let reference = wages(&none, 3);
    job_risk(&mut none).job_mobility = JobMobility::High;
    assert_eq!(wages(&none, 3), reference);
    let mut off = worker(30, 80_000.0, RiskLevel::Typical, 50.0);
    off.assumptions.career_risk = false;
    let reference = wages(&off, 3);
    job_risk(&mut off).job_mobility = JobMobility::High;
    assert_eq!(wages(&off, 3), reference);
}

#[test]
fn f16_moves_keep_the_average_career_on_the_curve_and_spread_careers_out() {
    let off = worker(30, 80_000.0, RiskLevel::Typical, 50.0);
    let mut on = off.clone();
    job_risk(&mut on).job_mobility = JobMobility::Typical;

    let paths = 300;
    let mut lifetime_off = Vec::new();
    let mut lifetime_on = Vec::new();
    let mut final_off = Vec::new();
    let mut final_on = Vec::new();
    let mut moves = Vec::new();
    for path in 0..paths {
        let a = wages(&off, path);
        let b = wages(&on, path);
        lifetime_off.push(a.iter().sum::<f64>());
        lifetime_on.push(b.iter().sum::<f64>());
        // Average monthly pay over the last five working years (age 45 to 50; the
        // household retires in month 240).
        let tail = |w: &[f64]| mean(&w[180..240]);
        final_off.push(tail(&a));
        final_on.push(tail(&b));
        moves.push(count_events(&on, path, "Changed jobs") as f64);
    }
    // The mean career is the curve: lifetime pay differs by well under 3%.
    let ratio = mean(&lifetime_on) / mean(&lifetime_off);
    assert!((ratio - 1.0).abs() < 0.03, "lifetime wage ratio {ratio}");
    // Careers spread out: pay in the last five years is clearly more dispersed.
    let spread = sd(&final_on) / sd(&final_off);
    assert!(spread > 1.10, "dispersion ratio {spread}");
    // Hand count: years 30-34 at -ln(0.8) = 0.223 a year, 35-44 at 0.128, 45-49 at
    // 0.083 gives 1.12 + 1.28 + 0.42 = 2.8 moves for someone employed throughout;
    // layoffs and recessions bring that down a little.
    let avg_moves = mean(&moves);
    assert!((2.0..3.2).contains(&avg_moves), "moves {avg_moves}");
}

#[test]
fn f16_movers_end_up_ahead_of_stayers_on_the_same_paths() {
    let off = worker(30, 80_000.0, RiskLevel::Typical, 50.0);
    let mut on = off.clone();
    job_risk(&mut on).job_mobility = JobMobility::High;
    let (mut hoppers, mut stayers) = (Vec::new(), Vec::new());
    for path in 0..300 {
        let n = count_events(&on, path, "Changed jobs");
        // Ratio of end pay to the same path without mobility: the job ladder.
        let a = wages(&off, path);
        let b = wages(&on, path);
        let (ta, tb) = (mean(&a[216..240]), mean(&b[216..240]));
        if ta <= 0.0 {
            continue;
        }
        if n >= 5 {
            hoppers.push(tb / ta);
        } else if n <= 2 {
            stayers.push(tb / ta);
        }
    }
    assert!(hoppers.len() > 20 && stayers.len() > 20);
    let (h, s) = (mean(&hoppers), mean(&stayers));
    // Movers gain on the curve, stayers lose against it, as the deflator intends.
    assert!(h > s + 0.15, "hoppers {h:.3} stayers {s:.3}");
    assert!(h > 1.0 && s < 1.0, "hoppers {h:.3} stayers {s:.3}");
}

#[test]
fn f16_a_move_forfeits_unvested_equity() {
    let mut p = worker(30, 120_000.0, RiskLevel::Typical, 40.0);
    if let IncomeKind::Job(j) = &mut p.incomes[0].kind {
        j.equity = vec![EquityGrant {
            id: "rsu".into(),
            unvested_value: 400_000.0,
            vest_months_remaining: 120,
            ..EquityGrant::default()
        }];
    }
    let off = p.clone();
    job_risk(&mut p).job_mobility = JobMobility::High;
    let total = |profile: &Profile, path: u64| -> (f64, usize) {
        let plan = compile(profile);
        let trace = sim::trace_path(&plan, SEED, path);
        let equity = trace
            .column("income_wages")
            .map_or(0.0, |c| c.iter().sum::<f64>());
        let n = trace
            .events
            .iter()
            .filter(|e| e.label.contains("Changed jobs"))
            .count();
        (equity, n)
    };
    let (mut with, mut without) = (0.0, 0.0);
    let mut paths_with_move = 0;
    for path in 0..60 {
        let (a, _) = total(&off, path);
        let (b, n) = total(&p, path);
        if n > 0 {
            paths_with_move += 1;
            with += b;
            without += a;
        }
    }
    assert!(paths_with_move > 40);
    // Wages include vesting RSUs; forfeiting the unvested part on a move costs more
    // than the pay jumps add back over ten years on the same paths.
    assert!(with < without, "with {with:.0} without {without:.0}");
}

// ---------------------------------------------------------------------------
// Conservation and finite values (every feature-enabled profile goes through here)
// ---------------------------------------------------------------------------

/// Runs `paths` paths with the monthly conservation checks on, then summarises them and
/// traces a few, and checks that nothing is NaN or infinite.
fn conserves_and_is_finite(profile: &Profile, paths: u64) {
    for kind in MARKETS {
        let p = with_market(profile.clone(), kind);
        let plan = compile(&p);
        let acc = run_checked(&plan, 5, 0..paths);
        let summary = plan.summarize(&acc);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
        assert!(summary.headline.median_terminal_net_worth.is_finite());
        for path in 0..4 {
            let trace = sim::trace_path(&plan, 5, path);
            let json = serde_json::to_string(&trace).unwrap();
            assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
        }
    }
}

#[test]
fn f16_profile_conserves_money_and_stays_finite() {
    let mut p = worker(30, 120_000.0, RiskLevel::Typical, 55.0);
    if let IncomeKind::Job(j) = &mut p.incomes[0].kind {
        j.risk.job_mobility = JobMobility::High;
        j.bonus = Some(Bonus::default());
        j.equity = vec![EquityGrant {
            id: "rsu".into(),
            unvested_value: 200_000.0,
            vest_months_remaining: 48,
            refresh_annual_value: 40_000.0,
            sell_on_vest: false,
            ..EquityGrant::default()
        }];
        j.plan = Some(EmployerPlan::default());
    }
    conserves_and_is_finite(&p, 150);
}
