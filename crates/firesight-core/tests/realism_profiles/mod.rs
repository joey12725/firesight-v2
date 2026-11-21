//! Profiles with the optional market refinements (`model::market_realism`) switched
//! on, for the conservation and finite-value tests.
#![allow(dead_code)]

use firesight_core::model::*;

/// `(label, profile)` for each refinement applied to `base`.
pub fn variants(base: &Profile) -> Vec<(String, Profile)> {
    let mut out = Vec::new();
    let bootstrap = MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    };
    let mut p = base.clone();
    p.assumptions.market.kind = bootstrap;
    p.assumptions.market.bond_pricing = BondPricing::FromYield;
    out.push(("bond_pricing".to_string(), p));
    let mut p = base.clone();
    p.assumptions.market.kind = MarketKind::FatTailed;
    out.push(("fat_tailed".to_string(), p));
    for kind in [
        bootstrap,
        MarketKind::Regime,
        MarketKind::Lognormal,
        MarketKind::FatTailed,
    ] {
        let mut p = base.clone();
        p.assumptions.market.kind = kind;
        p.assumptions.market.valuation = Some(Valuation::default());
        out.push((format!("valuation {kind:?}"), p));
    }
    for (i, episode) in [
        Episode::GreatDepression1929,
        Episode::Stagflation1966,
        Episode::DotCom2000,
        Episode::Financial2008,
    ]
    .into_iter()
    .enumerate()
    {
        let mut p = base.clone();
        // Spread the start dates so episodes land before and after retirement.
        p.assumptions.market.stress.push(Stress::HistoricalEpisode {
            start: When::date(YearMonth::ym(2029 + 7 * i as i32, 1)),
            episode,
        });
        out.push((format!("episode {episode:?}"), p));
    }
    let mut p = base.clone();
    p.assumptions.market.stress.push(Stress::LostGeneration {
        start: When::retirement(0.0),
        years: 20.0,
    });
    out.push(("lost generation".to_string(), p));
    for kind in [bootstrap, MarketKind::FatTailed, MarketKind::Historical] {
        let mut p = base.clone();
        p.assumptions.market.kind = kind;
        p.assumptions.market.dynamic_yields = true;
        out.push((format!("dynamic_yields {kind:?}"), p));
    }
    let mut p = base.clone();
    p.assumptions.market.dynamic_yields = true;
    p.assumptions.market.valuation = Some(Valuation::default());
    p.assumptions.market.stress.push(Stress::CrashAtRetirement {
        drawdown: 0.4,
        recovery_years: 5.0,
    });
    out.push(("dynamic_yields valuation crash".to_string(), p));
    out
}
