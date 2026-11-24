//! Tests of the opt-in market options: matched bootstrap joins, long-run
//! uncertainty and rare disasters. Expectations are derived by hand in the comments.
//! The `#[ignore]`d reports print the figures quoted in docs/methodology.md.

#![allow(clippy::needless_range_loop)]

use super::joins::{JoinTable, MIN_CELL};
use super::*;
use crate::model::{Disasters, Uncertain};
use crate::rng::Stream;

const SEED: u64 = 0x00A1_D157;
const US: usize = 0;
const INTL: usize = 1;
const BONDS: usize = 2;
const CASH: usize = 3;
const RE: usize = 4;
const GOLD: usize = 5;
const CRYPTO: usize = 6;
const STOCK: usize = 7;
const CUSTOM: usize = 8;

const KINDS: [(&str, MarketKind); 4] = [
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

fn bootstrap(mean_block_months: u32, joins: BootstrapJoins) -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months,
        from_year: None,
        joins,
    }
}

fn market(kind: MarketKind) -> MarketModel {
    MarketModel {
        kind,
        ..MarketModel::default()
    }
}

fn model(market: &MarketModel) -> EconomyModel {
    EconomyModel::new(market, Data::embedded())
}

/// Generates a path and applies the per-path overlays, as `State::begin` does.
fn generate(m: &EconomyModel, seed: u64, path: u64, months: usize, out: &mut EconomyPath) {
    let mut a = Rng::stream(seed, path, Stream::Market);
    let mut b = Rng::stream(seed, path, Stream::Inflation);
    m.generate(
        &mut EconomyStreams {
            market: &mut a,
            inflation: &mut b,
        },
        path,
        months,
        out,
    );
    m.apply_path_overlays(seed, path, out);
}

fn normal(sd: f64) -> Option<Uncertain> {
    Some(Uncertain::Normal { mean: 0.0, sd })
}

fn disasters(annual_probability: f64) -> Disasters {
    Disasters {
        annual_probability,
        ..Disasters::default()
    }
}

fn assert_sane(p: &EconomyPath, what: &str) {
    for t in 0..p.months {
        for (i, r) in p.returns_at(t).iter().enumerate() {
            assert!(
                r.is_finite() && *r >= -1.0 && *r < 1e3,
                "{what}: asset {i} month {t} = {r}"
            );
        }
        assert!(p.inflation[t].is_finite() && p.inflation[t] > -1.0);
        assert!(p.tbill[t].is_finite() && p.long_yield[t].is_finite());
        assert!(p.mortgage_rate[t].is_finite());
        assert!(p.home_national[t].is_finite() && p.home_national[t] > -1.0);
        assert!(p.recession[t] <= 1);
        let level = p.price_level[t] * (1.0 + p.inflation[t]);
        assert!(level.is_finite() && level > 0.0);
        assert!((p.price_level[t + 1] - level).abs() <= 1e-12 * level);
    }
}

/// Product of `1 + r` over `range`.
fn growth(series: &[f64], range: std::ops::Range<usize>) -> f64 {
    series[range].iter().map(|r| 1.0 + r).product()
}

// ---------------------------------------------------------------------------------
// Matched joins.
// ---------------------------------------------------------------------------------

#[test]
fn join_cells_partition_the_history() {
    let base = model(&market(bootstrap(60, BootstrapJoins::Matched))).base;
    let table = JoinTable::new(&base.rows);
    let n = base.rows.len();
    let mut seen = vec![0u32; n];
    let mut row = 0;
    while row < n {
        let cell = table.cell_of(row);
        // A row's cell contains the row, is large enough, and is sorted.
        assert!(cell.contains(&(row as u32)));
        assert!(cell.len() >= MIN_CELL, "cell of {row}: {}", cell.len());
        assert!(cell.windows(2).all(|w| w[0] < w[1]));
        // Every member of a cell has that same cell.
        for &member in cell {
            assert_eq!(table.cell_of(member as usize), cell);
        }
        if cell[0] == row as u32 {
            for &member in cell {
                seen[member as usize] += 1;
            }
        }
        row += 1;
    }
    assert!(seen.iter().all(|&k| k == 1), "cells overlap or miss rows");
    // The record supports a real grid: recessions are a seventh of it, so their
    // cells are the thin ones, but most of the 18 survive.
    let cells = table.cell_count();
    assert!((8..=18).contains(&cells), "{cells} cells");
    // Recession months are only ever matched with recession months.
    for row in 0..n {
        let flag = base.rows[row].recession;
        assert!(table
            .cell_of(row)
            .iter()
            .all(|&m| base.rows[m as usize].recession == flag));
    }
}

