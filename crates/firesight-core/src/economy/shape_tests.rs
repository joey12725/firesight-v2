//! Tests of the opt-in sleeve shapes (see `sleeves`): fat tails for the single stock
//! and the custom asset, the stock's own ruin hazard, crypto outlooks, token types
//! and cycles, and loss events on the custom asset. Expectations are derived by hand
//! in the comments. The `#[ignore]`d report prints the figures quoted in
//! docs/methodology.md.

#![allow(clippy::needless_range_loop)]

use super::sleeves::{AssetExt, ExtRt, Loss, WORDS_PER_MONTH};
use super::*;
use crate::model::{CryptoDynamics, CryptoOutlook, CryptoToken, LossEvent, MarketModel, Uncertain};
use crate::rng::Stream;

const SEED: u64 = 0x00A2_5EED;
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

fn model(kind: MarketKind, portfolio: &PortfolioPolicy) -> EconomyModel {
    let market = MarketModel {
        kind,
        ..MarketModel::default()
    };
    EconomyModel::new(&market, Data::embedded()).with_assets(portfolio)
}

fn lognormal(portfolio: &PortfolioPolicy) -> EconomyModel {
    model(MarketKind::Lognormal, portfolio)
}

/// Generates a path as `State::begin` does; returns the `AssetExt` stream after it.
fn generate(m: &EconomyModel, seed: u64, path: u64, months: usize, out: &mut EconomyPath) -> Rng {
    let mut a = Rng::stream(seed, path, Stream::Market);
    let mut b = Rng::stream(seed, path, Stream::Inflation);
    let mut ext = Rng::stream(seed, path, Stream::AssetExt);
    m.generate_with(
        &mut EconomyStreams {
            market: &mut a,
            inflation: &mut b,
            tails: &mut Rng::stream(0, 0, Stream::MarketTails),
        },
        m.has_asset_ext().then_some(&mut ext),
        path,
        months,
        out,
    );
    ext
}

/// All months of `paths` paths of one asset column.
fn column(m: &EconomyModel, asset: usize, paths: u64, months: usize) -> Vec<f64> {
    let mut out = EconomyPath::new();
    let mut all = Vec::with_capacity(paths as usize * months);
    for path in 0..paths {
        generate(m, SEED, path, months, &mut out);
        all.extend_from_slice(&out.returns[asset][..months]);
    }
    all
}

fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

fn share(v: &[f64], f: impl Fn(f64) -> bool) -> f64 {
    v.iter().filter(|x| f(**x)).count() as f64 / v.len() as f64
}

fn stock(volatility: f64, tails: TailShape, ruin_hazard: Option<f64>) -> PortfolioPolicy {
    PortfolioPolicy {
        single_stock: SingleStock {
            volatility,
            beta: 1.0,
            tails,
            ruin_hazard,
        },
        ..PortfolioPolicy::default()
    }
}

fn crypto(outlook: CryptoOutlook, token: CryptoToken, dynamics: CryptoDynamics) -> PortfolioPolicy {
    PortfolioPolicy {
        crypto: CryptoAssumptions {
            outlook,
            token,
            dynamics,
        },
        ..PortfolioPolicy::default()
    }
}

fn custom(volatility: f64, tails: TailShape, loss_event: Option<LossEvent>) -> PortfolioPolicy {
    PortfolioPolicy {
        custom_asset: Some(CustomAsset {
            mean: 0.06,
            volatility,
            equity_corr: 0.0,
            tails,
            loss_event,
            ..CustomAsset::default()
        }),
        ..PortfolioPolicy::default()
    }
}

/// Every column of the two models is the same bits over a few paths, except the
/// listed ones, which differ somewhere.
fn only_these_differ(a: &EconomyModel, b: &EconomyModel, differ: &[usize], what: &str) {
    let (mut pa, mut pb) = (EconomyPath::new(), EconomyPath::new());
    let mut changed = [false; N_ASSETS];
    for path in 0..6 {
        generate(a, SEED, path, 480, &mut pa);
        generate(b, SEED, path, 480, &mut pb);
        for i in 0..N_ASSETS {
            let same = pa.returns[i]
                .iter()
                .zip(&pb.returns[i])
                .all(|(x, y)| x.to_bits() == y.to_bits());
            if differ.contains(&i) {
                changed[i] |= !same;
            } else {
                assert!(same, "{what}: asset {i} moved on path {path}");
            }
        }
        assert!(pa.inflation == pb.inflation && pa.price_level == pb.price_level);
        assert!(pa.tbill == pb.tbill && pa.long_yield == pb.long_yield);
        assert!(pa.mortgage_rate == pb.mortgage_rate && pa.home_national == pb.home_national);
        assert!(pa.recession == pb.recession, "{what}");
    }
    for &i in differ {
        assert!(changed[i], "{what}: asset {i} did not change");
    }
}

// ---------------------------------------------------------------------------------
// Defaults.
// ---------------------------------------------------------------------------------

/// With nothing switched on there is no extension, the `AssetExt` stream is not
/// touched, and `generate_with` is `generate`.
#[test]
fn defaults_need_no_extension_and_draw_nothing() {
    for (name, kind) in KINDS {
        let m = model(kind, &PortfolioPolicy::default());
        assert!(!m.has_asset_ext() && !m.custom_fat, "{name}");
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        let mut rng_a = Rng::stream(SEED, 3, Stream::Market);
        let mut rng_b = Rng::stream(SEED, 3, Stream::Inflation);
        m.generate(
            &mut EconomyStreams {
                market: &mut rng_a,
                inflation: &mut rng_b,
                tails: &mut Rng::stream(0, 0, Stream::MarketTails),
            },
            3,
            240,
            &mut a,
        );
        let mut rng_a = Rng::stream(SEED, 3, Stream::Market);
        let mut rng_b = Rng::stream(SEED, 3, Stream::Inflation);
        let mut ext = Rng::stream(SEED, 3, Stream::AssetExt);
        m.generate_with(
            &mut EconomyStreams {
                market: &mut rng_a,
                inflation: &mut rng_b,
                tails: &mut Rng::stream(0, 0, Stream::MarketTails),
            },
            Some(&mut ext),
            3,
            240,
            &mut b,
        );
        for i in 0..N_ASSETS {
            assert!(a.returns[i] == b.returns[i], "{name} asset {i}");
        }
        assert_eq!(
            ext.next_u64(),
            Rng::stream(SEED, 3, Stream::AssetExt).next_u64(),
            "{name}: the stream was drawn from"
        );
    }
    // Spelling the defaults out is the same model as leaving them out.
    let spelled = crypto(
        CryptoOutlook::Neutral,
        CryptoToken::BitcoinOrEther,
        CryptoDynamics::Steady,
    );
    only_these_differ(
        &lognormal(&PortfolioPolicy::default()),
        &lognormal(&spelled),
        &[],
        "spelled-out defaults",
    );
}

