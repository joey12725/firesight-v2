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
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / v.len() as f64).sqrt()
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
// F17: recession pay freezes, hours cuts, smaller bonuses
// ---------------------------------------------------------------------------

fn set_recession_pay(p: &mut Profile, on: RecessionPay) {
    job_risk(p).recession_pay = on;
}

/// First recession month within the first `months` months of path `path`.
fn first_recession_month(profile: &Profile, path: u64, months: usize) -> Option<usize> {
    let plan = compile(profile);
    let mut ws = sim::Workspace::new(&plan);
    let mut acc = plan.new_accumulator();
    sim::run_path(&plan, SEED, path, &mut ws, &mut acc);
    ws.economy.recession[..months].iter().position(|r| *r != 0)
}

#[test]
fn f17_off_is_identical_to_the_baseline() {
    let base = worker(35, 90_000.0, RiskLevel::Typical, 55.0);
    let mut off = base.clone();
    set_recession_pay(&mut off, RecessionPay::Off);
    for path in 0..10 {
        assert_eq!(wages(&base, path), wages(&off, path));
    }
    assert!(!serde_json::to_string(&base)
        .unwrap()
        .contains("recession_pay"));
}

#[test]
fn f17_nothing_changes_before_the_first_recession_and_pay_never_rises() {
    let mut base = worker(35, 90_000.0, RiskLevel::Typical, 55.0);
    if let IncomeKind::Job(j) = &mut base.incomes[0].kind {
        j.bonus = Some(Bonus {
            target_fraction: 0.10,
            volatility: 0.0,
            month: 3,
        });
    }
    let mut on = base.clone();
    set_recession_pay(&mut on, RecessionPay::Typical);
    let (mut off_total, mut on_total) = (0.0, 0.0);
    let mut late_first = 0;
    for path in 0..300 {
        let a = wages(&base, path);
        let b = wages(&on, path);
        assert_eq!(a.len(), b.len());
        let first = first_recession_month(&base, path, 240).unwrap_or(240);
        // The feature cannot act before a recession month: identical pay until then.
        assert_eq!(a[..first], b[..first], "path {path}");
        if first >= 36 {
            late_first += 1;
        }
        for (x, y) in a.iter().zip(&b) {
            // Whole-dollar rounding in the trace.
            assert!(*y <= *x + 1.0, "path {path}: {y} > {x}");
        }
        off_total += a.iter().sum::<f64>();
        on_total += b.iter().sum::<f64>();
    }
    assert!(late_first > 30, "{late_first} paths start calm");
    // Lifetime pay falls by a few tenths of a percent to a few percent: a 25% chance
    // of losing a ~4% raise and a 15% chance of a ~12% cut for nine months in each
    // recession year, with recessions in about 13% of months, plus bonuses.
    let loss = 1.0 - on_total / off_total;
    assert!((0.002..0.06).contains(&loss), "loss over all paths {loss}");
}

#[test]
fn f17_recession_pay_does_not_depend_on_other_career_draws() {
    // A freeze or cut changes pay but never the layoff timing: the same months are
    // laid off with the feature on and off (it draws from its own stream).
    let base = worker(35, 90_000.0, RiskLevel::High, 55.0);
    let mut on = base.clone();
    set_recession_pay(&mut on, RecessionPay::Typical);
    for path in 0..40 {
        let laid = |p: &Profile| -> Vec<i32> {
            let plan = compile(p);
            sim::trace_path(&plan, SEED, path)
                .events
                .iter()
                .filter(|e| e.label.starts_with("Laid off"))
                .map(|e| e.month - plan.start())
                .collect()
        };
        assert_eq!(laid(&base), laid(&on), "path {path}");
    }
}

// ---------------------------------------------------------------------------
// F18: long-term disability, SSDI and LTD
// ---------------------------------------------------------------------------

/// A worker with a Social Security record built from earnings, earning a flat $90,000
/// (no career risk, so only the disability feature can interrupt the pay).
fn disability_worker(age: i32, retire_at: f64) -> Profile {
    let mut p = worker(age, 90_000.0, RiskLevel::None, retire_at);
    p.household.people[0].social_security.basis = SocialSecurityBasis::FromEarnings {
        career_start_age: 22.0,
    };
    p.assumptions.career_risk = false;
    p
}

fn disabled(p: &mut Profile, ltd: Option<LtdInsurance>) {
    p.household.people[0].disability_risk = DisabilityRisk::Typical;
    p.household.people[0].ltd_insurance = ltd;
}

/// A monthly series of one traced path (today's dollars, whole dollars); an all-zero
/// series is not stored by the trace, so it comes back as zeros.
fn column(profile: &Profile, path: u64, name: &str) -> Vec<f64> {
    let plan = compile(profile);
    let trace = sim::trace_path(&plan, SEED, path);
    let n = trace.column("income_wages").map_or(0, <[f64]>::len);
    trace
        .column(name)
        .map_or_else(|| vec![0.0; n], <[f64]>::to_vec)
}

