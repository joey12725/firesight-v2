//! Checks of the optional market-model refinements (`model::market_realism`).

use super::Report;
use crate::model::{BondPricing, MarketKind, MarketModel};

/// Errors and warnings for the optional market settings.
pub(super) fn check(m: &MarketModel, r: &mut Report) {
    if m.bond_pricing == BondPricing::FromYield && !matches!(m.kind, MarketKind::Bootstrap { .. }) {
        r.warn(
            "bond_pricing_ignored",
            "/assumptions/market/bond_pricing",
            "Bond pricing from the yield only applies to the block bootstrap: the regime model always prices bonds from its yield, the historical replay uses the record and the lognormal model has no yield path.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Profile;
    use crate::validate::validate;

    fn codes(p: &Profile) -> Vec<String> {
        let r = validate(p);
        r.errors
            .iter()
            .chain(&r.warnings)
            .map(|i| i.code.clone())
            .collect()
    }

    #[test]
    fn bond_pricing_outside_the_bootstrap_is_flagged() {
        let mut p = Profile::example();
        p.assumptions.market.bond_pricing = BondPricing::FromYield;
        assert!(!codes(&p).contains(&"bond_pricing_ignored".to_string()));
        p.assumptions.market.kind = MarketKind::Regime;
        assert!(codes(&p).contains(&"bond_pricing_ignored".to_string()));
    }
}
