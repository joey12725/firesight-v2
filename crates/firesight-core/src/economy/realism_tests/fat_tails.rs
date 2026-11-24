//! F39: the fat-tailed parametric model against the record
//! (`data/history/calibration.json`) and against the lognormal model it extends.

use super::*;
use serde_json::Value;

fn fx(pointer: &str) -> f64 {
    static F: OnceLock<Value> = OnceLock::new();
    F.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/history/calibration.json"
        )))
        .unwrap()
    })
    .pointer(pointer)
    .and_then(Value::as_f64)
    .unwrap_or_else(|| panic!("calibration.json has no number at {pointer}"))
}

const US: usize = 0;
const INTL: usize = 1;
const BOND: usize = 2;

fn kurtosis(x: &[f64]) -> f64 {
    let m = mean(x);
    let (mut m2, mut m4) = (0.0, 0.0);
    for v in x {
        let d = v - m;
        m2 += d * d;
        m4 += d * d * d * d;
    }
    let n = x.len() as f64;
    (m4 / n) / ((m2 / n) * (m2 / n)) - 3.0
}

fn abs_autocorr(x: &[f64], lag: usize) -> f64 {
    let a: Vec<f64> = x.iter().map(|v| v.abs()).collect();
    fit::autocorr(&a, lag)
}

/// Deepest fall of the real US stock index over the path.
fn max_real_drawdown(p: &EconomyPath, months: usize) -> f64 {
    let (mut level, mut peak, mut worst) = (1.0f64, 1.0f64, 0.0f64);
    for t in 0..months {
        level *= (1.0 + p.returns[US][t]) / (1.0 + p.inflation[t]);
        peak = peak.max(level);
        worst = worst.min(level / peak - 1.0);
    }
    worst
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(q * (v.len() - 1) as f64) as usize]
}

/// Per-sample statistics of 100-year samples, the record's length.
struct Samples {
    kurtosis: Vec<f64>,
    ac1: Vec<f64>,
    ac12: Vec<f64>,
    drawdown: Vec<f64>,
}

fn samples(kind: MarketKind, paths: u64) -> Samples {
    let m = build(&market(kind));
    let months = 1203;
    let mut out = EconomyPath::new();
    let mut s = Samples {
        kurtosis: Vec::new(),
        ac1: Vec::new(),
        ac12: Vec::new(),
        drawdown: Vec::new(),
    };
    for path in 0..paths {
        generate(&m, path, months, &mut out);
        let r = &out.returns[US][..months];
        s.kurtosis.push(kurtosis(r));
        s.ac1.push(abs_autocorr(r, 1));
        s.ac12.push(abs_autocorr(r, 12));
        s.drawdown.push(max_real_drawdown(&out, months));
    }
    s
}

/// Fat tails, volatility clustering and drawdowns as the record shows them, where
/// the lognormal model has none of the first two.
#[test]
fn fat_tailed_samples_look_like_the_record() {
    let fat = samples(MarketKind::FatTailed, 300);
    let ln = samples(MarketKind::Lognormal, 300);
    let k = median(fat.kurtosis.clone());
    let a1 = median(fat.ac1.clone());
    let a12 = median(fat.ac12.clone());
    let d50 = median(fat.drawdown.clone());
    let (d05, d95) = (
        quantile(fat.drawdown.clone(), 0.05),
        quantile(fat.drawdown.clone(), 0.95),
    );
    let ln_d10 = quantile(ln.drawdown.clone(), 0.10);
    let fat_d10 = quantile(fat.drawdown.clone(), 0.10);
    eprintln!(
        "fat-tailed: kurtosis {k:.2}, |r| ac1 {a1:.3}, ac12 {a12:.3}, drawdown median {d50:.3} (5% {d05:.3}, 95% {d95:.3}, 10% {fat_d10:.3}); lognormal: kurtosis {:.2}, ac1 {:.3}, drawdown median {:.3}, 10% {ln_d10:.3}",
        median(ln.kurtosis.clone()),
        median(ln.ac1.clone()),
        median(ln.drawdown.clone()),
    );
    let target_k = fx("/series_stats/full_sample/equity_us/nominal/excess_kurtosis_monthly");
    let target_a1 = fx("/autocorrelation/series/equity_us/abs_nominal/1");
    let target_a12 = fx("/autocorrelation/series/equity_us/abs_nominal/12");
    let target_dd = fx("/series_stats/full_sample/equity_us/real/max_drawdown/depth");
    // The fit's targets, at the median of 100-year samples.
    assert!((k - target_k).abs() < 1.5, "kurtosis {k} vs {target_k}");
    assert!((a1 - target_a1).abs() < 0.04, "ac1 {a1} vs {target_a1}");
    assert!(
        (a12 - target_a12).abs() < 0.04,
        "ac12 {a12} vs {target_a12}"
    );
    // The record's -79% real drawdown is a plausible draw, and a deep one.
    assert!(
        target_dd > d05 && target_dd < d95,
        "{target_dd} in {d05}..{d95}"
    );
    // The lognormal model: no excess kurtosis, no clustering, shallower bad tails.
    assert!(median(ln.kurtosis.clone()).abs() < 0.3);
    assert!(median(ln.ac1.clone()).abs() < 0.03);
    assert!(fat_d10 < ln_d10 - 0.05, "{fat_d10} vs {ln_d10}");
}

