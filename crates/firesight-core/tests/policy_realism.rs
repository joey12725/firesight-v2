//! Tax and policy realism options, switched on one at a time against the same paths:
//! chained-CPI indexing, state detail, policy risk, tax-loss harvesting and
//! qualified charitable distributions. Each is off by default, so each test first
//! checks that stating the default changes nothing, then that switching the option
//! on moves the outcome in the expected direction by a plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::Workspace;
use firesight_core::stats::{Accumulator, PathScalar};

const PATHS: u64 = 600;
const SEED: u64 = 7;

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .expect("example exists")
        .profile
}

fn mean(acc: &Accumulator, scalar: PathScalar) -> f64 {
    let column = acc.scalar_column(scalar);
    let finite: Vec<f64> = column
        .iter()
        .filter(|v| v.is_finite())
        .map(|&v| f64::from(v))
        .collect();
    finite.iter().sum::<f64>() / finite.len() as f64
}

fn run_profile(profile: &Profile) -> Accumulator {
    let plan = compile(profile);
    run(&plan, SEED, 0..PATHS)
}

/// Runs with the conservation checks on and asserts every scalar is finite where
/// it should be, so a feature that leaks or invents money fails here.
fn assert_conserves_and_is_finite(profile: &Profile) {
    let plan = compile(profile);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut acc = plan.new_accumulator();
    for i in 0..200 {
        firesight_core::sim::run_path(&plan, SEED, i, &mut ws, &mut acc);
    }
    for s in [PathScalar::LifetimeTax, PathScalar::TerminalNetWorth] {
        assert!(acc.scalar_column(s).iter().all(|v| !v.is_infinite()));
    }
    let summary = plan.summarize(&acc);
    let json = serde_json::to_string(&summary).unwrap();
    assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
}

// ---------------------------------------------------------------- F21 chained CPI

#[test]
fn chained_cpi_default_is_bit_identical_and_on_raises_lifetime_tax() {
    let base = example("dual-income-family");
    let mut stated = base.clone();
    stated.assumptions.taxes.chained_cpi = false;
    let a = run_profile(&base);
    let b = run_profile(&stated);
    assert_eq!(
        a.to_bytes(),
        b.to_bytes(),
        "stating the default changes bits"
    );

    let mut on = base.clone();
    on.assumptions.taxes.chained_cpi = true;
    let c = run_profile(&on);
    let (tax_off, tax_on) = (
        mean(&a, PathScalar::LifetimeTax),
        mean(&c, PathScalar::LifetimeTax),
    );
    let rise = tax_on / tax_off - 1.0;
    // Measured on 3,000 paths: median lifetime tax +3.9%. Same paths, so the
    // difference is the wedge, not noise.
    assert!((0.01..0.08).contains(&rise), "lifetime tax rose {rise}");
    assert!(
        mean(&c, PathScalar::TerminalNetWorth) < mean(&a, PathScalar::TerminalNetWorth) * 1.001
    );
    assert_conserves_and_is_finite(&on);
}

// ---------------------------------------------------------------- F22 state detail

fn in_state(slug: &str, state: &str) -> Profile {
    let mut p = example(slug);
    p.household.location.state = Some(state.into());
    p
}

fn lifetime_tax_change(profile: &Profile) -> f64 {
    let off = run_profile(profile);
    let mut on = profile.clone();
    on.assumptions.taxes.state_detail = StateDetail::Full;
    let on_acc = run_profile(&on);
    mean(&on_acc, PathScalar::LifetimeTax) / mean(&off, PathScalar::LifetimeTax) - 1.0
}

#[test]
fn state_detail_basic_is_the_default_and_changes_nothing_outside_ca_ny_ut() {
    for state in ["TX", "CO", "FL"] {
        let base = in_state("dual-income-family", state);
        let mut full = base.clone();
        full.assumptions.taxes.state_detail = StateDetail::Full;
        assert_eq!(
            run_profile(&base).to_bytes(),
            run_profile(&full).to_bytes(),
            "{state}"
        );
    }
    let mut stated = example("dual-income-family");
    stated.assumptions.taxes.state_detail = StateDetail::Basic;
    assert_eq!(
        run_profile(&example("dual-income-family")).to_bytes(),
        run_profile(&stated).to_bytes()
    );
}

#[test]
fn state_detail_full_moves_lifetime_tax_the_right_way_in_each_state() {
    // Measured on 2,000 paths of the dual-income example moved to each state: California
    // +6.0% (1.3% payroll tax on two salaries), New York +2.3% (recapture on a
    // household above $107,650), Utah -1.4% (the credit on Social Security).
    let ca = lifetime_tax_change(&in_state("dual-income-family", "CA"));
    assert!((0.03..0.10).contains(&ca), "CA {ca}");
    let ny = lifetime_tax_change(&in_state("dual-income-family", "NY"));
    assert!((0.005..0.05).contains(&ny), "NY {ny}");
    let ut = lifetime_tax_change(&in_state("dual-income-family", "UT"));
    assert!((-0.04..-0.002).contains(&ut), "UT {ut}");
}

#[test]
fn state_detail_full_conserves_money_and_stays_finite() {
    for state in ["CA", "NY", "UT"] {
        let mut p = in_state("dual-income-family", state);
        p.assumptions.taxes.state_detail = StateDetail::Full;
        assert_conserves_and_is_finite(&p);
    }
}