/// Month index (from the start) of the person's first disability on a path, and
/// whether they ever recover.
fn onset_month(profile: &Profile, path: u64) -> Option<(usize, bool)> {
    let plan = compile(profile);
    let trace = sim::trace_path(&plan, SEED, path);
    let first = trace
        .events
        .iter()
        .find(|e| e.label.contains("becomes disabled"))?;
    let recovered = trace.events.iter().any(|e| e.label.contains("recovers"));
    Some(((first.month - plan.start()) as usize, recovered))
}

#[test]
fn f18_off_is_identical_and_not_serialised() {
    let base = disability_worker(40, 62.0);
    let mut off = base.clone();
    off.household.people[0].disability_risk = DisabilityRisk::Off;
    for path in 0..6 {
        assert_eq!(wages(&base, path), wages(&off, path));
    }
    let json = serde_json::to_string(&base).unwrap();
    assert!(!json.contains("disability_risk") && !json.contains("ltd_insurance"));
    let back: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(back, base);
}

#[test]
fn f18_onset_frequency_matches_the_hazard_table() {
    // Age 35 to 60 (retirement at 60): the yearly incidences sum to
    // 5 * (2.2 + 3.6 + 5.5 + 8.0 + 12.0) = 156.5 per 1,000, so about 14.5% of workers
    // have an onset (1 - exp(-0.1565) = 0.145), a little fewer after deaths.
    let mut p = disability_worker(35, 60.0);
    disabled(&mut p, None);
    let paths = 600;
    let hit = (0..paths).filter(|k| onset_month(&p, *k).is_some()).count();
    let share = hit as f64 / paths as f64;
    assert!((0.10..0.19).contains(&share), "share with an onset {share}");
}

#[test]
fn f18_disability_ends_wages_and_pays_ssdi_frozen_then_converts_at_full_retirement_age() {
    // Aged 45 at the start; full retirement age 67 is month 264.
    let base = disability_worker(45, 62.0);
    let mut on = base.clone();
    disabled(&mut on, None);
    let mut checked = 0;
    for path in 0..800 {
        let Some((m, recovered)) = onset_month(&on, path) else {
            continue;
        };
        if recovered || !(24..=150).contains(&m) {
            continue;
        }
        let w_on = column(&on, path, "income_wages");
        let w_off = column(&base, path, "income_wages");
        let ss = column(&on, path, "income_social_security");
        // Pay is the baseline's until the onset month, nothing from then on.
        assert_eq!(w_on[..m], w_off[..m], "path {path}");
        assert!(w_on[m..].iter().all(|w| *w == 0.0), "path {path}");
        assert!(w_off[m] > 5_000.0);
        // SSDI: nothing during the five-month wait, then a constant real amount (the
        // record is frozen, later years without earnings do not lower it) up to full
        // retirement age, and the same amount as the retirement benefit after it.
        assert!(ss[..m + 5].iter().all(|x| *x == 0.0), "path {path}");
        let level = ss[m + 5];
        assert!(level > 800.0 && level < 5_000.0, "path {path}: {level}");
        let held = ss[m + 5..ss.len().min(300)].iter().enumerate();
        for (k, x) in held {
            assert!(
                (x - level).abs() <= 0.002 * level + 2.0,
                "path {path} month {}: {x} vs {level}",
                m + 5 + k
            );
        }
        checked += 1;
        if checked == 5 {
            break;
        }
    }
    assert!(checked >= 3, "only {checked} suitable paths");
}

#[test]
fn f18_ssdi_uses_the_disability_computation_years_not_thirty_five() {
    // A 30-year-old with eight years of work behind them on $90,000: the retirement
    // formula divides the best 35 years by 35 and would give about a third of the
    // benefit (AIME about 1,700, PIA about 1,100 a month); the disability rule
    // divides by the computation years (elapsed years less dropouts), so the PIA is
    // that of a full-time earner of $90,000, about 3,000 a month.
    let base = disability_worker(30, 60.0);
    let mut on = base.clone();
    disabled(&mut on, None);
    let mut seen = 0;
    for path in 0..2000 {
        let Some((m, recovered)) = onset_month(&on, path) else {
            continue;
        };
        if recovered || m > 150 {
            continue;
        }
        let ss = column(&on, path, "income_social_security");
        let level = ss[m + 6];
        assert!(level > 2_000.0 && level < 4_500.0, "path {path}: {level}");
        seen += 1;
        if seen == 3 {
            break;
        }
    }
    assert!(seen >= 1);
}

