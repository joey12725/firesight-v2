//! Simulation-level checks of the optional market refinements on paired paths: off
//! is the baseline bit for bit, on moves outcomes in the expected direction by a
//! plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{PathScalar, Summary, TraceEventKind};

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

// ---------------------------------------------------------------------------------
// F37: valuation-aware expected returns.
// ---------------------------------------------------------------------------------

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .profile
}

/// A CAPE of 40 against a normal 17.3 lowers stock returns by about three points a
/// year over the first decade: a saver reaches independence later and succeeds
/// less often. A fairly valued start changes little.
#[test]
fn an_expensive_market_delays_independence_and_lowers_success() {
    let mut off = example("early-career-saver");
    off.assumptions.market.equity_premium_adjustment = 0.0;
    let mut on = off.clone();
    on.assumptions.market.valuation = Some(Valuation::default());
    let n = 2000;
    let (a, b) = (summary(&off, SEED, n), summary(&on, SEED, n));
    let (sa, sb) = (a.headline.success_rate.value, b.headline.success_rate.value);
    let (fa, fb) = (a.headline.median_fi_age, b.headline.median_fi_age);
    eprintln!("early career: success {sa:.3} -> {sb:.3}; median FI age {fa:.2} -> {fb:.2}");
    assert!(sb < sa - 0.02 && sb > sa - 0.30, "{sa} {sb}");
    assert!(fb > fa + 0.25 && fb < fa + 8.0, "{fa} {fb}");

    let mut fair = off.clone();
    fair.assumptions.market.valuation = Some(Valuation {
        starting_cape: 17.3,
        ..Valuation::default()
    });
    let f = summary(&fair, SEED, n).headline.success_rate.value;
    eprintln!("early career, fairly valued start: success {f:.3}");
    assert!((f - sa).abs() < 0.03, "{sa} {f}");
}

#[test]
fn a_traced_path_says_where_valuations_start() {
    let mut p = example("early-career-saver");
    p.assumptions.market.valuation = Some(Valuation::default());
    let trace = sim::trace_path(&compile(&p), SEED, 1);
    let note = trace
        .events
        .iter()
        .find(|e| e.kind == TraceEventKind::Market)
        .expect("a market note");
    assert_eq!(note.month, START);
    assert!(note.label.contains("CAPE of 40.0"), "{}", note.label);
    let plain = sim::trace_path(&compile(&example("early-career-saver")), SEED, 1);
    assert!(!plain
        .events
        .iter()
        .any(|e| e.kind == TraceEventKind::Market));
}

// ---------------------------------------------------------------------------------
// F38: historical-episode and lost-generation stresses.
// ---------------------------------------------------------------------------------

fn retiree_with(stress: Option<Stress>) -> Profile {
    let mut p = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    p.assumptions.market.kind = bootstrap();
    p.assumptions.market.equity_premium_adjustment = 0.0;
    p.assumptions.market.stress = stress.into_iter().collect();
    p
}

fn episode_now(episode: Episode) -> Stress {
    Stress::HistoricalEpisode {
        start: When::date(START),
        episode,
    }
}

/// An episode dated after the plan ends changes nothing, bit for bit.
#[test]
fn an_episode_beyond_the_horizon_is_the_baseline_bit_for_bit() {
    let base = example("dual-income-family");
    let mut late = base.clone();
    late.assumptions
        .market
        .stress
        .push(Stress::HistoricalEpisode {
            start: When::date(YearMonth::ym(2200, 1)),
            episode: Episode::Stagflation1966,
        });
    late.assumptions.market.stress.push(Stress::LostGeneration {
        start: When::date(YearMonth::ym(2200, 1)),
        years: 20.0,
    });
    assert_eq!(digest_json(&base, 300), digest_json(&late, 300));
}