/// From a uniformly distributed row, a matched restart lands on a uniformly
/// distributed row: each row is the target of exactly `len` of the `len` equally
/// likely picks of each of the `len` rows of one cell, so its total inflow is
/// `len x (1 / len) = 1`.
#[test]
fn matched_restarts_keep_the_rows_uniform() {
    let base = model(&market(bootstrap(60, BootstrapJoins::Matched))).base;
    let table = JoinTable::new(&base.rows);
    let n = base.rows.len();
    let mut inflow = vec![0.0f64; n];
    for row in 0..n {
        let len = table.cell_of(row).len() as u64;
        for k in 0..len {
            // The smallest 32-bit value that selects member `k`.
            let bits = (k << 32).div_ceil(len);
            let next = table.restart(row, bits);
            let expected = (table.cell_of(row)[k as usize] as usize + 1) % n;
            assert_eq!(next, expected);
            inflow[next] += 1.0 / len as f64;
        }
        // The extremes of the 32 bits stay inside the cell.
        assert_eq!(
            table.restart(row, 0),
            (table.cell_of(row)[0] as usize + 1) % n
        );
        assert_eq!(
            table.restart(row, MASK_32),
            (*table.cell_of(row).last().unwrap() as usize + 1) % n
        );
    }
    for (row, total) in inflow.iter().enumerate() {
        assert!((total - 1.0).abs() < 1e-9, "row {row}: inflow {total}");
    }
}

/// Per-path statistics of a bootstrap run with short blocks, where seams are
/// frequent: `(mean monthly US return, mean monthly inflation, recession share,
/// recession flag changes per month, lag-12 autocorrelation of 12-month inflation)`.
fn seam_statistics(joins: BootstrapJoins) -> [f64; 5] {
    let m = model(&market(bootstrap(12, joins)));
    let mut out = EconomyPath::new();
    let (paths, months) = (1500u64, 480usize);
    let (mut eq, mut cpi, mut rec, mut flips) = (0.0, 0.0, 0.0, 0.0);
    let (mut xy, mut x, mut y, mut xx, mut yy, mut pairs) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for path in 0..paths {
        generate(&m, SEED, path, months, &mut out);
        for t in 0..months {
            eq += out.returns[US][t];
            cpi += out.inflation[t];
            rec += f64::from(out.recession[t]);
            if t > 0 && out.recession[t] != out.recession[t - 1] {
                flips += 1.0;
            }
        }
        let yearly = |t: usize| out.price_level[t + 12] / out.price_level[t] - 1.0;
        for t in (0..months - 24).step_by(12) {
            let (a, b) = (yearly(t), yearly(t + 12));
            xy += a * b;
            x += a;
            y += b;
            xx += a * a;
            yy += b * b;
            pairs += 1.0;
        }
    }
    let n = (paths as usize * months) as f64;
    let cov = xy / pairs - x / pairs * (y / pairs);
    let var_x = xx / pairs - (x / pairs) * (x / pairs);
    let var_y = yy / pairs - (y / pairs) * (y / pairs);
    [
        eq / n,
        cpi / n,
        rec / n,
        flips / n,
        cov / (var_x * var_y).sqrt(),
    ]
}

#[test]
fn matched_joins_keep_the_averages_and_lengthen_the_episodes() {
    let uniform = seam_statistics(BootstrapJoins::Uniform);
    let matched = seam_statistics(BootstrapJoins::Matched);
    // Same marginal distribution of rows, so the same averages up to sampling
    // noise (720,000 months each; the monthly equity sd is about 5.3%, and block
    // dependence roughly triples the standard error of an independent sample).
    assert!(
        (uniform[0] - matched[0]).abs() < 0.0006,
        "equity mean {uniform:?} vs {matched:?}"
    );
    assert!(
        (uniform[1] - matched[1]).abs() < 0.0003,
        "inflation mean {uniform:?} vs {matched:?}"
    );
    assert!(
        (uniform[2] - matched[2]).abs() < 0.02,
        "recession share {uniform:?} vs {matched:?}"
    );
    // With 12-month blocks one month in twelve is a seam. A uniform seam flips the
    // recession flag with probability 2 p (1 - p), about a quarter at p = 0.15;
    // a matched seam only when history itself flipped. So the flag changes far
    // less often, and a year's inflation says more about the next year's.
    assert!(
        matched[3] < 0.75 * uniform[3],
        "recession flips per month {} vs {}",
        matched[3],
        uniform[3]
    );
    assert!(
        matched[4] > uniform[4] + 0.05,
        "inflation persistence {} vs {}",
        matched[4],
        uniform[4]
    );
}

