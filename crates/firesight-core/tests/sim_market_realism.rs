//! Simulation-level checks of the optional market refinements on paired paths: off
//! is the baseline bit for bit, on moves outcomes in the expected direction by a
//! plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::stats::{PathScalar, Summary};

const SEED: u64 = 1800;

fn digest_json(p: &Profile, paths: u64) -> String {
    serde_json::to_string(&summary(p, SEED, paths).digest()).unwrap()
}

fn median(s: &Summary) -> f64 {
    s.headline.median_terminal_net_worth
}

fn bootstrap() -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    }
}

/// A 30-year retiree in a 40/60 portfolio spending 4% a year.
fn bond_heavy_retiree() -> Profile {
    let mut p = retiree(1_000_000.0, 0.4, 40_000.0, 30.0);
    p.assumptions.market.kind = bootstrap();
    p
}

fn scalar_mean(p: &Profile, paths: u64, s: PathScalar) -> f64 {
    let plan = compile(p);
    let acc = run(&plan, SEED, 0..paths);
    let v: Vec<f64> = acc
        .scalar_column(s)
        .iter()
        .map(|x| f64::from(*x))
        .filter(|x| x.is_finite())
        .collect();
    v.iter().sum::<f64>() / v.len() as f64
}

// ---------------------------------------------------------------------------------
// F36: bond returns from the yield path.
// ---------------------------------------------------------------------------------

#[test]
fn bond_pricing_sampled_is_the_baseline_bit_for_bit() {
    for example in Profile::examples(START) {
        let mut sampled = example.profile.clone();
        sampled.assumptions.market.bond_pricing = BondPricing::Sampled;
        assert_eq!(
            digest_json(&example.profile, 300),
            digest_json(&sampled, 300),
            "{}",
            example.slug
        );
    }
}

/// Priced from a yield that starts at today's 5.3% (above the record's 4.8% mean),
/// bonds return about half a point a year more after inflation than the record's
/// resampled bond months, so a bond-heavy retiree ends richer on the same paths.
#[test]
fn bonds_priced_from_the_yield_raise_a_bond_heavy_retirees_wealth_modestly() {
    let off = bond_heavy_retiree();
    let mut on = off.clone();
    on.assumptions.market.bond_pricing = BondPricing::FromYield;
    let (a, b) = (summary(&off, SEED, 2000), summary(&on, SEED, 2000));
    let ratio = median(&b) / median(&a);
    eprintln!(
        "40/60 retiree: median terminal {:.0} -> {:.0} ({ratio:.3}), success {:.3} -> {:.3}",
        median(&a),
        median(&b),
        a.headline.success_rate.value,
        b.headline.success_rate.value
    );
    // 0.5 points a year on 60% of the portfolio for 30 years is about +9%; the
    // spread of a market model run is wider, so allow +2% to +30%.
    assert!((1.02..1.30).contains(&ratio), "{ratio}");
    assert!(b.headline.success_rate.value >= a.headline.success_rate.value - 0.01);
    // The early-retirement real return rises too.
    let r_off = scalar_mean(&off, 2000, PathScalar::EarlyRetirementReturn);
    let r_on = scalar_mean(&on, 2000, PathScalar::EarlyRetirementReturn);
    assert!(r_on > r_off && r_on - r_off < 0.01, "{r_off} {r_on}");
}
