//! Economy tests: calibration against `data/history/calibration.json`, structure,
//! determinism, stress overlays, and `#[ignore]`d reports.

#![allow(clippy::needless_range_loop)]

use std::sync::OnceLock;

use super::*;
use crate::rng::Stream;

const SEED: u64 = 0x00F1_2E51_6477;

/// Series tracked by the test statistics, in this order.
const NAMES: [&str; 11] = [
    "equity_us",
    "equity_intl",
    "bond10",
    "tbill",
    "cpi",
    "home",
    "gold",
    "real_estate",
    "crypto",
    "single_stock",
    "custom",
];
const EQ: usize = 0;
const INTL: usize = 1;
const BOND: usize = 2;
const BILL: usize = 3;
const CPI: usize = 4;
const HOME: usize = 5;
const GOLD: usize = 6;
const RE: usize = 7;
const CRYPTO: usize = 8;
const STOCK: usize = 9;
const CUSTOM: usize = 10;
const K: usize = NAMES.len();

/// `(price level after 30 years, sum of all asset returns)` for seed 2026, path 3,
/// for the bootstrap, regime, lognormal and historical models.
const GOLDEN: [(f64, f64); 4] = [
    (1.575_000_426_538_396, 23.881_440_751_523_27),
    (1.161_311_638_006_061, 10.406_777_749_511_308),
    (2.192_109_053_393_755_5, 18.149_064_365_350_938),
    (1.565_714_359_980_529_2, 23.239_197_292_043_3),
];

fn fixture() -> &'static Value {
    static F: OnceLock<Value> = OnceLock::new();
    F.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/history/calibration.json"
        )))
        .unwrap()
    })
}

fn fx(pointer: &str) -> f64 {
    fixture()
        .pointer(pointer)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("calibration.json has no number at {pointer}"))
}

/// The record as it was: the tests compare the models with the history, so the
/// default equity adjustment is switched off.
fn market(kind: MarketKind) -> MarketModel {
    MarketModel {
        kind,
        equity_premium_adjustment: 0.0,
        ..MarketModel::default()
    }
}

fn bootstrap(mean_block_months: u32) -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months,
        from_year: None,
        joins: BootstrapJoins::Uniform,
    }
}

fn model(kind: MarketKind) -> EconomyModel {
    EconomyModel::new(&market(kind), Data::embedded())
}

const ALL_KINDS: [(&str, MarketKind); 4] = [
    (
        "bootstrap",
        MarketKind::Bootstrap {
            mean_block_months: 60,
            from_year: None,
            joins: BootstrapJoins::Uniform,
        },
    ),
    ("regime", MarketKind::Regime),
    ("lognormal", MarketKind::Lognormal),
    ("historical", MarketKind::Historical),
];

fn generate(m: &EconomyModel, seed: u64, path: u64, months: usize, out: &mut EconomyPath) {
    let mut a = Rng::stream(seed, path, Stream::Market);
    let mut b = Rng::stream(seed, path, Stream::Inflation);
    let mut streams = EconomyStreams {
        market: &mut a,
        inflation: &mut b,
    };
    m.generate(&mut streams, path, months, out);
}

fn series_at(p: &EconomyPath, t: usize) -> [f64; K] {
    let r = p.returns_at(t);
    [
        r[AssetClass::UsStocks.index()],
        r[AssetClass::IntlStocks.index()],
        r[AssetClass::Bonds.index()],
        r[AssetClass::Cash.index()],
        p.inflation[t],
        p.home_national[t],
        r[AssetClass::Gold.index()],
        r[AssetClass::RealEstate.index()],
        r[AssetClass::Crypto.index()],
        r[AssetClass::SingleStock.index()],
        r[AssetClass::Custom.index()],
    ]
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    fit::corr(x, y)
}

/// Monthly and calendar-year statistics pooled over many simulated paths.
struct Stats {
    n: f64,
    sum: [f64; K],
    cross: [[f64; K]; K],
    fourth_eq: f64,
    /// Sum over paths of lag-k cross products of demeaned series, for autocorrelation.
    lag1: [f64; K],
    lag12: [f64; K],
    lag_n: [f64; 2],
    /// Non-overlapping 12-month compounded returns, nominal.
    annual: [Vec<f64>; K],
    /// The same deflated by the year's inflation.
    annual_real: [Vec<f64>; K],
    recession_months: f64,
    /// 30-year real CAGR of US equities, one per path (when the path is long enough).
    cagr30: Vec<f64>,
    yield_range: (f64, f64),
}

impl Stats {
    fn collect(m: &EconomyModel, paths: u64, months: usize) -> Stats {
        let mut s = Stats {
            n: 0.0,
            sum: [0.0; K],
            cross: [[0.0; K]; K],
            fourth_eq: 0.0,
            lag1: [0.0; K],
            lag12: [0.0; K],
            lag_n: [0.0; 2],
            annual: Default::default(),
            annual_real: Default::default(),
            recession_months: 0.0,
            cagr30: Vec::new(),
            yield_range: (f64::INFINITY, f64::NEG_INFINITY),
        };
        let mut out = EconomyPath::new();
        let mut all: Vec<[f64; K]> = Vec::with_capacity(months);
        for path in 0..paths {
            generate(m, SEED, path, months, &mut out);
            all.clear();
            let mut gross = [1.0; K];
            for t in 0..months {
                let v = series_at(&out, t);
                all.push(v);
                s.n += 1.0;
                for i in 0..K {
                    s.sum[i] += v[i];
                    for j in 0..=i {
                        s.cross[i][j] += v[i] * v[j];
                    }
                    gross[i] *= 1.0 + v[i];
                }
                s.recession_months += f64::from(out.recession[t]);
                s.yield_range.0 = s.yield_range.0.min(out.long_yield[t]);
                s.yield_range.1 = s.yield_range.1.max(out.long_yield[t]);
                if t % 12 == 11 {
                    for i in 0..K {
                        s.annual[i].push(gross[i] - 1.0);
                        s.annual_real[i].push(gross[i] / gross[CPI] - 1.0);
                    }
                    gross = [1.0; K];
                }
            }
            if months >= 360 {
                let mut real = 1.0;
                for v in &all[..360] {
                    real *= (1.0 + v[EQ]) / (1.0 + v[CPI]);
                }
                s.cagr30.push(math::pow(real, 1.0 / 30.0) - 1.0);
            }
            // Autocorrelation sums use the path's own mean.
            for i in 0..K {
                let mean = all.iter().map(|v| v[i]).sum::<f64>() / months as f64;
                for t in 1..months {
                    s.lag1[i] += (all[t][i] - mean) * (all[t - 1][i] - mean);
                }
                for t in 12..months {
                    s.lag12[i] += (all[t][i] - mean) * (all[t - 12][i] - mean);
                }
            }
            s.lag_n[0] += (months - 1) as f64;
            s.lag_n[1] += months.saturating_sub(12) as f64;
            for v in &all {
                let d = v[EQ];
                s.fourth_eq += d * d * d * d;
            }
        }
        s
    }

    fn mean(&self, i: usize) -> f64 {
        self.sum[i] / self.n
    }

    fn var(&self, i: usize) -> f64 {
        (self.cross[i][i] / self.n - self.mean(i) * self.mean(i)).max(0.0)
    }

    fn sd(&self, i: usize) -> f64 {
        self.var(i).sqrt()
    }

    /// Annualised arithmetic mean (12 x monthly mean), as in calibration.json.
    fn mean_ann(&self, i: usize) -> f64 {
        12.0 * self.mean(i)
    }

    /// Annualised volatility (monthly sd x sqrt 12).
    fn vol_ann(&self, i: usize) -> f64 {
        self.sd(i) * SQRT_12
    }

    fn corr(&self, i: usize, j: usize) -> f64 {
        let (a, b) = if i >= j { (i, j) } else { (j, i) };
        (self.cross[a][b] / self.n - self.mean(a) * self.mean(b)) / (self.sd(a) * self.sd(b))
    }

    fn autocorr1(&self, i: usize) -> f64 {
        self.lag1[i] / self.lag_n[0] / self.var(i)
    }

    fn autocorr12(&self, i: usize) -> f64 {
        self.lag12[i] / self.lag_n[1] / self.var(i)
    }

    fn annual_sd(&self, i: usize, real: bool) -> f64 {
        let v = if real {
            &self.annual_real[i]
        } else {
            &self.annual[i]
        };
        fit::mean_sd(v.iter().copied()).1
    }

    fn annual_corr_real_vs_inflation(&self, i: usize) -> f64 {
        pearson(&self.annual_real[i], &self.annual[CPI])
    }

    fn annual_corr(&self, i: usize, j: usize) -> f64 {
        pearson(&self.annual[i], &self.annual[j])
    }

    fn recession_share(&self) -> f64 {
        self.recession_months / self.n
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let pos = p * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (pos - lo as f64) * (sorted[hi] - sorted[lo])
}

/// Collects every failed comparison so one run shows the whole picture.
#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    fn close(&mut self, what: &str, actual: f64, expected: f64, tol: f64) {
        let ok = (actual - expected).abs() <= tol;
        if !ok {
            self.0.push(format!(
                "{what}: got {actual:.5}, expected {expected:.5} +/- {tol:.5}"
            ));
        }
    }

    /// Relative tolerance.
    fn close_rel(&mut self, what: &str, actual: f64, expected: f64, rel: f64) {
        self.close(what, actual, expected, rel * expected.abs());
    }

    fn within(&mut self, what: &str, actual: f64, lo: f64, hi: f64) {
        let ok = actual >= lo && actual <= hi;
        if !ok {
            self.0
                .push(format!("{what}: got {actual:.5}, expected in [{lo}, {hi}]"));
        }
    }

    fn finish(self) {
        assert!(self.0.is_empty(), "\n{}", self.0.join("\n"));
    }
}

fn history_stat(series: &str, field: &str) -> f64 {
    fx(&format!(
        "/series_stats/full_sample/{series}/nominal/{field}"
    ))
}

fn monthly_corr(a: &str, b: &str) -> f64 {
    fx(&format!("/correlations/monthly_nominal_full/corr/{a}/{b}"))
}

fn excess_kurtosis_eq(s: &Stats) -> f64 {
    s.fourth_eq / s.n / (s.var(EQ) * s.var(EQ)) - 3.0
}

// ---------------------------------------------------------------------------------
// Buffers.
// ---------------------------------------------------------------------------------

#[test]
fn path_buffers_reset_and_convert() {
    let mut p = EconomyPath::with_capacity(24);
    assert_eq!(p.price_level.len(), 25);
    p.inflation.iter_mut().for_each(|x| *x = 0.01);
    p.rebuild_price_level();
    assert!((p.price_level[2] - 1.0201).abs() < 1e-12);
    assert!((p.to_real(p.to_nominal(100.0, 12), 12) - 100.0).abs() < 1e-9);
    p.returns[AssetClass::Bonds.index()][3] = 0.02;
    assert_eq!(p.returns_at(3)[AssetClass::Bonds.index()], 0.02);
    assert_eq!(p.asset_return(AssetClass::Bonds, 3), 0.02);
    p.reset(12);
    assert_eq!((p.months, p.inflation.len(), p.inflation[0]), (12, 12, 0.0));
}

#[test]
fn generate_reuses_buffers_without_reallocating() {
    for (name, kind) in ALL_KINDS {
        let m = model(kind);
        let mut out = EconomyPath::with_capacity(720);
        let pointers = |p: &EconomyPath| {
            let mut v = vec![
                p.inflation.as_ptr() as usize,
                p.price_level.as_ptr() as usize,
                p.tbill.as_ptr() as usize,
                p.long_yield.as_ptr() as usize,
                p.mortgage_rate.as_ptr() as usize,
                p.home_national.as_ptr() as usize,
                p.recession.as_ptr() as usize,
            ];
            v.extend(p.returns.iter().map(|r| r.as_ptr() as usize));
            v
        };
        let before = pointers(&out);
        for (path, months) in [(0, 720), (1, 300), (2, 720), (3, 0), (4, 1), (5, 719)] {
            generate(&m, SEED, path, months, &mut out);
            assert_eq!(out.months, months, "{name}");
            assert_eq!(out.inflation.len(), months);
            assert_eq!(out.price_level.len(), months + 1);
            assert_eq!(out.price_level[0], 1.0);
            assert!(out.returns.iter().all(|r| r.len() == months));
            assert_eq!(out.recession.len(), months);
            assert_eq!(pointers(&out), before, "{name}: a buffer was reallocated");
        }
        // A shorter path after a longer one is the same as generating it fresh.
        let mut fresh = EconomyPath::new();
        generate(&m, SEED, 9, 120, &mut fresh);
        generate(&m, SEED, 8, 720, &mut out);
        generate(&m, SEED, 9, 120, &mut out);
        assert_eq!(out.returns, fresh.returns, "{name}");
        assert_eq!(out.price_level, fresh.price_level, "{name}");
    }
}

// ---------------------------------------------------------------------------------
// Bootstrap.
// ---------------------------------------------------------------------------------

