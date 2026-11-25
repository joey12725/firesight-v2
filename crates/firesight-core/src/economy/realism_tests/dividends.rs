//! F40: dividend yields that move with the market.

use super::*;
use crate::model::{Episode, Stress, Valuation, When};

const US: usize = 0;
const INTL: usize = 1;

fn dynamic(kind: MarketKind) -> MarketModel {
    MarketModel {
        dynamic_yields: true,
        ..market(kind)
    }
}

#[test]
fn off_leaves_the_yields_constant_and_on_leaves_returns_alone() {
    for kind in [
        bootstrap(),
        MarketKind::Regime,
        MarketKind::Lognormal,
        MarketKind::FatTailed,
    ] {
        let off = build(&market(kind));
        let on = build(&dynamic(kind));
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        generate(&off, 2, 480, &mut a);
        generate(&on, 2, 480, &mut b);
        assert!(a.dividend_yields.is_empty());
        assert_eq!(b.dividend_yields.len(), 480);
        // Total returns, rates and inflation: the same bits.
        assert!(same_path(&a, &b), "{kind:?}");
        for t in [0, 100, 479] {
            let (ya, yb) = (a.income_yields_at(t), b.income_yields_at(t));
            assert_eq!(ya, DIVIDEND_YIELD.tap(&a, t));
            // Bonds and cash are the path's own yields either way.
            for c in [AssetClass::Bonds, AssetClass::Cash] {
                assert_eq!(ya[c.index()], yb[c.index()]);
            }
            assert_eq!(
                yb[AssetClass::SingleStock.index()],
                yb[AssetClass::UsStocks.index()]
            );
        }
        // Reusing the buffer for a model with the feature off clears it.
        generate(&off, 2, 480, &mut b);
        assert!(b.dividend_yields.is_empty());
    }
}

trait Tap {
    fn tap(self, p: &EconomyPath, t: usize) -> [f64; N_ASSETS];
}

impl Tap for [f64; N_ASSETS] {
    /// The constant table with the path's bond and bill yields, as before the
    /// feature existed.
    fn tap(mut self, p: &EconomyPath, t: usize) -> [f64; N_ASSETS] {
        self[AssetClass::Bonds.index()] = p.long_yield[t].max(0.0);
        self[AssetClass::Cash.index()] = p.tbill[t].max(0.0);
        self
    }
}

/// Three months by hand: a 10% fall, a flat month, a recession month.
#[test]
fn the_recursion_by_hand() {
    let mu = [0.007, 0.006];
    let d = income_yields::DividendModel::new(mu, 0.2, None);
    let (k, cut, target) = d.parts();
    assert!((k - (1.0 - math::pow(0.7, 1.0 / 12.0))).abs() < 1e-12);
    assert!((cut - (-math::ln(0.9) / 12.0)).abs() < 1e-12);
    let lift = cut * 0.2 / k;
    assert!((target[US] - (math::ln(0.015) + lift)).abs() < 1e-12);
    let mut p = EconomyPath::with_capacity(3);
    p.returns[AssetClass::UsStocks.index()] = vec![-0.10, 0.0, 0.01];
    p.returns[AssetClass::IntlStocks.index()] = vec![0.0, 0.0, 0.0];
    p.recession = vec![0, 0, 1];
    d.fill(&mut p);
    let mut ly = math::ln(0.015);
    let mut expect = Vec::new();
    for (r, rec) in [(-0.10f64, 0.0), (0.0, 0.0), (0.01, 1.0)] {
        expect.push(math::exp(ly));
        ly = ly - (math::ln_1p(r) - mu[US]) + k * (target[US] - ly) - cut * rec;
    }
    for t in 0..3 {
        assert!(
            (p.dividend_yields[t][US] - expect[t]).abs() < 1e-12,
            "{t}: {} vs {}",
            p.dividend_yields[t][US],
            expect[t]
        );
    }
    // After a 10% fall the yield is about 10% higher.
    let jump = p.dividend_yields[1][US] / p.dividend_yields[0][US];
    assert!((jump - 1.12).abs() < 0.02, "{jump}");
    // International: flat returns above nothing, so the yield drifts with -mu and
    // the pull only.
    let li = math::ln(0.03);
    let next = li + mu[INTL] + k * (target[INTL] - li);
    assert!((p.dividend_yields[1][INTL] - math::exp(next)).abs() < 1e-12);
}