#[test]
fn matched_joins_only_change_restarts() {
    // With blocks so long that no restart happens after the first month, the two
    // settings are the same path: the first month is a uniform pick in both.
    let months = 240;
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    let kind = |joins| MarketKind::Bootstrap {
        mean_block_months: u32::MAX,
        from_year: None,
        joins,
    };
    generate(&model(&market(kind(BootstrapJoins::Uniform))), 9, 4, months, &mut a);
    generate(&model(&market(kind(BootstrapJoins::Matched))), 9, 4, months, &mut b);
    assert_eq!(a.returns, b.returns);
    assert_eq!(a.inflation, b.inflation);
    // With ordinary blocks they differ, are finite, and a longer path has the
    // shorter one as its prefix.
    let m = model(&market(bootstrap(60, BootstrapJoins::Matched)));
    generate(&model(&market(bootstrap(60, BootstrapJoins::Uniform))), 9, 4, 600, &mut a);
    generate(&m, 9, 4, 600, &mut b);
    assert_ne!(a.returns[US], b.returns[US]);
    assert_eq!(a.returns[US][0], b.returns[US][0]);
    assert_sane(&b, "matched");
    let mut longer = EconomyPath::new();
    generate(&m, 9, 4, 840, &mut longer);
    assert_eq!(b.returns[US][..], longer.returns[US][..600]);
    assert_eq!(b.price_level[..], longer.price_level[..601]);
    // A restricted window still builds a table.
    let short = model(&market(MarketKind::Bootstrap {
        mean_block_months: 6,
        from_year: Some(2020),
        joins: BootstrapJoins::Matched,
    }));
    for path in 0..40 {
        generate(&short, 9, path, 600, &mut b);
        assert_sane(&b, "matched from 2020");
    }
}

// ---------------------------------------------------------------------------------
// Long-run uncertainty.
// ---------------------------------------------------------------------------------

/// The shift is the user's distribution with its mean removed: a normal with sd
/// 0.01 cut at 4 sd has mean 0 and sd 0.01 x sqrt(1 - 2 x 4 x pdf(4) / (1 - 2 x
/// cdf(-4))) = 0.01 x 0.99946; its 90th percentile is 0.0128155.
#[test]
fn long_run_shift_has_the_stated_spread_and_mean_zero() {
    let mut mk = market(KINDS[0].1);
    mk.equity_premium_uncertainty = normal(0.01);
    mk.inflation_uncertainty = Some(Uncertain::Triangular {
        low: -0.005,
        mode: 0.0,
        high: 0.015,
    });
    let m = model(&mk);
    let n = 200_000u64;
    let (mut se, mut see, mut si, mut sii, mut cross) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut equity = Vec::with_capacity(n as usize);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for path in 0..n {
        let (e, i) = m.long_run_shift(SEED, path);
        se += e;
        see += e * e;
        si += i;
        sii += i * i;
        cross += e * i;
        equity.push(e);
        lo = lo.min(i);
        hi = hi.max(i);
    }
    let nf = n as f64;
    let (mean_e, mean_i) = (se / nf, si / nf);
    let sd_e = (see / nf - mean_e * mean_e).sqrt();
    let sd_i = (sii / nf - mean_i * mean_i).sqrt();
    // Standard error of the mean: 0.01 / sqrt(200,000) = 2.2e-5.
    assert!(mean_e.abs() < 1e-4, "{mean_e}");
    assert!((sd_e - 0.009_994_6).abs() < 6e-5, "{sd_e}");
    equity.sort_by(f64::total_cmp);
    let p90 = equity[(0.9 * nf) as usize];
    assert!((p90 - 0.012_815_5).abs() < 1.5e-4, "{p90}");
    assert!(equity[0] >= -0.04 - 1e-12 && equity[n as usize - 1] <= 0.04 + 1e-12);
    // Triangular(-0.005, 0, 0.015): mean 0.01 / 3, variance (a^2 + b^2 + c^2 - ab -
    // ac - bc) / 18 = (0.000025 + 0.000225 + 0.000075) / 18, sd 0.004249. With the
    // mean removed the support is [-0.008333, 0.011667].
    assert!(mean_i.abs() < 5e-5, "{mean_i}");
    assert!((sd_i - 0.004_249).abs() < 4e-5, "{sd_i}");
    assert!(lo >= -0.008_334 && lo < -0.008 && hi <= 0.011_667 && hi > 0.0113);
    // The two shifts come from different words: uncorrelated.
    let corr = (cross / nf - mean_e * mean_i) / (sd_e * sd_i);
    assert!(corr.abs() < 0.01, "{corr}");
}

#[test]
fn long_run_shift_ignores_where_the_range_is_centred_and_keeps_word_positions() {
    let base = market(KINDS[0].1);
    let both = MarketModel {
        equity_premium_uncertainty: normal(0.01),
        inflation_uncertainty: normal(0.004),
        ..base.clone()
    };
    // The same spread written around another centre gives the same shifts.
    let moved = MarketModel {
        equity_premium_uncertainty: Some(Uncertain::Normal {
            mean: -0.03,
            sd: 0.01,
        }),
        ..both.clone()
    };
    // Only one of the two set: the other's word is still drawn, so positions hold.
    let equity_only = MarketModel {
        inflation_uncertainty: None,
        ..both.clone()
    };
    let inflation_only = MarketModel {
        equity_premium_uncertainty: None,
        ..both.clone()
    };
    let (m_both, m_moved) = (model(&both), model(&moved));
    let (m_e, m_i, m_off) = (model(&equity_only), model(&inflation_only), model(&base));
    assert!(m_both.has_path_overlays() && !m_off.has_path_overlays());
    for path in 0..200 {
        let (e, i) = m_both.long_run_shift(SEED, path);
        assert!((m_moved.long_run_shift(SEED, path).0 - e).abs() < 1e-12);
        assert_eq!(m_e.long_run_shift(SEED, path), (e, 0.0));
        assert_eq!(m_i.long_run_shift(SEED, path), (0.0, i));
        assert_eq!(m_off.long_run_shift(SEED, path), (0.0, 0.0));
        // One word each, in this order, through the shared normal quantile.
        let mut rng = Rng::stream(SEED, path, Stream::MarketParams);
        let want = Uncertain::Normal {
            mean: 0.0,
            sd: 0.01,
        }
        .compile()
        .sample(rng.next_u64());
        assert!((e - want).abs() < 1e-12);
    }
}