#[test]
fn bootstrap_with_block_one_reproduces_the_unconditional_monthly_moments() {
    let m = model(bootstrap(1));
    let s = Stats::collect(&m, 400, 600);
    let h = &Data::embedded().history;
    let mut c = Checks::default();
    // Independent row draws: the sampled mean and sd of every full-history column
    // must equal the history's, within sampling error of 240,000 draws (4 standard
    // errors for the mean, 3% for the sd).
    let columns: [(usize, &[f64]); 4] = [
        (EQ, &h.equity_us),
        (BOND, &h.bond10),
        (BILL, &h.tbill),
        (CPI, &h.cpi),
    ];
    for (i, column) in columns {
        let (mean, sd, _) = fit::mean_sd(column.iter().copied());
        c.close(
            &format!("mean {}", NAMES[i]),
            s.mean(i),
            mean,
            4.0 * sd / s.n.sqrt(),
        );
        c.close_rel(&format!("sd {}", NAMES[i]), s.sd(i), sd, 0.03);
    }
    // And the published calibration figures (annualised).
    for (i, name) in [
        (EQ, "equity_us"),
        (BOND, "bond10"),
        (BILL, "tbill"),
        (CPI, "cpi"),
        (HOME, "home"),
    ] {
        c.close_rel(
            &format!("vol_ann {name}"),
            s.vol_ann(i),
            history_stat(name, "vol_ann"),
            0.03,
        );
        c.close(
            &format!("mean_ann {name}"),
            s.mean_ann(i),
            history_stat(name, "mean_arith_ann"),
            4.0 * history_stat(name, "vol_ann") / (s.n / 12.0).sqrt() + 1e-4,
        );
    }
    // Rows are sampled whole, so same-month correlations survive.
    for (i, j, a, b) in [
        (EQ, BOND, "equity_us", "bond10"),
        (EQ, CPI, "equity_us", "cpi"),
        (CPI, HOME, "cpi", "home"),
        (CPI, BILL, "cpi", "tbill"),
        (BOND, BILL, "bond10", "tbill"),
    ] {
        c.close(
            &format!("corr {a}/{b}"),
            s.corr(i, j),
            monthly_corr(a, b),
            0.02,
        );
    }
    // But all serial dependence is gone.
    for i in [EQ, CPI, HOME, BILL] {
        c.close(
            &format!("lag-1 autocorrelation {}", NAMES[i]),
            s.autocorr1(i),
            0.0,
            0.01,
        );
    }
    c.close(
        "recession share",
        s.recession_share(),
        fx("/business_cycle/durations_full_sample/recession_months_share"),
        0.005,
    );
    c.finish();
}

#[test]
fn block_bootstrap_matches_calibration_and_keeps_the_joint_structure() {
    let m = model(bootstrap(60));
    let s = Stats::collect(&m, 400, 720);
    let mut c = Checks::default();
    // Annualised means within 0.6 points (equities 1.2), volatilities within 6%.
    for (i, name, tol) in [
        (EQ, "equity_us", 0.012),
        (BOND, "bond10", 0.006),
        (BILL, "tbill", 0.006),
        (CPI, "cpi", 0.006),
        (HOME, "home", 0.006),
    ] {
        c.close(
            &format!("mean_ann {name}"),
            s.mean_ann(i),
            history_stat(name, "mean_arith_ann"),
            tol,
        );
        c.close_rel(
            &format!("vol_ann {name}"),
            s.vol_ann(i),
            history_stat(name, "vol_ann"),
            0.06,
        );
    }
    for (i, j, a, b) in [
        (EQ, BOND, "equity_us", "bond10"),
        (EQ, CPI, "equity_us", "cpi"),
        (CPI, HOME, "cpi", "home"),
        (CPI, BILL, "cpi", "tbill"),
    ] {
        c.close(
            &format!("corr {a}/{b}"),
            s.corr(i, j),
            monthly_corr(a, b),
            0.04,
        );
    }
    // International equities are real from 1975 and synthesised before; the pooled
    // correlation with US equities stays near the 1975+ figure.
    c.close(
        "corr equity_us/equity_intl",
        s.corr(EQ, INTL),
        monthly_corr("equity_us", "equity_intl"),
        0.09,
    );
    // The joint structure that needs blocks: calendar-year real bond returns against
    // inflation (-0.50 in history), home prices against inflation (+0.56), real
    // equities against inflation (-0.18).
    c.close(
        "annual corr real bond10 / cpi",
        s.annual_corr_real_vs_inflation(BOND),
        fx("/correlations/annual_real_full/corr/bond10/cpi"),
        0.08,
    );
    c.close(
        "annual corr home / cpi",
        s.annual_corr(HOME, CPI),
        fx("/correlations/annual_nominal_full/corr/cpi/home"),
        0.08,
    );
    c.close(
        "annual corr real equity_us / cpi",
        s.annual_corr_real_vs_inflation(EQ),
        fx("/correlations/annual_real_full/corr/equity_us/cpi"),
        0.08,
    );
    // Serial dependence inside blocks: inflation and home-price persistence, bill
    // rates as a slow level.
    c.close(
        "lag-1 autocorrelation cpi",
        s.autocorr1(CPI),
        fx("/autocorrelation/series/cpi/nominal/1"),
        0.06,
    );
    c.close(
        "lag-12 autocorrelation cpi",
        s.autocorr12(CPI),
        fx("/autocorrelation/series/cpi/nominal/12"),
        0.10,
    );
    c.close(
        "lag-1 autocorrelation home",
        s.autocorr1(HOME),
        fx("/autocorrelation/series/home/nominal/1"),
        0.06,
    );
    c.within("lag-1 autocorrelation tbill", s.autocorr1(BILL), 0.8, 1.0);
    // Calendar-year volatility, which the smoothed monthly series understate.
    c.close_rel(
        "annual sd home",
        s.annual_sd(HOME, false),
        fx("/annual_return_stats/nominal/home/sd"),
        0.12,
    );
    c.close_rel(
        "annual sd cpi",
        s.annual_sd(CPI, false),
        fx("/annual_return_stats/nominal/cpi/sd"),
        0.12,
    );
    c.close_rel(
        "annual sd equity_us",
        s.annual_sd(EQ, false),
        fx("/annual_return_stats/nominal/equity_us/sd"),
        0.10,
    );
    c.close(
        "recession share",
        s.recession_share(),
        fx("/business_cycle/durations_full_sample/recession_months_share"),
        0.02,
    );
    c.finish();
}

/// Index of the history row whose (equity, cpi, bond) triple equals month `t` of the
/// path, if any.
fn find_row(h: &History, p: &EconomyPath, t: usize) -> Option<usize> {
    let r = p.returns_at(t);
    (0..h.len()).find(|&i| {
        h.equity_us[i] == r[AssetClass::UsStocks.index()]
            && h.cpi[i] == p.inflation[t]
            && h.bond10[i] == r[AssetClass::Bonds.index()]
    })
}

#[test]
fn bootstrap_samples_whole_rows_in_geometric_blocks_with_circular_wrap() {
    let h = &Data::embedded().history;
    let n = h.len();
    let m = model(bootstrap(60));
    let mut out = EconomyPath::new();
    let (mut steps, mut continued, mut wrapped) = (0usize, 0usize, 0usize);
    let mut first_rows = Vec::new();
    for path in 0..40 {
        generate(&m, SEED, path, 720, &mut out);
        let rows: Vec<usize> = (0..720)
            .map(|t| find_row(h, &out, t).expect("every month is a historical row"))
            .collect();
        first_rows.push(rows[0]);
        // Rate levels are carried, not sampled: every path starts from the last
        // row of the history, whatever row its returns start from.
        assert_eq!(out.long_yield[0], h.long_yield[n - 1]);
        assert_eq!(out.mortgage_rate[0], h.mortgage30[n - 1]);
        let (k_intl, k_gold) = m.base.short_scale;
        for t in 0..720 {
            let i = rows[t];
            // Everything in the month comes from that one row.
            assert_eq!(out.returns_at(t)[AssetClass::Cash.index()], h.tbill[i]);
            assert_eq!(out.recession[t], h.recession[i]);
            if h.home[i].is_finite() {
                assert_eq!(out.home_national[t], h.home[i]);
            }
            // International equities and gold: the row's return, at the assumed
            // long-run level (one constant factor on every month).
            if h.equity_intl[i].is_finite() {
                assert_eq!(
                    out.returns_at(t)[AssetClass::IntlStocks.index()],
                    (1.0 + h.equity_intl[i]) * k_intl - 1.0
                );
            }
            if h.gold[i].is_finite() {
                assert_eq!(
                    out.returns_at(t)[AssetClass::Gold.index()],
                    (1.0 + h.gold[i]) * k_gold - 1.0
                );
            }
            if t > 0 && i > 0 {
                // The long yield moves by the sampled month's own change plus a
                // weak pull to its mean: never by a jump between unrelated rows.
                let own = h.long_yield[i] - h.long_yield[i - 1];
                let step = out.long_yield[t] - out.long_yield[t - 1];
                assert!(
                    (step - own).abs() < 0.0012 || out.long_yield[t] <= super::MIN_LONG_YIELD,
                    "month {t}: yield moved {step}, the row moved {own}"
                );
                let spread = out.mortgage_rate[t] - out.long_yield[t];
                assert!(spread > 0.0049 && spread < 0.0401, "{spread}");
            }
            if t > 0 {
                steps += 1;
                if i == (rows[t - 1] + 1) % n {
                    continued += 1;
                    wrapped += usize::from(i == 0);
                }
            }
        }
    }
    // Restart probability 1/60 (a restart lands on the next row by chance 1 in 1203).
    let share = continued as f64 / steps as f64;
    assert!((share - (1.0 - 1.0 / 60.0)).abs() < 0.004, "{share}");
    // Starting rows are spread over the whole history, including its ends.
    assert!(first_rows.iter().any(|&r| r < n / 4) && first_rows.iter().any(|&r| r > 3 * n / 4));
    // A block that reaches the last row continues at the first (circular).
    let long_blocks = model(bootstrap(100_000));
    let mut seen_wrap = wrapped > 0;
    for path in 0..200 {
        if seen_wrap {
            break;
        }
        generate(&long_blocks, SEED, path, 1300, &mut out);
        let start = find_row(h, &out, 0).unwrap();
        let to_end = n - 1 - start;
        if to_end + 1 < 1300 && find_row(h, &out, to_end) == Some(n - 1) {
            assert_eq!(find_row(h, &out, to_end + 1), Some(0));
            seen_wrap = true;
        }
    }
    assert!(seen_wrap);
}

#[test]
fn bootstrap_respects_from_year_and_clamps_silly_values() {
    let data = Data::embedded();
    let h = &data.history;
    let from = |year: i32, block: u32| {
        EconomyModel::new(
            &market(MarketKind::Bootstrap {
                mean_block_months: block,
                from_year: Some(year),
                joins: BootstrapJoins::Uniform,
            }),
            data,
        )
    };
    let mut out = EconomyPath::new();
    let first_1990 = h.index_of(YearMonth::ym(1990, 1)).unwrap();
    let m = from(1990, 24);
    assert_eq!(m.rows.len(), h.len() - first_1990);
    for path in 0..20 {
        generate(&m, SEED, path, 600, &mut out);
        for t in 0..600 {
            let i = find_row(h, &out, t).unwrap();
            assert!(i >= first_1990, "month {t} came from row {i}");
            // From 1990 nothing is synthesised.
            let (k_intl, k_gold) = m.base.short_scale;
            assert_eq!(
                out.returns_at(t)[AssetClass::IntlStocks.index()],
                (1.0 + h.equity_intl[i]) * k_intl - 1.0
            );
            assert_eq!(
                out.returns_at(t)[AssetClass::Gold.index()],
                (1.0 + h.gold[i]) * k_gold - 1.0
            );
        }
    }
    // Before the data: everything. After it: the last ten years.
    assert_eq!(from(1800, 12).rows.len(), h.len());
    assert_eq!(from(1926, 12).rows.len(), h.len());
    assert_eq!(from(2500, 12).rows.len(), MIN_BOOTSTRAP_MONTHS);
    assert_eq!(from(i32::MAX, 12).rows.len(), MIN_BOOTSTRAP_MONTHS);
    assert_eq!(from(i32::MIN, 12).rows.len(), h.len());
    // Block lengths of 0 and 1 are both independent months; a huge block is one
    // contiguous replay.
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    generate(&model(bootstrap(0)), SEED, 3, 240, &mut a);
    generate(&model(bootstrap(1)), SEED, 3, 240, &mut b);
    assert_eq!(a.returns, b.returns);
    generate(&model(bootstrap(u32::MAX)), SEED, 3, 240, &mut a);
    let start = find_row(h, &a, 0).unwrap();
    assert_eq!(find_row(h, &a, 239), Some((start + 239) % h.len()));
}