#[test]
fn f18_ltd_adds_income_offset_by_ssdi() {
    let mut no_ltd = disability_worker(45, 62.0);
    disabled(&mut no_ltd, None);
    let mut ltd = disability_worker(45, 62.0);
    disabled(
        &mut ltd,
        Some(LtdInsurance {
            replacement: 0.6,
            to_age: 65.0,
        }),
    );
    let mut checked = 0;
    for path in 0..800 {
        let Some((m, recovered)) = onset_month(&ltd, path) else {
            continue;
        };
        if recovered || !(24..=150).contains(&m) {
            continue;
        }
        let other_with = column(&ltd, path, "income_other");
        let other_without = column(&no_ltd, path, "income_other");
        let ss = column(&ltd, path, "income_social_security");
        let pay_before = column(&ltd, path, "income_wages")[m - 1];
        // The policy pays nothing before the three-month elimination period ends.
        assert_eq!(other_with[..m + 3], other_without[..m + 3]);
        assert!(other_with[m + 3] > other_without[m + 3] + 1.0);
        // The first LTD month: 60% of the pay before (a little under if prices
        // rose since the onset, LTD being fixed in nominal dollars).
        let first = other_with[m + 3] - other_without[m + 3];
        assert!(
            first > 0.55 * pay_before && first < 0.62 * pay_before,
            "path {path}: ltd {first} vs pay {pay_before}"
        );
        // When SSDI starts two months later, LTD falls by about the SSDI amount, so
        // that the two together stay near the same replacement.
        assert!(ss[m + 5] > 800.0);
        let together = other_with[m + 5] - other_without[m + 5] + ss[m + 5];
        assert!(
            together > 0.9 * first && together < 1.05 * first,
            "path {path}: ltd+ssdi {together} vs first LTD {first}"
        );
        assert!(other_with[m + 5] - other_without[m + 5] < 0.8 * first);
        // Benefits stop at 65 (month 240).
        assert_eq!(other_with[250], other_without[250]);
        checked += 1;
        if checked == 4 {
            break;
        }
    }
    assert!(checked >= 2, "only {checked} suitable paths");
}

#[test]
fn f18_recovery_returns_to_work_with_a_scar_and_releases_the_record() {
    let base = disability_worker(30, 62.0);
    let mut on = base.clone();
    disabled(&mut on, None);
    let mut seen = 0;
    for path in 0..6000 {
        let plan = compile(&on);
        let trace = sim::trace_path(&plan, SEED, path);
        let Some(down) = trace
            .events
            .iter()
            .find(|e| e.label.contains("becomes disabled"))
        else {
            continue;
        };
        let Some(up) = trace.events.iter().find(|e| e.label.contains("recovers")) else {
            continue;
        };
        let (d, u) = (
            (down.month - plan.start()) as usize,
            (up.month - plan.start()) as usize,
        );
        if u + 12 >= 12 * 30 || u < d + 8 {
            continue;
        }
        let w = column(&on, path, "income_wages");
        let before = w[d - 1];
        assert!(w[d..u].iter().all(|x| *x == 0.0), "path {path}");
        // Back at work after the idle spell; pay is the baseline's less the 10% scar
        // (about 9.5% in levels) within the noise of the other draws.
        let back = w[u + 2];
        assert!(back > 0.0, "path {path}");
        let off = column(&base, path, "income_wages");
        let ratio = back / off[u + 2];
        assert!((0.88..0.93).contains(&ratio), "path {path}: {ratio}");
        assert!(before > 0.0);
        // SSDI stops on recovery.
        let ss = column(&on, path, "income_social_security");
        assert!(ss[u + 2] == 0.0, "path {path}");
        seen += 1;
        if seen == 3 {
            break;
        }
    }
    assert!(seen >= 1, "no recovering path found");
}

#[test]
fn f18_lowers_outcomes_in_the_expected_direction() {
    let off = disability_worker(40, 62.0);
    let mut on = off.clone();
    disabled(&mut on, None);
    let (mut off_total, mut on_total, mut worse, mut better) = (0.0, 0.0, 0, 0);
    for path in 0..300 {
        let a: f64 = wages(&off, path).iter().sum();
        let b: f64 = wages(&on, path).iter().sum();
        off_total += a;
        on_total += b;
        worse += usize::from(b < a - 1.0);
        better += usize::from(b > a + 1.0);
    }
    assert_eq!(better, 0);
    assert!(worse > 10, "{worse} paths with lost pay");
    let loss = 1.0 - on_total / off_total;
    // About a 10% chance of an onset over 22 years, losing around half the remaining
    // career each: lifetime pay down by a few percent.
    assert!((0.01..0.12).contains(&loss), "lifetime pay loss {loss}");
}