/// A 60/40 retiree at a 4% withdrawal who retires into 1966 is Bengen's worst case:
/// success falls hard. 2008 hurts less (a deep fall but a five-year recovery with
/// bonds rallying), and a Japan-style lost generation hurts a great deal.
#[test]
fn retiring_into_history_lowers_success_in_the_expected_order() {
    let n = 1500;
    let base = summary(&retiree_with(None), SEED, n)
        .headline
        .success_rate
        .value;
    let rate = |s: Stress| {
        summary(&retiree_with(Some(s)), SEED, n)
            .headline
            .success_rate
            .value
    };
    let stag = rate(episode_now(Episode::Stagflation1966));
    let gfc = rate(episode_now(Episode::Financial2008));
    let dep = rate(episode_now(Episode::GreatDepression1929));
    let dot = rate(episode_now(Episode::DotCom2000));
    let lost = rate(Stress::LostGeneration {
        start: When::date(START),
        years: 20.0,
    });
    eprintln!(
        "60/40 retiree at 4%: base {base:.3}, 1966 {stag:.3}, 2008 {gfc:.3}, 1929 {dep:.3}, 2000 {dot:.3}, lost generation {lost:.3}"
    );
    assert!(stag < base - 0.25, "{base} {stag}");
    assert!(gfc < base && gfc > stag, "{gfc}");
    assert!(dot < base, "{dot}");
    assert!(dep < base, "{dep}");
    assert!(lost < base - 0.25, "{base} {lost}");
}

#[test]
fn a_traced_path_announces_the_episode() {
    let p = retiree_with(Some(episode_now(Episode::GreatDepression1929)));
    let trace = sim::trace_path(&compile(&p), SEED, 1);
    let note = trace
        .events
        .iter()
        .find(|e| e.kind == TraceEventKind::Market)
        .expect("a market note");
    assert_eq!(note.month, START);
    assert!(note.label.contains("Great Depression"), "{}", note.label);
}

// ---------------------------------------------------------------------------------
// F39: the fat-tailed parametric model.
// ---------------------------------------------------------------------------------

fn scalar_values(p: &Profile, paths: u64, s: PathScalar) -> Vec<f64> {
    let acc = run(&compile(p), SEED, 0..paths);
    let mut v: Vec<f64> = acc
        .scalar_column(s)
        .iter()
        .map(|x| f64::from(*x))
        .filter(|x| x.is_finite())
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

/// Same means and volatilities as the lognormal model, on the same normal shocks,
/// but fat tails and clustered volatility. A 60/40 retiree spending 4.5% lives
/// through deeper worst falls of the portfolio in the bad tail of paths. Success
/// barely moves: volatility is concentrated in fewer, wilder paths, so most paths
/// see calmer markets than the lognormal model's and a few far rougher ones, and
/// over 30 years the two roughly cancel.
#[test]
fn fat_tails_deepen_the_worst_falls_of_a_retiree() {
    let n = 3000;
    let mut ln = retiree(1_000_000.0, 0.6, 45_000.0, 30.0);
    ln.assumptions.market.kind = MarketKind::Lognormal;
    let mut fat = ln.clone();
    fat.assumptions.market.kind = MarketKind::FatTailed;
    let (a, b) = (summary(&ln, SEED, n), summary(&fat, SEED, n));
    let (sa, sb) = (a.headline.success_rate.value, b.headline.success_rate.value);
    let da = scalar_values(&ln, n, PathScalar::WorstDrawdown);
    let db = scalar_values(&fat, n, PathScalar::WorstDrawdown);
    let q = |v: &[f64], p: f64| v[(p * (v.len() - 1) as f64) as usize];
    eprintln!(
        "60/40 retiree at 4.5%: success lognormal {sa:.3} fat-tailed {sb:.3}; worst drawdown quantiles 5/50/95% lognormal {:.3} {:.3} {:.3}, fat-tailed {:.3} {:.3} {:.3}; median terminal {:.0} -> {:.0}",
        q(&da, 0.05), q(&da, 0.5), q(&da, 0.95), q(&db, 0.05), q(&db, 0.5), q(&db, 0.95),
        median(&a),
        median(&b)
    );
    // The worst falls (a positive fraction of the peak) are deeper in the bad tail
    // and shallower in the calm one.
    assert!(q(&db, 0.95) > q(&da, 0.95) + 0.04);
    assert!(q(&db, 0.05) < q(&da, 0.05));
    assert!((sb - sa).abs() < 0.04, "{sa} {sb}");
    let ratio = median(&b) / median(&a);
    assert!((0.85..1.15).contains(&ratio), "{ratio}");
}

/// A fat-tailed trace announces its crash months; a lognormal one with the same
/// shocks has none to announce.
#[test]
fn a_fat_tailed_trace_announces_crash_months() {
    let mut fat = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    fat.assumptions.market.kind = MarketKind::FatTailed;
    let mut ln = fat.clone();
    ln.assumptions.market.kind = MarketKind::Lognormal;
    let crash = |p: &Profile, path: u64| {
        sim::trace_path(&compile(p), SEED, path)
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::Market && e.label.contains("in one month"))
            .count()
    };
    let fat_notes: usize = (0..40).map(|i| crash(&fat, i)).sum();
    let ln_notes: usize = (0..40).map(|i| crash(&ln, i)).sum();
    eprintln!("crash-month notes over 40 traced paths: {fat_notes}");
    assert!(fat_notes > 0);
    assert_eq!(ln_notes, 0);
}