/// Same means and volatilities as the lognormal model.
#[test]
fn fat_tailed_keeps_the_lognormal_moments() {
    let fat = build(&market(MarketKind::FatTailed));
    let ln = build(&market(MarketKind::Lognormal));
    let months = 720;
    let collect = |m: &EconomyModel| {
        let mut out = EconomyPath::new();
        let mut series: [Vec<f64>; 5] = Default::default();
        for path in 0..400 {
            generate(m, path, months, &mut out);
            series[0].extend_from_slice(&out.returns[US][..months]);
            series[1].extend_from_slice(&out.returns[INTL][..months]);
            series[2].extend_from_slice(&out.returns[BOND][..months]);
            series[3].extend_from_slice(&out.returns[AssetClass::Cash.index()][..months]);
            series[4].extend_from_slice(&out.inflation[..months]);
        }
        series.map(|s| {
            let logs: Vec<f64> = s.iter().map(|&r| math::ln_1p(r)).collect();
            let (m, _, _) = fit::mean_sd(logs.iter().copied());
            let (_, sd, _) = fit::mean_sd(s.iter().copied());
            (12.0 * m, sd * SQRT_12)
        })
    };
    let (f, l) = (collect(&fat), collect(&ln));
    for (i, name) in ["us", "intl", "bond", "bill", "cpi"].iter().enumerate() {
        eprintln!(
            "{name}: log mean {:.4} vs {:.4}, vol {:.4} vs {:.4}",
            f[i].0, l[i].0, f[i].1, l[i].1
        );
        // 288,000 months, but the clustered variance makes the equity means noisy:
        // 1.2 points for stocks, 0.3 for the rest.
        let tol = if i < 2 { 0.012 } else { 0.003 };
        assert!((f[i].0 - l[i].0).abs() < tol, "{name} mean");
        assert!((f[i].1 / l[i].1 - 1.0).abs() < 0.06, "{name} vol");
    }
}