#[test]
fn short_history_columns_are_synthesised_from_factors_only_where_missing() {
    let data = Data::embedded();
    let h = &data.history;
    let m = model(MarketKind::Historical);
    let (intl, intl_sd) = m.base.intl_fit;
    let (gold, gold_sd) = m.base.gold_fit;
    // The factor fits: international equities load on US equities with a beta near
    // 0.7 (correlation 0.67, similar volatility); gold loads on inflation, not on
    // equities.
    assert!(intl[1] > 0.6 && intl[1] < 0.85, "{intl:?}");
    assert!(gold[1].abs() < 0.15 && gold[3] > 0.5, "{gold:?}");
    assert!(intl_sd > 0.02 && intl_sd < 0.05 && gold_sd > 0.03 && gold_sd < 0.06);
    // Residual sd is no more than sqrt(1 - R^2) x sample sd of the one-factor fit.
    let sample_sd = fit::mean_sd(h.equity_intl.iter().copied()).1;
    let r = fit::corr(&h.equity_intl, &h.equity_us);
    assert!(intl_sd <= sample_sd * (1.0 - r * r).sqrt() * 1.01);

    let first_intl = h.index_of(YearMonth::ym(1975, 1)).unwrap();
    let first_gold = h.index_of(YearMonth::ym(1968, 4)).unwrap();
    let months = h.len();
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    generate(&m, 1, 0, months, &mut a);
    generate(&m, 2, 0, months, &mut b);
    let (ia, ib) = (
        &a.returns[AssetClass::IntlStocks.index()],
        &b.returns[AssetClass::IntlStocks.index()],
    );
    let (ga, gb) = (
        &a.returns[AssetClass::Gold.index()],
        &b.returns[AssetClass::Gold.index()],
    );
    assert!(ia.iter().chain(ga).all(|v| v.is_finite() && *v > -1.0));
    // Real months: the data, whatever the seed. Synthesised months: seed-dependent.
    assert_eq!(&ia[first_intl..], &h.equity_intl[first_intl..]);
    assert_eq!(&ia[first_intl..], &ib[first_intl..]);
    assert_eq!(&ga[first_gold..], &h.gold[first_gold..]);
    assert!(ia[..first_intl]
        .iter()
        .zip(&ib[..first_intl])
        .all(|(x, y)| x != y));
    assert!(ga[..first_gold]
        .iter()
        .zip(&gb[..first_gold])
        .all(|(x, y)| x != y));
    // The synthesised stretch looks like the asset: similar volatility to the real
    // sample (the 1930s make it somewhat higher), the fitted link to its factors,
    // and a mean implied by the factors.
    let synth = &ia[..first_intl];
    let (mean, sd, _) = fit::mean_sd(synth.iter().copied());
    assert!(
        sd / sample_sd > 0.8 && sd / sample_sd < 1.5,
        "{sd} vs {sample_sd}"
    );
    assert!(mean * 12.0 > 0.02 && mean * 12.0 < 0.16, "{mean}");
    let link = fit::corr(synth, &h.equity_us[..first_intl]);
    assert!(link > 0.6 && link < 0.95, "{link}");
    let gold_link = fit::corr(&ga[..first_gold], &h.cpi[..first_gold]);
    assert!(gold_link > 0.05 && gold_link < 0.5, "{gold_link}");
}

// ---------------------------------------------------------------------------------
// Historical cohorts.
// ---------------------------------------------------------------------------------

#[test]
fn historical_cohort_zero_equals_the_data_and_cohorts_wrap() {
    let data = Data::embedded();
    let h = &data.history;
    let n = h.len();
    let m = model(MarketKind::Historical);
    assert_eq!(m.cohort_count(), n);
    assert_eq!(model(bootstrap(60)).cohort_count(), 0);
    assert_eq!(model(MarketKind::Regime).cohort_count(), 0);
    assert_eq!(model(MarketKind::Lognormal).cohort_count(), 0);

    let mut out = EconomyPath::new();
    generate(&m, SEED, 0, n + 24, &mut out);
    let r = &out.returns;
    assert_eq!(&r[AssetClass::UsStocks.index()][..n], &h.equity_us[..]);
    assert_eq!(&r[AssetClass::Bonds.index()][..n], &h.bond10[..]);
    assert_eq!(&r[AssetClass::Cash.index()][..n], &h.tbill[..]);
    assert_eq!(&out.inflation[..n], &h.cpi[..]);
    assert_eq!(&out.long_yield[..n], &h.long_yield[..]);
    assert_eq!(&out.mortgage_rate[..n], &h.mortgage30[..]);
    assert_eq!(&out.recession[..n], &h.recession[..]);
    // Home prices: the one blank (the latest month) repeats the month before.
    assert_eq!(&out.home_national[..n - 1], &h.home[..n - 1]);
    assert!(h.home[n - 1].is_nan());
    assert_eq!(out.home_national[n - 1], h.home[n - 2]);
    // The price level is the compounded CPI: 1926-07 to 2026-09 is about 18.6x.
    let cumulative: f64 = h.cpi.iter().map(|c| 1.0 + c).product();
    assert!((out.price_level[n] / cumulative - 1.0).abs() < 1e-12);
    assert!(cumulative > 15.0 && cumulative < 22.0);
    // The bill yield is the bill return annualised.
    for t in [0, 500, n - 1] {
        assert!((out.tbill[t] - (math::powi(1.0 + h.tbill[t], 12) - 1.0)).abs() < 1e-15);
        assert!((math::annual_to_monthly(out.tbill[t]) - h.tbill[t]).abs() < 1e-12);
    }
    // When history runs out the path wraps to the first row.
    for t in 0..24 {
        assert_eq!(r[AssetClass::UsStocks.index()][n + t], h.equity_us[t]);
        assert_eq!(out.inflation[n + t], h.cpi[t]);
    }
    // Cohort k starts at row k; path indices beyond the cohort count wrap.
    let k = h.index_of(YearMonth::ym(1966, 1)).unwrap();
    let mut cohort = EconomyPath::new();
    generate(&m, SEED, k as u64, 360, &mut cohort);
    assert_eq!(
        &cohort.returns[AssetClass::UsStocks.index()][..],
        &h.equity_us[k..k + 360]
    );
    let mut wrapped = EconomyPath::new();
    generate(&m, SEED, (n + k) as u64, 360, &mut wrapped);
    assert_eq!(
        wrapped.returns[AssetClass::UsStocks.index()],
        cohort.returns[AssetClass::UsStocks.index()]
    );
    // The 1966 cohort is the famous bad one: real equity value after 16 years
    // (to the end of 1981) is below where it started.
    let real: f64 = (0..192)
        .map(|t| (1.0 + cohort.returns[0][t]) / (1.0 + cohort.inflation[t]))
        .product();
    assert!(real < 1.0, "{real}");
}

// ---------------------------------------------------------------------------------
// Parametric models.
// ---------------------------------------------------------------------------------

#[test]
fn regime_calibration_reproduces_the_published_hmm_fit() {
    let p = RegimeParams::calibrate(
        &Data::embedded().history,
        &super::long_run_from_json(&Data::embedded().actuarial),
    );
    let hmm = "/equity_regimes/gaussian_hmm";
    let mut c = Checks::default();
    c.close(
        "calm mu",
        p.hmm.mu[0],
        fx(&format!("{hmm}/calm/mu_monthly")),
        2e-4,
    );
    c.close(
        "turbulent mu",
        p.hmm.mu[1],
        fx(&format!("{hmm}/turbulent/mu_monthly")),
        5e-4,
    );
    c.close(
        "calm sigma",
        p.hmm.sigma[0],
        fx(&format!("{hmm}/calm/sigma_monthly")),
        2e-4,
    );
    c.close(
        "turbulent sigma",
        p.hmm.sigma[1],
        fx(&format!("{hmm}/turbulent/sigma_monthly")),
        5e-4,
    );
    c.close(
        "P(calm to turbulent)",
        p.hmm.trans[0][1],
        fx(&format!(
            "{hmm}/hmm_transition_matrix_rows_from_calm_turbulent/0/1"
        )),
        5e-4,
    );
    c.close(
        "P(turbulent to calm)",
        p.hmm.trans[1][0],
        fx(&format!(
            "{hmm}/hmm_transition_matrix_rows_from_calm_turbulent/1/0"
        )),
        2e-3,
    );
    c.close(
        "log-likelihood",
        p.hmm.loglik,
        fx(&format!("{hmm}/loglik")),
        0.01,
    );
    c.close(
        "stationary turbulent share",
        p.hmm.stationary_turbulent(),
        fx(&format!("{hmm}/hmm_stationary_turbulent_prob")),
        2e-3,
    );
    c.close(
        "calm months",
        p.calm_months as f64,
        fx(&format!("{hmm}/classified/calm/months")),
        3.0,
    );
    // Structure of the fitted dynamics.
    c.within("inflation persistence", p.inflation.phi, 0.9, 0.99);
    c.within("inflation gain", p.inflation.gain, 0.05, 0.5);
    c.close(
        "inflation mean (annual)",
        12.0 * p.inflation_mean,
        history_stat("cpi", "mean_arith_ann"),
        1e-5,
    );
    c.close(
        "yield mean",
        p.yield_mean,
        fx("/levels/long_yield/mean"),
        1e-5,
    );
    c.within("yield reversion", -p.yield_reversion, 0.003, 0.05);
    c.within(
        "yield pass-through",
        p.yield_inflation / -p.yield_reversion,
        0.0,
        1.0,
    );
    c.within("home momentum persistence", p.home.phi, 0.8, 0.995);
    // Recessions are tied to the turbulent state: far likelier to start and to
    // continue there.
    c.within(
        "P(recession starts | calm)",
        p.recession_prob[0][0],
        0.003,
        0.03,
    );
    c.within(
        "P(recession starts | turbulent)",
        p.recession_prob[0][1],
        0.05,
        0.2,
    );
    c.within(
        "P(recession continues | turbulent)",
        p.recession_prob[1][1],
        0.9,
        1.0,
    );
    // Turbulent months: lower returns and higher volatility for both equity series,
    // more volatile inflation surprises.
    c.within("intl turbulent mean", p.mean[1][1], -0.06, -0.01);
    c.within("intl vol ratio", p.sd[1][1] / p.sd[0][1], 1.4, 3.0);
    c.within("cpi surprise vol ratio", p.sd[1][5] / p.sd[0][5], 1.1, 2.5);
    // The path starts from the latest state in the data.
    c.close(
        "start yield",
        p.start.long_yield,
        fx("/levels/latest/long_yield"),
        1e-9,
    );
    c.close(
        "start spread",
        p.start.spread,
        fx("/levels/latest/long_yield") - 12.0 * Data::embedded().history.tbill.last().unwrap(),
        1e-9,
    );
    c.within(
        "start expected inflation (annual)",
        12.0 * p.start.expected_inflation,
        0.02,
        0.06,
    );
    c.finish();
}

#[test]
fn bond_return_formula_matches_published_examples_and_the_history() {
    // Duration of a 10-year par bond at 4%: (1 - 1.02^-20) / 0.04 = 8.1757.
    assert!((regime::par_bond_duration(0.04) - 8.175_717).abs() < 1e-5);
    assert!((regime::par_bond_duration(0.0) - 10.0).abs() < 1e-12);
    // At zero yield the bond is a 10-year zero: convexity 20 x 21 / 4 = 105.
    assert!((regime::par_bond_convexity(0.0) - 105.0).abs() < 1e-9);
    // At 4% the 10-year par bond's convexity is in the high 70s (years squared).
    let c4 = regime::par_bond_convexity(0.04);
    assert!(c4 > 74.0 && c4 < 82.0, "{c4}");
    let h = &Data::embedded().history;
    let p = RegimeParams::calibrate(h, &super::long_run_from_json(&Data::embedded().actuarial));
    // Carry only when the yield does not move; a 1-point rise at 4% costs duration
    // less the convexity gain.
    assert!((p.bond_return(0.04, 0.0) - 0.04 / 12.0).abs() < 1e-12);
    let hit = p.bond_return(0.04, 0.01);
    assert!(
        (hit - (0.04 / 12.0 - 0.081_757_17 + 0.5 * c4 * 1e-4)).abs() < 1e-7,
        "{hit}"
    );
    assert!(hit < -0.07 && hit > -0.08);
    // The bond10 column is this bond repriced exactly from month-end yields since
    // 1962-02; the duration-convexity form agrees closely in every month (README:
    // largest gap 0.052% for the textbook form).
    let first = h.index_of(YearMonth::ym(1962, 3)).unwrap();
    let mut worst: f64 = 0.0;
    for t in first..h.len() {
        let model = p.bond_return(h.long_yield[t - 1], h.long_yield[t] - h.long_yield[t - 1]);
        worst = worst.max((model - h.bond10[t]).abs());
    }
    assert!(worst < 1.2e-3, "{worst}");
}

#[test]
fn regime_model_matches_calibration_within_tolerance() {
    let m = model(MarketKind::Regime);
    let s = Stats::collect(&m, 400, 720);
    let mut c = Checks::default();
    // Annualised means: equities within 1.2 points, the rest within 0.8. Bills and
    // bonds run slightly above history because paths start from today's 5.3% yield
    // and the bill rate is floored at zero.
    for (i, name, tol) in [
        (EQ, "equity_us", 0.012),
        (BOND, "bond10", 0.008),
        (BILL, "tbill", 0.008),
        (CPI, "cpi", 0.005),
        (HOME, "home", 0.008),
    ] {
        c.close(
            &format!("mean_ann {name}"),
            s.mean_ann(i),
            history_stat(name, "mean_arith_ann"),
            tol,
        );
    }
    // Volatilities: equities and inflation within 8%, bonds within 15% (the sample
    // mixes smoothed pre-1962 yields, 6.5%, with month-end yields, 7.8%), home
    // prices within 25% (monthly), bills within 35%.
    for (i, name, rel) in [
        (EQ, "equity_us", 0.08),
        (CPI, "cpi", 0.08),
        (BOND, "bond10", 0.15),
        (HOME, "home", 0.25),
        (BILL, "tbill", 0.35),
    ] {
        c.close_rel(
            &format!("vol_ann {name}"),
            s.vol_ann(i),
            history_stat(name, "vol_ann"),
            rel,
        );
    }
    // International equities and gold against their own samples.
    c.close(
        "mean_ann equity_intl",
        s.mean_ann(INTL),
        history_stat("equity_intl", "mean_arith_ann"),
        0.02,
    );
    c.close_rel(
        "vol_ann equity_intl",
        s.vol_ann(INTL),
        history_stat("equity_intl", "vol_ann"),
        0.15,
    );
    // Gold follows the long-run assumption (1% real, geometric), not its 1968+
    // sample mean: inflation + 1% + half its variance.
    c.close(
        "mean_ann gold",
        s.mean_ann(GOLD),
        s.mean_ann(CPI) + 0.01 + 0.5 * s.vol_ann(GOLD) * s.vol_ann(GOLD),
        0.01,
    );
    c.close_rel(
        "vol_ann gold",
        s.vol_ann(GOLD),
        history_stat("gold", "vol_ann"),
        0.10,
    );
    // Correlations.
    c.close(
        "corr equity_us/equity_intl",
        s.corr(EQ, INTL),
        monthly_corr("equity_us", "equity_intl"),
        0.08,
    );
    c.close(
        "corr equity_us/bond10",
        s.corr(EQ, BOND),
        monthly_corr("equity_us", "bond10"),
        0.08,
    );
    c.close(
        "corr equity_us/cpi",
        s.corr(EQ, CPI),
        monthly_corr("equity_us", "cpi"),
        0.08,
    );
    c.close(
        "corr cpi/home",
        s.corr(CPI, HOME),
        monthly_corr("cpi", "home"),
        0.20,
    );
    c.close(
        "annual corr real bond10 / cpi",
        s.annual_corr_real_vs_inflation(BOND),
        fx("/correlations/annual_real_full/corr/bond10/cpi"),
        0.17,
    );
    c.close(
        "annual corr home / cpi",
        s.annual_corr(HOME, CPI),
        fx("/correlations/annual_nominal_full/corr/cpi/home"),
        0.18,
    );
    // Dynamics: inflation persistence, home-price momentum, fat equity tails,
    // recessions about one month in six.
    c.close(
        "lag-1 autocorrelation cpi",
        s.autocorr1(CPI),
        fx("/autocorrelation/series/cpi/nominal/1"),
        0.06,
    );
    c.close(
        "lag-12 autocorrelation cpi",
        s.autocorr12(CPI),
        fx("/autocorrelation/series/cpi/nominal/12"),
        0.08,
    );
    c.close(
        "lag-1 autocorrelation home",
        s.autocorr1(HOME),
        fx("/autocorrelation/series/home_from_1953/nominal/1"),
        0.08,
    );
    c.close(
        "lag-12 autocorrelation home",
        s.autocorr12(HOME),
        fx("/autocorrelation/series/home_from_1953/nominal/12"),
        0.15,
    );
    c.within("annual sd home", s.annual_sd(HOME, false), 0.045, 0.085);
    c.within("annual sd cpi", s.annual_sd(CPI, false), 0.033, 0.052);
    c.within("annual sd equity_us", s.annual_sd(EQ, false), 0.19, 0.23);
    c.within(
        "excess kurtosis equity_us",
        excess_kurtosis_eq(&s),
        4.0,
        14.0,
    );
    c.close(
        "recession share",
        s.recession_share(),
        fx("/business_cycle/durations_full_sample/recession_months_share"),
        0.04,
    );
    c.within("lowest yield", s.yield_range.0, 0.0025, 0.02);
    c.within("highest yield", s.yield_range.1, 0.08, 0.25);
    c.finish();
}