/// The default crypto sleeve is built exactly as it was before the options
/// existed: the original construction, written out here, gives the same bits.
#[test]
fn the_default_crypto_sleeve_is_bit_identical_to_the_original_construction() {
    for (name, kind) in KINDS {
        let m = model(kind, &PortfolioPolicy::default());
        let base = &m.base;
        let spec = base.crypto;
        let sigma_c = spec.sigma_annual;
        let eq_vol = (base.eq_sd * SQRT_12).max(1e-6);
        let crypto_beta = spec.equity_corr * sigma_c / eq_vol;
        let crypto_beta_extra = (spec.drawdown_corr - spec.equity_corr).max(0.0) * sigma_c / eq_vol;
        let (_, link_sd, _) = fit::mean_sd(
            base.rows
                .iter()
                .map(|r| math::ln(base.crypto_link(crypto_beta, crypto_beta_extra, r.eq))),
        );
        let total_var = sigma_c * sigma_c / 12.0;
        let idio_var = (total_var - link_sd * link_sd).max(CRYPTO_MIN_IDIO_SHARE * total_var);
        let idio_sd = idio_var.sqrt();
        let crypto_shock = sampler::student_t(spec.dof).transformed(|x| {
            math::exp(idio_sd * x.clamp(-CRYPTO_SHOCK_CAP_SD, CRYPTO_SHOCK_CAP_SD))
        });
        let link_arithmetic = fit::mean_sd(
            base.rows
                .iter()
                .map(|r| base.crypto_link(crypto_beta, crypto_beta_extra, r.eq)),
        )
        .0;
        let crypto_ruin = probability_bits(monthly_hazard(spec.ruin_annual));
        let survive = 1.0 - crypto_ruin as f64 / TWO_POW_32;
        let crypto_norm = math::pow(1.0 + spec.expected_real_return, 1.0 / 12.0)
            / (link_arithmetic * crypto_shock.mean() * survive).max(1e-9);
        assert_eq!(
            m.assets.crypto_beta.to_bits(),
            crypto_beta.to_bits(),
            "{name}"
        );
        assert_eq!(
            m.assets.crypto_beta_extra.to_bits(),
            crypto_beta_extra.to_bits()
        );
        assert_eq!(m.assets.crypto_norm.to_bits(), crypto_norm.to_bits());
        assert_eq!(m.assets.crypto_ruin, crypto_ruin);
        let mut rng = Rng::from_seed(3);
        for _ in 0..20_000 {
            let w = rng.next_u64();
            assert_eq!(
                m.crypto_shock.sample(w).to_bits(),
                crypto_shock.sample(w).to_bits()
            );
        }
    }
}

/// The unit-variance tables behind the two fat shapes: how often a draw is beyond
/// four standard deviations. Student-t(4) with unit variance is `T / sqrt 2`, so
/// the event is `|T| > 5.657`, probability 0.00481 by the closed-form CDF (once in
/// 208 months, about 17 years). Student-t(6) is `T / sqrt 1.5`: `|T| > 4.899`,
/// probability 0.00271 (once in 368 months, about 30 years). The normal: 0.0000633
/// (once in 15,800 months, about 1,300 years).
#[test]
fn four_sigma_months_are_as_common_as_the_schema_says() {
    let exact4 = 2.0 * (1.0 - sampler::student_t_cdf_even(4.0 * 2f64.sqrt(), 4));
    let exact6 = 2.0 * (1.0 - sampler::student_t_cdf_even(4.0 * 1.5f64.sqrt(), 6));
    assert!((exact4 - 0.004_81).abs() < 2e-5, "{exact4}");
    assert!((exact6 - 0.002_71).abs() < 2e-5, "{exact6}");
    let n = 3_000_000;
    let freq = |table: &ShockTable| {
        let mut rng = Rng::from_seed(41);
        (0..n)
            .filter(|_| table.sample(rng.next_u64()).abs() > 4.0)
            .count() as f64
            / n as f64
    };
    // The tables are rescaled by a fraction of a percent to unit variance after
    // truncation, which moves these a little.
    let f4 = freq(&sampler::student_t4().table);
    let f6 = freq(&sampler::student_t6().table);
    let fz = freq(&sampler::normal().table);
    assert!((f4 - exact4).abs() < 4e-4, "t4 {f4}");
    assert!((f6 - exact6).abs() < 3e-4, "t6 {f6}");
    assert!((fz - 6.33e-5).abs() < 2.5e-5, "normal {fz}");
}

// ---------------------------------------------------------------------------------
// Single stock.
// ---------------------------------------------------------------------------------

