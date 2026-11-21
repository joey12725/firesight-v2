//! Constants that were written down from memory rather than fetched from a source
//! are listed in `data/actuarial/params.json` under `unverified_from_memory`, so a
//! reviewer can find and check them in one place. This test keeps the code and that
//! list equal.

use firesight_core::data::Data;
use firesight_core::model::AssetClass;
use firesight_core::{decumulation, economy, health};

#[test]
fn constants_quoted_from_memory_match_the_data_file() {
    let section = &Data::embedded().actuarial["unverified_from_memory"];
    assert!(section["warning"]
        .as_str()
        .unwrap()
        .starts_with("UNVERIFIED"));
    let num = |v: &serde_json::Value| v.as_f64().unwrap();
    for asset in AssetClass::ALL {
        let i = asset.index();
        let yield_ = section["dividend_yield_annual"][asset.key()]
            .as_f64()
            .unwrap_or(0.0);
        assert_eq!(economy::DIVIDEND_YIELD[i], yield_, "{}", asset.key());
        let qualified = section["qualified_dividend_fraction"][asset.key()]
            .as_f64()
            .unwrap_or(0.0);
        assert_eq!(economy::QUALIFIED_FRACTION[i], qualified, "{}", asset.key());
    }
    let reit = &section["listed_real_estate"];
    assert_eq!(economy::REIT_EQUITY_BETA, num(&reit["equity_beta"]));
    assert_eq!(economy::REIT_HOME_BETA, num(&reit["home_price_beta"]));
    assert_eq!(
        economy::REIT_IDIOSYNCRATIC_VOL,
        num(&reit["idiosyncratic_volatility"])
    );
    let table: Vec<f64> = section["irs_single_life_table"]["life_expectancy"]
        .as_array()
        .unwrap()
        .iter()
        .map(num)
        .collect();
    assert_eq!(decumulation::SINGLE_LIFE_EXPECTANCY.to_vec(), table);
    assert_eq!(
        health::GOOD_HEALTH_MULTIPLIER,
        num(&section["good_health_mortality_multiplier"]["value"])
    );
}

/// The optional market refinements keep their sourced values and judgements in
/// `market_realism`, each with a source and a confidence note.
#[test]
fn market_realism_constants_match_the_data_file() {
    use firesight_core::model;
    let section = &Data::embedded().actuarial["market_realism"];
    let num = |v: &serde_json::Value| v.as_f64().unwrap();
    for (_, entry) in section
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| *k != "summary")
    {
        assert!(entry["source"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(entry["confidence"].as_str().is_some_and(|s| !s.is_empty()));
    }
    let v = &section["valuation"];
    assert_eq!(model::DEFAULT_STARTING_CAPE, num(&v["starting_cape"]));
    assert_eq!(model::DEFAULT_FAIR_CAPE, num(&v["fair_cape"]));
    assert_eq!(
        model::DEFAULT_VALUATION_HALF_LIFE_YEARS,
        num(&v["half_life_years"])
    );
    let l = &section["lost_generation"];
    assert_eq!(model::DEFAULT_LOST_GENERATION_YEARS, num(&l["years"]));
    assert_eq!(
        model::LOST_GENERATION_EQUITY_REAL,
        num(&l["equity_real_return"])
    );
    assert_eq!(model::LOST_GENERATION_INFLATION, num(&l["inflation"]));
    assert_eq!(model::LOST_GENERATION_LONG_YIELD, num(&l["long_yield"]));
    assert_eq!(model::LOST_GENERATION_GLIDE_YEARS, num(&l["glide_years"]));
    assert_eq!(model::LOST_GENERATION_BILL_YIELD, num(&l["bill_yield"]));
    let f = &section["fat_tailed"];
    assert_eq!(economy::FAT_TAIL_ARCH, num(&f["arch"]));
    assert_eq!(economy::FAT_TAIL_GARCH, num(&f["garch"]));
    assert_eq!(economy::FAT_TAIL_DOF, num(&f["dof"]));
    assert_eq!(economy::FAT_TAIL_VARIANCE_CAP, num(&f["variance_cap"]));
    assert_eq!(economy::FAT_TAIL_MIXING_CAP, num(&f["mixing_cap"]));
    let d = &section["dynamic_yields"];
    assert_eq!(
        economy::DIVIDEND_ADJUSTMENT_ANNUAL,
        num(&d["adjustment_annual"])
    );
    assert_eq!(
        economy::RECESSION_DIVIDEND_CUT_ANNUAL,
        num(&d["recession_cut_annual"])
    );
    assert_eq!(economy::DIVIDEND_YIELD_RANGE.0, num(&d["yield_range"][0]));
    assert_eq!(economy::DIVIDEND_YIELD_RANGE.1, num(&d["yield_range"][1]));
}