#[test]
fn regime_recessions_cluster_in_turbulent_markets() {
    // In recession months equities do much worse and are more volatile, as in the
    // NBER-conditional statistics of the history (-2% vs +14% a year, 29% vs 15% vol).
    let m = model(MarketKind::Regime);
    let mut out = EconomyPath::new();
    let mut sums = [[0.0f64; 3]; 2];
    let (mut spells, mut recession_months) = (0usize, 0usize);
    for path in 0..300 {
        generate(&m, SEED, path, 720, &mut out);
        for t in 0..720 {
            let k = usize::from(out.recession[t]);
            let r = out.returns[0][t];
            sums[k][0] += 1.0;
            sums[k][1] += r;
            sums[k][2] += r * r;
            if out.recession[t] == 1 {
                recession_months += 1;
                if t == 0 || out.recession[t - 1] == 0 {
                    spells += 1;
                }
            }
        }
    }
    let stat = |k: usize| {
        let mean = sums[k][1] / sums[k][0];
        (
            12.0 * mean,
            (sums[k][2] / sums[k][0] - mean * mean).sqrt() * SQRT_12,
        )
    };
    let (expansion, recession) = (stat(0), stat(1));
    assert!(
        recession.0 < expansion.0 - 0.08,
        "{recession:?} vs {expansion:?}"
    );
    assert!(
        recession.1 > 1.3 * expansion.1,
        "{recession:?} vs {expansion:?}"
    );
    // Average recession length near the historical 12.6 months.
    let average = recession_months as f64 / spells as f64;
    assert!(average > 8.0 && average < 18.0, "{average}");
}

#[test]
fn lognormal_model_matches_full_sample_moments_and_has_no_memory() {
    let m = model(MarketKind::Lognormal);
    let s = Stats::collect(&m, 400, 720);
    let mut c = Checks::default();
    // 288,000 independent months: means within 4 standard errors, vols within 3%.
    for (i, name) in [
        (EQ, "equity_us"),
        (INTL, "equity_intl"),
        (BOND, "bond10"),
        (BILL, "tbill"),
        (CPI, "cpi"),
        (HOME, "home"),
        (GOLD, "gold"),
    ] {
        let vol = history_stat(name, "vol_ann");
        // International equities and gold follow the long-run assumptions, not
        // their short samples: US equities less 1 point and inflation plus 1 point
        // (geometric), i.e. arithmetic means shifted by half the variances.
        let half_var = |v: f64| 0.5 * v * v;
        let expected = match i {
            INTL => {
                history_stat("equity_us", "mean_arith_ann")
                    - half_var(history_stat("equity_us", "vol_ann"))
                    - 0.01
                    + half_var(vol)
            }
            GOLD => {
                history_stat("cpi", "mean_arith_ann") - half_var(history_stat("cpi", "vol_ann"))
                    + 0.01
                    + half_var(vol)
            }
            _ => history_stat(name, "mean_arith_ann"),
        };
        c.close(
            &format!("mean_ann {name}"),
            s.mean_ann(i),
            expected,
            4.0 * vol / (s.n / 12.0).sqrt() + if matches!(i, INTL | GOLD) { 3e-3 } else { 4e-4 },
        );
        // (The model matches the sd of log returns; for skewed gold the sd of simple
        // returns then differs by a few percent. Bills are floored at zero, which
        // takes about a tenth off their volatility.)
        c.close_rel(
            &format!("vol_ann {name}"),
            s.vol_ann(i),
            vol,
            if i == BILL { 0.2 } else { 0.05 },
        );
    }
    // No month has a negative bill return.
    let mut out = EconomyPath::new();
    generate(&m, SEED, 0, 720, &mut out);
    assert!(out.returns[AssetClass::Cash.index()]
        .iter()
        .all(|r| *r >= 0.0));
    for (i, j, a, b) in [
        (EQ, INTL, "equity_us", "equity_intl"),
        (EQ, BOND, "equity_us", "bond10"),
        (EQ, CPI, "equity_us", "cpi"),
        (CPI, HOME, "cpi", "home"),
        (CPI, GOLD, "cpi", "gold"),
        (CPI, BILL, "cpi", "tbill"),
    ] {
        c.close(
            &format!("corr {a}/{b}"),
            s.corr(i, j),
            monthly_corr(a, b),
            0.03,
        );
    }
    // The naive baseline has no serial dependence and thin tails, which is what the
    // other models are compared against.
    for i in [EQ, BOND, CPI, HOME, BILL] {
        c.close(
            &format!("lag-1 autocorrelation {}", NAMES[i]),
            s.autocorr1(i),
            0.0,
            0.01,
        );
    }
    c.within(
        "excess kurtosis equity_us",
        excess_kurtosis_eq(&s),
        -0.1,
        1.0,
    );
    // So calendar-year inflation and home prices are far too quiet (history: 3.9%
    // and 5.6%) and bonds lose much of their inflation link.
    c.within("annual sd cpi", s.annual_sd(CPI, false), 0.015, 0.022);
    c.within("annual sd home", s.annual_sd(HOME, false), 0.017, 0.025);
    c.within(
        "annual corr real bond10 / cpi",
        s.annual_corr_real_vs_inflation(BOND),
        -0.45,
        -0.2,
    );
    c.close(
        "recession share",
        s.recession_share(),
        fx("/business_cycle/durations_full_sample/recession_months_share"),
        0.03,
    );
    c.close(
        "constant long yield",
        s.yield_range.0,
        fx("/levels/long_yield/mean"),
        1e-5,
    );
    c.close("constant long yield", s.yield_range.1, s.yield_range.0, 0.0);
    c.finish();
}

// ---------------------------------------------------------------------------------
// Derived asset classes.
// ---------------------------------------------------------------------------------

#[test]
fn derived_assets_have_their_documented_risk_and_return() {
    for (name, kind) in [("bootstrap", bootstrap(60)), ("regime", MarketKind::Regime)] {
        let m = model(kind);
        let s = Stats::collect(&m, 500, 720);
        let mut c = Checks::default();
        let eq_vol = s.vol_ann(EQ);
        let premium = s.mean_ann(EQ) - s.mean_ann(BILL);

        // Listed real estate: beta 0.70, total volatility about 19%, no alpha.
        let re_beta = s.corr(EQ, RE) * s.vol_ann(RE) / eq_vol;
        c.close(
            &format!("{name}: real estate beta"),
            re_beta,
            REIT_EQUITY_BETA,
            0.03,
        );
        c.within(
            &format!("{name}: real estate volatility"),
            s.vol_ann(RE),
            0.17,
            0.21,
        );
        c.within(
            &format!("{name}: real estate equity correlation"),
            s.corr(EQ, RE),
            0.6,
            0.75,
        );
        c.close(
            &format!("{name}: real estate mean"),
            s.mean_ann(RE),
            s.mean_ann(BILL) + REIT_EQUITY_BETA * premium,
            0.006,
        );
        c.within(
            &format!("{name}: real estate / home correlation"),
            s.corr(HOME, RE),
            0.0,
            0.2,
        );

        // Single stock (defaults: beta 1.1, volatility 35%): the market-implied mean
        // (mean-preserving) and the configured volatility.
        c.close(
            &format!("{name}: single stock mean"),
            s.mean_ann(STOCK),
            s.mean_ann(BILL) + 1.1 * premium,
            0.012,
        );
        c.close(
            &format!("{name}: single stock volatility"),
            s.vol_ann(STOCK),
            0.35,
            0.02,
        );
        c.close(
            &format!("{name}: single stock beta"),
            s.corr(EQ, STOCK) * s.vol_ann(STOCK) / eq_vol,
            1.1,
            0.05,
        );

        // Custom asset (defaults: mean 6%, volatility 12%, equity correlation 0.3).
        c.close(
            &format!("{name}: custom mean"),
            s.mean_ann(CUSTOM),
            12.0 * math::annual_to_monthly(0.06),
            0.004,
        );
        c.close(
            &format!("{name}: custom volatility"),
            s.vol_ann(CUSTOM),
            0.12,
            0.004,
        );
        c.close(
            &format!("{name}: custom equity correlation"),
            s.corr(EQ, CUSTOM),
            0.3,
            0.03,
        );

        // Crypto: equity correlation between the normal 0.35 and the drawdown 0.6.
        c.within(
            &format!("{name}: crypto equity correlation"),
            s.corr(EQ, CRYPTO),
            0.33,
            0.48,
        );
        c.finish();
    }
}

#[test]
fn crypto_matches_its_stated_assumptions() {
    // params.json: 70% volatility of log returns, Student-t(4) tails, median real
    // growth zero, 1% a year chance of collapse.
    let m = model(bootstrap(60));
    let mut out = EconomyPath::new();
    let (mut n, mut sum, mut sum_sq, mut sum_4) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut collapses, mut months) = (0usize, 0usize);
    let mut growth_30y = Vec::new();
    let mut worst_month: f64 = 0.0;
    let paths = 1500;
    for path in 0..paths {
        generate(&m, SEED, path, 360, &mut out);
        let mut log_real = 0.0;
        let mut alive = true;
        for t in 0..360 {
            let r = out.returns[AssetClass::Crypto.index()][t];
            months += 1;
            if r == -1.0 {
                collapses += 1;
                alive = false;
                continue;
            }
            assert!(r > -1.0 && r.is_finite());
            let x = math::ln_1p(r) - math::ln_1p(out.inflation[t]);
            worst_month = worst_month.min(r);
            log_real += x;
            n += 1.0;
            sum += x;
            sum_sq += x * x;
            sum_4 += x * x * x * x;
        }
        if alive {
            growth_30y.push(log_real / 30.0);
        }
    }
    let mean = sum / n;
    let var = sum_sq / n - mean * mean;
    let vol = var.sqrt() * SQRT_12;
    assert!((vol - 0.70).abs() < 0.03, "volatility {vol}");
    let kurtosis = sum_4 / n / (var * var) - 3.0;
    assert!(kurtosis > 2.0, "excess kurtosis {kurtosis}");
    // Monthly collapse probability 1 - 0.99^(1/12) = 0.0837%.
    let hazard = collapses as f64 / months as f64;
    assert!((hazard - 0.000_837).abs() < 0.000_15, "{hazard}");
    // 26% of 30-year paths see a collapse: 1 - 0.99^30 = 0.26.
    let survived = growth_30y.len() as f64 / paths as f64;
    assert!((survived - 0.74).abs() < 0.04, "{survived}");
    // The preset fixes the expected (arithmetic) real return at zero, collapses
    // included. At 70% volatility that means the typical surviving path loses about
    // a fifth of its real value a year (half the variance, 0.245, a little less
    // with the capped tails).
    growth_30y.sort_by(f64::total_cmp);
    let median = percentile(&growth_30y, 0.5);
    assert!(
        median > -0.32 && median < -0.15,
        "median real log growth {median}"
    );
    // The average nominal monthly return, annualised, is inflation (3%) plus the
    // expected real return of zero: not the +25% a year a zero *median* implied.
    let s = Stats::collect(&m, 600, 360);
    let real_mean = s.mean_ann(CRYPTO) - s.mean_ann(CPI);
    assert!(real_mean.abs() < 0.04, "{real_mean}");
    assert!(worst_month > -0.97 && worst_month < -0.5, "{worst_month}");
}

