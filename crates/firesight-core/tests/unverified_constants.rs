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
