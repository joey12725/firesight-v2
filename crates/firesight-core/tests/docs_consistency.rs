//! The published documents quote figures and facts that live elsewhere in the
//! repository. These tests keep the quotes that have gone stale before equal to their
//! source: the timing table of the methodology, the number of example plans the README
//! counts, and what the assistant document says the wasm wrapper exports.

use firesight_core::data::Data;

const METHODOLOGY: &str = include_str!("../../../docs/methodology.md");
const README: &str = include_str!("../../../README.md");
const ASSISTANT: &str = include_str!("../../../docs/assistant.md");
const PRIMER: &str = include_str!("../../../docs/primer.md");
const WASM: &str = include_str!("../../firesight-wasm/src/lib.rs");

/// The six per-stage numbers of the table row that starts with `label`, and its basis.
fn table_row(label: &str) -> (Vec<f64>, String) {
    let line = METHODOLOGY
        .lines()
        .find(|l| l.starts_with(&format!("| {label} |")))
        .unwrap_or_else(|| panic!("no table row {label:?}"));
    let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(cells.len(), 8, "{line}");
    let numbers = cells[1..7].iter().map(|c| c.parse().unwrap()).collect();
    (numbers, cells[7].to_string())
}

#[test]
fn the_methodology_timing_table_is_the_data_files() {
    let timing =
        &Data::embedded().actuarial["equity_compensation"]["startup_equity"]["years_to_outcome"];
    let stage = |node: &serde_json::Value| -> Vec<f64> {
        node.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    // The parameters of the curve are labelled as such and tagged as the file tags them.
    for (label, key) in [
        (
            "Years to failure: median of the timing curve before the ten-year cut",
            "fails_median",
        ),
        ("Years to acquisition: the same", "acquired_median"),
        ("Years to IPO: the same", "ipo_median"),
    ] {
        let (numbers, basis) = table_row(label);
        assert_eq!(numbers, stage(&timing[key]), "{label}");
        assert_eq!(basis, timing["basis"].as_str().unwrap(), "{label}");
    }
    // The medians the engine draws, to one decimal place.
    for (label, key) in [
        (
            "Median years to acquisition as drawn, within a ten-year round",
            "acquired_median",
        ),
        (
            "Median years to IPO as drawn, within a ten-year round",
            "ipo_median",
        ),
    ] {
        let (numbers, basis) = table_row(label);
        let recorded = stage(&timing["as_drawn"][key]);
        for (shown, want) in numbers.iter().zip(&recorded) {
            assert!(
                (shown - want).abs() < 0.051,
                "{label}: {shown} against {want}"
            );
        }
        assert_eq!(basis, timing["as_drawn"]["basis"].as_str().unwrap());
    }
    // No row calls a parameter of the uncut curve the median.
    assert!(!METHODOLOGY.contains("| Median years to IPO |"));
}

#[test]
fn the_readme_counts_the_example_plans() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let plans = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .count();
    let word = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
    ][plans];
    assert!(
        README.contains(&format!("for {word} complete ones")),
        "examples/ holds {plans} plans; the README says otherwise"
    );
    // The trust section says what the audits did not cover.
    assert!(README.contains("were added after these audits"));
}

#[test]
fn the_assistant_document_names_what_the_wasm_wrapper_exports() {
    for name in ["compare", "planPreview", "equityPreview"] {
        assert!(ASSISTANT.contains(&format!("`{name}`")), "{name}");
    }
    assert!(WASM.contains("pub fn compare("));
    assert!(WASM.contains("js_name = planPreview"));
    assert!(WASM.contains("js_name = equityPreview"));
    assert!(!ASSISTANT.contains("does not export"));
    assert!(!ASSISTANT.contains("still to come"));
}

#[test]
fn statements_the_review_withdrew_stay_withdrawn() {
    // The cap on price multiples does not remove the concentration of value.
    assert!(!METHODOLOGY.contains("carried 74%"));
    assert!(METHODOLOGY.contains("about two thirds of"));
    // The tender price is seen in summary only.
    assert!(!METHODOLOGY.contains("the sourced median discount"));
    // The part of a small-business gain not excluded at three and four years is taxed
    // too lightly, and the text says in which direction.
    assert!(METHODOLOGY.contains("understates federal tax"));
    // The minimum tax credit is not promised back.
    assert!(!PRIMER.contains("Most of what it charges"));
    // The two uses of the credit in the hand example are alternatives.
    assert!(METHODOLOGY.contains("of the original $293,806"));
}