#[test]
fn single_stock_is_mean_preserving_with_a_ruin_hazard() {
    let data = Data::embedded();
    let base = EconomyModel::new(&market(bootstrap(60)), data);
    let with_stock = |volatility: f64, beta: f64| {
        base.clone().with_assets(&PortfolioPolicy {
            single_stock: SingleStock {
                volatility,
                beta,
                ..SingleStock::default()
            },
            ..PortfolioPolicy::default()
        })
    };
    // Hazard interpolated from the params.json presets (idiosyncratic volatility
    // 25% -> 0.3%, 35% -> 1%, 50% -> 3% a year).
    let presets = base.base.stock_presets;
    assert_eq!(presets, [(0.25, 0.003), (0.35, 0.01), (0.5, 0.03)]);
    assert_eq!(stock_ruin_hazard(&presets, 0.0), 0.0);
    assert!((stock_ruin_hazard(&presets, 0.125) - 0.0015).abs() < 1e-12);
    assert!((stock_ruin_hazard(&presets, 0.25) - 0.003).abs() < 1e-12);
    assert!((stock_ruin_hazard(&presets, 0.30) - 0.0065).abs() < 1e-12);
    assert!((stock_ruin_hazard(&presets, 0.50) - 0.03).abs() < 1e-12);
    assert_eq!(stock_ruin_hazard(&presets, 2.0), 0.03);
    assert!((monthly_hazard(0.03) - (1.0 - math::pow(0.97, 1.0 / 12.0))).abs() < 1e-15);
    assert_eq!(
        (
            monthly_hazard(0.0),
            monthly_hazard(1.0),
            monthly_hazard(7.0)
        ),
        (0.0, 1.0, 1.0)
    );

    let mut out = EconomyPath::new();
    let stock = AssetClass::SingleStock.index();
    for (volatility, beta) in [(0.60, 1.3), (0.35, 1.1)] {
        let m = with_stock(volatility, beta);
        let eq_vol = m.base.eq_sd * SQRT_12;
        let idio = (volatility * volatility - beta * beta * eq_vol * eq_vol).sqrt();
        let hazard = monthly_hazard(stock_ruin_hazard(&presets, idio));
        let (mut n, mut ruins, mut sum, mut sum_expected, mut sum_sq) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut median_gap = Vec::new();
        for path in 0..1500 {
            generate(&m, SEED, path, 360, &mut out);
            let (mut log_stock, mut log_market, mut alive) = (0.0, 0.0, true);
            for t in 0..360 {
                let r = out.returns[stock][t];
                let (eq, cash) = (out.returns[0][t], out.returns[3][t]);
                n += 1.0;
                sum += r;
                sum_sq += r * r;
                sum_expected += cash + beta * (eq - cash);
                if r == -1.0 {
                    ruins += 1.0;
                    alive = false;
                } else {
                    log_stock += math::ln_1p(r);
                }
                log_market += math::ln_1p(eq);
            }
            if alive {
                median_gap.push((log_stock - log_market) / 30.0);
            }
        }
        // Ruin frequency as specified.
        let se = (hazard / n).sqrt();
        assert!(
            (ruins / n - hazard).abs() < 4.0 * se,
            "{} vs {hazard}",
            ruins / n
        );
        // Mean-preserving: the average return equals the market-implied return,
        // ruin included (within 4 standard errors of the idiosyncratic part).
        let mean = sum / n;
        assert!(
            (mean - sum_expected / n).abs() < 4.0 * idio / SQRT_12 / n.sqrt() + 4.0 * se,
            "mean {mean} vs {}",
            sum_expected / n
        );
        // Total volatility as configured, in the months without a ruin. (Ruin is on
        // top: a 3% annual hazard adds about 0.03 to annual variance.)
        let alive_n = n - ruins;
        let alive_mean = (sum + ruins) / alive_n;
        let alive_sd = ((sum_sq - ruins) / alive_n - alive_mean * alive_mean).sqrt();
        assert!(
            (alive_sd * SQRT_12 / volatility - 1.0).abs() < 0.04,
            "{}",
            alive_sd * SQRT_12
        );
        let total_sd = (sum_sq / n - mean * mean).sqrt() * SQRT_12;
        assert!(total_sd > alive_sd * SQRT_12 && total_sd < volatility * 1.1);
        // Concentration costs growth: the median surviving path trails the market,
        // by up to about half the difference of variances (params.json
        // "volatility_drag"; beta above 1 gives some of it back).
        median_gap.sort_by(f64::total_cmp);
        let gap = percentile(&median_gap, 0.5);
        let drag = 0.5 * (volatility * volatility - eq_vol * eq_vol);
        assert!(gap < 0.03 && gap > -drag - 0.03, "gap {gap}, drag {drag}");
        if volatility > 0.5 {
            assert!(gap < -0.03, "gap {gap}");
        }
    }

    // A stock no more volatile than its market exposure has no idiosyncratic risk
    // and no ruin: it is exactly the market.
    let m = with_stock(0.05, 1.0);
    generate(&m, SEED, 0, 240, &mut out);
    for t in 0..240 {
        assert!((out.returns[stock][t] - out.returns[0][t]).abs() < 1e-14);
    }
    // Hostile parameters stay finite.
    for (volatility, beta) in [
        (f64::NAN, f64::NAN),
        (-1.0, -3.0),
        (50.0, 100.0),
        (0.0, 0.0),
    ] {
        let m = with_stock(volatility, beta);
        generate(&m, SEED, 1, 240, &mut out);
        assert!(out.returns[stock]
            .iter()
            .all(|r| r.is_finite() && *r >= -1.0));
    }
}

#[test]
fn custom_asset_follows_the_user_parameters() {
    let data = Data::embedded();
    for kind in [bootstrap(60), MarketKind::Lognormal] {
        let base = EconomyModel::new(&market(kind), data);
        for (mean, volatility, equity_corr) in
            [(0.09, 0.25, 0.8), (0.03, 0.04, -0.4), (0.05, 0.0, 0.0)]
        {
            let m = base.clone().with_assets(&PortfolioPolicy {
                custom_asset: Some(CustomAsset {
                    name: "x".into(),
                    mean,
                    volatility,
                    equity_corr,
                    income_yield: 0.0,
                    ..CustomAsset::default()
                }),
                ..PortfolioPolicy::default()
            });
            let s = Stats::collect(&m, 200, 720);
            assert!(
                (s.mean_ann(CUSTOM) - 12.0 * math::annual_to_monthly(mean)).abs()
                    < 0.004 + 4.0 * volatility / (s.n / 12.0).sqrt(),
                "mean {} for {mean}",
                s.mean_ann(CUSTOM)
            );
            assert!(
                (s.vol_ann(CUSTOM) - volatility).abs() < 0.03 * volatility + 1e-6,
                "volatility {} for {volatility}",
                s.vol_ann(CUSTOM)
            );
            if volatility > 0.0 {
                assert!(
                    (s.corr(EQ, CUSTOM) - equity_corr).abs() < 0.03,
                    "{}",
                    s.corr(EQ, CUSTOM)
                );
            }
        }
    }
    // Out-of-range inputs are clamped, not propagated.
    let m = EconomyModel::new(&market(bootstrap(60)), data).with_assets(&PortfolioPolicy {
        custom_asset: Some(CustomAsset {
            name: "bad".into(),
            mean: f64::INFINITY,
            volatility: f64::NAN,
            equity_corr: 9.0,
            income_yield: 0.0,
            ..CustomAsset::default()
        }),
        ..PortfolioPolicy::default()
    });
    let mut out = EconomyPath::new();
    generate(&m, SEED, 0, 120, &mut out);
    assert!(out.returns[AssetClass::Custom.index()]
        .iter()
        .all(|r| r.is_finite() && *r >= -1.0));
}

#[test]
fn income_yields_are_exposed_for_the_tax_module() {
    let m = model(bootstrap(60));
    let mut out = EconomyPath::new();
    generate(&m, SEED, 0, 120, &mut out);
    for t in [0, 57, 119] {
        let y = out.income_yields_at(t);
        assert_eq!(y[AssetClass::UsStocks.index()], 0.015);
        assert_eq!(y[AssetClass::IntlStocks.index()], 0.030);
        assert_eq!(y[AssetClass::RealEstate.index()], 0.040);
        assert_eq!(y[AssetClass::Bonds.index()], out.long_yield[t]);
        assert_eq!(y[AssetClass::Cash.index()], out.tbill[t].max(0.0));
        assert_eq!(y[AssetClass::Gold.index()], 0.0);
        assert_eq!(y[AssetClass::Crypto.index()], 0.0);
        // Cash interest is the whole cash return.
        let cash = out.returns[AssetClass::Cash.index()][t];
        assert!((math::annual_to_monthly(out.tbill[t]) - cash).abs() < 1e-12);
    }
    assert!(QUALIFIED_FRACTION.iter().all(|q| (0.0..=1.0).contains(q)));
    assert_eq!(QUALIFIED_FRACTION[AssetClass::Bonds.index()], 0.0);
    assert_eq!(QUALIFIED_FRACTION[AssetClass::RealEstate.index()], 0.0);
}

// ---------------------------------------------------------------------------------
// Adjustments.
// ---------------------------------------------------------------------------------

#[test]
fn equity_premium_and_inflation_adjustments_shift_means_consistently() {
    let data = Data::embedded();
    for (name, kind) in ALL_KINDS {
        let base = EconomyModel::new(&market(kind), data);
        let adjusted = EconomyModel::new(
            &MarketModel {
                kind,
                equity_premium_adjustment: -0.02,
                inflation_adjustment: 0.015,
                ..MarketModel::default()
            },
            data,
        );
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        generate(&base, SEED, 5, 600, &mut a);
        generate(&adjusted, SEED, 5, 600, &mut b);
        let ke = math::pow(0.98, 1.0 / 12.0);
        let ki = math::pow(1.015, 1.0 / 12.0);
        let ratio = |x: f64, y: f64| (1.0 + y) / (1.0 + x);
        for t in 0..600 {
            let (ra, rb) = (a.returns_at(t), b.returns_at(t));
            let close = |actual: f64, expected: f64, what: &str| {
                assert!(
                    (actual - expected).abs() < 1e-11,
                    "{name} month {t} {what}: {actual} vs {expected}"
                );
            };
            // Inflation up by exactly 1.5% a year, geometrically.
            close(ratio(a.inflation[t], b.inflation[t]), ki, "cpi");
            // Equities: the premium shift on top of the inflation shift.
            close(ratio(ra[0], rb[0]), ke * ki, "us");
            if ra[1] > MIN_RETURN && rb[1] > MIN_RETURN {
                close(ratio(ra[1], rb[1]), ke * ki, "intl");
            }
            // Everything else nominal keeps its real return.
            close(ratio(ra[2], rb[2]), ki, "bonds");
            close(ratio(ra[3], rb[3]), ki, "cash");
            close(ratio(a.home_national[t], b.home_national[t]), ki, "home");
            if ra[5] > MIN_RETURN && rb[5] > MIN_RETURN {
                close(ratio(ra[5], rb[5]), ki, "gold");
            }
            // Nominal rate levels rise with inflation; bills stay consistent.
            close(b.long_yield[t] - a.long_yield[t], 0.015, "long yield");
            close(b.mortgage_rate[t] - a.mortgage_rate[t], 0.015, "mortgage");
            close(math::annual_to_monthly(b.tbill[t]), rb[3], "tbill");
            assert_eq!(a.recession[t], b.recession[t]);
        }
        // Over the path: real equity return lower by 2% a year, real bond return and
        // the real bill rate unchanged.
        let real = |p: &EconomyPath, i: usize| -> f64 {
            (0..600)
                .map(|t| math::ln_1p(p.returns[i][t]) - math::ln_1p(p.inflation[t]))
                .sum::<f64>()
                / 50.0
        };
        assert!(
            (real(&b, 0) - real(&a, 0) - math::ln(0.98)).abs() < 1e-9,
            "{name}"
        );
        assert!((real(&b, 2) - real(&a, 2)).abs() < 1e-9, "{name}");
        assert!((real(&b, 3) - real(&a, 3)).abs() < 1e-9, "{name}");
        // Real estate carries 0.70 of the equity shift and the single stock its beta
        // (1.1) of it, on top of the inflation shift; crypto and the custom asset
        // keep their real returns.
        let mean = |p: &EconomyPath, i: usize| p.returns[i].iter().sum::<f64>() / 600.0 * 12.0;
        let shift = |i: usize| mean(&b, i) - mean(&a, i);
        assert!(
            (shift(4) - (0.015 - 0.7 * 0.02)).abs() < 0.003,
            "{name}: real estate {}",
            shift(4)
        );
        assert!(
            (shift(7) - (0.015 - 1.1 * 0.02)).abs() < 0.004,
            "{name}: single stock {}",
            shift(7)
        );
        assert!(
            (shift(8) - 0.015).abs() < 0.003,
            "{name}: custom {}",
            shift(8)
        );
        let crypto_real = real(&b, 6) - real(&a, 6);
        if a.returns[6].iter().all(|r| *r > -1.0) {
            assert!(crypto_real.abs() < 0.01, "{name}: crypto {crypto_real}");
        }
    }
    // Absurd adjustments are bounded and stay finite.
    let m = EconomyModel::new(
        &MarketModel {
            equity_premium_adjustment: f64::NAN,
            inflation_adjustment: -5.0,
            ..MarketModel::default()
        },
        data,
    );
    let mut out = EconomyPath::new();
    generate(&m, SEED, 0, 120, &mut out);
    assert_all_finite(&out, "absurd adjustments");
}

// ---------------------------------------------------------------------------------
// Determinism, stream isolation, sanity.
// ---------------------------------------------------------------------------------