/// The fat factor has mean exactly 1 and exactly the lognormal factor's standard
/// deviation, at every volatility validation accepts; it never exceeds five times
/// its pre-normalisation median.
#[test]
fn fat_stock_factor_keeps_the_mean_and_the_volatility() {
    for volatility in [0.2, 0.35, 0.6, 1.0, 2.0] {
        let thin = lognormal(&stock(volatility, TailShape::Normal, None));
        let fat = lognormal(&stock(volatility, TailShape::Fat, None));
        let sd = |t: &ShockTable| (t.second_moment() - t.mean() * t.mean()).sqrt();
        assert!((thin.stock_shock.mean() - 1.0).abs() < 1e-12);
        assert!(
            (fat.stock_shock.mean() - 1.0).abs() < 1e-12,
            "vol {volatility}"
        );
        let (a, b) = (sd(&thin.stock_shock), sd(&fat.stock_shock));
        assert!((a - b).abs() < 1e-9 * a, "vol {volatility}: {a} vs {b}");
        // Same hazard and survivor boost: the shape is the only change.
        assert_eq!(thin.assets.stock_ruin, fat.assets.stock_ruin);
        assert_eq!(
            thin.assets.stock_survivor_boost,
            fat.assets.stock_survivor_boost
        );
        let mut rng = Rng::from_seed(9);
        let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
        for _ in 0..400_000 {
            let x = fat.stock_shock.sample(rng.next_u64());
            lo = lo.min(x);
            hi = hi.max(x);
        }
        assert!(lo > 0.0 && hi / lo <= 25.0 * (1.0 + 1e-9), "{lo} {hi}");
    }
    // No idiosyncratic volatility: the factor is the constant 1 in either shape.
    let none = lognormal(&PortfolioPolicy {
        single_stock: SingleStock {
            volatility: 0.05,
            beta: 1.0,
            tails: TailShape::Fat,
            ruin_hazard: None,
        },
        ..PortfolioPolicy::default()
    });
    assert_eq!(none.stock_shock.sample(0x1234_5678_9ABC_DEF0), 1.0);
}

/// On simulated paths: the same average and the same volatility, more months near
/// the middle and more far out, and no other series moves.
#[test]
fn fat_stock_sleeve_has_the_same_moments_and_wilder_extremes() {
    // No total losses, so the comparison is of the shape alone.
    let thin = lognormal(&stock(0.5, TailShape::Normal, Some(0.0)));
    let fat = lognormal(&stock(0.5, TailShape::Fat, Some(0.0)));
    only_these_differ(&thin, &fat, &[STOCK], "fat stock");
    let (a, b) = (
        column(&thin, STOCK, 500, 600),
        column(&fat, STOCK, 500, 600),
    );
    let ((ma, sa), (mb, sb)) = (mean_sd(&a), mean_sd(&b));
    // Monthly sd about 0.145, 300,000 months: the standard error of a mean is
    // 0.00026; the two columns share every market move, so their difference is
    // tighter still.
    assert!((ma - mb).abs() < 0.001, "means {ma} {mb}");
    assert!((sb / sa - 1.0).abs() < 0.04, "sds {sa} {sb}");
    // The far tails: a month worse than -35% (2.6 monthly sds below the mean).
    let (crash_a, crash_b) = (share(&a, |r| r < -0.35), share(&b, |r| r < -0.35));
    assert!(crash_b > 2.0 * crash_a, "crashes {crash_a} {crash_b}");
    assert!(crash_b > 0.001, "{crash_b}");
    let (boom_a, boom_b) = (share(&a, |r| r > 0.6), share(&b, |r| r > 0.6));
    assert!(boom_b > 2.0 * boom_a, "booms {boom_a} {boom_b}");
    // The middle: more months within five points of flat.
    let (calm_a, calm_b) = (share(&a, |r| r.abs() < 0.05), share(&b, |r| r.abs() < 0.05));
    assert!(calm_b > calm_a + 0.02, "calm {calm_a} {calm_b}");
}

/// A stated hazard replaces the preset one: 12% a year is `1 - 0.88^(1/12)` =
/// 1.0596% of months, and the survivors' boost keeps the average return.
#[test]
fn a_stated_ruin_hazard_overrides_the_preset_and_keeps_the_mean() {
    let none = lognormal(&stock(0.35, TailShape::Normal, Some(0.0)));
    let preset = lognormal(&stock(0.35, TailShape::Normal, None));
    let high = lognormal(&stock(0.35, TailShape::Normal, Some(0.12)));
    assert_eq!(none.assets.stock_ruin, 0);
    assert_eq!(none.assets.stock_survivor_boost, 1.0);
    assert!(preset.assets.stock_ruin > 0 && high.assets.stock_ruin > preset.assets.stock_ruin);
    let q = high.assets.stock_ruin as f64 / TWO_POW_32;
    assert!((q - 0.010_596).abs() < 1e-6, "{q}");
    assert!((high.assets.stock_survivor_boost * (1.0 - q) - 1.0).abs() < 1e-15);
    only_these_differ(&none, &high, &[STOCK], "ruin hazard");
    let (a, b) = (
        column(&none, STOCK, 600, 600),
        column(&high, STOCK, 600, 600),
    );
    let lost = share(&b, |r| r == -1.0);
    // 360,000 months: standard error sqrt(q (1 - q) / n) = 0.00017.
    assert!((lost - q).abs() < 0.0007, "{lost}");
    assert_eq!(share(&a, |r| r == -1.0), 0.0);
    // Monthly sd about 0.14 with the losses: standard error of the mean 0.00024.
    let (ma, mb) = (mean_sd(&a).0, mean_sd(&b).0);
    assert!((ma - mb).abs() < 0.001, "means {ma} {mb}");
    // Not finite or out of range is repaired, not propagated.
    let odd = lognormal(&stock(0.35, TailShape::Fat, Some(f64::NAN)));
    assert_eq!(odd.assets.stock_ruin, preset.assets.stock_ruin);
    let huge = lognormal(&stock(0.35, TailShape::Fat, Some(7.0)));
    assert!(huge.assets.stock_survivor_boost.is_finite());
}

// ---------------------------------------------------------------------------------
// Crypto.
// ---------------------------------------------------------------------------------

/// The expected gross real return of a crypto month, from the compiled pieces:
/// normalisation x mean link x mean shock x survival.
fn crypto_expected_growth(m: &EconomyModel) -> f64 {
    let link = fit::mean_sd(m.base.rows.iter().map(|r| {
        m.base
            .crypto_link(m.assets.crypto_beta, m.assets.crypto_beta_extra, r.eq)
    }))
    .0;
    let survive = 1.0 - m.assets.crypto_ruin as f64 / TWO_POW_32;
    m.assets.crypto_norm * link * m.crypto_shock.mean() * survive
}