/// The rewrite gives what the generator itself produces with the adjustments moved
/// by the drawn amounts: exactly (to rounding) for the factor series, the single
/// stock and the rates, and to a small approximation for listed real estate, crypto
/// and the custom asset, whose noise and stated means are not rescaled.
#[test]
fn long_run_shift_matches_a_direct_change_of_the_adjustments() {
    let portfolio = PortfolioPolicy {
        single_stock: SingleStock {
            volatility: 0.5,
            beta: 1.3,
        },
        ..PortfolioPolicy::default()
    };
    for (name, kind) in KINDS {
        let fixed = MarketModel {
            kind,
            equity_premium_adjustment: -0.015,
            inflation_adjustment: 0.004,
            ..MarketModel::default()
        };
        let uncertain = MarketModel {
            equity_premium_uncertainty: normal(0.015),
            inflation_uncertainty: normal(0.006),
            ..fixed.clone()
        };
        let m = model(&uncertain).with_assets(&portfolio);
        let unshifted = model(&fixed).with_assets(&portfolio);
        let (mut a, mut b, mut plain) =
            (EconomyPath::new(), EconomyPath::new(), EconomyPath::new());
        let months = 480;
        for path in 0..12 {
            let (d_e, d_i) = m.long_run_shift(SEED, path);
            assert!(d_e != 0.0 && d_i != 0.0);
            let direct = model(&MarketModel {
                equity_premium_adjustment: -0.015 + d_e,
                inflation_adjustment: 0.004 + d_i,
                ..fixed.clone()
            })
            .with_assets(&portfolio);
            generate(&m, SEED, path, months, &mut a);
            generate(&direct, SEED, path, months, &mut b);
            generate(&unshifted, SEED, path, months, &mut plain);
            assert_sane(&a, name);
            let mut moved = 0.0f64;
            for t in 0..months {
                for i in [US, INTL, BONDS, CASH, GOLD, STOCK] {
                    let (x, y) = (a.returns[i][t], b.returns[i][t]);
                    assert!(
                        (x - y).abs() < 1e-11 * (1.0 + y.abs()),
                        "{name} path {path} asset {i} month {t}: {x} vs {y}"
                    );
                }
                for i in [RE, CRYPTO, CUSTOM] {
                    let (x, y) = (a.returns[i][t], b.returns[i][t]);
                    assert!(
                        (x - y).abs() < 2e-3 * (1.0 + y.abs()),
                        "{name} path {path} asset {i} month {t}: {x} vs {y}"
                    );
                }
                for (x, y) in [
                    (a.inflation[t], b.inflation[t]),
                    (a.home_national[t], b.home_national[t]),
                    (a.tbill[t], b.tbill[t]),
                    (a.long_yield[t], b.long_yield[t]),
                    (a.mortgage_rate[t], b.mortgage_rate[t]),
                    (a.price_level[t + 1], b.price_level[t + 1]),
                ] {
                    assert!(
                        (x - y).abs() < 1e-10 * (1.0 + y.abs()),
                        "{name} path {path} month {t}: {x} vs {y}"
                    );
                }
                assert_eq!(a.recession[t], b.recession[t]);
                moved = moved.max((a.returns[US][t] - plain.returns[US][t]).abs());
                // Real returns of bonds and cash do not depend on either shift.
                for i in [BONDS, CASH] {
                    let real = |p: &EconomyPath| (1.0 + p.returns[i][t]) / (1.0 + p.inflation[t]);
                    assert!((real(&a) - real(&plain)).abs() < 1e-12);
                }
            }
            assert!(moved > 0.0, "{name}: the shift changed nothing");
        }
    }
}