fn assert_all_finite(p: &EconomyPath, what: &str) {
    for t in 0..p.months {
        for (i, r) in p.returns_at(t).iter().enumerate() {
            assert!(
                r.is_finite() && *r >= -1.0 && *r < 1e3,
                "{what}: asset {i} month {t} = {r}"
            );
        }
        assert!(
            p.inflation[t].is_finite() && p.inflation[t] > -1.0,
            "{what}: cpi {t}"
        );
        assert!(p.price_level[t + 1].is_finite() && p.price_level[t + 1] > 0.0);
        assert!(p.tbill[t].is_finite() && p.long_yield[t].is_finite());
        assert!(p.mortgage_rate[t].is_finite() && p.home_national[t].is_finite());
        assert!(p.home_national[t] > -1.0 && p.recession[t] <= 1);
        let level = p.price_level[t] * (1.0 + p.inflation[t]);
        assert!(
            (p.price_level[t + 1] - level).abs() <= 1e-12 * level,
            "{what}: price level {t}"
        );
    }
}

fn same_bits(a: &EconomyPath, b: &EconomyPath) -> bool {
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
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

#[test]
fn no_nan_anywhere_in_any_model() {
    let data = Data::embedded();
    let mut out = EconomyPath::new();
    let mut kinds: Vec<(String, MarketKind)> =
        ALL_KINDS.iter().map(|(n, k)| (n.to_string(), *k)).collect();
    for (block, from_year) in [
        (1, None),
        (12, Some(1930)),
        (600, Some(1975)),
        (60, Some(2020)),
    ] {
        kinds.push((
            format!("bootstrap {block} {from_year:?}"),
            MarketKind::Bootstrap {
                mean_block_months: block,
                from_year,
                joins: BootstrapJoins::Uniform,
            },
        ));
    }
    for (name, kind) in kinds {
        let m = EconomyModel::new(&market(kind), data);
        for path in 0..60 {
            generate(&m, SEED, path * 37, 1200, &mut out);
            assert_all_finite(&out, &name);
        }
    }
    // Historical: cohort starts across the whole record, so every row (including the
    // short-history columns before 1975 and 1968 and the blank latest home value) is
    // exercised.
    let m = model(MarketKind::Historical);
    for cohort in (0..m.cohort_count() as u64).step_by(7) {
        generate(&m, SEED, cohort, 240, &mut out);
        assert_all_finite(&out, "historical");
    }
}

#[test]
fn generation_is_deterministic() {
    for (name, kind) in ALL_KINDS {
        let (m1, m2) = (model(kind), model(kind));
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::with_capacity(900));
        generate(&m1, 42, 7, 600, &mut a);
        generate(&m2, 42, 7, 600, &mut b);
        assert!(same_bits(&a, &b), "{name}: same inputs gave different bits");
        // A longer path has the shorter one as its prefix.
        generate(&m2, 42, 7, 840, &mut b);
        assert_eq!(a.returns[0][..], b.returns[0][..600], "{name}");
        assert_eq!(a.returns[6][..], b.returns[6][..600], "{name}");
        assert_eq!(a.price_level[..], b.price_level[..601], "{name}");
        // Another seed gives another economy (the derived assets always differ;
        // the historical factor series do not).
        generate(&m1, 43, 7, 600, &mut b);
        assert_ne!(a.returns[4], b.returns[4], "{name}");
        if !matches!(kind, MarketKind::Historical) {
            assert_ne!(a.returns[0], b.returns[0], "{name}");
            generate(&m1, 42, 8, 600, &mut b);
            assert_ne!(a.returns[0], b.returns[0], "{name}");
        }
    }
}