/// The outlooks set the expected real return (-5%, 0%, +6% a year) and the token
/// type the collapse hazard (1% or 8% a year), as `params.json` lists them.
#[test]
fn outlook_and_token_presets_reach_the_sleeve() {
    let steady = CryptoDynamics::Steady;
    let btc = CryptoToken::BitcoinOrEther;
    for dynamics in [steady, CryptoDynamics::cycles()] {
        for (outlook, annual) in [
            (CryptoOutlook::Skeptic, -0.05),
            (CryptoOutlook::Neutral, 0.0),
            (CryptoOutlook::Believer, 0.06),
        ] {
            for (token, hazard) in [(btc, 0.01), (CryptoToken::OtherTokens, 0.08)] {
                let m = lognormal(&crypto(outlook, token, dynamics));
                let growth = crypto_expected_growth(&m);
                let want = math::pow(1.0 + annual, 1.0 / 12.0);
                assert!((growth - want).abs() < 1e-12, "{outlook:?} {token:?}");
                let q = m.assets.crypto_ruin as f64 / TWO_POW_32;
                let want = 1.0 - math::pow(1.0 - hazard, 1.0 / 12.0);
                assert!((q - want).abs() < 1e-9, "{token:?}: {q}");
            }
        }
    }
    // Month by month with the same random numbers: the believer's gross return is
    // the neutral one times (1.06)^(1/12), and nothing else moves.
    let neutral = lognormal(&PortfolioPolicy::default());
    let believer = lognormal(&crypto(CryptoOutlook::Believer, btc, steady));
    only_these_differ(&neutral, &believer, &[CRYPTO], "outlook");
    let (a, b) = (
        column(&neutral, CRYPTO, 20, 480),
        column(&believer, CRYPTO, 20, 480),
    );
    let k = math::pow(1.06, 1.0 / 12.0);
    for (x, y) in a.iter().zip(&b) {
        if *x == -1.0 {
            assert_eq!(*y, -1.0);
        } else {
            assert!(((1.0 + y) / (1.0 + x) - k).abs() < 1e-12);
        }
    }
    // Other tokens: the same months collapse and more besides. 8% a year is
    // 0.6924% of months.
    let other = lognormal(&crypto(
        CryptoOutlook::Neutral,
        CryptoToken::OtherTokens,
        steady,
    ));
    only_these_differ(&neutral, &other, &[CRYPTO], "token");
    let (a, b) = (
        column(&neutral, CRYPTO, 500, 600),
        column(&other, CRYPTO, 500, 600),
    );
    assert!(a.iter().zip(&b).all(|(x, y)| *x != -1.0 || *y == -1.0));
    let lost = share(&b, |r| r == -1.0);
    assert!((lost - 0.006_924).abs() < 0.0006, "{lost}");
}

fn default_cycles() -> EconomyModel {
    lognormal(&crypto(
        CryptoOutlook::Neutral,
        CryptoToken::BitcoinOrEther,
        CryptoDynamics::cycles(),
    ))
}

/// The compiled chain: each state's shock has mean exactly 1; wild volatility is
/// the ratio times calm; and the long-run variance of the log shock is the steady
/// model's. Without the cap the calm scale would be the steady one over
/// `sqrt(5/7 + 2/7 x 2.5^2)` = 1.5811, that is 0.632 of it; the cap trims the wild
/// state, so the solved scale is a little higher.
#[test]
fn cycles_match_the_steady_variance_and_keep_the_mean() {
    let m = default_cycles();
    let c = m.ext.cycles.as_deref().expect("cycles are on");
    assert!(m.has_asset_ext());
    let unit = sampler::student_t4();
    // The steady scale, recovered from the default model's table: the median of
    // exp(s x) is 1 and its top value exp(8 s).
    let steady = lognormal(&PortfolioPolicy::default());
    let top = steady.crypto_shock.sample(u64::MAX >> 1);
    let s = math::ln(top) / CRYPTO_SHOCK_CAP_SD;
    assert!(s > 0.15 && s < 0.21, "steady monthly scale {s}");
    let cap = CRYPTO_SHOCK_CAP_SD * s;
    let log_var = |k: f64| {
        unit.transformed(|x| (k * x).clamp(-cap, cap))
            .second_moment()
    };
    assert!((c.sigma[1] / c.sigma[0] - 2.5).abs() < 1e-12);
    let ratio = c.sigma[0] / s;
    assert!(ratio > 0.632 && ratio < 0.70, "calm scale {ratio}");
    let wild_share = 12.0 / 42.0;
    let mixed = (1.0 - wild_share) * log_var(c.sigma[0]) + wild_share * log_var(c.sigma[1]);
    assert!(
        (mixed / log_var(s) - 1.0).abs() < 1e-8,
        "{mixed} vs {}",
        log_var(s)
    );
    // Annualised: about 45% calm and 110% wild at the default 70% total.
    let annual = |k: f64| k * SQRT_12;
    assert!(
        (annual(c.sigma[0]) - 0.42).abs() < 0.05,
        "{}",
        annual(c.sigma[0])
    );
    assert!(
        (annual(c.sigma[1]) - 1.05).abs() < 0.12,
        "{}",
        annual(c.sigma[1])
    );
    // The expected return is the outlook's whatever the state.
    assert!((crypto_expected_growth(&m) - 1.0).abs() < 1e-12);
    // A ratio of 1 is the steady model with a normalised table.
    let flat = lognormal(&crypto(
        CryptoOutlook::Neutral,
        CryptoToken::BitcoinOrEther,
        CryptoDynamics::Cycles {
            calm_months: 30.0,
            wild_months: 12.0,
            volatility_ratio: 1.0,
        },
    ));
    let (a, b) = (
        column(&steady, CRYPTO, 5, 480),
        column(&flat, CRYPTO, 5, 480),
    );
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() < 1e-12 * (1.0 + x.abs()), "{x} {y}");
    }
}

/// Runs the chain alone (crypto base 1, so the output is the shock itself) and
/// returns each month's `(wild, shock)`.
fn run_chain(ext: &AssetExt, seed: u64, months: usize) -> Vec<(bool, f64)> {
    let mut rng = Rng::from_seed(seed);
    let mut words = Rng::from_seed(seed ^ 0xABCD);
    let mut rt = ExtRt::new(ext, &mut rng);
    let mut out = Vec::with_capacity(months);
    for t in 0..months {
        let (mut crypto, mut custom) = (0.0, 0.0);
        rt.step(t, 1.0, words.next_u64(), &mut crypto, &mut custom);
        out.push((rt.is_wild(), 1.0 + crypto));
        assert_eq!(custom, 0.0);
    }
    out
}