/// Across paths the long-run real equity return moves one for one with the shift:
/// 30-year annualised real returns have the same mean and a wider spread, by about
/// the variance of the shift (independent of the market draws).
#[test]
fn long_run_uncertainty_widens_long_horizon_returns_without_moving_their_centre() {
    let base = market(KINDS[0].1);
    let wide = MarketModel {
        equity_premium_uncertainty: normal(0.015),
        ..base.clone()
    };
    let (m0, m1) = (model(&base), model(&wide));
    let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
    let months = 360;
    let paths = 3000u64;
    let (mut s0, mut s00, mut s1, mut s11, mut gap) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for path in 0..paths {
        generate(&m0, SEED, path, months, &mut a);
        generate(&m1, SEED, path, months, &mut b);
        let cagr = |p: &EconomyPath| {
            math::pow(growth(&p.returns[US], 0..months) / p.price_level[months], 1.0 / 30.0) - 1.0
        };
        let (x, y) = (cagr(&a), cagr(&b));
        s0 += x;
        s00 += x * x;
        s1 += y;
        s11 += y * y;
        // Path by path the gross annual return is scaled by (1 + adj + d) / (1 + adj)
        // with the default adjustment of -0.015.
        let (d, _) = m1.long_run_shift(SEED, path);
        let want = (1.0 - 0.015 + d) / (1.0 - 0.015);
        assert!(((1.0 + y) / (1.0 + x) - want).abs() < 1e-9, "{x} {y} {d}");
        gap += y - x;
        // Inflation and bonds are the same economy.
        assert_eq!(a.inflation, b.inflation);
        assert_eq!(a.returns[BONDS], b.returns[BONDS]);
    }
    let n = paths as f64;
    let var0 = s00 / n - (s0 / n) * (s0 / n);
    let var1 = s11 / n - (s1 / n) * (s1 / n);
    // Mean shift over 3,000 paths: standard error 0.015 / sqrt(3000) = 2.7e-4.
    assert!((gap / n).abs() < 1.1e-3, "{}", gap / n);
    // Added variance: about 0.015^2 = 2.25e-4 (times (1 + r)^2, about 1.1).
    let added = var1 - var0;
    assert!(
        (1.7e-4..3.2e-4).contains(&added),
        "variance {var0} -> {var1}"
    );
}

// ---------------------------------------------------------------------------------
// Rare disasters.
// ---------------------------------------------------------------------------------

/// Hand-derived expectations for the default shapes at 1% a year:
/// * P(first episode starts within 30 years) = 1 - 0.99^30 = 0.2603;
/// * mean wait = 12 / -ln(0.99) = 1194.0 months (less half a month of rounding
///   down);
/// * depth PERT(0.2, 0.4, 0.75): mean (0.2 + 1.6 + 0.75) / 6 = 0.425;
/// * length PERT(3, 8, 20) years: mean (3 + 32 + 20) / 6 = 9.1667 years = 110 months;
/// * a burst in 30% of episodes, of mean (0.02 + 0.2 + 0.12) / 6 = 0.05667.
#[test]
fn disaster_draws_have_the_stated_distribution() {
    let mut mk = market(KINDS[0].1);
    mk.disasters = disasters(0.01);
    let m = model(&mk);
    let n = 200_000u64;
    let (mut within_30, mut wait, mut depth, mut months, mut bursts, mut burst_size) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut min_depth, mut max_depth) = (1.0f64, 0.0f64);
    let (mut min_months, mut max_months) = (usize::MAX, 0usize);
    for path in 0..n {
        let episodes = m.disaster_episodes(SEED, path);
        let first = episodes[0].expect("the first episode always exists");
        within_30 += f64::from(u8::from(first.start < 360));
        wait += first.start as f64;
        depth += first.depth;
        months += first.months as f64;
        min_depth = min_depth.min(first.depth);
        max_depth = max_depth.max(first.depth);
        min_months = min_months.min(first.months);
        max_months = max_months.max(first.months);
        if first.inflation > 0.0 {
            bursts += 1.0;
            burst_size += first.inflation;
            assert!((0.02..=0.12).contains(&first.inflation));
        }
        // In time order, never overlapping.
        let mut end = 0;
        for e in episodes.iter().flatten() {
            assert!(e.start >= end);
            end = e.start + e.months;
        }
    }
    let nf = n as f64;
    // Binomial standard error sqrt(0.26 x 0.74 / 200,000) = 0.00098.
    assert!(
        (within_30 / nf - 0.2603).abs() < 0.004,
        "{}",
        within_30 / nf
    );
    // Exponential: sd equals the mean, so the standard error is 1194 / 447 = 2.7.
    assert!((wait / nf - 1193.5).abs() < 11.0, "{}", wait / nf);
    assert!((depth / nf - 0.425).abs() < 0.002, "{}", depth / nf);
    assert!((months / nf - 110.0).abs() < 0.6, "{}", months / nf);
    assert!(min_depth >= 0.2 && max_depth <= 0.75);
    assert!(min_months >= 36 && max_months <= 240);
    assert!((bursts / nf - 0.3).abs() < 0.005, "{}", bursts / nf);
    assert!(
        (burst_size / bursts - 0.056_67).abs() < 0.0006,
        "{}",
        burst_size / bursts
    );
}