#[test]
fn f18_validation_rejects_bad_ltd_and_warns_about_inert_settings() {
    let mut p = disability_worker(40, 62.0);
    disabled(
        &mut p,
        Some(LtdInsurance {
            replacement: 1.5,
            to_age: 65.0,
        }),
    );
    let report = firesight_core::validate::validate(&p);
    assert!(report
        .errors
        .iter()
        .any(|e| e.path.ends_with("ltd_insurance/replacement")));
    let mut q = disability_worker(40, 62.0);
    q.household.people[0].ltd_insurance = Some(LtdInsurance::default());
    let report = firesight_core::validate::validate(&q);
    assert!(report.warnings.iter().any(|w| w.code == "ltd_inert"));
}

// ---------------------------------------------------------------------------
// F19: older workers and involuntary retirement
// ---------------------------------------------------------------------------

fn late(p: &mut Profile, on: LateCareer) {
    job_risk(p).late_career = on;
}

/// A 56-year-old with a high layoff risk, due to retire at 66.
fn older_worker() -> Profile {
    worker(56, 90_000.0, RiskLevel::High, 66.0)
}

fn event_months(profile: &Profile, path: u64, text: &str) -> Vec<i32> {
    let plan = compile(profile);
    sim::trace_path(&plan, SEED, path)
        .events
        .iter()
        .filter(|e| e.label.contains(text))
        .map(|e| e.month - plan.start())
        .collect()
}

#[test]
fn f19_off_is_identical_and_not_serialised() {
    let base = older_worker();
    let mut off = base.clone();
    late(&mut off, LateCareer::Off);
    for path in 0..10 {
        assert_eq!(wages(&base, path), wages(&off, path));
    }
    assert!(!serde_json::to_string(&base)
        .unwrap()
        .contains("late_career"));
}

#[test]
fn f19_is_inert_without_career_risk_and_before_55() {
    let mut none = worker(56, 90_000.0, RiskLevel::None, 66.0);
    let reference = wages(&none, 2);
    late(&mut none, LateCareer::Realistic);
    assert_eq!(wages(&none, 2), reference);
    // Retiring before 55: the feature never has a layoff to act on.
    let young = worker(40, 90_000.0, RiskLevel::High, 54.0);
    let mut on = young.clone();
    late(&mut on, LateCareer::Realistic);
    for path in 0..60 {
        assert_eq!(wages(&young, path), wages(&on, path), "path {path}");
    }
}

#[test]
fn f19_layoffs_are_the_same_and_a_share_end_in_retirement_that_rises_with_age() {
    let off = older_worker();
    let mut on = off.clone();
    late(&mut on, LateCareer::Realistic);
    let (mut layoffs, mut never) = (0, 0);
    let (mut spell_off, mut spell_on, mut compared) = (0.0, 0.0, 0);
    let mut age_sum_never = 0.0;
    let mut age_sum_all = 0.0;
    for path in 0..500 {
        let l_off = event_months(&off, path, "Laid off");
        let l_on = event_months(&on, path, "Laid off");
        // The first layoff is in the same month; later ones need not match (a longer
        // spell shifts exposure, and a retired person is no longer exposed).
        assert_eq!(l_off.first(), l_on.first(), "path {path}");
        let retired = event_months(&on, path, "Involuntary retirement");
        // Every layoff on the feature's path is one trial of the never-again draw.
        for m in &l_on {
            layoffs += 1;
            let age = 56.0 + f64::from(*m) / 12.0;
            age_sum_all += age;
            if retired.contains(m) {
                never += 1;
                age_sum_never += age;
            }
        }
        if let Some(r) = retired.first() {
            // The single-person household retires the next month (or on its plan date).
            assert_eq!(event_months(&on, path, "Retired"), vec![*r + 1]);
            // No pay again.
            let w = wages(&on, path);
            // (An all-zero column is omitted from the trace, hence the `get`.)
            let after = w.get(*r as usize + 1..).unwrap_or(&[]);
            assert!(after.iter().all(|x| *x == 0.0), "path {path}");
        } else if l_off.len() == 1 {
            // A layoff that was survived: the idle months are longer with the feature.
            let first = l_off[0];
            let idle = |p: &Profile| {
                column(p, path, "income_wages")[first as usize..120]
                    .iter()
                    .take_while(|w| **w == 0.0)
                    .count() as f64
            };
            spell_off += idle(&off);
            spell_on += idle(&on);
            compared += 1;
        }
    }
    assert!(layoffs > 100, "{layoffs} paths with a layoff");
    // Probability of never working again averages the age profile (about 15% to 50%).
    let share = f64::from(never) / f64::from(layoffs);
    assert!((0.15..0.5).contains(&share), "never-again share {share}");
    // Those who never work again were laid off later (older) than the rest.
    let mean_never = age_sum_never / f64::from(never);
    let mean_all = age_sum_all / f64::from(layoffs);
    assert!(mean_never > mean_all, "{mean_never} vs {mean_all}");
    assert!(compared > 20, "{compared} surviving spells compared");
    assert!(
        spell_on > 1.15 * spell_off,
        "idle months with the feature {spell_on} vs {spell_off}"
    );
}

