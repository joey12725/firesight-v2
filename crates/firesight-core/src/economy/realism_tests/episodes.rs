//! F38: historical-episode and lost-generation stresses.

use super::*;
use crate::model::{
    Episode, Stress, When, LOST_GENERATION_EQUITY_REAL, LOST_GENERATION_INFLATION,
    LOST_GENERATION_LONG_YIELD,
};

fn with_episode(kind: MarketKind, episode: Episode) -> MarketModel {
    MarketModel {
        stress: vec![Stress::HistoricalEpisode {
            start: When::date(YearMonth::ym(2030, 1)),
            episode,
        }],
        ..market(kind)
    }
}

fn stressed(
    m: &EconomyModel,
    path: u64,
    months: usize,
    s: ResolvedStress,
) -> (EconomyPath, EconomyPath) {
    let mut a = EconomyPath::new();
    generate(m, path, months, &mut a);
    let mut b = a.clone();
    m.apply_stress(&s, &mut b);
    (a, b)
}

const EPISODES: [Episode; 4] = [
    Episode::GreatDepression1929,
    Episode::Stagflation1966,
    Episode::DotCom2000,
    Episode::Financial2008,
];

/// Every core series of the window is the record's own month, bit for bit, and the
/// months before it are untouched.
#[test]
fn an_episode_replays_the_record_month_by_month() {
    let h = &Data::embedded().history;
    for episode in EPISODES {
        let m = build(&with_episode(bootstrap(), episode));
        let from = 30;
        let len = episodes::episode_months(episode);
        let (a, b) = stressed(
            &m,
            3,
            480,
            ResolvedStress::HistoricalEpisode {
                from_month: from,
                episode,
            },
        );
        let first = h.index_of(episodes::episode_first_month(episode)).unwrap();
        for k in 0..len {
            let (t, i) = (from + k, first + k);
            assert_eq!(b.returns[AssetClass::UsStocks.index()][t], h.equity_us[i]);
            assert_eq!(b.returns[AssetClass::Bonds.index()][t], h.bond10[i]);
            assert_eq!(b.returns[AssetClass::Cash.index()][t], h.tbill[i]);
            assert_eq!(b.inflation[t], h.cpi[i]);
            assert_eq!(b.recession[t], h.recession[i]);
            if h.equity_intl[i].is_finite() {
                assert_eq!(
                    b.returns[AssetClass::IntlStocks.index()][t],
                    h.equity_intl[i]
                );
            }
            // Rate levels move by the record's changes from the path's level.
            let d = b.long_yield[t] - b.long_yield[t - 1];
            let dh = h.long_yield[i] - h.long_yield[i - 1];
            assert!(
                (d - dh).abs() < 1e-12 || b.long_yield[t] == MIN_LONG_YIELD,
                "{episode:?} {k}"
            );
        }
        for t in 0..from {
            assert_eq!(
                a.returns[AssetClass::UsStocks.index()][t],
                b.returns[AssetClass::UsStocks.index()][t]
            );
            assert_eq!(a.long_yield[t], b.long_yield[t]);
        }
        // The price level follows the replayed inflation.
        let p = b.price_level[from + len] / b.price_level[from];
        let ph: f64 = (first..first + len).map(|i| 1.0 + h.cpi[i]).product();
        assert!((p / ph - 1.0).abs() < 1e-12);
        // After the window the path's own returns resume.
        let t = from + len + 5;
        assert_eq!(
            a.returns[AssetClass::UsStocks.index()][t],
            b.returns[AssetClass::UsStocks.index()][t]
        );
    }
}

/// The Great Depression costs about 79% at the trough in real terms: the replayed
/// path's real US stock index falls by exactly the record's drawdown.
#[test]
fn the_depression_drawdown_is_the_record() {
    let m = build(&with_episode(bootstrap(), Episode::GreatDepression1929));
    let (_, b) = stressed(
        &m,
        1,
        400,
        ResolvedStress::HistoricalEpisode {
            from_month: 12,
            episode: Episode::GreatDepression1929,
        },
    );
    let us = AssetClass::UsStocks.index();
    let mut level = 1.0;
    let mut low: f64 = 1.0;
    for t in 12..12 + 186 {
        level *= (1.0 + b.returns[us][t]) / (1.0 + b.inflation[t]);
        low = low.min(level);
    }
    assert!((low - (1.0 - 0.792)).abs() < 0.001, "{low}");
    assert!((1.0..1.06).contains(&level), "{level}");
}