/// The words of an episode are drawn whatever is decided from them: changing the
/// burst probability or the shapes moves nothing else.
#[test]
fn disaster_words_are_fixed() {
    let with = |f: &dyn Fn(&mut Disasters)| {
        let mut mk = market(KINDS[0].1);
        mk.disasters = disasters(0.05);
        f(&mut mk.disasters);
        model(&mk)
    };
    let base = with(&|_| {});
    let never = with(&|d| d.inflation_probability = 0.0);
    let always = with(&|d| d.inflation_probability = 1.0);
    let deeper = with(&|d| d.depth = Uncertain::point(0.9));
    for path in 0..300 {
        let a = base.disaster_episodes(SEED, path);
        let (b, c, d) = (
            never.disaster_episodes(SEED, path),
            always.disaster_episodes(SEED, path),
            deeper.disaster_episodes(SEED, path),
        );
        for k in 0..MAX_EPISODES {
            let Some(e) = a[k] else {
                assert!(b[k].is_none() && c[k].is_none() && d[k].is_none());
                continue;
            };
            let (eb, ec, ed) = (b[k].unwrap(), c[k].unwrap(), d[k].unwrap());
            assert_eq!((e.start, e.months, e.depth), (eb.start, eb.months, eb.depth));
            assert_eq!((e.start, e.months, e.depth), (ec.start, ec.months, ec.depth));
            assert_eq!((e.start, e.months, e.inflation), (ed.start, ed.months, ed.inflation));
            assert_eq!(eb.inflation, 0.0);
            assert!(ec.inflation > 0.0 && ed.depth == 0.9);
            if e.inflation > 0.0 {
                assert_eq!(e.inflation, ec.inflation);
            }
        }
        // The stream position after the fixed set of words.
        let mut rng = Rng::stream(SEED, path, Stream::Disaster);
        let first = rng.next_u64();
        let u = ((first >> 11) + 1) as f64 / (1u64 << 53) as f64;
        let wait = (-math::ln(u) / (-math::ln_1p(-0.05) / 12.0)).floor();
        assert_eq!(a[0].unwrap().start, wait as usize);
    }
}

/// One episode, start to finish, against the unstressed path of the same seed.
#[test]
fn a_disaster_takes_the_drawn_depth_from_equities_and_leaves_the_rest() {
    for (name, kind) in KINDS {
        let plain_market = MarketModel {
            kind,
            ..MarketModel::default()
        };
        let mut mk = plain_market.clone();
        mk.disasters = disasters(0.03);
        let (m, plain_model) = (model(&mk), model(&plain_market));
        let (mut a, mut plain) = (EconomyPath::new(), EconomyPath::new());
        let months = 1000;
        let (mut checked, mut with_burst) = (0, 0);
        for path in 0..400 {
            let episodes = m.disaster_episodes(SEED, path);
            let Some(e) = episodes[0] else { continue };
            let end = e.start + e.months;
            // Whole first episode on the path, and the second not straight after.
            if end + 12 > months || episodes[1].is_some_and(|x| x.start < end + 12) {
                continue;
            }
            generate(&m, SEED, path, months, &mut a);
            generate(&plain_model, SEED, path, months, &mut plain);
            assert_sane(&a, name);
            checked += 1;
            // Before the episode nothing has changed, bit for bit.
            for i in 0..N_ASSETS {
                assert_eq!(a.returns[i][..e.start], plain.returns[i][..e.start]);
            }
            assert_eq!(a.price_level[..=e.start], plain.price_level[..=e.start]);
            assert_eq!(a.recession[..e.start], plain.recession[..e.start]);
            // Over the episode equities end at (1 - depth) of the unstressed path,
            // after inflation.
            let span = e.start..end;
            let real = |p: &EconomyPath, i: usize| {
                growth(&p.returns[i], span.clone()) * p.price_level[e.start] / p.price_level[end]
            };
            for i in [US, INTL] {
                let ratio = real(&a, i) / real(&plain, i);
                assert!(
                    (ratio / (1.0 - e.depth) - 1.0).abs() < 1e-9,
                    "{name} path {path} asset {i}: {ratio} vs {}",
                    1.0 - e.depth
                );
            }
            // About three quarters of the fall (in logs) is over at half time: the
            // first m of n weights 2 (n - k) / (n (n + 1)) sum to
            // m (2n - m + 1) / (n (n + 1)), which is (3n/4 + 1/2) / (n + 1) at m = n/2.
            let half = e.start..e.start + e.months / 2;
            let early = growth(&a.returns[US], half.clone()) * plain.price_level[half.end]
                / (growth(&plain.returns[US], half.clone()) * a.price_level[half.end])
                * a.price_level[e.start]
                / plain.price_level[e.start];
            let share = math::ln(early) / math::ln_1p(-e.depth);
            let (nf, mf) = (e.months as f64, (e.months / 2) as f64);
            let want_share = mf * (2.0 * nf - mf + 1.0) / (nf * (nf + 1.0));
            assert!((share - want_share).abs() < 1e-9, "{name}: {share}");
            assert!((0.72..0.78).contains(&share), "{name}: {share}");
            // Listed real estate falls by the factor to the power of its beta
            // (every month's gross return is scaled, so the total is exact unless
            // the unstressed return was a floor value).
            let re = real(&a, RE) / real(&plain, RE);
            assert!(
                (re / math::pow(1.0 - e.depth, REIT_EQUITY_BETA) - 1.0).abs() < 1e-9,
                "{name}: real estate {re}"
            );
            // Gold barely moves (its beta is small); crypto falls more than stocks.
            let betas = m.stress_betas();
            let gold = real(&a, GOLD) / real(&plain, GOLD);
            let gold_beta = betas[GOLD].clamp(0.0, 3.0);
            assert!((gold / math::pow(1.0 - e.depth, gold_beta) - 1.0).abs() < 1e-9);
            assert!(betas[CRYPTO] > 1.0);
            // Bonds: unchanged in nominal terms. Cash: half the burst.
            let burst = a.price_level[end] / a.price_level[e.start]
                / (plain.price_level[end] / plain.price_level[e.start]);
            let want = math::pow(1.0 + e.inflation, e.months as f64 / 12.0).min(10.0);
            assert!((burst / want - 1.0).abs() < 1e-9, "{name}: {burst} vs {want}");
            let bonds = growth(&a.returns[BONDS], span.clone())
                / growth(&plain.returns[BONDS], span.clone());
            assert!((bonds - 1.0).abs() < 1e-12);
            let cash =
                growth(&a.returns[CASH], span.clone()) / growth(&plain.returns[CASH], span.clone());
            assert!((cash / want.sqrt() - 1.0).abs() < 1e-9, "{name}: cash {cash}");
            if e.inflation > 0.0 {
                with_burst += 1;
                let mid = e.start + 1;
                assert!(a.long_yield[mid] > plain.long_yield[mid]);
                assert!(a.mortgage_rate[mid] > plain.mortgage_rate[mid]);
                assert!(
                    (a.tbill[mid] - (math::powi(1.0 + a.returns[CASH][mid], 12) - 1.0)).abs()
                        < 1e-15
                );
            } else {
                assert!(a.inflation[..end] == plain.inflation[..end]);
                assert!(a.long_yield[..end] == plain.long_yield[..end]);
            }
            // The first third, at most 36 months, is a recession.
            let flagged = (e.months / 3).min(DISASTER_RECESSION_MONTHS);
            assert!(a.recession[e.start..e.start + flagged].iter().all(|&r| r == 1));
            // Afterwards the generated months resume unchanged (until the next
            // episode), and prices stay where the burst left them.
            for t in end..end + 12 {
                for i in 0..N_ASSETS {
                    assert_eq!(a.returns[i][t], plain.returns[i][t]);
                }
                assert_eq!(a.inflation[t], plain.inflation[t]);
                assert_eq!(a.long_yield[t], plain.long_yield[t]);
            }
        }
        assert!(checked > 60, "{name}: only {checked} episodes checked");
        assert!(with_burst > 10, "{name}: only {with_burst} bursts");
    }
}

