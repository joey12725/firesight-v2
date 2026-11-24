//! F37: valuation-aware expected returns.

use super::*;
use crate::model::Valuation;

fn with_valuation(kind: MarketKind, start: f64, fair: f64) -> MarketModel {
    MarketModel {
        valuation: Some(Valuation {
            starting_cape: start,
            fair_cape: fair,
            half_life_years: 15.0,
        }),
        ..market(kind)
    }
}

/// Real log return of US equities over months `from..to`, summed, per path.
fn real_log_us(m: &EconomyModel, paths: u64, months: usize, from: usize, to: usize) -> Vec<f64> {
    let mut out = EconomyPath::new();
    let us = AssetClass::UsStocks.index();
    (0..paths)
        .map(|path| {
            generate(m, path, months, &mut out);
            (from..to)
                .map(|t| math::ln_1p(out.returns[us][t]) - math::ln_1p(out.inflation[t]))
                .sum()
        })
        .collect()
}

const VALUATION_KINDS: [MarketKind; 3] = [
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    },
    MarketKind::Regime,
    MarketKind::Lognormal,
];

#[test]
fn the_replay_ignores_valuation() {
    let mut a = EconomyPath::new();
    let mut b = EconomyPath::new();
    let off = build(&market(MarketKind::Historical));
    let on = build(&with_valuation(MarketKind::Historical, 40.0, 17.3));
    generate(&off, 5, 300, &mut a);
    generate(&on, 5, 300, &mut b);
    assert!(same_path(&a, &b));
    assert!(on.valuation_outlook().is_none());
    assert!(b.valuation_gap.is_empty());
}

/// Only US equities and the classes that follow them move; every other series of
/// the path is the model's own, bit for bit.
#[test]
fn valuation_leaves_every_other_series_untouched() {
    for kind in VALUATION_KINDS {
        let off = build(&market(kind));
        let on = build(&with_valuation(kind, 40.0, 17.3));
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        generate(&off, 7, 240, &mut a);
        generate(&on, 7, 240, &mut b);
        let mut b2 = b.clone();
        for c in [
            AssetClass::UsStocks,
            AssetClass::RealEstate,
            AssetClass::SingleStock,
            AssetClass::Custom,
        ] {
            b2.returns[c.index()] = a.returns[c.index()].clone();
        }
        assert!(same_path(&a, &b2), "{kind:?}");
        let us = AssetClass::UsStocks.index();
        assert!(b.returns[us][0] < a.returns[us][0], "{kind:?}");
        assert_eq!(b.valuation_gap.len(), 240);
    }
}

/// Fairly valued at the start, the overlay only feeds price surprises back into
/// expected returns: the long-run mean is the model's own.
#[test]
fn a_fairly_valued_start_keeps_the_long_run_mean() {
    for kind in VALUATION_KINDS {
        let off = build(&market(kind));
        let on = build(&with_valuation(kind, 25.0, 25.0));
        let months = 720;
        let a = real_log_us(&off, 600, months, 0, months);
        let b = real_log_us(&on, 600, months, 0, months);
        let per_year = |v: &[f64]| mean(v) * 12.0 / months as f64;
        let (ra, rb) = (per_year(&a), per_year(&b));
        eprintln!("{kind:?}: fair start, real log return {ra:.4} -> {rb:.4}");
        assert!((ra - rb).abs() < 0.003, "{kind:?}: {ra} {rb}");
        // The model's anchor is its own mean (to within the regime model's
        // approximation and the sampling error of 600 paths).
        let m = on.valuation.unwrap().mean_real_log * 12.0;
        assert!((m - ra).abs() < 0.004, "{kind:?}: anchor {m}, mean {ra}");
    }
}

/// From a CAPE of 40 against a normal 17.3 the first decade loses, on average,
/// exactly the gap the reversion closes: ln(40 / 17.3) (1 - 0.5^(10/15)) = 0.310 in
/// log terms, about 3.1 points a year; the shocks are the model's own.
#[test]
fn an_expensive_start_lowers_the_first_decade_by_the_gap_that_closes() {
    let expected = -math::ln(40.0 / 17.3) * (1.0 - math::pow(0.5, 10.0 / 15.0));
    assert!((expected + 0.3102).abs() < 1e-3, "{expected}");
    for kind in VALUATION_KINDS {
        let off = build(&market(kind));
        let on = build(&with_valuation(kind, 40.0, 17.3));
        let a = real_log_us(&off, 1000, 360, 0, 120);
        let b = real_log_us(&on, 1000, 360, 0, 120);
        let diff: Vec<f64> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
        let d = mean(&diff);
        eprintln!("{kind:?}: first decade log shortfall {d:.4} (expected {expected:.4})");
        assert!((d - expected).abs() < 0.03, "{kind:?}: {d}");
        let outlook = on.valuation_outlook().unwrap();
        assert!((outlook.2 * 10.0 - expected).abs() < 1e-12);
        // The third decade still pays for part of the remaining gap, less than
        // half as much as the first.
        let a3 = real_log_us(&off, 1000, 360, 240, 360);
        let b3 = real_log_us(&on, 1000, 360, 240, 360);
        let d3 = mean(&b3) - mean(&a3);
        eprintln!("{kind:?}: third decade log shortfall {d3:.4}");
        assert!(
            d3 < 0.0 && d3 > expected / 2.0,
            "{kind:?}: third decade {d3}"
        );
    }
}

/// The feedback of price surprises adds mean reversion: ten-year real returns
/// vary less than without it.
#[test]
fn valuation_feedback_narrows_ten_year_dispersion() {
    for kind in VALUATION_KINDS {
        let off = build(&market(kind));
        let on = build(&with_valuation(kind, 25.0, 25.0));
        let a = real_log_us(&off, 1500, 120, 0, 120);
        let b = real_log_us(&on, 1500, 120, 0, 120);
        let sd = |v: &[f64]| fit::mean_sd(v.iter().copied()).1;
        let ratio = sd(&b) / sd(&a);
        eprintln!("{kind:?}: ten-year real log return sd ratio {ratio:.3}");
        assert!((0.6..1.0).contains(&ratio), "{kind:?}: {ratio}");
    }
}