#[test]
fn f19_lowers_lifetime_pay_in_the_expected_direction() {
    let off = older_worker();
    let mut on = off.clone();
    late(&mut on, LateCareer::Realistic);
    let (mut a_total, mut b_total, mut better) = (0.0, 0.0, 0);
    for path in 0..300 {
        let a: f64 = wages(&off, path).iter().sum();
        let b: f64 = wages(&on, path).iter().sum();
        a_total += a;
        b_total += b;
        better += usize::from(b > a + 1.0);
    }
    // A longer spell can skip a later layoff (the worker is still idle when the
    // hazard would have struck), so a few paths end up ahead; they are rare.
    assert!(better * 40 < 300, "{better} of 300 paths paid more");
    let loss = 1.0 - b_total / a_total;
    assert!((0.005..0.25).contains(&loss), "lifetime pay loss {loss}");
}

#[test]
fn f19_a_couple_keeps_working_when_one_partner_is_pushed_out() {
    let mut p = older_worker();
    let mut b = person("b", 50, Sex::Unspecified);
    b.social_security.basis = SocialSecurityBasis::None;
    p.household.people.push(b);
    p.incomes.push(job("job_b", 1, 60_000.0));
    p.accounts
        .push(account("cash_b", 1, AccountKind::Cash, 0.0));
    late(&mut p, LateCareer::Realistic);
    let mut found = 0;
    for path in 0..600 {
        let pushed = event_months(&p, path, "Involuntary retirement");
        let Some(first) = pushed.first() else {
            continue;
        };
        // The household does not retire in the next month while the partner works
        // (partner is 50, due to retire with the household later).
        let retired = event_months(&p, path, "Retired");
        assert!(retired.iter().all(|m| *m != first + 1), "path {path}");
        found += 1;
        if found == 3 {
            break;
        }
    }
    assert!(found >= 1);
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

#[test]
fn f17_profile_conserves_money_and_stays_finite() {
    let mut p = worker(35, 150_000.0, RiskLevel::High, 55.0);
    if let IncomeKind::Job(j) = &mut p.incomes[0].kind {
        j.risk.recession_pay = RecessionPay::Typical;
        j.bonus = Some(Bonus::default());
        j.commission = Some(Commission {
            expected_annual: 12_000.0,
            volatility: 0.4,
        });
        j.plan = Some(EmployerPlan::default());
    }
    conserves_and_is_finite(&p, 150);
}

#[test]
fn f18_profile_conserves_money_and_stays_finite() {
    let mut p = disability_worker(40, 65.0);
    disabled(
        &mut p,
        Some(LtdInsurance {
            replacement: 0.6,
            to_age: 65.0,
        }),
    );
    if let IncomeKind::Job(j) = &mut p.incomes[0].kind {
        j.risk.level = RiskLevel::High;
        j.plan = Some(EmployerPlan::default());
    }
    p.assumptions.career_risk = true;
    conserves_and_is_finite(&p, 200);
}

#[test]
fn f19_profile_conserves_money_and_stays_finite() {
    let mut p = older_worker();
    late(&mut p, LateCareer::Realistic);
    job_risk(&mut p).recession_pay = RecessionPay::Typical;
    job_risk(&mut p).job_mobility = JobMobility::Typical;
    p.household.people[0].disability_risk = DisabilityRisk::Typical;
    conserves_and_is_finite(&p, 200);
}

// ---------------------------------------------------------------------------
// F20: self-employment cycles and business failure
// ---------------------------------------------------------------------------

/// A 40-year-old running a business of `profit` a year, retiring at 60.
fn owner(profit: f64) -> Profile {
    let mut who = person("o", 40, Sex::Unspecified);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = quiet(vec![who], 95.0);
    p.assumptions.career_risk = true;
    p.incomes = vec![Income {
        id: "shop".into(),
        name: "Shop".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: profit,
            volatility: 0.2,
            ..SelfEmployment::default()
        }),
        ..Income::default()
    }];
    p.spending.items = vec![item("living", Category::Other, 2_000.0, true)];
    p.accounts = vec![account("cash", 0, AccountKind::Cash, 50_000.0)];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 60.0,
    };
    p
}

fn business(p: &mut Profile, risk: BusinessRisk, fallback: Option<FallbackWage>, years: f64) {
    if let IncomeKind::SelfEmployment(se) = &mut p.incomes[0].kind {
        se.business_risk = risk;
        se.fallback_wage = fallback;
        se.years_operating = years;
    }
}

fn profit(p: &Profile, path: u64) -> Vec<f64> {
    column(p, path, "income_self_employment")
}