/// The chain itself: 12 / 42 = 28.6% of months are wild, calm spells average 30
/// months and wild ones 12, each state's shock averages 1, and the size of
/// consecutive moves is correlated. With calm scale `s`, wild `2.5 s` and
/// `E|x|` = 0.707 for the unit Student-t(4): the mean absolute log move is
/// `0.707 s` calm and `1.77 s` wild; the variance of those two means across states
/// is `(2/7)(5/7)(1.06 s)^2 = 0.23 s^2`, the total variance of the absolute move
/// `2.5 s^2 - (1.01 s)^2 = 1.48 s^2`, and the state persists with `1 - 1/30 - 1/12`
/// = 0.883 a month, so the lag-one correlation is `0.883 x 0.23 / 1.48` = 0.137
/// (a little less with the cap). The steady model has none.
#[test]
fn the_cycle_chain_has_the_stated_spells_and_clusters_volatility() {
    let m = default_cycles();
    let months = 1_200_000;
    let path = run_chain(&m.ext, 77, months);
    let wild = path.iter().filter(|p| p.0).count() as f64 / months as f64;
    assert!((wild - 12.0 / 42.0).abs() < 0.01, "wild share {wild}");
    // Spell lengths.
    let (mut spells, mut total) = ([0usize; 2], [0usize; 2]);
    let mut run = 1usize;
    for t in 1..months {
        if path[t].0 == path[t - 1].0 {
            run += 1;
        } else {
            let k = usize::from(path[t - 1].0);
            spells[k] += 1;
            total[k] += run;
            run = 1;
        }
    }
    let calm_mean = total[0] as f64 / spells[0] as f64;
    let wild_mean = total[1] as f64 / spells[1] as f64;
    assert!((calm_mean - 30.0).abs() < 1.0, "calm spell {calm_mean}");
    assert!((wild_mean - 12.0).abs() < 0.5, "wild spell {wild_mean}");
    // Mean shock by state. The shocks are below 4.6 with sd about 0.15 calm and
    // 0.4 wild: standard errors about 0.0002 and 0.0007.
    for state in [false, true] {
        let v: Vec<f64> = path.iter().filter(|p| p.0 == state).map(|p| p.1).collect();
        let mean = mean_sd(&v).0;
        assert!((mean - 1.0).abs() < 0.004, "state {state}: mean {mean}");
    }
    let abs_log: Vec<f64> = path.iter().map(|p| math::ln(p.1).abs()).collect();
    let (mean, sd) = mean_sd(&abs_log);
    let lag1 = abs_log
        .windows(2)
        .map(|w| (w[0] - mean) * (w[1] - mean))
        .sum::<f64>()
        / (months as f64 - 1.0)
        / (sd * sd);
    assert!(lag1 > 0.09 && lag1 < 0.17, "lag-one correlation {lag1}");
    // For the figure in docs/methodology.md (run with --nocapture).
    println!("lag-one correlation of absolute log moves: {lag1:.3}");
}

/// `with_assets` sets every sleeve from the portfolio it is given, so a model
/// that already carries another portfolio's sleeves ends up the same bits as one
/// built fresh: a believer's cycling small token followed by the defaults leaves
/// nothing behind (once, the crypto sleeve was only rebuilt for a non-default
/// portfolio, and survived), and two different portfolios applied in turn give
/// the second.
#[test]
fn with_assets_replaces_every_sleeve_whatever_was_applied_before() {
    let loud = PortfolioPolicy {
        single_stock: stock(0.6, TailShape::Fat, Some(0.05)).single_stock,
        custom_asset: custom(
            0.3,
            TailShape::Fat,
            Some(LossEvent {
                annual_probability: 0.05,
                ..LossEvent::default()
            }),
        )
        .custom_asset,
        ..crypto(
            CryptoOutlook::Believer,
            CryptoToken::OtherTokens,
            CryptoDynamics::cycles(),
        )
    };
    let other = PortfolioPolicy {
        single_stock: stock(0.4, TailShape::Normal, None).single_stock,
        ..crypto(
            CryptoOutlook::Skeptic,
            CryptoToken::BitcoinOrEther,
            CryptoDynamics::Steady,
        )
    };
    for (name, kind) in KINDS {
        let fresh_loud = model(kind, &loud);
        assert!(fresh_loud.has_asset_ext(), "{name}");
        for target in [&PortfolioPolicy::default(), &other, &loud] {
            let fresh = model(kind, target);
            for first in [&loud, &other, &PortfolioPolicy::default()] {
                let reused = model(kind, first).with_assets(target);
                assert_eq!(reused.has_asset_ext(), fresh.has_asset_ext(), "{name}");
                assert_eq!(reused.assets.crypto_ruin, fresh.assets.crypto_ruin);
                for (a, b) in [
                    (reused.assets.crypto_norm, fresh.assets.crypto_norm),
                    (reused.assets.crypto_beta, fresh.assets.crypto_beta),
                    (
                        reused.assets.crypto_beta_extra,
                        fresh.assets.crypto_beta_extra,
                    ),
                ] {
                    assert_eq!(a.to_bits(), b.to_bits(), "{name}");
                }
                let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
                for path in 0..6 {
                    generate(&reused, SEED, path, 480, &mut a);
                    generate(&fresh, SEED, path, 480, &mut b);
                    for i in 0..N_ASSETS {
                        assert!(a.returns[i] == b.returns[i], "{name}: asset {i}");
                    }
                }
            }
        }
        // The settings are not vacuous: the loud portfolio's crypto differs from
        // the default's, which is what used to survive.
        let default = model(kind, &PortfolioPolicy::default());
        assert!(!default.has_asset_ext());
        assert_ne!(fresh_loud.assets.crypto_ruin, default.assets.crypto_ruin);
        assert_ne!(fresh_loud.assets.crypto_norm, default.assets.crypto_norm);
    }
}