// ---------------------------------------------------------------------------------
// F40: dividend yields that move with the market.
// ---------------------------------------------------------------------------------

#[test]
fn dynamic_yields_off_is_the_baseline_bit_for_bit() {
    for example in Profile::examples(START) {
        let mut off = example.profile.clone();
        off.assumptions.market.dynamic_yields = false;
        assert_eq!(
            digest_json(&example.profile, 300),
            digest_json(&off, 300),
            "{}",
            example.slug
        );
    }
}

/// From a CAPE of 40 the dividend yield rises toward 3.5% as valuations normalise,
/// so a wealthy retiree holding stocks in a taxable account receives more of the
/// same total return as taxable dividends: more tax, a little less wealth.
#[test]
fn dynamic_yields_with_valuation_raise_a_taxable_retirees_tax() {
    let n = 1500;
    let mut off = retiree(3_000_000.0, 0.8, 120_000.0, 30.0);
    off.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    off.assumptions.market.kind = bootstrap();
    off.assumptions.market.valuation = Some(Valuation::default());
    let mut on = off.clone();
    on.assumptions.market.dynamic_yields = true;
    let (a, b) = (summary(&off, SEED, n), summary(&on, SEED, n));
    let (ta, tb) = (
        a.headline.median_lifetime_tax,
        b.headline.median_lifetime_tax,
    );
    eprintln!(
        "80/20 taxable retiree with $3M, CAPE 40: median lifetime tax {ta:.0} -> {tb:.0}, median terminal {:.0} -> {:.0}, success {:.3} -> {:.3}",
        median(&a),
        median(&b),
        a.headline.success_rate.value,
        b.headline.success_rate.value
    );
    assert!(tb > ta * 1.05, "{ta} {tb}");
    assert!(tb < ta * 2.0, "{ta} {tb}");
    let ratio = median(&b) / median(&a);
    assert!((0.85..1.0).contains(&ratio), "{ratio}");
}

/// Retiring into 2008 with dynamic yields, the trace says when dividends are cut.
#[test]
fn a_dynamic_yield_trace_announces_dividend_cuts() {
    let mut p = retiree_with(Some(episode_now(Episode::Financial2008)));
    p.assumptions.market.dynamic_yields = true;
    let trace = sim::trace_path(&compile(&p), SEED, 1);
    let note = trace
        .events
        .iter()
        .find(|e| e.label.contains("cut dividends"))
        .expect("a dividend-cut note");
    assert_eq!(note.kind, TraceEventKind::Market);
    assert!(
        note.month > START && note.month < START + 36,
        "{}",
        note.month
    );
    let mut off = p.clone();
    off.assumptions.market.dynamic_yields = false;
    assert!(!sim::trace_path(&compile(&off), SEED, 1)
        .events
        .iter()
        .any(|e| e.label.contains("cut dividends")));
}