#[test]
fn golden_values_are_frozen() {
    // A change here is a change to every user's results: make it deliberately.
    let mut out = EconomyPath::new();
    let mut digest = Vec::new();
    for (_, kind) in ALL_KINDS {
        generate(&model(kind), 2026, 3, 360, &mut out);
        let sum: f64 = (0..360)
            .map(|t| out.returns_at(t).iter().sum::<f64>())
            .sum();
        digest.push((out.price_level[360], sum));
    }
    for (i, (got, want)) in digest.iter().zip(GOLDEN).enumerate() {
        assert!(
            (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
            "model {i}: got {digest:?}"
        );
    }
}

/// Stream positions after generating: the number of draws per month is fixed.
#[test]
fn draw_counts_are_fixed_per_month() {
    let months = 500usize;
    for (name, kind, market_draws, inflation_draws) in [
        ("bootstrap", bootstrap(60), 8 * months, 0),
        ("bootstrap iid", bootstrap(1), 8 * months, 0),
        (
            "bootstrap matched",
            MarketKind::Bootstrap {
                mean_block_months: 12,
                from_year: None,
                joins: BootstrapJoins::Matched,
            },
            8 * months,
            0,
        ),
        ("historical", MarketKind::Historical, 7 * months, 0),
        ("regime", MarketKind::Regime, 17 * months, months),
        ("lognormal", MarketKind::Lognormal, 12 * months + 1, months),
    ] {
        let m = model(kind);
        for path in [0u64, 11] {
            let mut a = Rng::stream(SEED, path, Stream::Market);
            let mut b = Rng::stream(SEED, path, Stream::Inflation);
            let mut out = EconomyPath::new();
            m.generate(
                &mut EconomyStreams {
                    market: &mut a,
                    inflation: &mut b,
                },
                path,
                months,
                &mut out,
            );
            let mut expect_a = Rng::stream(SEED, path, Stream::Market);
            let mut expect_b = Rng::stream(SEED, path, Stream::Inflation);
            for _ in 0..market_draws {
                expect_a.next_u64();
            }
            for _ in 0..inflation_draws {
                expect_b.next_u64();
            }
            assert_eq!(a.next_u64(), expect_a.next_u64(), "{name}: market draws");
            assert_eq!(b.next_u64(), expect_b.next_u64(), "{name}: inflation draws");
        }
    }
}

#[test]
fn portfolio_parameters_do_not_move_the_market() {
    let data = Data::embedded();
    let wild = PortfolioPolicy {
        single_stock: SingleStock {
            volatility: 0.9,
            beta: 2.0,
            ..SingleStock::default()
        },
        custom_asset: Some(CustomAsset {
            name: "x".into(),
            mean: 0.2,
            volatility: 0.5,
            equity_corr: -0.9,
            income_yield: 0.0,
            ..CustomAsset::default()
        }),
        ..PortfolioPolicy::default()
    };
    for (name, kind) in ALL_KINDS {
        let base = EconomyModel::new(&market(kind), data);
        let default_assets = base.clone().with_assets(&PortfolioPolicy::default());
        let changed = base.clone().with_assets(&wild);
        let (mut a, mut b, mut c) = (EconomyPath::new(), EconomyPath::new(), EconomyPath::new());
        generate(&base, SEED, 21, 600, &mut a);
        generate(&default_assets, SEED, 21, 600, &mut b);
        generate(&changed, SEED, 21, 600, &mut c);
        // `new` already uses the default single stock and custom asset.
        assert!(same_bits(&a, &b), "{name}");
        // Changing them changes only those two columns.
        for asset in AssetClass::ALL {
            let i = asset.index();
            let same = a.returns[i] == c.returns[i];
            let should_differ = matches!(asset, AssetClass::SingleStock | AssetClass::Custom);
            assert_eq!(same, !should_differ, "{name}: {asset:?}");
        }
        assert_eq!(a.inflation, c.inflation);
        assert_eq!(a.price_level, c.price_level);
        assert_eq!(a.tbill, c.tbill);
        assert_eq!(a.long_yield, c.long_yield);
        assert_eq!(a.mortgage_rate, c.mortgage_rate);
        assert_eq!(a.home_national, c.home_national);
        assert_eq!(a.recession, c.recession);
    }
}

#[test]
fn the_two_streams_are_isolated() {
    let run = |m: &EconomyModel, market_seed: u64, inflation_seed: u64| {
        let mut a = Rng::stream(market_seed, 4, Stream::Market);
        let mut b = Rng::stream(inflation_seed, 4, Stream::Inflation);
        let mut out = EconomyPath::new();
        m.generate(
            &mut EconomyStreams {
                market: &mut a,
                inflation: &mut b,
            },
            4,
            480,
            &mut out,
        );
        out
    };
    // History-based models never touch the inflation stream.
    for kind in [bootstrap(60), MarketKind::Historical] {
        let m = model(kind);
        assert!(same_bits(&run(&m, 1, 1), &run(&m, 1, 2)));
    }
    // Lognormal: CPI is last in the factor order, so the inflation stream moves
    // inflation only. Returns of every asset drawn before it are untouched (crypto
    // and real estate are tied to realised inflation by construction).
    let m = model(MarketKind::Lognormal);
    let (a, b) = (run(&m, 1, 1), run(&m, 1, 2));
    assert_ne!(a.inflation, b.inflation);
    for asset in [
        AssetClass::UsStocks,
        AssetClass::IntlStocks,
        AssetClass::Bonds,
        AssetClass::Cash,
        AssetClass::Gold,
        AssetClass::SingleStock,
        AssetClass::Custom,
    ] {
        assert_eq!(
            a.returns[asset.index()],
            b.returns[asset.index()],
            "{asset:?}"
        );
    }
    assert_eq!(a.home_national, b.home_national);
    assert_eq!(a.recession, b.recession);
    // The market stream moves the assets.
    let c = run(&m, 2, 1);
    assert_ne!(a.returns[0], c.returns[0]);
    // Regime: the regime path, the recession flag and equity excess returns over
    // bills come from the market stream only; inflation has its own innovation.
    let m = model(MarketKind::Regime);
    let (a, b) = (run(&m, 1, 1), run(&m, 1, 2));
    assert_ne!(a.inflation, b.inflation);
    assert_eq!(a.recession, b.recession);
    for t in 0..480 {
        let xa = (1.0 + a.returns[0][t]) / (1.0 + a.returns[3][t]);
        let xb = (1.0 + b.returns[0][t]) / (1.0 + b.returns[3][t]);
        assert!((xa / xb - 1.0).abs() < 1e-12);
    }
}

// ---------------------------------------------------------------------------------
// Stress overlays.
// ---------------------------------------------------------------------------------

/// A hand-made flat path: equities +1% a month, bonds +0.4%, cash +0.3%, CPI +0.2%.
fn flat_path(months: usize) -> EconomyPath {
    let mut p = EconomyPath::with_capacity(months);
    for t in 0..months {
        p.inflation[t] = 0.002;
        let flat = [0.01, 0.008, 0.004, 0.003, 0.007, 0.002, 0.02, 0.012, 0.005];
        for (series, value) in p.returns.iter_mut().zip(flat) {
            series[t] = value;
        }
        p.tbill[t] = math::powi(1.003, 12) - 1.0;
        p.long_yield[t] = 0.045;
        p.mortgage_rate[t] = 0.0625;
        p.home_national[t] = 0.003;
    }
    p.rebuild_price_level();
    p
}

fn cumulative(series: &[f64], range: std::ops::Range<usize>) -> f64 {
    series[range].iter().map(|r| 1.0 + r).product()
}

#[test]
fn crash_overlay_hand_computed() {
    let m = model(bootstrap(60));
    let betas = m.stress_betas();
    let mut p = flat_path(60);
    let before = p.clone();
    m.apply_stress(
        &ResolvedStress::Crash {
            at_month: 12,
            drawdown: 0.5,
            recovery_months: 12,
        },
        &mut p,
    );
    let us = &p.returns[0];
    // Six months of 0.5^(1/6) - 1 = -10.9101%.
    for t in 12..18 {
        assert!((us[t] + 0.109_101_281_859_661).abs() < 1e-12, "{}", us[t]);
        assert_eq!(p.recession[t], 1);
    }
    assert!((cumulative(us, 12..18) - 0.5).abs() < 1e-12);
    // Recovery: twelve months whose sampled returns (+1% each) are scaled by one
    // constant so that together they just undo the fall: 2^(1/12) - 1 = 5.9463% a
    // month, and the index is back at its pre-crash level, not on the unstressed
    // trend.
    for t in 18..30 {
        assert!((us[t] - (math::pow(2.0, 1.0 / 12.0) - 1.0)).abs() < 1e-12);
        assert_eq!(p.recession[t], 0);
    }
    assert!((cumulative(us, 12..30) - 1.0).abs() < 1e-12);
    // The loss against the unstressed path is permanent: untouched outside the
    // window, so the stressed index stays 1.01^18 behind.
    for t in (0..12).chain(30..60) {
        assert_eq!(p.returns_at(t), before.returns_at(t));
        assert_eq!(p.recession[t], 0);
    }
    // Other risky assets move by beta x the change imposed on US equities; bonds,
    // cash, inflation, home prices and rates do not move.
    for t in 12..30 {
        let delta = us[t] - 0.01;
        for asset in AssetClass::ALL {
            let i = asset.index();
            let expected = match asset {
                AssetClass::UsStocks => us[t],
                AssetClass::Bonds | AssetClass::Cash => before.returns[i][t],
                _ => (before.returns[i][t] + betas[i] * delta).max(-1.0),
            };
            assert!(
                (p.returns[i][t] - expected).abs() < 1e-15,
                "{asset:?} month {t}"
            );
        }
    }
    assert_eq!(p.inflation, before.inflation);
    assert_eq!(p.price_level, before.price_level);
    assert_eq!(p.home_national, before.home_national);
    assert_eq!(p.long_yield, before.long_yield);
    assert_eq!(p.tbill, before.tbill);
    // Betas: international about 0.7, real estate 0.70, single stock its beta (1.1),
    // gold about zero, bonds and cash zero, crypto its drawdown beta.
    assert_eq!(betas[AssetClass::UsStocks.index()], 1.0);
    assert!((betas[AssetClass::IntlStocks.index()] - 0.717).abs() < 0.01);
    assert_eq!(betas[AssetClass::RealEstate.index()], REIT_EQUITY_BETA);
    assert_eq!(betas[AssetClass::SingleStock.index()], 1.1);
    assert!(betas[AssetClass::Gold.index()].abs() < 0.1);
    assert_eq!(betas[AssetClass::Bonds.index()], 0.0);
    assert_eq!(betas[AssetClass::Cash.index()], 0.0);
    let crypto_beta = 0.6 * 0.7 / (m.base.eq_sd * SQRT_12);
    assert!((betas[AssetClass::Crypto.index()] - crypto_beta).abs() < 1e-12);
}

#[test]
fn crash_overlay_on_simulated_paths_and_edge_cases() {
    for (name, kind) in ALL_KINDS {
        let m = model(kind);
        let mut p = EconomyPath::new();
        generate(&m, SEED, 17, 480, &mut p);
        let before = p.clone();
        let (at, recovery) = (120usize, 60usize);
        m.apply_stress(
            &ResolvedStress::Crash {
                at_month: at,
                drawdown: 0.4,
                recovery_months: recovery,
            },
            &mut p,
        );
        let us = &p.returns[0];
        // The intended drawdown: exactly 40% below the level at the start month,
        // reached at the end of the crash window, with no month up on the way.
        let end = at + stress::CRASH_MONTHS;
        assert!((cumulative(us, at..end) - 0.6).abs() < 1e-12, "{name}");
        assert!(us[at..end].iter().all(|r| *r < 0.0));
        // At the end of the recovery the index is back at its pre-crash level, and
        // the shape of the sampled path is kept: every recovery month's gross
        // return is the sampled one times the same constant.
        let rejoin = end + recovery;
        assert!((cumulative(us, at..rejoin) - 1.0).abs() < 1e-10, "{name}");
        let k = (1.0 + us[end]) / (1.0 + before.returns[0][end]);
        for t in end..rejoin {
            let ratio = (1.0 + us[t]) / (1.0 + before.returns[0][t]);
            assert!((ratio / k - 1.0).abs() < 1e-9, "{name} month {t}");
        }
        // Nothing before the start or after the recovery is touched.
        for t in (0..at).chain(rejoin..480) {
            assert_eq!(p.returns_at(t), before.returns_at(t), "{name} month {t}");
            assert_eq!(p.recession[t], before.recession[t]);
        }
        assert_eq!(p.inflation, before.inflation);
        assert_all_finite(&p, name);
        // International stocks fall too, relative to their unstressed path, by about
        // beta x the US fall.
        let relative = cumulative(&p.returns[1], at..end) / cumulative(&before.returns[1], at..end);
        assert!(relative < 0.9, "{name}: intl {relative}");
    }

    let m = model(bootstrap(60));
    let mut p = flat_path(24);
    let before = p.clone();
    // Starting past the end, or with a non-finite depth: nothing happens.
    for stress in [
        ResolvedStress::Crash {
            at_month: 24,
            drawdown: 0.4,
            recovery_months: 12,
        },
        ResolvedStress::Crash {
            at_month: usize::MAX,
            drawdown: 0.4,
            recovery_months: 12,
        },
        ResolvedStress::Crash {
            at_month: 3,
            drawdown: f64::NAN,
            recovery_months: 12,
        },
        ResolvedStress::LostDecade { from_month: 24 },
        ResolvedStress::Stagflation {
            from_month: 24,
            months: 12,
            inflation: 0.08,
        },
        ResolvedStress::Stagflation {
            from_month: 2,
            months: 0,
            inflation: 0.08,
        },
        ResolvedStress::Stagflation {
            from_month: 2,
            months: 12,
            inflation: f64::NAN,
        },
    ] {
        m.apply_stress(&stress, &mut p);
        assert!(same_bits(&p, &before), "{stress:?}");
    }
    // A crash that runs off the end of the path is truncated: the months that
    // exist fall at the full monthly rate.
    m.apply_stress(
        &ResolvedStress::Crash {
            at_month: 21,
            drawdown: 0.4,
            recovery_months: usize::MAX,
        },
        &mut p,
    );
    assert!((cumulative(&p.returns[0], 21..24) - math::pow(0.6, 0.5)).abs() < 1e-12);
    assert_eq!(p.returns_at(20), before.returns_at(20));
    // Depths outside (0, 1) are clamped: at most a 99% fall, at least none; a
    // zero-month recovery is treated as one month.
    let mut p = flat_path(24);
    m.apply_stress(
        &ResolvedStress::Crash {
            at_month: 0,
            drawdown: 7.0,
            recovery_months: 0,
        },
        &mut p,
    );
    assert!((cumulative(&p.returns[0], 0..6) - 0.01).abs() < 1e-12);
    assert!((cumulative(&p.returns[0], 0..7) - 1.0).abs() < 1e-9);
    assert_all_finite(&p, "deep crash");
    // No fall, no stress.
    let mut p = flat_path(24);
    let before = p.clone();
    m.apply_stress(
        &ResolvedStress::Crash {
            at_month: 0,
            drawdown: -1.0,
            recovery_months: 6,
        },
        &mut p,
    );
    assert!(same_bits(&p, &before));
    // A collapsed token or company stays collapsed through a recovery.
    let mut p = flat_path(36);
    p.returns[AssetClass::Crypto.index()][20] = -1.0;
    m.apply_stress(
        &ResolvedStress::Crash {
            at_month: 12,
            drawdown: 0.4,
            recovery_months: 12,
        },
        &mut p,
    );
    assert_eq!(p.returns[AssetClass::Crypto.index()][20], -1.0);
}

#[test]
fn lost_decade_gives_zero_real_equity_return_and_keeps_volatility() {
    for (name, kind) in ALL_KINDS {
        let m = model(kind);
        let mut p = EconomyPath::new();
        generate(&m, SEED, 23, 480, &mut p);
        let before = p.clone();
        let from = 100usize;
        m.apply_stress(&ResolvedStress::LostDecade { from_month: from }, &mut p);
        let window = from..from + stress::LOST_DECADE_MONTHS;
        let real_logs = |p: &EconomyPath, i: usize| -> Vec<f64> {
            window
                .clone()
                .map(|t| math::ln_1p(p.returns[i][t]) - math::ln_1p(p.inflation[t]))
                .collect()
        };
        for i in [0, 1] {
            let (after, original) = (real_logs(&p, i), real_logs(&before, i));
            // Zero real return over the decade, in total.
            assert!(after.iter().sum::<f64>().abs() < 1e-10, "{name} asset {i}");
            // Monthly volatility preserved: every month shifted by one constant.
            let (sd_after, sd_before) = (
                fit::mean_sd(after.iter().copied()).1,
                fit::mean_sd(original.iter().copied()).1,
            );
            assert!((sd_after / sd_before - 1.0).abs() < 1e-9, "{name}");
            let shift = after[0] - original[0];
            assert!(after
                .iter()
                .zip(&original)
                .all(|(a, b)| (a - b - shift).abs() < 1e-12));
        }
        // Derived assets move with US equities by their beta; the rest is untouched.
        let betas = m.stress_betas();
        for t in window.clone().step_by(13) {
            let delta = p.returns[0][t] - before.returns[0][t];
            for asset in [
                AssetClass::RealEstate,
                AssetClass::SingleStock,
                AssetClass::Custom,
            ] {
                let i = asset.index();
                let expected = (before.returns[i][t] + betas[i] * delta).max(-1.0);
                assert!((p.returns[i][t] - expected).abs() < 1e-15);
            }
            assert_eq!(p.returns[2][t], before.returns[2][t]);
            assert_eq!(p.returns[3][t], before.returns[3][t]);
        }
        for t in (0..from).chain(window.end..480) {
            assert_eq!(p.returns_at(t), before.returns_at(t));
        }
        assert_eq!(p.inflation, before.inflation);
        assert_eq!(p.price_level, before.price_level);
        assert_all_finite(&p, name);
    }
    // Hand-computed: +1% a month nominal with 0.2% inflation becomes exactly
    // inflation, 0.2% a month. A decade cut short by the horizon is still flat.
    let m = model(bootstrap(60));
    let mut p = flat_path(150);
    m.apply_stress(&ResolvedStress::LostDecade { from_month: 100 }, &mut p);
    for t in 100..150 {
        assert!((p.returns[0][t] - 0.002).abs() < 1e-12);
        assert!((p.returns[1][t] - 0.002).abs() < 1e-12);
    }
    assert_eq!(p.returns[0][99], 0.01);
}

#[test]
fn stagflation_sets_inflation_and_depresses_real_returns_as_in_1973_81() {
    let data = Data::embedded();
    let h = &data.history;
    // The calibration, recomputed here from the rows: real annual geometric returns
    // 1973-01..1981-12 of about -2.8% (equities), -5.2% (bonds), -0.9% (bills).
    let (a, b) = (
        h.index_of(YearMonth::ym(1973, 1)).unwrap(),
        h.index_of(YearMonth::ym(1981, 12)).unwrap(),
    );
    assert_eq!(b - a + 1, 108);
    let real_annual = |series: &[f64]| {
        let log: f64 = (a..=b)
            .map(|t| math::ln_1p(series[t]) - math::ln_1p(h.cpi[t]))
            .sum();
        math::exp(log / 9.0) - 1.0
    };
    let targets = [
        real_annual(&h.equity_us),
        real_annual(&h.bond10),
        real_annual(&h.tbill),
    ];
    assert!((targets[0] + 0.0283).abs() < 5e-4, "{targets:?}");
    assert!((targets[1] + 0.0524).abs() < 5e-4, "{targets:?}");
    assert!((targets[2] + 0.0090).abs() < 5e-4, "{targets:?}");

    for (name, kind) in ALL_KINDS {
        let m = model(kind);
        let mut p = EconomyPath::new();
        generate(&m, SEED, 29, 480, &mut p);
        let before = p.clone();
        let (from, months) = (60usize, 96usize);
        m.apply_stress(
            &ResolvedStress::Stagflation {
                from_month: from,
                months,
                inflation: 0.08,
            },
            &mut p,
        );
        let window = from..from + months;
        let years = months as f64 / 12.0;
        // Inflation over the window is exactly 8% a year, compounded.
        let price = p.price_level[window.end] / p.price_level[from];
        assert!(
            (math::pow(price, 1.0 / years) - 1.08).abs() < 1e-10,
            "{name}"
        );
        // The path's own monthly variation in inflation is kept.
        let ratio = (1.0 + p.inflation[from]) / (1.0 + before.inflation[from]);
        for t in window.clone() {
            let r = (1.0 + p.inflation[t]) / (1.0 + before.inflation[t]);
            assert!((r - ratio).abs() < 1e-12);
        }
        // Real returns over the window as in 1973-81.
        let real = |i: usize| {
            let log: f64 = window
                .clone()
                .map(|t| math::ln_1p(p.returns[i][t]) - math::ln_1p(p.inflation[t]))
                .sum();
            math::exp(log / years) - 1.0
        };
        assert!(
            (real(0) - targets[0]).abs() < 1e-9,
            "{name}: equities {}",
            real(0)
        );
        assert!(
            (real(1) - targets[0]).abs() < 1e-9,
            "{name}: international {}",
            real(1)
        );
        assert!(
            (real(2) - targets[1]).abs() < 1e-9,
            "{name}: bonds {}",
            real(2)
        );
        assert!(
            (real(3) - targets[2]).abs() < 1e-9,
            "{name}: bills {}",
            real(3)
        );
        // Home prices keep their real return month by month; the bill yield stays
        // consistent with the cash return.
        for t in window.clone().step_by(7) {
            let real_home = |q: &EconomyPath| (1.0 + q.home_national[t]) / (1.0 + q.inflation[t]);
            assert!((real_home(&p) / real_home(&before) - 1.0).abs() < 1e-12);
            assert!((math::annual_to_monthly(p.tbill[t]) - p.returns[3][t]).abs() < 1e-12);
        }
        // Nominal rates shift by the change in inflation.
        let own = math::pow(
            before.price_level[window.end] / before.price_level[from],
            1.0 / years,
        ) - 1.0;
        for t in window.clone() {
            assert!((p.long_yield[t] - before.long_yield[t] - (0.08 - own)).abs() < 1e-10);
            assert!((p.mortgage_rate[t] - before.mortgage_rate[t] - (0.08 - own)).abs() < 1e-10);
        }
        // Outside the window returns and inflation are untouched; the price level is
        // rebuilt consistently to the end of the path.
        for t in (0..from).chain(window.end..480) {
            assert_eq!(p.returns_at(t), before.returns_at(t));
            assert_eq!(p.inflation[t], before.inflation[t]);
            assert_eq!(p.long_yield[t], before.long_yield[t]);
        }
        assert_eq!(p.price_level[..=from], before.price_level[..=from]);
        assert_all_finite(&p, name);
    }
    // Hand-computed on the flat path: 12 months at 6%.
    let m = model(bootstrap(60));
    let mut p = flat_path(36);
    m.apply_stress(
        &ResolvedStress::Stagflation {
            from_month: 12,
            months: 12,
            inflation: 0.06,
        },
        &mut p,
    );
    let monthly = math::pow(1.06, 1.0 / 12.0) - 1.0;
    for t in 12..24 {
        assert!((p.inflation[t] - monthly).abs() < 1e-12);
        // Equities: inflation plus one month of the 1973-81 real return.
        let expected = (1.0 + monthly) * math::pow(1.0 + targets[0], 1.0 / 12.0) - 1.0;
        assert!((p.returns[0][t] - expected).abs() < 1e-12);
        // Rates: flat-path inflation was 1.002^12 - 1 = 2.4266%, so +3.5734 points.
        let shifted = 0.045 + 0.06 - (math::powi(1.002, 12) - 1.0);
        assert!((p.long_yield[t] - shifted).abs() < 1e-12);
    }
    assert!((p.price_level[36] - math::powi(1.002, 24) * 1.06).abs() < 1e-12);
}

#[test]
fn stresses_compose_and_can_be_applied_mid_path() {
    let m = model(bootstrap(60));
    let mut p = EconomyPath::new();
    generate(&m, SEED, 31, 600, &mut p);
    let before = p.clone();
    let stresses = [
        ResolvedStress::Stagflation {
            from_month: 240,
            months: 96,
            inflation: 0.09,
        },
        ResolvedStress::Crash {
            at_month: 300,
            drawdown: 0.45,
            recovery_months: 48,
        },
        ResolvedStress::LostDecade { from_month: 420 },
    ];
    for s in &stresses {
        m.apply_stress(s, &mut p);
    }
    // Months already simulated (before the first start) are never rewritten.
    for t in 0..240 {
        assert_eq!(p.returns_at(t), before.returns_at(t));
        assert_eq!(p.inflation[t], before.inflation[t]);
        assert_eq!(p.price_level[t], before.price_level[t]);
    }
    assert!((cumulative(&p.returns[0], 300..306) - 0.55).abs() < 1e-12);
    assert_all_finite(&p, "composed");
    // Deterministic: the same overlays on the same path give the same bits.
    let mut q = before.clone();
    for s in &stresses {
        m.apply_stress(s, &mut q);
    }
    assert!(same_bits(&p, &q));
}

// ---------------------------------------------------------------------------------
// Reports: cargo test --release -p firesight-core economy::tests::report -- --ignored --nocapture
// ---------------------------------------------------------------------------------

#[test]
#[ignore = "report: prints the 30-year real equity CAGR distribution of each model"]
fn report_thirty_year_real_equity_cagr_percentiles() {
    println!("30-year real US equity CAGR, 5,000 paths (historical: every cohort, wrapping)");
    println!(
        "{:<22} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "model", "p1", "p5", "p25", "p50", "p75", "p95", "p99"
    );
    let row = |name: &str, c: &[f64]| {
        let p = |q: f64| 100.0 * percentile(c, q);
        println!(
            "{name:<22} {:>6.2}% {:>6.2}% {:>6.2}% {:>6.2}% {:>6.2}% {:>6.2}% {:>6.2}%",
            p(0.01),
            p(0.05),
            p(0.25),
            p(0.5),
            p(0.75),
            p(0.95),
            p(0.99)
        );
    };
    let mut kinds: Vec<(String, MarketKind)> = vec![
        ("bootstrap (block 60)".into(), bootstrap(60)),
        ("bootstrap (block 12)".into(), bootstrap(12)),
        ("bootstrap (block 120)".into(), bootstrap(120)),
        ("bootstrap (block 1)".into(), bootstrap(1)),
    ];
    kinds.extend(ALL_KINDS[1..].iter().map(|(n, k)| (n.to_string(), *k)));
    for (name, kind) in kinds {
        let m = model(kind);
        let paths = if matches!(kind, MarketKind::Historical) {
            m.cohort_count() as u64
        } else {
            5000
        };
        let mut c = Stats::collect(&m, paths, 360).cagr30;
        c.sort_by(f64::total_cmp);
        row(&name, &c);
    }
    // Actual non-wrapping 30-year windows of the history, for reference.
    let h = &Data::embedded().history;
    let mut actual: Vec<f64> = (0..h.len() - 360)
        .map(|s| {
            let real: f64 = (s..s + 360)
                .map(|t| (1.0 + h.equity_us[t]) / (1.0 + h.cpi[t]))
                .product();
            math::pow(real, 1.0 / 30.0) - 1.0
        })
        .collect();
    actual.sort_by(f64::total_cmp);
    row("history (no wrap)", &actual);
}

#[test]
#[ignore = "report: prints simulated moments of every series for each model"]
fn report_moments() {
    for (name, kind) in ALL_KINDS {
        let m = model(kind);
        let s = Stats::collect(&m, 1000, 720);
        println!("== {name}");
        for i in 0..K {
            println!(
                "{:>13}: mean {:7.4} vol {:7.4} ac1 {:6.3} ac12 {:6.3} annual sd {:6.4} real {:6.4}",
                NAMES[i],
                s.mean_ann(i),
                s.vol_ann(i),
                s.autocorr1(i),
                s.autocorr12(i),
                s.annual_sd(i, false),
                s.annual_sd(i, true)
            );
        }
        println!(
            "monthly corr eq/intl {:.3} eq/bond {:.3} eq/cpi {:.3} cpi/home {:.3} cpi/gold {:.3} eq/re {:.3} eq/crypto {:.3} eq/stock {:.3} eq/custom {:.3} cpi/bill {:.3}",
            s.corr(EQ, INTL),
            s.corr(EQ, BOND),
            s.corr(EQ, CPI),
            s.corr(CPI, HOME),
            s.corr(CPI, GOLD),
            s.corr(EQ, RE),
            s.corr(EQ, CRYPTO),
            s.corr(EQ, STOCK),
            s.corr(EQ, CUSTOM),
            s.corr(CPI, BILL),
        );
        println!(
            "annual: real bond/cpi {:.3} real eq/cpi {:.3} home/cpi {:.3} eq/intl {:.3} eq/home {:.3}; recession {:.3}; kurt eq {:.2}; yield range {:?}",
            s.annual_corr_real_vs_inflation(BOND),
            s.annual_corr_real_vs_inflation(EQ),
            s.annual_corr(HOME, CPI),
            s.annual_corr(EQ, INTL),
            s.annual_corr(EQ, HOME),
            s.recession_share(),
            excess_kurtosis_eq(&s),
            s.yield_range
        );
    }
}

#[test]
#[ignore = "report: prints nanoseconds per generated month and construction time"]
fn report_speed() {
    let data = Data::embedded();
    let mut kinds: Vec<(String, MarketKind)> =
        ALL_KINDS.iter().map(|(n, k)| (n.to_string(), *k)).collect();
    kinds.push(("bootstrap (block 1)".into(), bootstrap(1)));
    for (name, kind) in kinds {
        let t0 = std::time::Instant::now();
        let m = EconomyModel::new(&market(kind), data);
        let built = t0.elapsed();
        let t0 = std::time::Instant::now();
        let m = m.with_assets(&PortfolioPolicy::default());
        let with_assets = t0.elapsed();
        let months = 720usize;
        let mut out = EconomyPath::with_capacity(months);
        let mut sink = 0.0;
        for path in 0..200 {
            generate(&m, SEED, path, months, &mut out);
        }
        let paths = 5000u64;
        let t0 = std::time::Instant::now();
        for path in 0..paths {
            generate(&m, SEED, path, months, &mut out);
            sink += out.price_level[months];
        }
        let elapsed = t0.elapsed();
        println!(
            "{name:<22} {:6.1} ns/month ({:5.1} us per 60-year path); new {built:?}, with_assets {with_assets:?} [{sink:.1}]",
            elapsed.as_nanos() as f64 / (paths as f64 * months as f64),
            elapsed.as_micros() as f64 / paths as f64,
        );
    }
    let mut p = EconomyPath::new();
    let m = model(bootstrap(60));
    generate(&m, SEED, 0, 720, &mut p);
    let reps = 20_000u32;
    for (name, stress) in [
        (
            "crash",
            ResolvedStress::Crash {
                at_month: 300,
                drawdown: 0.4,
                recovery_months: 60,
            },
        ),
        (
            "lost decade",
            ResolvedStress::LostDecade { from_month: 300 },
        ),
        (
            "stagflation",
            ResolvedStress::Stagflation {
                from_month: 300,
                months: 96,
                inflation: 0.08,
            },
        ),
    ] {
        let t0 = std::time::Instant::now();
        for _ in 0..reps {
            m.apply_stress(&stress, &mut p);
        }
        println!(
            "apply_stress {name:<12} {:7.0} ns",
            t0.elapsed().as_nanos() as f64 / f64::from(reps)
        );
    }
}

// ---------------------------------------------------------------------------------
// Audit regressions.
// ---------------------------------------------------------------------------------

/// Annualised geometric real return of series `i` and of US equities, pooled.
fn real_geometric(m: &EconomyModel, paths: u64, i: usize) -> f64 {
    let mut out = EconomyPath::new();
    let (mut log, mut n) = (0.0, 0.0);
    for path in 0..paths {
        generate(m, SEED, path, 720, &mut out);
        for t in 0..720 {
            log += math::ln_1p(series_at(&out, t)[i]) - math::ln_1p(out.inflation[t]);
            n += 1.0;
        }
    }
    math::exp(log / n * 12.0) - 1.0
}

/// Gold and international equities have short histories whose own means used to
/// be set against full-sample inflation: gold returned 3.4%, 4.3% or 5.3% real
/// depending on the model, and international equities beat the US in one of them.
/// The long-run level is now one stated assumption in every resampling model.
#[test]
fn gold_and_international_follow_one_long_run_assumption_in_every_model() {
    for (name, kind, tol) in [
        ("bootstrap", bootstrap(60), 0.006),
        ("regime", MarketKind::Regime, 0.012),
        ("lognormal", MarketKind::Lognormal, 0.006),
    ] {
        let m = model(kind);
        let gold = real_geometric(&m, 600, GOLD);
        let spread = real_geometric(&m, 600, INTL) - real_geometric(&m, 600, EQ);
        assert!((gold - 0.01).abs() < tol, "{name}: gold real {gold}");
        assert!((spread + 0.01).abs() < tol, "{name}: intl less US {spread}");
    }
}

/// The bootstrap used to copy rate *levels* from the sampled row: every path began
/// at a random historical mortgage rate (5% of them above 12.8%) and half of all
/// households with a mortgage at today's rate could refinance a point lower two
/// months in. Levels now start from the last observation and move by the sampled
/// months' own changes.
#[test]
fn bootstrap_rates_start_from_today_and_move_like_history() {
    let h = &Data::embedded().history;
    let n = h.len();
    let today = h.mortgage30[n - 1];
    let m = model(bootstrap(60));
    let mut out = EconomyPath::new();
    let paths = 2000;
    let (mut cheaper_soon, mut big_moves, mut months) = (0usize, 0usize, 0usize);
    let mut after_30y = Vec::new();
    for path in 0..paths {
        generate(&m, SEED, path, 720, &mut out);
        assert_eq!(out.mortgage_rate[0], today);
        // The first January of a plan that starts in November is month 2.
        cheaper_soon += usize::from(out.mortgage_rate[2] <= today - 0.01);
        for t in 1..720 {
            months += 1;
            big_moves +=
                usize::from((out.mortgage_rate[t] - out.mortgage_rate[t - 1]).abs() > 0.01);
            assert!(out.long_yield[t] >= super::MIN_LONG_YIELD && out.long_yield[t] < 0.30);
        }
        after_30y.push(out.mortgage_rate[360]);
    }
    assert!(
        (cheaper_soon as f64) < 0.02 * paths as f64,
        "{cheaper_soon} of {paths} paths a point lower in two months"
    );
    // History: 0.42% of months moved the mortgage rate by more than a point.
    let share = big_moves as f64 / months as f64;
    assert!(share < 0.008, "one-month moves above a point: {share}");
    // Thirty years out the level is uncertain but bounded: a spread like history's.
    after_30y.sort_by(f64::total_cmp);
    let (p5, p50, p95) = (
        percentile(&after_30y, 0.05),
        percentile(&after_30y, 0.5),
        percentile(&after_30y, 0.95),
    );
    println!(
        "bootstrap mortgage rate: a point lower by the first January on {cheaper_soon} of {paths} paths; \
         one-month moves above a point {share:.4}; after 30 years p5 {p5:.4} p50 {p50:.4} p95 {p95:.4}"
    );
    assert!(p5 > 0.02 && p5 < 0.055, "{p5}");
    assert!(p50 > 0.05 && p50 < 0.09, "{p50}");
    assert!(p95 > 0.09 && p95 < 0.20, "{p95}");
}

/// The regime model's home prices had momentum but nothing pulling the real level
/// back: a 5% chance of losing 40% of real value in a decade, which a century of
/// data never shows (history p5 -10%), and every path began with a real decline.
#[test]
fn regime_home_prices_have_the_dispersion_of_the_record() {
    let m = model(MarketKind::Regime);
    let mut out = EconomyPath::new();
    let mut ten = Vec::new();
    let mut first_year = 0.0;
    let paths = 3000;
    for path in 0..paths {
        generate(&m, SEED, path, 360, &mut out);
        let real = |range: std::ops::Range<usize>| -> f64 {
            range
                .map(|t| (1.0 + out.home_national[t]) / (1.0 + out.inflation[t]))
                .product()
        };
        ten.push(real(0..120) - 1.0);
        ten.push(real(240..360) - 1.0);
        first_year += real(0..12) - 1.0;
    }
    ten.sort_by(f64::total_cmp);
    let (p5, p50, p95) = (
        percentile(&ten, 0.05),
        percentile(&ten, 0.5),
        percentile(&ten, 0.95),
    );
    // History (rolling ten-year real changes): -10% / +6% / +56%.
    println!("regime ten-year real home-price change: p5 {p5:.3} p50 {p50:.3} p95 {p95:.3}");
    assert!(p5 > -0.25, "p5 {p5}");
    assert!(p50 > -0.03 && p50 < 0.2, "p50 {p50}");
    assert!(p95 < 0.75, "p95 {p95}");
    let first_year = first_year / paths as f64;
    println!("regime mean real home-price growth in year 1: {first_year:.4}");
    assert!(
        first_year > -0.012,
        "mean real growth in year 1: {first_year}"
    );
}