/// In the generator: three `AssetExt` words a month in every market kind, nothing
/// but crypto moves, the same months collapse, a longer path extends a shorter
/// one, and the long-run volatility of the sleeve is the steady model's.
#[test]
fn cycles_in_the_generator_touch_only_crypto() {
    for (name, kind) in KINDS {
        let steady = model(kind, &PortfolioPolicy::default());
        let cycles = model(
            kind,
            &crypto(
                CryptoOutlook::Neutral,
                CryptoToken::BitcoinOrEther,
                CryptoDynamics::cycles(),
            ),
        );
        only_these_differ(&steady, &cycles, &[CRYPTO], name);
        let (mut a, mut b) = (EconomyPath::new(), EconomyPath::new());
        let months = 300;
        let mut after = generate(&cycles, SEED, 5, months, &mut a);
        let mut expect = Rng::stream(SEED, 5, Stream::AssetExt);
        for _ in 0..months * WORDS_PER_MONTH {
            expect.next_u64();
        }
        assert_eq!(after.next_u64(), expect.next_u64(), "{name}: draw count");
        generate(&cycles, SEED, 5, 120, &mut b);
        assert!(
            a.returns[CRYPTO][..120] == b.returns[CRYPTO][..120],
            "{name}"
        );
        generate(&steady, SEED, 5, months, &mut b);
        for t in 0..months {
            assert_eq!(
                a.returns[CRYPTO][t] == -1.0,
                b.returns[CRYPTO][t] == -1.0,
                "{name} month {t}"
            );
            assert!(a.returns[CRYPTO][t] >= -1.0 && a.returns[CRYPTO][t].is_finite());
        }
    }
    // Volatility of monthly log returns, collapses left out: equal by
    // construction of the variance match (sampling error about 1%).
    let log_sd = |m: &EconomyModel| {
        let v: Vec<f64> = column(m, CRYPTO, 600, 600)
            .into_iter()
            .filter(|r| *r > -1.0)
            .map(math::ln_1p)
            .collect();
        mean_sd(&v).1
    };
    let (a, b) = (
        log_sd(&lognormal(&PortfolioPolicy::default())),
        log_sd(&default_cycles()),
    );
    assert!((b / a - 1.0).abs() < 0.03, "log sd {a} vs {b}");
}

// ---------------------------------------------------------------------------------
// Custom asset.
// ---------------------------------------------------------------------------------

/// Fat tails: the same mean and volatility from the same words, and four-sigma
/// months at the Student-t(6) rate (0.27%) instead of the normal one (0.006%).
#[test]
fn fat_custom_residual_keeps_mean_and_volatility() {
    let thin = lognormal(&custom(0.12, TailShape::Normal, None));
    let fat = lognormal(&custom(0.12, TailShape::Fat, None));
    assert!(fat.custom_fat && !fat.has_asset_ext());
    only_these_differ(&thin, &fat, &[CUSTOM], "fat custom");
    let (a, b) = (
        column(&thin, CUSTOM, 600, 600),
        column(&fat, CUSTOM, 600, 600),
    );
    let ((ma, sa), (mb, sb)) = (mean_sd(&a), mean_sd(&b));
    // Zero correlation, so the return is the monthly mean plus the residual:
    // sd 0.12 / sqrt 12 = 0.03464, standard error of the mean 0.00006.
    let monthly = math::annual_to_monthly(0.06);
    assert!(
        (ma - monthly).abs() < 0.0003 && (mb - monthly).abs() < 0.0003,
        "{ma} {mb}"
    );
    let sigma = 0.12 / SQRT_12;
    assert!(
        (sa / sigma - 1.0).abs() < 0.01 && (sb / sigma - 1.0).abs() < 0.02,
        "{sa} {sb}"
    );
    let far = |v: &[f64]| share(v, |r| (r - monthly).abs() > 4.0 * sigma);
    let (fa, fb) = (far(&a), far(&b));
    assert!(fa < 0.000_2, "normal {fa}");
    assert!((fb - 0.002_71).abs() < 0.000_5, "fat {fb}");
}

fn loss(annual_probability: f64, low: f64, high: f64, mean_includes_losses: bool) -> LossEvent {
    LossEvent {
        annual_probability,
        loss: Uncertain::Uniform { low, high },
        mean_includes_losses,
    }
}

/// A custom asset with no volatility, so every month is the stated mean times the
/// loss factor. At 20% a year the monthly chance is `1 - 0.8^(1/12)` = 1.8423%;
/// losses uniform on 20%..60% average 40%.
#[test]
fn loss_events_arrive_at_the_stated_rate_and_size() {
    let base = lognormal(&custom(0.0, TailShape::Normal, None));
    let gross0 = 1.0 + base.assets.custom_mean;
    for compensated in [true, false] {
        let m = lognormal(&custom(
            0.0,
            TailShape::Normal,
            Some(loss(0.2, 0.2, 0.6, compensated)),
        ));
        assert!(m.has_asset_ext());
        only_these_differ(&base, &m, &[CUSTOM], "loss events");
        let l = m.ext.loss.as_deref().unwrap();
        let q = 0.018_423;
        let boost = if compensated {
            1.0 / (1.0 - q * 0.4)
        } else {
            1.0
        };
        let expected = if compensated { 1.0 } else { 1.0 - q * 0.4 };
        assert!((l.expected_factor() - expected).abs() < 1e-6);
        if compensated {
            assert!((l.expected_factor() - 1.0).abs() < 1e-15);
        }
        let factors: Vec<f64> = column(&m, CUSTOM, 600, 600)
            .into_iter()
            .map(|r| (1.0 + r) / gross0)
            .collect();
        let hit: Vec<f64> = factors
            .iter()
            .copied()
            .filter(|f| *f < 0.9 * boost)
            .collect();
        let freq = hit.len() as f64 / factors.len() as f64;
        // 360,000 months: standard error 0.00022.
        assert!((freq - q).abs() < 0.001, "frequency {freq}");
        // Kept share 1 - L is uniform on 0.4..0.8.
        let kept: Vec<f64> = hit.iter().map(|f| f / boost).collect();
        let (mean, sd) = mean_sd(&kept);
        assert!((mean - 0.6).abs() < 0.005, "kept {mean}");
        assert!((sd - 0.4 / 12f64.sqrt()).abs() < 0.004, "sd {sd}");
        assert!(kept.iter().all(|k| *k > 0.4 - 1e-4 && *k < 0.8 + 1e-4));
        // Months without a loss earn the stated return times the boost.
        assert!(factors
            .iter()
            .all(|f| *f < 0.9 * boost || (f - boost).abs() < 1e-5));
        let mean = mean_sd(&factors).0;
        // sd of the factor about 0.4 sqrt(q) = 0.054: standard error 0.00009.
        assert!((mean - expected).abs() < 0.0004, "mean factor {mean}");
    }
    // A zero chance is no option at all; a certain total loss stays finite.
    let off = lognormal(&custom(
        0.1,
        TailShape::Normal,
        Some(loss(0.0, 0.2, 0.6, true)),
    ));
    assert!(!off.has_asset_ext());
    assert!(Loss::new(&loss(f64::NAN, 0.2, 0.6, true)).is_none());
    let total = Loss::new(&loss(9.0, 1.0, 1.0, true)).unwrap();
    assert!(total.expected_factor().is_finite());
}

