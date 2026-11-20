//! Tests of the optional market refinements: bond pricing from the yield,
//! valuation-aware returns, historical-episode stresses, the fat-tailed model and
//! dynamic income yields.

#![allow(clippy::needless_range_loop)]

use super::*;
use crate::rng::Stream;

mod episodes;
mod valuation;

const SEED: u64 = 0x5EED_1800;

fn market(kind: MarketKind) -> MarketModel {
    MarketModel {
        kind,
        equity_premium_adjustment: 0.0,
        ..MarketModel::default()
    }
}

fn bootstrap() -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    }
}

fn build(m: &MarketModel) -> EconomyModel {
    EconomyModel::new(m, Data::embedded())
}

fn generate(m: &EconomyModel, path: u64, months: usize, out: &mut EconomyPath) {
    let mut a = Rng::stream(SEED, path, Stream::Market);
    let mut b = Rng::stream(SEED, path, Stream::Inflation);
    m.generate(
        &mut EconomyStreams {
            market: &mut a,
            inflation: &mut b,
        },
        path,
        months,
        out,
    );
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn same_path(a: &EconomyPath, b: &EconomyPath) -> bool {
    a.months == b.months
        && bits(&a.inflation) == bits(&b.inflation)
        && bits(&a.price_level) == bits(&b.price_level)
        && a.returns
            .iter()
            .zip(&b.returns)
            .all(|(x, y)| bits(x) == bits(y))
        && bits(&a.tbill) == bits(&b.tbill)
        && bits(&a.long_yield) == bits(&b.long_yield)
        && bits(&a.mortgage_rate) == bits(&b.mortgage_rate)
        && bits(&a.home_national) == bits(&b.home_national)
        && a.recession == b.recession
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn corr(x: &[f64], y: &[f64]) -> f64 {
    fit::corr(x, y)
}

// ---------------------------------------------------------------------------------
// F36: bond returns priced from the yield path.
// ---------------------------------------------------------------------------------

#[test]
fn sampled_bond_pricing_is_the_default_and_changes_nothing() {
    let default = build(&market(bootstrap()));
    let sampled = build(&MarketModel {
        bond_pricing: BondPricing::Sampled,
        ..market(bootstrap())
    });
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    for path in 0..20 {
        generate(&default, path, 600, &mut a);
        generate(&sampled, path, 600, &mut b);
        assert!(same_path(&a, &b), "path {path}");
    }
    // Outside the bootstrap the setting does nothing at all.
    for kind in [
        MarketKind::Regime,
        MarketKind::Lognormal,
        MarketKind::Historical,
    ] {
        let off = build(&market(kind));
        let on = build(&MarketModel {
            bond_pricing: BondPricing::FromYield,
            ..market(kind)
        });
        generate(&off, 3, 400, &mut a);
        generate(&on, 3, 400, &mut b);
        assert!(same_path(&a, &b), "{kind:?}");
    }
}

#[test]
fn from_yield_reprices_only_the_bond_and_from_the_paths_own_yield() {
    for adj in [0.0, 0.02] {
        let base = MarketModel {
            inflation_adjustment: adj,
            ..market(bootstrap())
        };
        let off = build(&base);
        let on = build(&MarketModel {
            bond_pricing: BondPricing::FromYield,
            ..base.clone()
        });
        let p = bonds::BondPricer::new();
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        for path in 0..10 {
            generate(&off, path, 480, &mut a);
            generate(&on, path, 480, &mut b);
            let bond = AssetClass::Bonds.index();
            // Everything but the bond return is bit for bit the same path.
            let mut b2 = b.clone();
            b2.returns[bond] = a.returns[bond].clone();
            assert!(same_path(&a, &b2));
            // Month 0 earns today's (adjusted) yield; later months follow the path.
            let y_now = off.base.rate_now.0 + adj;
            assert!((b.returns[bond][0] - y_now / 12.0).abs() < 1e-15);
            for t in 1..480 {
                let expected = p.bond_return(b.long_yield[t - 1], b.long_yield[t]);
                assert_eq!(b.returns[bond][t].to_bits(), expected.to_bits());
            }
        }
    }
}

/// Over a decade a bond's return is its average yield less its duration times the
/// change in the yield. With sampled returns a path's decade of bond returns has
/// little to do with what its own yield did; priced from the yield it follows it.
#[test]
fn from_yield_makes_a_paths_bond_returns_follow_its_yield() {
    let off = build(&market(bootstrap()));
    let on = build(&MarketModel {
        bond_pricing: BondPricing::FromYield,
        ..market(bootstrap())
    });
    let months = 120;
    let paths = 1500;
    let bond = AssetClass::Bonds.index();
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    let (mut implied, mut sampled, mut priced) = (Vec::new(), Vec::new(), Vec::new());
    let (mut vol_off, mut vol_on) = (Vec::new(), Vec::new());
    for path in 0..paths {
        generate(&off, path, months, &mut a);
        generate(&on, path, months, &mut b);
        // Same yield path in both (only bond returns differ).
        let y0 = off.base.rate_now.0;
        let carry = (y0 + b.long_yield[..months - 1].iter().sum::<f64>()) / 12.0;
        let dur = regime::par_bond_duration(mean(&b.long_yield[..months]));
        implied.push(carry - dur * (b.long_yield[months - 1] - y0));
        sampled.push(a.returns[bond][..months].iter().sum::<f64>());
        priced.push(b.returns[bond][..months].iter().sum::<f64>());
        vol_off.extend_from_slice(&a.returns[bond][..months]);
        vol_on.extend_from_slice(&b.returns[bond][..months]);
    }
    let c_off = corr(&implied, &sampled);
    let c_on = corr(&implied, &priced);
    eprintln!("decade bond return vs own yield: sampled {c_off:.3}, from yield {c_on:.3}");
    assert!(c_on > 0.98, "priced from the yield: {c_on}");
    assert!(c_off < 0.7, "sampled: {c_off}");
    // Monthly volatility stays that of history's bond (the yield moves are the
    // sampled rows' own), within a tenth.
    let sd = |v: &[f64]| fit::mean_sd(v.iter().copied()).1;
    let ratio = sd(&vol_on) / sd(&vol_off);
    assert!((0.9..1.1).contains(&ratio), "volatility ratio {ratio}");
}

#[test]
fn from_yield_bond_returns_are_plausible_over_long_horizons() {
    let on = build(&MarketModel {
        bond_pricing: BondPricing::FromYield,
        ..market(bootstrap())
    });
    let off = build(&market(bootstrap()));
    let bond = AssetClass::Bonds.index();
    let real = |m: &EconomyModel| {
        let mut out = EconomyPath::new();
        let (mut log, mut n) = (0.0, 0.0);
        for path in 0..800 {
            generate(m, path, 720, &mut out);
            for t in 0..720 {
                log += math::ln_1p(out.returns[bond][t]) - math::ln_1p(out.inflation[t]);
                n += 1.0;
            }
        }
        math::exp(log / n * 12.0) - 1.0
    };
    let (r_on, r_off) = (real(&on), real(&off));
    eprintln!("real geometric bond return: from yield {r_on:.4}, sampled {r_off:.4}");
    // History: 1.56% a year real (geometric). Priced from a yield that starts at
    // today's 5.3% and is pulled toward the record's mean of 4.8%, with sampled
    // inflation averaging about 3%, bonds return roughly 1.5 to 2.5% real.
    assert!((0.008..0.03).contains(&r_on), "from yield {r_on}");
    assert!(
        (r_on - r_off).abs() < 0.012,
        "from yield {r_on}, sampled {r_off}"
    );
}
