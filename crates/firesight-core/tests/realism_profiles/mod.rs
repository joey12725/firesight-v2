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
    out
}