/// Loss events and cycles share the `AssetExt` stream at fixed positions: each
/// leaves the other's sleeve untouched, in every market kind, and the draw count
/// is three words a month with either or both on.
#[test]
fn cycles_and_loss_events_do_not_move_each_other() {
    let event = Some(loss(0.2, 0.2, 0.6, true));
    for (name, kind) in KINDS {
        let mut both = custom(0.1, TailShape::Fat, event);
        both.crypto.dynamics = CryptoDynamics::cycles();
        let mut cycles_only = custom(0.1, TailShape::Fat, None);
        cycles_only.crypto.dynamics = CryptoDynamics::cycles();
        let loss_only = custom(0.1, TailShape::Fat, event);
        let (both, cycles_only, loss_only) = (
            model(kind, &both),
            model(kind, &cycles_only),
            model(kind, &loss_only),
        );
        only_these_differ(&cycles_only, &both, &[CUSTOM], name);
        only_these_differ(&loss_only, &both, &[CRYPTO], name);
        let mut out = EconomyPath::new();
        for m in [&both, &cycles_only, &loss_only] {
            let mut after = generate(m, SEED, 9, 200, &mut out);
            let mut expect = Rng::stream(SEED, 9, Stream::AssetExt);
            for _ in 0..200 * WORDS_PER_MONTH {
                expect.next_u64();
            }
            assert_eq!(after.next_u64(), expect.next_u64(), "{name}");
        }
    }
}