#[test]
fn f20_off_is_identical_and_not_serialised() {
    let base = owner(60_000.0);
    let mut off = base.clone();
    business(&mut off, BusinessRisk::Off, None, 0.0);
    for path in 0..10 {
        assert_eq!(profit(&base, path), profit(&off, path));
    }
    let json = serde_json::to_string(&base).unwrap();
    assert!(!json.contains("business_risk") && !json.contains("fallback_wage"));
    // Without career_risk the feature is inert.
    let mut on = base.clone();
    business(&mut on, BusinessRisk::High, None, 0.0);
    on.assumptions.career_risk = false;
    let mut reference = base.clone();
    reference.assumptions.career_risk = false;
    assert_eq!(profit(&on, 3), profit(&reference, 3));
}

#[test]
fn f20_first_year_failure_matches_the_bls_rate_and_stops_income() {
    let off = owner(60_000.0);
    let mut on = off.clone();
    business(&mut on, BusinessRisk::Typical, None, 0.0);
    let (mut failed_in_year, mut n) = (0, 0);
    for path in 0..800 {
        n += 1;
        let months = event_months(&on, path, "Business failed");
        assert!(months.len() <= 1, "fails at most once");
        if let Some(m) = months.first() {
            if *m < 12 {
                failed_in_year += 1;
            }
            let pr = profit(&on, path);
            assert!(
                pr[*m as usize..].iter().all(|x| *x == 0.0),
                "profit after failure, path {path}"
            );
            assert!(pr[..*m as usize].iter().any(|x| *x > 0.0) || *m < 2);
        }
    }
    // BLS: 20.4% close in the first year; recessions on some paths push it up a bit.
    let share = f64::from(failed_in_year) / f64::from(n);
    assert!(
        (0.15..0.28).contains(&share),
        "first-year failure share {share}"
    );
}

#[test]
fn f20_fallback_wage_starts_after_the_search_spell() {
    let mut p = owner(60_000.0);
    business(
        &mut p,
        BusinessRisk::High,
        Some(FallbackWage {
            annual: 36_000.0,
            search_months: 5,
        }),
        0.0,
    );
    let mut checked = 0;
    for path in 0..200 {
        let Some(m) = event_months(&p, path, "Business failed").first().copied() else {
            continue;
        };
        let m = m as usize;
        if m + 8 >= 240 {
            continue;
        }
        let w = column(&p, path, "income_wages");
        assert!(
            w[m..m + 5].iter().all(|x| *x == 0.0),
            "search spell, path {path}"
        );
        // 36,000 a year in today's dollars is 3,000 a month before inflation; the
        // trace is in today's dollars.
        assert!(
            (w[m + 5] - 3_000.0).abs() < 1.0,
            "path {path}: {}",
            w[m + 5]
        );
        assert!((w[m + 7] - 3_000.0).abs() < 1.0);
        checked += 1;
    }
    assert!(checked > 30, "{checked} failures checked");
}

#[test]
fn f20_lowers_lifetime_profit_in_the_expected_direction() {
    let off = owner(60_000.0);
    let mut typical = off.clone();
    business(&mut typical, BusinessRisk::Typical, None, 0.0);
    let mut high = off.clone();
    business(&mut high, BusinessRisk::High, None, 0.0);
    let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
    for path in 0..300 {
        a += profit(&off, path).iter().sum::<f64>();
        b += profit(&typical, path).iter().sum::<f64>();
        c += profit(&high, path).iter().sum::<f64>();
    }
    // Twenty years of the hazard table alone average to a 61% loss of profit (mean
    // survival over the years, worked out by hand from FAIL_HAZARD_BY_YEAR); the
    // recession drop and the recession and regime hazards add a few points.
    let (lt, lh) = (1.0 - b / a, 1.0 - c / a);
    assert!((0.58..0.72).contains(&lt), "typical loss {lt}");
    assert!(lh > lt + 0.03, "high {lh} vs typical {lt}");
    // An established business (ten years old) fails far less.
    let mut old = off.clone();
    business(&mut old, BusinessRisk::Typical, None, 10.0);
    let mut d = 0.0;
    for path in 0..300 {
        d += profit(&old, path).iter().sum::<f64>();
    }
    assert!(1.0 - d / a < lt - 0.1, "established loss {}", 1.0 - d / a);
}

#[test]
fn f20_profile_conserves_money_and_stays_finite() {
    let mut p = owner(60_000.0);
    business(
        &mut p,
        BusinessRisk::High,
        Some(FallbackWage {
            annual: 40_000.0,
            search_months: 3,
        }),
        1.0,
    );
    if let IncomeKind::SelfEmployment(se) = &mut p.incomes[0].kind {
        se.solo_401k_fraction = 0.1;
    }
    conserves_and_is_finite(&p, 200);
}

// ---------------------------------------------------------------------------
// Edges and combinations
// ---------------------------------------------------------------------------