/// The calm and turbulent correlation matrices are the record's, and the simulated
/// months reproduce them when split the way the record was.
#[test]
fn correlations_rise_between_stock_markets_and_fall_with_bonds_in_turbulence() {
    let m = build(&market(MarketKind::FatTailed));
    let Kind::FatTailed(p) = &m.kind else {
        panic!()
    };
    let rec = |state: &str, other: &str| {
        fx(&format!(
            "/equity_regimes/threshold_method/{state}/corr_monthly/corr/equity_us/{other}"
        ))
    };
    // Log against simple returns and pairwise-complete samples: within 0.03.
    for (turbulent, state) in [(false, "calm"), (true, "turbulent")] {
        let intl = p.correlation(turbulent, US, INTL);
        let bond = p.correlation(turbulent, US, BOND);
        assert!(
            (intl - rec(state, "equity_intl")).abs() < 0.03,
            "{state} {intl}"
        );
        assert!((bond - rec(state, "bond10")).abs() < 0.03, "{state} {bond}");
    }
    assert!(p.correlation(true, US, INTL) > p.correlation(false, US, INTL) + 0.1);
    assert!(p.correlation(true, US, BOND) < p.correlation(false, US, BOND) - 0.08);

    // Simulated, split by trailing 12-month volatility as the record was.
    let months = 720;
    let mut out = EconomyPath::new();
    let (mut us, mut intl, mut bond, mut turb) = (vec![], vec![], vec![], vec![]);
    for path in 0..300 {
        generate(&m, path, months, &mut out);
        let r = &out.returns[US][..months];
        turb.extend(fit::threshold_classification(r, 12, 0.75));
        us.extend_from_slice(r);
        intl.extend_from_slice(&out.returns[INTL][..months]);
        bond.extend_from_slice(&out.returns[BOND][..months]);
    }
    let c = |x: &[f64], state: bool| fit::corr_where(&us, x, |t| turb[t] == state);
    let (ci, ti, cb, tb) = (
        c(&intl, false),
        c(&intl, true),
        c(&bond, false),
        c(&bond, true),
    );
    eprintln!(
        "simulated: intl calm {ci:.3} turbulent {ti:.3}; bond calm {cb:.3} turbulent {tb:.3}"
    );
    assert!(ti > ci + 0.08, "{ci} {ti}");
    assert!(tb < cb - 0.05, "{cb} {tb}");
    assert!((ti - rec("turbulent", "equity_intl")).abs() < 0.08);
    assert!((ci - rec("calm", "equity_intl")).abs() < 0.08);
}

/// The fat-tailed model draws from `Market` exactly what the lognormal model draws,
/// so on one path both see the same normal shocks: every US stock month has the
/// same sign relative to its mean, and the streams end in the same state.
#[test]
fn fat_tailed_is_paired_with_lognormal_on_the_market_stream() {
    let fat = build(&market(MarketKind::FatTailed));
    let ln = build(&market(MarketKind::Lognormal));
    let Kind::Lognormal(lp) = &ln.kind else {
        panic!()
    };
    let mu = lp.mu[0];
    let run = |m: &EconomyModel, path: u64| {
        let mut a = Rng::stream(SEED, path, Stream::Market);
        let mut b = Rng::stream(SEED, path, Stream::Inflation);
        let mut c = Rng::stream(SEED, path, Stream::MarketTails);
        let mut out = EconomyPath::new();
        m.generate(
            &mut EconomyStreams {
                market: &mut a,
                inflation: &mut b,
                tails: &mut c,
            },
            path,
            360,
            &mut out,
        );
        (out, a.next_u64(), b.next_u64(), c.next_u64())
    };
    for path in 0..5 {
        let (f, fa, fb, fc) = run(&fat, path);
        let (l, la, lb, lc) = run(&ln, path);
        assert_eq!((fa, fb), (la, lb));
        // The lognormal model never touches the tail stream; the fat-tailed one
        // draws 3 a month from it.
        let mut fresh = Rng::stream(SEED, path, Stream::MarketTails);
        assert_eq!(lc, fresh.next_u64());
        assert_ne!(fc, lc);
        for t in 0..360 {
            let s = |p: &EconomyPath| math::ln_1p(p.returns[US][t]) > mu;
            assert_eq!(s(&f), s(&l), "path {path} month {t}");
        }
        // The variance state differs, so the sizes do.
        assert!(f.returns[US] != l.returns[US]);
    }
}

/// The valuation tilt applies to the fat-tailed model as to the lognormal one: from
/// a CAPE of 40 the first decade's US stock returns are lower on the same shocks.
#[test]
fn fat_tailed_supports_the_valuation_tilt() {
    let off = build(&market(MarketKind::FatTailed));
    let on = build(&MarketModel {
        valuation: Some(crate::model::Valuation::default()),
        ..market(MarketKind::FatTailed)
    });
    let (_, _, tilt) = on.valuation_outlook().unwrap();
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    let mut gap = 0.0;
    for path in 0..50 {
        generate(&off, path, 120, &mut a);
        generate(&on, path, 120, &mut b);
        for t in 0..120 {
            gap += math::ln_1p(a.returns[US][t]) - math::ln_1p(b.returns[US][t]);
        }
    }
    let annual = gap / 50.0 / 10.0;
    eprintln!("valuation shortfall {annual:.4}, expected {tilt:.4}");
    assert!((annual - tilt.abs()).abs() < 0.01, "{annual} vs {tilt}");
}