#[test]
fn extreme_disasters_stay_finite_and_paths_keep_their_prefix() {
    for (name, kind) in KINDS {
        let mut mk = MarketModel {
            kind,
            equity_premium_uncertainty: normal(0.05),
            inflation_uncertainty: normal(0.025),
            ..MarketModel::default()
        };
        mk.disasters = Disasters {
            annual_probability: 0.1,
            depth: Uncertain::point(0.95),
            years: Uncertain::Uniform {
                low: 1.0,
                high: 40.0,
            },
            inflation_probability: 1.0,
            inflation: Uncertain::point(0.25),
        };
        let m = model(&mk);
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        let mut top = 0.0f64;
        for path in 0..150 {
            generate(&m, SEED, path, 1000, &mut a);
            assert_sane(&a, name);
            top = top.max(a.price_level[1000]);
            // A shorter path is the prefix of the longer one.
            generate(&m, SEED, path, 421, &mut b);
            for i in 0..N_ASSETS {
                assert_eq!(a.returns[i][..421], b.returns[i][..421], "{name}");
            }
            assert_eq!(a.price_level[..422], b.price_level[..422], "{name}");
            assert_eq!(a.recession[..421], b.recession[..421], "{name}");
            assert_eq!(a.long_yield[..421], b.long_yield[..421], "{name}");
        }
        // Three bursts of at most tenfold each on top of ordinary inflation.
        assert!(top < 1e9, "{name}: price level {top}");
    }
}