#[test]
fn f18_disability_also_stops_self_employment_income() {
    let base = owner(60_000.0);
    let mut on = base.clone();
    on.household.people[0].disability_risk = DisabilityRisk::Typical;
    let mut checked = 0;
    for path in 0..800 {
        let Some((m, recovered)) = onset_month(&on, path) else {
            continue;
        };
        if recovered || !(12..200).contains(&m) {
            continue;
        }
        let with = profit(&on, path);
        let without = profit(&base, path);
        assert_eq!(with[..m], without[..m], "path {path}");
        assert!(without[m] > 0.0, "path {path}");
        assert!(with[m..].iter().all(|x| *x == 0.0), "path {path}");
        checked += 1;
    }
    // About 13% of owners aged 40 have an onset before 60.
    assert!(checked > 30, "{checked} onsets checked");
}

#[test]
fn f19_a_partner_without_work_does_not_keep_the_household_from_retiring() {
    let mut p = older_worker();
    let mut b = person("b", 50, Sex::Unspecified);
    b.social_security.basis = SocialSecurityBasis::None;
    p.household.people.push(b);
    late(&mut p, LateCareer::Realistic);
    let mut found = 0;
    for path in 0..400 {
        let pushed = event_months(&p, path, "Involuntary retirement");
        let Some(first) = pushed.first() else {
            continue;
        };
        assert_eq!(
            event_months(&p, path, "Retired"),
            vec![*first + 1],
            "path {path}"
        );
        found += 1;
    }
    assert!(found > 20, "{found} involuntary retirements");
}

#[test]
fn f19_spell_is_scaled_from_its_full_length() {
    // A survived first layoff at 56 to 59 is 1.3 times as long as on the same path
    // without the feature (whole spell, not the part left after the first month):
    // summed over paths the ratio of idle months is close to 1.3.
    let off = older_worker();
    let mut on = off.clone();
    late(&mut on, LateCareer::Realistic);
    let (mut idle_off, mut idle_on, mut n) = (0.0, 0.0, 0);
    for path in 0..500 {
        let l_off = event_months(&off, path, "Laid off");
        let Some(first) = l_off.first().copied() else {
            continue;
        };
        // In the 55-59 band, and neither spell runs into another event.
        if first >= 36 || !event_months(&on, path, "Involuntary retirement").is_empty() {
            continue;
        }
        let back = |p: &Profile| {
            event_months(p, path, "Back at work")
                .into_iter()
                .find(|m| *m >= first)
        };
        let (Some(a), Some(b)) = (back(&off), back(&on)) else {
            continue;
        };
        idle_off += f64::from(a - first);
        idle_on += f64::from(b - first);
        n += 1;
    }
    assert!(n > 30, "{n} spells compared");
    let ratio = idle_on / idle_off;
    assert!((1.2..1.4).contains(&ratio), "spell ratio {ratio}");
}

#[test]
fn f20_fallback_wage_stops_at_the_end_date_of_the_income() {
    let mut p = owner(60_000.0);
    p.incomes[0].end = Some(When::age(0, 50.0));
    business(
        &mut p,
        BusinessRisk::High,
        Some(FallbackWage {
            annual: 36_000.0,
            search_months: 0,
        }),
        0.0,
    );
    let mut checked = 0;
    for path in 0..200 {
        let Some(m) = event_months(&p, path, "Business failed").first().copied() else {
            continue;
        };
        let m = m as usize;
        if m >= 100 {
            continue;
        }
        let w = column(&p, path, "income_wages");
        assert!((w[m + 1] - 3_000.0).abs() < 1.0, "path {path}");
        assert!((w[118] - 3_000.0).abs() < 1.0, "path {path}");
        // The business was to stop at 50 (month 120): so does the job taken instead.
        assert!(w[121..].iter().all(|x| *x == 0.0), "path {path}");
        checked += 1;
    }
    assert!(checked > 30, "{checked} failures checked");
}

/// The demo household `slug` with a fixed retirement date.
fn demo(slug: &str) -> Profile {
    let mut p = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .profile;
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 62.0,
    };
    p
}

const FEATURE_EVENTS: [&str; 7] = [
    "Changed jobs",
    "Pay freeze",
    "Pay cut",
    "becomes disabled",
    "recovers",
    "Involuntary retirement",
    "Business failed",
];

