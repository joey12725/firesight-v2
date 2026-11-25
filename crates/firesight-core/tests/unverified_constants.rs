//! Constants that were written down from memory rather than fetched from a source
//! are listed in `data/actuarial/params.json` under `unverified_from_memory`, so a
//! reviewer can find and check them in one place. This test keeps the code and that
//! list equal.

use firesight_core::data::Data;
use firesight_core::model::{
    default_granted_months_ago, AssetClass, CompanyStage, EquityPolicy, OutcomeKind,
    PrivateCompany, TenderOffers,
};
use firesight_core::{decumulation, economy, equity, health, math, stats};
use serde_json::Value;

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

/// Every number the startup-equity model reads from the data file says where it comes
/// from: `sourced` (and then names its source), `derived` or `judgement`.
#[test]
fn startup_equity_numbers_are_tagged() {
    let data = Data::embedded();
    let block = &data.actuarial["equity_compensation"]["startup_equity"];
    let sources = data.actuarial["sources"].as_object().unwrap();
    assert!(block.is_object(), "startup_equity is missing");

    fn numbers(v: &Value) -> Option<Vec<f64>> {
        v.as_array()?.iter().map(Value::as_f64).collect()
    }

    fn walk(
        path: &str,
        node: &Value,
        sources: &serde_json::Map<String, Value>,
        problems: &mut Vec<String>,
    ) {
        let Some(obj) = node.as_object() else { return };
        let basis_of = |key: &str| {
            obj.get(&format!("{key}_basis"))
                .or_else(|| obj.get("basis"))
                .and_then(Value::as_str)
        };
        for (key, value) in obj {
            let here = format!("{path}.{key}");
            // Lists of two (bounds, quantiles) are not per-stage arrays.
            let pair = matches!(key.as_str(), "range_quantiles")
                || (key == "value" && path.ends_with("outlook_failure_bounds"));
            let stage_array = numbers(value).filter(|_| !pair);
            if let Some(list) = &stage_array {
                if list.len() != 6 {
                    problems.push(format!("{here}: {} entries, not one per stage", list.len()));
                }
            }
            if matches!(key.as_str(), "value" | "median" | "sigma") || stage_array.is_some() {
                match basis_of(key) {
                    Some("sourced" | "derived" | "judgement") => {}
                    other => problems.push(format!("{here}: basis is {other:?}")),
                }
            }
            if key == "source" {
                match value.as_str() {
                    Some(name) if sources.contains_key(name) => {}
                    other => problems.push(format!("{here}: {other:?} is not in sources")),
                }
            }
            walk(&here, value, sources, problems);
        }
        let sourced = obj
            .iter()
            .any(|(k, v)| (k == "basis" || k.ends_with("_basis")) && v == "sourced");
        if sourced && !obj.contains_key("source") {
            problems.push(format!("{path}: sourced, but names no source"));
        }
        if obj.contains_key("basis") {
            match obj["basis"].as_str() {
                Some("sourced" | "derived" | "judgement") => {}
                other => problems.push(format!("{path}.basis: {other:?}")),
            }
        }
    }

    let mut problems = Vec::new();
    walk("startup_equity", block, sources, &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");

    let stages = block["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 6);
    let probability = &block["outcome_probability"];
    for (i, stage) in stages.iter().enumerate() {
        let sum: f64 = ["fails", "acquired", "ipo", "still_private"]
            .iter()
            .map(|k| probability[*k][i].as_f64().unwrap())
            .sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "{stage}: probabilities sum to {sum}"
        );
    }
    // The sources the block's notes cite by name are listed.
    for key in [
        "newview_tender_2026",
        "carta_tender_h1_2026",
        "carta_ptep",
        "stout_vc_q4_2025",
        "nvca_yearbook_2025",
        "pitchbook_nvca_q3_2026",
        "ritter_ipo_stats",
        "ritter_ipo_age",
        "collective_liquidity_series_c",
        "onlycfo_options",
    ] {
        assert!(sources.contains_key(key), "sources lacks {key}");
    }
}

/// Constants of the startup-equity model that live in the code and are recalled or
/// mirror a schema default are listed in the data file and pinned here.
#[test]
fn startup_equity_constants_match_the_data() {
    let data = Data::embedded();
    let num = |v: &Value| v.as_f64().unwrap();
    let memory = &data.actuarial["unverified_from_memory"]["startup_equity"];
    assert_eq!(
        f64::from(equity::DEFAULT_LOCKUP_MONTHS),
        num(&memory["lockup_months"])
    );
    assert_eq!(
        f64::from(equity::OPTION_TERM_MONTHS),
        num(&memory["option_term_months"])
    );
    assert_eq!(
        f64::from(equity::ISO_POST_TERMINATION_MONTHS),
        num(&memory["iso_post_termination_months"])
    );
    assert_eq!(
        PrivateCompany::default().lockup_months,
        equity::DEFAULT_LOCKUP_MONTHS
    );

    let block = &data.actuarial["equity_compensation"]["startup_equity"];
    let tenders = TenderOffers::default();
    assert_eq!(
        tenders.max_fraction,
        num(&block["tender"]["max_fraction"]["value"])
    );
    assert_eq!(
        tenders.price_fraction,
        num(&block["tender"]["price_fraction"]["value"])
    );
    assert_eq!(
        tenders.former_employees,
        block["tender"]["former_employees"]["value"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        f64::from(PrivateCompany::default().exercise_window_months),
        num(&block["exercise_window_months"]["value"])
    );

    let defaults = &block["policy_defaults"];
    assert_eq!(
        EquityPolicy::default().tender_sell_fraction,
        num(&defaults["tender_sell_fraction"])
    );
    assert_eq!(
        f64::from(default_granted_months_ago()),
        num(&defaults["granted_months_ago"])
    );
    assert_eq!(equity::OVERHANG_MAX, num(&defaults["overhang_max"]));
    let quantiles = &defaults["range_quantiles"];
    assert!((math::normal_cdf(-equity::Z90) - num(&quantiles[0])).abs() < 1e-12);
    assert!((math::normal_cdf(equity::Z90) - num(&quantiles[1])).abs() < 1e-12);
    assert_eq!(
        stats::MIN_PATHS_PER_OUTCOME as f64,
        num(&defaults["min_paths_per_outcome"])
    );
    let amt_rates = &data.tax_federal["income_tax"]["alternative_minimum_tax"]["rates"];
    assert_eq!(num(&defaults["iso_amt_estimate_rate"]), num(&amt_rates[1]));

    // The parameters the engine reads are the ones in the file.
    let params = equity::EquityParams::from_data(data);
    assert_eq!(
        params.window_open_share,
        num(&block["window_open_share"]["value"])
    );
    assert_eq!(params.z_cap, num(&block["multiple_z_cap"]["value"]));
    assert_eq!(
        f64::from(params.rounds),
        num(&block["outcome_rounds"]["value"])
    );
    assert_eq!(
        f64::from(params.horizon_months),
        12.0 * num(&block["horizon_years"]["value"])
    );
}

/// A `derived` number says what it was derived from, and a `sourced` one says whether
/// its source was read directly or seen only in a summary: "derived" must not read as
/// "sourced" when its inputs are judgements.
#[test]
fn startup_equity_provenance_is_spelled_out() {
    let data = Data::embedded();
    let block = &data.actuarial["equity_compensation"]["startup_equity"];

    fn walk(path: &str, node: &Value, problems: &mut Vec<String>) {
        let Some(obj) = node.as_object() else { return };
        let tagged = |tag: &str| {
            obj.iter()
                .any(|(k, v)| (k == "basis" || k.ends_with("_basis")) && v == tag)
        };
        if tagged("derived") && obj.get("derived_from").and_then(Value::as_str).is_none() {
            problems.push(format!("{path}: derived, but does not say from what"));
        }
        if tagged("sourced") && !matches!(obj["seen"].as_str(), Some("primary" | "summary")) {
            problems.push(format!(
                "{path}: sourced, but not marked primary or summary"
            ));
        }
        for (key, value) in obj {
            walk(&format!("{path}.{key}"), value, problems);
        }
    }
    let mut problems = Vec::new();
    walk("startup_equity", block, &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");

    // The timing medians are chosen, not computed.
    assert_eq!(block["years_to_outcome"]["basis"], "judgement");
    // The one figure read directly is the tender cap; the confidence line may not claim
    // more than that.
    let confidence = block["confidence"].as_str().unwrap();
    assert!(
        !confidence.contains("tender terms are sourced"),
        "{confidence}"
    );
    assert_eq!(block["tender"]["max_fraction"]["seen"], "primary");
    assert_eq!(block["tender"]["price_fraction"]["seen"], "summary");
    assert_eq!(block["exercise_window_months"]["seen"], "summary");
    assert_eq!(block["tender"]["annual_probability"]["basis"], "judgement");
    assert_eq!(block["lockup_log_sd"]["basis"], "judgement");
}

/// The medians the engine draws are earlier than the `median` parameters of the timing
/// curve, because the curve is cut at the horizon. The data file records both, and the
/// recorded "as drawn" figures are the engine's.
#[test]
fn timing_medians_as_drawn_match_the_engine() {
    let data = Data::embedded();
    let params = equity::EquityParams::from_data(data);
    let timing = &data.actuarial["equity_compensation"]["startup_equity"]["years_to_outcome"];
    let recorded = &timing["as_drawn"];
    for (i, stage) in CompanyStage::ALL.iter().enumerate() {
        let company = PrivateCompany {
            stage: *stage,
            ..PrivateCompany::default()
        };
        let table = equity::resolved_table(&company, &params);
        let mut below = 0.0;
        for row in table.rows() {
            let key = match row.kind {
                OutcomeKind::Fails => "fails_median",
                OutcomeKind::Acquired => "acquired_median",
                OutcomeKind::Ipo => "ipo_median",
                OutcomeKind::StillPrivate => {
                    below = row.cumulative;
                    continue;
                }
            };
            // The middle of the row in the first round, at the median of the timing.
            let u1 = 0.5 * (below + row.cumulative);
            below = row.cumulative;
            let fate = equity::fate_from_words(&table, &params, u1, 0.5, 0, 0);
            assert_eq!((fate.kind, fate.round), (row.kind, 0));
            let years = f64::from(fate.month) / 12.0;
            let want = recorded[key][i].as_f64().unwrap();
            // The engine rounds to a month and the file to two places.
            assert!(
                (years - want).abs() <= 1.0 / 24.0 + 0.006,
                "{stage:?} {key}: drawn {years}, recorded {want}"
            );
            let parameter = timing[key][i].as_f64().unwrap();
            assert!(want <= parameter + 0.006, "{stage:?} {key}");
            if key == "ipo_median" {
                let sigma = timing["sigma"][i].as_f64().unwrap();
                let beyond = 1.0 - math::normal_cdf(math::ln(10.0 / parameter) / sigma);
                let noted = recorded["ipo_beyond_cut"][i].as_f64().unwrap();
                assert!((beyond - noted).abs() < 0.0006, "{stage:?}: {beyond}");
            }
        }
    }
    // The case the methodology quotes: a seed IPO's median is 6.4 years, not 9.
    assert_eq!(timing["ipo_median"][0], 9.0);
    assert_eq!(recorded["ipo_median"][0], 6.4);
}

/// What the cap on price multiples does and does not do, as the data file and the
/// methodology state it: it lowers the seed mean by a few percent and leaves about two
/// thirds of that mean in the best 1% of futures.
#[test]
fn the_multiple_cap_trims_the_tail_and_leaves_the_concentration() {
    let data = Data::embedded();
    let params = equity::EquityParams::from_data(data);
    let cap = &data.actuarial["equity_compensation"]["startup_equity"]["multiple_z_cap"];
    let effect = &cap["effect"];
    assert!(!cap["note"].as_str().unwrap().contains("74%"));

    let company = PrivateCompany {
        stage: CompanyStage::Seed,
        ..PrivateCompany::default()
    };
    let table = equity::resolved_table(&company, &params);
    let mean = equity::implied_multiple(&table, table.overhang, &params);
    let uncapped = equity::EquityParams {
        z_cap: 8.0,
        ..params
    };
    let mean_uncapped = equity::implied_multiple(&table, table.overhang, &uncapped);
    let ratio = mean / mean_uncapped;
    assert!(
        (ratio - effect["seed_mean_over_uncapped"].as_f64().unwrap()).abs() < 0.005,
        "{mean} / {mean_uncapped} = {ratio}"
    );

    // The best 1% of futures are all IPOs above some multiple: the chance of an IPO over
    // the three rounds is above 1%, and the dearest acquisition is cheaper than that
    // multiple.
    let c = params.z_cap;
    let rows = table.rows();
    let ipo = rows.iter().find(|r| r.kind == OutcomeKind::Ipo).unwrap();
    let sale = rows
        .iter()
        .find(|r| r.kind == OutcomeKind::Acquired)
        .unwrap();
    let p_ipo =
        ipo.probability * equity::lifetime_factor(table.still_private_probability(), params.rounds);
    assert!(p_ipo > 0.01);
    // z* with p_ipo x (1 - Phi(z*)) = 1%, by bisection.
    let (mut lo, mut hi) = (-c, c);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if p_ipo * (1.0 - math::normal_cdf(mid)) > 0.01 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let z = 0.5 * (lo + hi);
    assert!(math::exp(ipo.mu + ipo.sigma * z) > math::exp(sale.mu + sale.sigma * c));
    // E[m; z > z*] for a lognormal whose normal is held at c.
    let s = ipo.sigma;
    let top = math::exp(ipo.mu)
        * (math::exp(0.5 * s * s) * (math::normal_cdf(c - s) - math::normal_cdf(z - s))
            + math::exp(s * c) * (1.0 - math::normal_cdf(c)));
    let share = p_ipo * top * params.lockup_mean / mean;
    let recorded = effect["seed_top_1_percent_share_of_mean"].as_f64().unwrap();
    assert!((share - recorded).abs() < 0.01, "top 1% carry {share}");
    assert!(share > 0.6, "the cap does not remove the concentration");
}

/// The older `private_company` block describes grants entered on a job only, and what
/// the file says about the default agrees with the schema: a position under
/// `private_equity` is counted in the plan unless the user says otherwise.
#[test]
fn the_older_private_company_block_does_not_contradict_the_schema() {
    let data = Data::embedded();
    let block = &data.actuarial["equity_compensation"];
    assert!(PrivateCompany::default().included);
    assert!(block["private_company"]["applies_to"]
        .as_str()
        .unwrap()
        .contains("startup_equity"));
    let default = block["private_company"]["model"]["default_in_base_plan"]
        .as_str()
        .unwrap();
    assert!(default.contains("included defaults to true"), "{default}");
    let knob = block["knobs"][0].as_str().unwrap();
    assert!(
        knob.contains("default yes") && !knob.contains("default no"),
        "{knob}"
    );
    // What is modelled is not listed as left out.
    for item in block["left_out"].as_array().unwrap() {
        let item = item.as_str().unwrap();
        assert!(
            !item.starts_with("option exercise strategy") && !item.starts_with("AMT"),
            "{item}"
        );
    }
}