/// The opt-in options never touch the generator's own streams, and an option that
/// is off draws nothing.
#[test]
fn overlays_leave_the_market_streams_alone() {
    for (name, kind) in KINDS {
        let off = MarketModel {
            kind,
            ..MarketModel::default()
        };
        let mut on = MarketModel {
            equity_premium_uncertainty: normal(0.01),
            inflation_uncertainty: normal(0.005),
            ..off.clone()
        };
        on.disasters = disasters(0.02);
        let (m_off, m_on) = (model(&off), model(&on));
        let months = 300;
        for path in [0u64, 5] {
            let run = |m: &EconomyModel, out: &mut EconomyPath| -> (u64, u64) {
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
                (a.next_u64(), b.next_u64())
            };
            let (mut raw_off, mut raw_on) = (EconomyPath::new(), EconomyPath::new());
            assert_eq!(run(&m_off, &mut raw_off), run(&m_on, &mut raw_on), "{name}");
            // Before the overlays the two paths are the same bits.
            assert_eq!(raw_off.returns, raw_on.returns, "{name}");
            assert_eq!(raw_off.price_level, raw_on.price_level, "{name}");
            // Off: applying the overlays is a no-op.
            let before = raw_off.clone();
            m_off.apply_path_overlays(SEED, path, &mut raw_off);
            assert_eq!(before.returns, raw_off.returns);
            assert_eq!(before.price_level, raw_off.price_level);
            m_on.apply_path_overlays(SEED, path, &mut raw_on);
            assert_ne!(before.returns[US], raw_on.returns[US], "{name}");
        }
    }
}

// ---------------------------------------------------------------------------------
// Reports (cargo test -p firesight-core --release rich_report -- --ignored --nocapture).
// ---------------------------------------------------------------------------------

#[test]
#[ignore = "report: prints 30-year real equity return statistics for each opt-in market option"]
fn rich_report_thirty_year_real_equity_returns() {
    let paths = 20_000u64;
    let months = 360;
    let default_market = MarketModel::default();
    let mut cases: Vec<(String, MarketModel)> = vec![("default".into(), default_market.clone())];
    for sd in [0.01, 0.015] {
        cases.push((
            format!("equity uncertainty sd {sd}"),
            MarketModel {
                equity_premium_uncertainty: normal(sd),
                ..default_market.clone()
            },
        ));
    }
    for p in [0.005, 0.01, 0.02] {
        let mut mk = default_market.clone();
        mk.disasters = disasters(p);
        cases.push((format!("disasters {p}"), mk));
    }
    cases.push((
        "matched joins".into(),
        market(bootstrap(60, BootstrapJoins::Matched)),
    ));
    for kind in [MarketKind::Regime, MarketKind::Lognormal] {
        let mut mk = market(kind);
        cases.push((format!("{kind:?}"), mk.clone()));
        mk.disasters = disasters(0.01);
        cases.push((format!("{kind:?} disasters 0.01"), mk));
    }
    let mut out = EconomyPath::new();
    for (name, mk) in cases {
        let m = model(&mk);
        let mut cagr = Vec::with_capacity(paths as usize);
        let (mut inflation, mut hit) = (Vec::with_capacity(paths as usize), 0u64);
        for path in 0..paths {
            generate(&m, SEED, path, months, &mut out);
            let real = growth(&out.returns[US], 0..months) / out.price_level[months];
            cagr.push(math::pow(real, 1.0 / 30.0) - 1.0);
            inflation.push(math::pow(out.price_level[months], 1.0 / 30.0) - 1.0);
            hit += u64::from(
                m.disaster_episodes(SEED, path)
                    .iter()
                    .flatten()
                    .any(|e| e.start < months),
            );
        }
        cagr.sort_by(f64::total_cmp);
        inflation.sort_by(f64::total_cmp);
        let q = |v: &[f64], p: f64| v[((v.len() - 1) as f64 * p) as usize] * 100.0;
        let mean = cagr.iter().sum::<f64>() / cagr.len() as f64 * 100.0;
        let loss = cagr.iter().filter(|&&c| c < 0.0).count() as f64 / cagr.len() as f64 * 100.0;
        println!(
            "{name:32} real US equity CAGR mean {mean:5.2}  p5 {:5.2}  p25 {:5.2}  p50 {:5.2}  p75 {:5.2}  p95 {:5.2}  P(<0) {loss:5.2}%  inflation p5 {:4.2} p50 {:4.2} p95 {:4.2}  paths with a disaster {:5.2}%",
            q(&cagr, 0.05),
            q(&cagr, 0.25),
            q(&cagr, 0.5),
            q(&cagr, 0.75),
            q(&cagr, 0.95),
            q(&inflation, 0.05),
            q(&inflation, 0.5),
            q(&inflation, 0.95),
            hit as f64 / paths as f64 * 100.0,
        );
    }
}

#[test]
#[ignore = "report: prints the join table's cells and seam statistics"]
fn rich_report_join_cells() {
    let base = model(&market(bootstrap(60, BootstrapJoins::Matched))).base;
    let table = JoinTable::new(&base.rows);
    let mut sizes: Vec<usize> = (0..base.rows.len())
        .filter(|&r| table.cell_of(r)[0] == r as u32)
        .map(|r| table.cell_of(r).len())
        .collect();
    sizes.sort_unstable();
    println!("{} rows, {} cells, sizes {sizes:?}", base.rows.len(), sizes.len());
    println!("12-month blocks, uniform: {:?}", seam_statistics(BootstrapJoins::Uniform));
    println!("12-month blocks, matched: {:?}", seam_statistics(BootstrapJoins::Matched));
}