/// Long run, the average yield is today's (the constant table), in every model.
#[test]
fn the_average_yield_is_kept() {
    for kind in [
        bootstrap(),
        MarketKind::Regime,
        MarketKind::Lognormal,
        MarketKind::FatTailed,
    ] {
        let m = build(&dynamic(kind));
        let mut out = EconomyPath::new();
        let (mut sum, mut n) = ([0.0; 2], 0.0);
        let mut lows: f64 = 1.0;
        for path in 0..200 {
            generate(&m, path, 720, &mut out);
            for t in 120..720 {
                sum[US] += out.dividend_yields[t][US];
                sum[INTL] += out.dividend_yields[t][INTL];
                n += 1.0;
                lows = lows.min(out.dividend_yields[t][US]);
            }
        }
        let avg = [sum[US] / n, sum[INTL] / n];
        eprintln!("{kind:?}: average yields {avg:?}, lowest US {lows:.4}");
        assert!((avg[US] / 0.015 - 1.0).abs() < 0.2, "{kind:?} {avg:?}");
        assert!((avg[INTL] / 0.030 - 1.0).abs() < 0.2, "{kind:?} {avg:?}");
    }
}

/// The US dividend run rate (yield times price) of each month.
fn dividend_run_rate(p: &EconomyPath, from: usize, months: usize) -> Vec<f64> {
    let us = AssetClass::UsStocks.index();
    let mut price = 1.0;
    let mut out = Vec::new();
    for t in from..from + months {
        let y = p.dividend_yields[t][US];
        out.push(y * price);
        price *= 1.0 + p.returns[us][t] - y / 12.0;
    }
    out
}

/// Replaying 2007-11 to 2013-01, the US dividend run rate falls by about the 21%
/// that S&P 500 dividends per share fell from 2008 to 2009 (Shiller's data).
#[test]
fn the_2008_replay_cuts_dividends_by_about_a_fifth() {
    let m = build(&MarketModel {
        stress: vec![Stress::HistoricalEpisode {
            start: When::date(YearMonth::ym(2030, 1)),
            episode: Episode::Financial2008,
        }],
        ..dynamic(bootstrap())
    });
    let mut cuts = Vec::new();
    let mut out = EconomyPath::new();
    for path in 0..50 {
        generate(&m, path, 240, &mut out);
        m.apply_stress(
            &ResolvedStress::HistoricalEpisode {
                from_month: 24,
                episode: Episode::Financial2008,
            },
            &mut out,
        );
        let d = dividend_run_rate(&out, 24, 63);
        let mut peak: f64 = 0.0;
        let mut worst: f64 = 0.0;
        for x in d {
            peak = peak.max(x);
            worst = worst.min(x / peak - 1.0);
        }
        cuts.push(worst);
    }
    let avg = mean(&cuts);
    eprintln!("2008 replay: dividend run-rate peak to trough {avg:.3}");
    assert!((avg + 0.21).abs() < 0.05, "{avg}");
}

/// A stress overlay re-derives the yields from its start month on and leaves the
/// earlier months alone.
#[test]
fn a_stress_refreshes_the_yields_after_its_start() {
    let m = build(&dynamic(bootstrap()));
    let mut a = EconomyPath::new();
    generate(&m, 5, 360, &mut a);
    let mut b = a.clone();
    m.apply_stress(&ResolvedStress::LostDecade { from_month: 60 }, &mut b);
    assert_eq!(a.dividend_yields[..61], b.dividend_yields[..61]);
    assert!(a.dividend_yields[100..] != b.dividend_yields[100..]);
}

/// With the valuation overlay, the US yield rises as an expensive market drifts
/// back to its normal CAPE: from 1.5% toward 1.5% x 40 / 17.3 = 3.5%.
#[test]
fn with_valuation_the_yield_rises_as_the_cape_normalises() {
    let m = build(&MarketModel {
        valuation: Some(Valuation::default()),
        ..dynamic(bootstrap())
    });
    let plain = build(&dynamic(bootstrap()));
    let mut out = EconomyPath::new();
    let (mut late, mut late_plain) = (0.0, 0.0);
    for path in 0..200 {
        generate(&m, path, 720, &mut out);
        late += out.dividend_yields[600][US];
        generate(&plain, path, 720, &mut out);
        late_plain += out.dividend_yields[600][US];
    }
    let (late, late_plain) = (late / 200.0, late_plain / 200.0);
    eprintln!("US yield after 50 years: with valuation {late:.4}, without {late_plain:.4}");
    assert!(late > 0.025 && late < 0.045, "{late}");
    assert!(late > late_plain + 0.008);
}