/// Rates move with the record inside the window, and the shift they leave fades
/// instead of snapping back: after 1966-82 yields end about 9 points higher, and
/// ten years later about 3.9 points of that shift remain (0.991667^120 = 0.367).
#[test]
fn rate_shifts_fade_after_the_window() {
    let m = build(&with_episode(bootstrap(), Episode::Stagflation1966));
    let from = 24;
    let (a, b) = stressed(
        &m,
        2,
        600,
        ResolvedStress::HistoricalEpisode {
            from_month: from,
            episode: Episode::Stagflation1966,
        },
    );
    let end = from + 199;
    let s_end = b.long_yield[end - 1] - a.long_yield[end - 1];
    assert!(s_end > 0.06, "{s_end}");
    let s_later = b.long_yield[end + 119] - a.long_yield[end + 119];
    let expected = s_end * math::powi(1.0 - RATE_REVERSION, 120);
    assert!((s_later - expected).abs() < 1e-9, "{s_later} {expected}");
    // Mortgage rates are never below the yield.
    assert!((0..600).all(|t| b.mortgage_rate[t] >= b.long_yield[t]));
}

/// Assets without history follow the imposed US move by beta; a collapsed holding
/// stays collapsed.
#[test]
fn followers_move_by_beta_and_collapses_stay() {
    let m = build(&with_episode(bootstrap(), Episode::Financial2008));
    let mut a = EconomyPath::new();
    generate(&m, 4, 200, &mut a);
    a.returns[AssetClass::SingleStock.index()][20] = -1.0;
    let mut b = a.clone();
    m.apply_stress(
        &ResolvedStress::HistoricalEpisode {
            from_month: 20,
            episode: Episode::Financial2008,
        },
        &mut b,
    );
    assert_eq!(b.returns[AssetClass::SingleStock.index()][20], -1.0);
    let us = AssetClass::UsStocks.index();
    let re = AssetClass::RealEstate.index();
    let beta = m.stress_betas()[re];
    let t = 25;
    let expected = shifted_ref(a.returns[re][t], beta, b.returns[us][t] - a.returns[us][t]);
    assert_eq!(b.returns[re][t], expected);
}

fn shifted_ref(r: f64, beta: f64, delta: f64) -> f64 {
    if r <= -1.0 {
        -1.0
    } else {
        (r + beta * delta).max(-1.0)
    }
}

/// Without a `HistoricalEpisode` in the profile the model holds no episode tables.
#[test]
fn episode_tables_are_built_only_when_needed() {
    assert!(build(&market(bootstrap())).episodes.is_none());
    assert!(
        build(&with_episode(MarketKind::Regime, Episode::DotCom2000))
            .episodes
            .is_some()
    );
}

/// A lost generation hits its targets exactly: real stock return -2% a year,
/// inflation -0.3% a year, and the yield at the floor plus the path's own moves
/// once the glide is over.
#[test]
fn a_lost_generation_hits_its_targets() {
    for kind in [bootstrap(), MarketKind::Regime, MarketKind::Lognormal] {
        let m = build(&market(kind));
        let (from, months) = (36, 240);
        let (a, b) = stressed(
            &m,
            9,
            480,
            ResolvedStress::LostGeneration {
                from_month: from,
                months,
            },
        );
        let n = months as f64;
        let us = AssetClass::UsStocks.index();
        let intl = AssetClass::IntlStocks.index();
        let mut real = (0.0, 0.0);
        let mut infl = 0.0;
        for t in from..from + months {
            let d = math::ln_1p(b.inflation[t]);
            infl += d;
            real.0 += math::ln_1p(b.returns[us][t]) - d;
            real.1 += math::ln_1p(b.returns[intl][t]) - d;
        }
        let ann = |l: f64| math::exp(12.0 * l / n) - 1.0;
        assert!(
            (ann(real.0) - LOST_GENERATION_EQUITY_REAL).abs() < 1e-9,
            "{kind:?}"
        );
        assert!(
            (ann(real.1) - LOST_GENERATION_EQUITY_REAL).abs() < 1e-9,
            "{kind:?}"
        );
        assert!(
            (ann(infl) - LOST_GENERATION_INFLATION).abs() < 1e-9,
            "{kind:?}"
        );
        // After the eight-year glide the yield is the floor plus the path's own
        // change since the start.
        let t = from + 150;
        let own = a.long_yield[t] - a.long_yield[from - 1];
        let want = (LOST_GENERATION_LONG_YIELD + own).max(MIN_LONG_YIELD);
        assert!((b.long_yield[t] - want).abs() < 1e-12, "{kind:?}");
        // Bonds are priced from that yield.
        let br = m.bonds.bond_return(b.long_yield[t - 1], b.long_yield[t]);
        assert_eq!(b.returns[AssetClass::Bonds.index()][t], br);
        // Monthly equity volatility is the path's own (one constant factor).
        let ratio = |t: usize| (1.0 + b.returns[us][t]) / (1.0 + a.returns[us][t]);
        assert!((ratio(from) - ratio(from + 100)).abs() < 1e-12);
        // Nothing before the window moves.
        assert!(bits(&a.inflation[..from]) == bits(&b.inflation[..from]));
    }
}