#[test]
fn streams_switching_the_features_on_leaves_markets_and_lifespans_untouched() {
    use firesight_core::stats::PathScalar;
    for slug in ["dual-income-family", "side-hustle-landlord"] {
        let base = demo(slug);
        let on = career_realism_on(base.clone());
        let (plan_a, plan_b) = (compile(&base), compile(&on));
        let mut feature_events = 0;
        for path in 0..25 {
            let a = sim::trace_path(&plan_a, SEED, path);
            let b = sim::trace_path(&plan_b, SEED, path);
            assert_eq!(a.months, b.months, "{slug} path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::Inflation,
            ] {
                let get = |t: &firesight_core::stats::PathTrace| {
                    t.scalars.iter().find(|x| x.scalar == s).unwrap().value
                };
                assert_eq!(
                    get(&a).to_bits(),
                    get(&b).to_bits(),
                    "{slug} path {path} {s:?}"
                );
            }
            for key in [
                "price_level",
                "return_us_stocks",
                "return_intl_stocks",
                "return_bonds",
                "return_cash",
            ] {
                assert_eq!(a.column(key), b.column(key), "{slug} path {path} {key}");
            }
            // The baseline has none of the features' events, the other run does.
            assert!(!a
                .events
                .iter()
                .any(|e| FEATURE_EVENTS.iter().any(|f| e.label.contains(f))));
            feature_events += b
                .events
                .iter()
                .filter(|e| FEATURE_EVENTS.iter().any(|f| e.label.contains(f)))
                .count();
        }
        assert!(
            feature_events > 25,
            "{slug}: {feature_events} feature events"
        );
    }
}

#[test]
fn streams_the_features_draws_do_not_depend_on_an_unrelated_decision() {
    // Spending 30% less (with a fixed retirement date) must not move a single
    // job change, freeze, cut, disability, involuntary retirement or business
    // failure: the features draw a fixed number of variates a month from their own
    // streams, whatever the household does.
    for slug in ["dual-income-family", "side-hustle-landlord"] {
        let base = career_realism_on(demo(slug));
        let mut frugal = base.clone();
        for item in &mut frugal.spending.items {
            item.amount *= 0.7;
        }
        let (plan_a, plan_b) = (compile(&base), compile(&frugal));
        let mut seen = 0;
        for path in 0..40 {
            let events = |plan: &sim::Plan| -> Vec<(YearMonth, String)> {
                sim::trace_path(plan, SEED, path)
                    .events
                    .iter()
                    .filter(|e| FEATURE_EVENTS.iter().any(|f| e.label.contains(f)))
                    .map(|e| (e.month, e.label.clone()))
                    .collect()
            };
            let (a, b) = (events(&plan_a), events(&plan_b));
            assert_eq!(a, b, "{slug} path {path}");
            seen += a.len();
        }
        assert!(seen > 40, "{slug}: {seen} feature events");
    }
}

#[test]
fn stream_ids_are_inside_the_reserved_range_and_distinct() {
    use firesight_core::career_realism::{KIND_JOB, KIND_PERSON, KIND_SE};
    use firesight_core::rng::Stream;
    let mut ids = Vec::new();
    for kind in [KIND_JOB, KIND_PERSON, KIND_SE] {
        for index in [0usize, 1, 7, 63] {
            let id = Stream::CareerExt(kind, index).id();
            assert!((0x1400_0000..0x1480_0000).contains(&id), "{id:#x}");
            ids.push(id);
        }
    }
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n);
}

#[test]
fn every_feature_at_once_conserves_money_on_the_demo_households() {
    for slug in ["dual-income-family", "side-hustle-landlord"] {
        let mut p = career_realism_on(demo(slug));
        p.retirement.trigger = Profile::examples(START)
            .into_iter()
            .find(|e| e.slug == slug)
            .unwrap()
            .profile
            .retirement
            .trigger;
        conserves_and_is_finite(&p, 60);
    }
}

#[test]
fn f18_no_recovery_after_full_retirement_age_so_the_conversion_holds() {
    // Aged 56 at the start, planning to work to 72: full retirement age (67) is month
    // 132. A disability before then converts to the retirement benefit at 67 for
    // good; a "recovery" after it would hand the planned claim age (67 here, but 70
    // for many) back and stop the benefit.
    let mut p = disability_worker(56, 72.0);
    p.household.people[0].social_security.claim_age = 70.0;
    disabled(&mut p, None);
    let (mut checked, mut recoveries_before) = (0, 0);
    for path in 0..1500 {
        let Some((m, _)) = onset_month(&p, path) else {
            continue;
        };
        if m + 6 >= 132 {
            continue;
        }
        let ups = event_months(&p, path, "recovers");
        assert!(ups.iter().all(|u| *u < 132), "path {path}: {ups:?}");
        recoveries_before += usize::from(!ups.is_empty());
        if !ups.is_empty() {
            continue;
        }
        // Benefit paid without a break from the end of the wait to the end of the
        // plan's working years and beyond.
        let ss = column(&p, path, "income_social_security");
        assert!(
            ss[m + 6..ss.len().min(200)].iter().all(|x| *x > 500.0),
            "path {path}"
        );
        checked += 1;
    }
    // About 11% of 56-year-olds have an onset before 67 at these rates.
    assert!(checked > 50, "{checked} disabled paths");
    assert!(recoveries_before < checked, "{recoveries_before}");
}
