//! Checks of the optional market-model refinements (`model::market_realism`).

use super::Report;
use crate::model::{BondPricing, MarketKind, MarketModel, Valuation};

/// Errors and warnings for the optional market settings.
pub(super) fn check(m: &MarketModel, r: &mut Report) {
    if m.bond_pricing == BondPricing::FromYield && !matches!(m.kind, MarketKind::Bootstrap { .. }) {
        r.warn(
            "bond_pricing_ignored",
            "/assumptions/market/bond_pricing",
            "Bond pricing from the yield only applies to the block bootstrap: the regime model always prices bonds from its yield, the historical replay uses the record and the lognormal and fat-tailed models have no yield path.",
        );
    }
    if let Some(v) = &m.valuation {
        valuation(m, v, r);
    }
}

/// Checks the length of a `LostGeneration` stress at `sp`.
pub(super) fn lost_generation_years(sp: &str, years: f64, r: &mut Report) {
    let path = format!("{sp}/years");
    if r.range(&path, "Duration", years, 0.0, 80.0) && years > 40.0 {
        r.warn(
            "lost_generation_long",
            &path,
            "Japan's stagnation lasted about two decades; a lost generation of more than 40 years is far beyond any national record.",
        );
    }
}

fn valuation(m: &MarketModel, v: &Valuation, r: &mut Report) {
    const P: &str = "/assumptions/market/valuation";
    let start = r.range(
        &format!("{P}/starting_cape"),
        "Starting CAPE",
        v.starting_cape,
        1.0,
        200.0,
    );
    r.range(
        &format!("{P}/fair_cape"),
        "Normal CAPE",
        v.fair_cape,
        1.0,
        200.0,
    );
    let half = r.range(
        &format!("{P}/half_life_years"),
        "Valuation half-life",
        v.half_life_years,
        0.5,
        100.0,
    );
    if start && !(5.0..=60.0).contains(&v.starting_cape) {
        r.warn("starting_cape_implausible", &format!("{P}/starting_cape"), "The US market's CAPE has ranged from about 5 to 44 since 1881; check the starting value.");
    }
    if half && !(5.0..=40.0).contains(&v.half_life_years) {
        r.warn("valuation_half_life_implausible", &format!("{P}/half_life_years"), "Valuations have historically drifted back to normal over one to two decades; a half-life under 5 or over 40 years is outside that.");
    }
    if matches!(m.kind, MarketKind::Historical) {
        r.warn("valuation_ignored", P, "The historical replay replays the record as it was; the valuation setting does not apply to it.");
    }
    if m.equity_premium_adjustment < 0.0 && v.starting_cape > v.fair_cape {
        r.warn("valuation_double_counted", "/assumptions/market/equity_premium_adjustment", "Both the equity premium adjustment and the valuation setting lower expected stock returns, partly for the same reason (today's high valuations). Consider setting the adjustment to 0 when valuation is on.");
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

    #[test]
    fn valuation_values_are_checked() {
        let mut p = Profile::example();
        p.assumptions.market.valuation = Some(Valuation::default());
        let c = codes(&p);
        // The default equity adjustment also lowers returns.
        assert!(c.contains(&"valuation_double_counted".to_string()));
        assert!(validate(&p).errors.is_empty());
        p.assumptions.market.equity_premium_adjustment = 0.0;
        assert!(!codes(&p).iter().any(|c| c.starts_with("valuation")));
        p.assumptions.market.valuation = Some(Valuation {
            fair_cape: 0.0,
            half_life_years: 60.0,
            starting_cape: 70.0,
        });
        let r = validate(&p);
        assert!(r
            .errors
            .iter()
            .any(|i| i.path == "/assumptions/market/valuation/fair_cape"));
        let c = codes(&p);
        assert!(c.contains(&"starting_cape_implausible".to_string()));
        assert!(c.contains(&"valuation_half_life_implausible".to_string()));
        p.assumptions.market.valuation = Some(Valuation::default());
        p.assumptions.market.kind = MarketKind::Historical;
        assert!(codes(&p).contains(&"valuation_ignored".to_string()));
    }

    #[test]
    fn lost_generation_years_are_checked() {
        use crate::model::{Stress, When};
        let mut p = Profile::example();
        p.assumptions.market.stress = vec![Stress::LostGeneration {
            start: When::retirement(0.0),
            years: 50.0,
        }];
        assert!(codes(&p).contains(&"lost_generation_long".to_string()));
        assert!(validate(&p).errors.is_empty());
        p.assumptions.market.stress = vec![Stress::LostGeneration {
            start: When::retirement(0.0),
            years: -1.0,
        }];
        assert!(validate(&p)
            .errors
            .iter()
            .any(|i| i.path == "/assumptions/market/stress/0/years"));
    }
}