/// The hardest settings validation accepts, all at once: every return is finite
/// and at least -100%.
#[test]
fn extreme_settings_stay_finite() {
    let portfolio = PortfolioPolicy {
        single_stock: SingleStock {
            volatility: 2.0,
            beta: 4.0,
            tails: TailShape::Fat,
            ruin_hazard: Some(0.5),
        },
        crypto: CryptoAssumptions {
            outlook: CryptoOutlook::Believer,
            token: CryptoToken::OtherTokens,
            dynamics: CryptoDynamics::Cycles {
                calm_months: 600.0,
                wild_months: 1.0,
                volatility_ratio: 5.0,
            },
        },
        custom_asset: Some(CustomAsset {
            mean: 0.5,
            volatility: 2.0,
            equity_corr: -1.0,
            tails: TailShape::Fat,
            loss_event: Some(loss(0.5, 0.0, 1.0, true)),
            ..CustomAsset::default()
        }),
        ..PortfolioPolicy::default()
    };
    for (name, kind) in KINDS {
        let m = model(kind, &portfolio);
        let mut out = EconomyPath::new();
        for path in 0..40 {
            generate(&m, SEED, path, 900, &mut out);
            for i in [CRYPTO, STOCK, CUSTOM] {
                for r in &out.returns[i][..900] {
                    assert!(r.is_finite() && *r >= -1.0 && *r < 1e3, "{name} {i} {r}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Report.
// ---------------------------------------------------------------------------------

/// Growth of 1 held for `years` in one sleeve, in today's dollars, per path.
fn real_growth(m: &EconomyModel, asset: usize, paths: u64, years: usize) -> Vec<f64> {
    let mut out = EconomyPath::new();
    let mut v = Vec::with_capacity(paths as usize);
    for path in 0..paths {
        generate(m, SEED, path, years * 12, &mut out);
        let mut g = 1.0;
        for t in 0..years * 12 {
            g *= 1.0 + out.returns[asset][t];
        }
        v.push(g / out.price_level[years * 12]);
    }
    v.sort_by(f64::total_cmp);
    v
}

/// Deepest fall of a sleeve from an earlier peak within each path, in nominal
/// terms; sorted.
fn worst_drawdowns(m: &EconomyModel, asset: usize, paths: u64, years: usize) -> Vec<f64> {
    let mut out = EconomyPath::new();
    let mut v = Vec::with_capacity(paths as usize);
    for path in 0..paths {
        generate(m, SEED, path, years * 12, &mut out);
        let (mut g, mut peak, mut worst) = (1.0f64, 1.0f64, 0.0f64);
        for t in 0..years * 12 {
            g *= 1.0 + out.returns[asset][t];
            peak = peak.max(g);
            worst = worst.max(1.0 - g / peak);
        }
        v.push(worst);
    }
    v.sort_by(f64::total_cmp);
    v
}

fn q(v: &[f64], p: f64) -> f64 {
    v[((v.len() - 1) as f64 * p) as usize]
}

/// Prints the figures quoted in docs/methodology.md:
/// `cargo test -p firesight-core --release --lib shape_tests::report -- --ignored --nocapture`.
#[test]
#[ignore = "report: prints the measured effect of each sleeve option"]
fn report_sleeve_shapes() {
    let kind = KINDS[0].1;
    let paths = 20_000;
    let line = |label: &str, m: &EconomyModel, asset: usize| {
        let monthly = column(m, asset, 2_000, 360);
        let (mean, sd) = mean_sd(&monthly);
        let g = real_growth(m, asset, paths, 10);
        let avg = g.iter().sum::<f64>() / g.len() as f64;
        println!(
            "{label:34} monthly mean {:6.3}% sd {:5.2}%  <-30% {:6.3}%  >+40% {:6.3}%  |r|<5% {:5.1}%  10y real growth p5 {:5.2} p25 {:5.2} p50 {:5.2} p75 {:5.2} p95 {:5.2} mean {:5.2}  lost>90% {:5.1}%",
            mean * 100.0,
            sd * 100.0,
            share(&monthly, |r| r < -0.30) * 100.0,
            share(&monthly, |r| r > 0.40) * 100.0,
            share(&monthly, |r| r.abs() < 0.05) * 100.0,
            q(&g, 0.05),
            q(&g, 0.25),
            q(&g, 0.5),
            q(&g, 0.75),
            q(&g, 0.95),
            avg,
            share(&g, |x| x < 0.1) * 100.0,
        );
    };
    println!("-- single stock, 35% volatility, beta 1.1, {paths} paths of 10 years");
    for (label, tails, hazard) in [
        ("lognormal (default)", TailShape::Normal, None),
        ("fat tails", TailShape::Fat, None),
        ("lognormal, hazard 3%", TailShape::Normal, Some(0.03)),
        ("fat tails, hazard 3%", TailShape::Fat, Some(0.03)),
    ] {
        let mut p = stock(0.35, tails, hazard);
        p.single_stock.beta = 1.1;
        line(label, &model(kind, &p), STOCK);
    }
    println!("-- crypto");
    {
        let m = model(
            kind,
            &crypto(
                CryptoOutlook::Neutral,
                CryptoToken::BitcoinOrEther,
                CryptoDynamics::cycles(),
            ),
        );
        let c = m.ext.cycles.as_deref().unwrap();
        let steady = model(kind, &PortfolioPolicy::default());
        let s = math::ln(steady.crypto_shock.sample(u64::MAX >> 1)) / CRYPTO_SHOCK_CAP_SD;
        println!(
            "cycles: own (idiosyncratic) volatility a year: steady {:.1}%, calm {:.1}%, wild {:.1}%",
            s * SQRT_12 * 100.0,
            c.sigma[0] * SQRT_12 * 100.0,
            c.sigma[1] * SQRT_12 * 100.0
        );
    }
    let btc = CryptoToken::BitcoinOrEther;
    for (label, outlook, token, dynamics) in [
        (
            "neutral, steady (default)",
            CryptoOutlook::Neutral,
            btc,
            CryptoDynamics::Steady,
        ),
        (
            "skeptic",
            CryptoOutlook::Skeptic,
            btc,
            CryptoDynamics::Steady,
        ),
        (
            "believer",
            CryptoOutlook::Believer,
            btc,
            CryptoDynamics::Steady,
        ),
        (
            "neutral, other tokens",
            CryptoOutlook::Neutral,
            CryptoToken::OtherTokens,
            CryptoDynamics::Steady,
        ),
        (
            "neutral, cycles",
            CryptoOutlook::Neutral,
            btc,
            CryptoDynamics::cycles(),
        ),
    ] {
        let m = model(kind, &crypto(outlook, token, dynamics));
        line(label, &m, CRYPTO);
        let dd = worst_drawdowns(&m, CRYPTO, 5_000, 10);
        // Calendar-year volatility: how different one year's is from the next.
        let monthly = column(&m, CRYPTO, 2_000, 360);
        let mut yearly: Vec<f64> = monthly
            .chunks(12)
            .filter(|c| c.iter().all(|r| *r > -1.0))
            .map(|c| {
                let logs: Vec<f64> = c.iter().map(|r| math::ln_1p(*r)).collect();
                mean_sd(&logs).1 * SQRT_12
            })
            .collect();
        yearly.sort_by(f64::total_cmp);
        println!(
            "{:34} worst 10y drawdown p25 {:4.1}% p50 {:4.1}% p75 {:4.1}%; one-year volatility p10 {:4.1}% p50 {:4.1}% p90 {:4.1}%",
            "",
            q(&dd, 0.25) * 100.0,
            q(&dd, 0.5) * 100.0,
            q(&dd, 0.75) * 100.0,
            q(&yearly, 0.1) * 100.0,
            q(&yearly, 0.5) * 100.0,
            q(&yearly, 0.9) * 100.0,
        );
    }
    println!("-- custom asset: mean 8%, volatility 6%, correlation 0.3 (a credit fund)");
    let fund = |tails, loss_event| PortfolioPolicy {
        custom_asset: Some(CustomAsset {
            mean: 0.08,
            volatility: 0.06,
            equity_corr: 0.3,
            tails,
            loss_event,
            ..CustomAsset::default()
        }),
        ..PortfolioPolicy::default()
    };
    for (label, tails, event) in [
        ("normal (default)", TailShape::Normal, None),
        ("fat tails", TailShape::Fat, None),
        (
            "loss event, mean includes",
            TailShape::Normal,
            Some(LossEvent::default()),
        ),
        (
            "loss event, mean before losses",
            TailShape::Normal,
            Some(LossEvent {
                mean_includes_losses: false,
                ..LossEvent::default()
            }),
        ),
    ] {
        let m = model(kind, &fund(tails, event));
        let monthly = column(&m, CUSTOM, 2_000, 360);
        let (mean, sd) = mean_sd(&monthly);
        let g = real_growth(&m, CUSTOM, paths, 10);
        let dd = worst_drawdowns(&m, CUSTOM, 5_000, 10);
        println!(
            "{label:34} annualised mean {:5.2}% monthly sd x sqrt12 {:5.2}%  |resid|>4sd-ish(<-6%) {:6.3}%  10y real growth p5 {:5.2} p25 {:5.2} p50 {:5.2} p95 {:5.2}  worst drawdown p50 {:4.1}% p90 {:4.1}% p99 {:4.1}%",
            (math::powi(1.0 + mean, 12) - 1.0) * 100.0,
            sd * SQRT_12 * 100.0,
            share(&monthly, |r| r < -0.06) * 100.0,
            q(&g, 0.05),
            q(&g, 0.25),
            q(&g, 0.5),
            q(&g, 0.95),
            q(&dd, 0.5) * 100.0,
            q(&dd, 0.9) * 100.0,
            q(&dd, 0.99) * 100.0,
        );
    }
}
