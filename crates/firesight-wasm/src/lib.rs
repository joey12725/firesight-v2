//! The browser-facing API of the engine (architecture section 6).
//!
//! Everything crosses the boundary as JSON strings or byte arrays: the surface is
//! small and the Web Worker protocol stays trivial. The logic lives in [`api`] as
//! plain functions returning `Result<_, String>` so it is unit-tested natively; the
//! `#[wasm_bindgen]` items below only convert errors into thrown JS `Error`s whose
//! `message` is the error text (a JSON document where noted).
//!
//! Status: `validate`, `applyPatch`, `migrateV1`, `levers`, `schema`,
//! `defaultProfile`, `exampleProfiles`, `mergeChunks` and the `Engine` constructor
//! work. `Engine.runChunk`, `Engine.trace`, `summarize` and `taxDetail` throw
//! "not implemented" until the corresponding core modules land; they are marked
//! `TODO(core)` below and each is a one-line change.

use firesight_core::model::YearMonth;
use wasm_bindgen::prelude::*;

/// The current calendar month from the JS clock. Core has no clock of its own.
fn today() -> YearMonth {
    let now = js_sys::Date::new_0();
    YearMonth::new(now.get_full_year() as i32, now.get_month() + 1).unwrap_or_default()
}

fn throw(message: String) -> JsError {
    JsError::new(&message)
}

/// Installs the panic hook that forwards Rust panics to `console.error`. Called
/// automatically when the module is instantiated.
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "panic-hook")]
    console_error_panic_hook::set_once();
}

/// A compiled profile, ready to simulate. One per worker.
#[wasm_bindgen]
pub struct Engine {
    inner: api::Engine,
}

#[wasm_bindgen]
impl Engine {
    /// Validates and compiles a profile. Throws an `Error` whose message is the JSON
    /// report `{ "errors": [...], "warnings": [...] }` when the profile cannot run.
    #[wasm_bindgen(constructor)]
    pub fn new(profile_json: &str) -> Result<Engine, JsError> {
        api::Engine::new(profile_json, today())
            .map(|inner| Engine { inner })
            .map_err(throw)
    }

    /// Number of calendar years in the plan: the length of every per-year array.
    #[wasm_bindgen(js_name = horizonYears)]
    pub fn horizon_years(&self) -> u32 {
        self.inner.horizon_years()
    }

    /// The warnings found when the profile was compiled, as a JSON array.
    pub fn warnings(&self) -> String {
        self.inner.warnings_json()
    }

    /// Simulates paths `first_path .. first_path + count` and returns the serialised
    /// accumulator for the chunk.
    #[wasm_bindgen(js_name = runChunk)]
    pub fn run_chunk(&mut self, first_path: u32, count: u32) -> Result<Vec<u8>, JsError> {
        self.inner
            .run_chunk(u64::from(first_path), u64::from(count))
            .map_err(throw)
    }

    /// Re-simulates one path with full detail and returns the `PathTrace` JSON.
    pub fn trace(&mut self, path: u32) -> Result<String, JsError> {
        self.inner.trace(u64::from(path)).map_err(throw)
    }
}

/// Merges two serialised accumulators. The result does not depend on the order of
/// the arguments.
#[wasm_bindgen(js_name = mergeChunks)]
pub fn merge_chunks(a: &[u8], b: &[u8]) -> Result<Vec<u8>, JsError> {
    api::merge_chunks(a, b).map_err(throw)
}

/// Turns a serialised accumulator into the `Summary` JSON. `profile_json` supplies
/// the start date and ages for the axes.
#[wasm_bindgen]
pub fn summarize(acc: &[u8], profile_json: &str) -> Result<String, JsError> {
    api::summarize(acc, profile_json, today()).map_err(throw)
}

/// Validates a profile and returns `{ "errors": [...], "warnings": [...] }`. Never
/// throws: JSON that does not parse as a profile is reported as a `schema` error.
#[wasm_bindgen]
pub fn validate(profile_json: &str) -> String {
    api::validate(profile_json, today())
}

/// Applies a patch (a JSON array of `{op, path, value}`) and returns
/// `{ "profile": {...}, "errors": [...], "warnings": [...] }`. Throws with a message
/// that names the failing operation and lists the valid ids or fields.
#[wasm_bindgen(js_name = applyPatch)]
pub fn apply_patch(profile_json: &str, patch_json: &str) -> Result<String, JsError> {
    api::apply_patch(profile_json, patch_json, today()).map_err(throw)
}

/// Converts a v1 profile and returns `{ "profile": {...}, "notes": [...] }`.
#[wasm_bindgen(js_name = migrateV1)]
pub fn migrate_v1(v1_json: &str) -> Result<String, JsError> {
    api::migrate_v1(v1_json, today()).map_err(throw)
}

/// The sensitivity levers for a profile as a JSON array of
/// `{ id, label, low_label, high_label, low, high }` where `low` and `high` are patches.
#[wasm_bindgen]
pub fn levers(profile_json: &str) -> Result<String, JsError> {
    api::levers(profile_json).map_err(throw)
}

/// The JSON Schema of the profile format.
#[wasm_bindgen]
pub fn schema() -> String {
    api::schema()
}

/// A minimal valid profile for a new user, starting this month.
#[wasm_bindgen(js_name = defaultProfile)]
pub fn default_profile() -> String {
    api::default_profile(today())
}

/// The demo households as a JSON array of `{ slug, title, summary, profile }`.
#[wasm_bindgen(js_name = exampleProfiles)]
pub fn example_profiles() -> String {
    api::example_profiles(today())
}

/// Explains one tax computation: `inputs_json` is a `TaxInputs` document in nominal
/// dollars of `year`; the result is a `TaxDetail` JSON.
#[wasm_bindgen(js_name = taxDetail)]
pub fn tax_detail(profile_json: &str, year: i32, inputs_json: &str) -> Result<String, JsError> {
    api::tax_detail(profile_json, year, inputs_json).map_err(throw)
}

/// The API as plain Rust, independent of wasm-bindgen.
pub mod api {
    use firesight_core::data::Data;
    use firesight_core::model::{Profile, YearMonth};
    use firesight_core::patch::{self, PatchOp};
    use firesight_core::sim::Plan;
    use firesight_core::stats::Accumulator;
    use firesight_core::validate::{self, Issue, Report};
    use firesight_core::{legacy, scenario};
    use serde_json::{json, Value};

    const NOT_IMPLEMENTED: &str =
        "not implemented: the simulation core is still a stub in this build";

    fn to_json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("serialisation cannot fail")
    }

    fn parse_profile(profile_json: &str, today: YearMonth) -> Result<Profile, String> {
        Profile::from_json(profile_json).map(|p| p.with_start(today))
    }

    fn schema_error_report(message: String) -> Report {
        Report {
            errors: vec![Issue {
                code: "schema".into(),
                path: String::new(),
                message,
            }],
            warnings: Vec::new(),
        }
    }

    /// A compiled profile.
    pub struct Engine {
        plan: Plan,
        warnings: Vec<Issue>,
    }

    impl Engine {
        /// Parses, validates and compiles. The error is the JSON report.
        pub fn new(profile_json: &str, today: YearMonth) -> Result<Engine, String> {
            let profile =
                parse_profile(profile_json, today).map_err(|e| to_json(&schema_error_report(e)))?;
            let warnings = validate::validate(&profile).warnings;
            let plan =
                Plan::compile(&profile, Data::embedded()).map_err(|report| to_json(&report))?;
            Ok(Engine { plan, warnings })
        }

        /// See the wasm export.
        pub fn horizon_years(&self) -> u32 {
            self.plan.horizon_years()
        }

        /// See the wasm export.
        pub fn warnings_json(&self) -> String {
            to_json(&self.warnings)
        }

        /// See the wasm export.
        pub fn run_chunk(&mut self, _first_path: u64, _count: u64) -> Result<Vec<u8>, String> {
            // TODO(core): let mut acc = self.plan.new_accumulator();
            // sim::run_range(&self.plan, self.plan.seed(), first..first + count, &mut acc);
            // Ok(acc.to_bytes())
            Err(NOT_IMPLEMENTED.into())
        }

        /// See the wasm export.
        pub fn trace(&mut self, _path: u64) -> Result<String, String> {
            // TODO(core): to_json(&sim::trace_path(&self.plan, self.plan.seed(), path))
            Err(NOT_IMPLEMENTED.into())
        }
    }

    /// See the wasm export.
    pub fn merge_chunks(a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
        let mut acc = Accumulator::from_bytes(a).map_err(|e| e.to_string())?;
        let other = Accumulator::from_bytes(b).map_err(|e| e.to_string())?;
        acc.merge(&other).map_err(|e| e.to_string())?;
        Ok(acc.to_bytes())
    }

    /// See the wasm export.
    pub fn summarize(acc: &[u8], profile_json: &str, today: YearMonth) -> Result<String, String> {
        let _acc = Accumulator::from_bytes(acc).map_err(|e| e.to_string())?;
        let _profile = parse_profile(profile_json, today)?;
        // TODO(core): to_json(&Summary::from_accumulator(&acc, &SummaryContext { .. }))
        Err(NOT_IMPLEMENTED.into())
    }

    /// See the wasm export.
    pub fn validate(profile_json: &str, today: YearMonth) -> String {
        let report = match parse_profile(profile_json, today) {
            Ok(profile) => validate::validate(&profile),
            Err(message) => schema_error_report(message),
        };
        to_json(&report)
    }

    /// See the wasm export.
    pub fn apply_patch(
        profile_json: &str,
        patch_json: &str,
        today: YearMonth,
    ) -> Result<String, String> {
        let base: Value = serde_json::from_str(profile_json)
            .map_err(|e| format!("profile is not valid JSON: {e}"))?;
        let ops: Vec<PatchOp> = serde_json::from_str(patch_json).map_err(|e| {
            format!("patch must be a JSON array of {{\"op\": \"set\"|\"remove\"|\"append\", \"path\": \"/...\", \"value\": ...}}: {e}")
        })?;
        let profile = patch::apply_typed(&base, &ops).map_err(|e| e.to_string())?;
        // Validate against the caller's clock without writing a start into the result.
        let report = validate::validate(&profile.clone().with_start(today));
        Ok(to_json(
            &json!({ "profile": profile, "errors": report.errors, "warnings": report.warnings }),
        ))
    }

    /// See the wasm export.
    pub fn migrate_v1(v1_json: &str, today: YearMonth) -> Result<String, String> {
        let v1: Value =
            serde_json::from_str(v1_json).map_err(|e| format!("not valid JSON: {e}"))?;
        let (profile, notes) = legacy::migrate_v1_at(&v1, today)?;
        Ok(to_json(&json!({ "profile": profile, "notes": notes })))
    }

    /// See the wasm export.
    pub fn levers(profile_json: &str) -> Result<String, String> {
        let profile = Profile::from_json(profile_json)?;
        Ok(to_json(&scenario::levers(&profile)))
    }

    /// See the wasm export.
    pub fn schema() -> String {
        to_json(&Profile::json_schema())
    }

    /// See the wasm export.
    pub fn default_profile(today: YearMonth) -> String {
        to_json(&Profile::starter(today))
    }

    /// See the wasm export.
    pub fn example_profiles(today: YearMonth) -> String {
        let list: Vec<Value> = Profile::examples(today)
            .into_iter()
            .map(|ex| json!({ "slug": ex.slug, "title": ex.title, "summary": ex.summary, "profile": ex.profile }))
            .collect();
        to_json(&list)
    }

    /// See the wasm export.
    pub fn tax_detail(
        _profile_json: &str,
        _year: i32,
        _inputs_json: &str,
    ) -> Result<String, String> {
        // TODO(core): TaxLaw::from_data + for_year + tax::explain. TaxInputs needs a
        // Deserialize derive when this is wired.
        Err(NOT_IMPLEMENTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::api;
    use firesight_core::model::{Profile, YearMonth};
    use firesight_core::stats::Accumulator;
    use serde_json::Value;

    const TODAY: YearMonth = YearMonth::ym(2026, 11);

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn default_and_example_profiles_validate() {
        let report = parse(&api::validate(&api::default_profile(TODAY), TODAY));
        assert_eq!(report["errors"].as_array().unwrap().len(), 0, "{report}");
        let examples = parse(&api::example_profiles(TODAY));
        assert_eq!(examples.as_array().unwrap().len(), 3);
        for ex in examples.as_array().unwrap() {
            let report = parse(&api::validate(&ex["profile"].to_string(), TODAY));
            assert_eq!(report["errors"].as_array().unwrap().len(), 0, "{report}");
        }
    }

    #[test]
    fn validate_reports_schema_errors_without_throwing() {
        let report = parse(&api::validate("{\"household\": {\"peeple\": []}}", TODAY));
        assert_eq!(report["errors"][0]["code"], "schema");
        assert!(report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("peeple"));
        let report = parse(&api::validate("{}", TODAY));
        assert_eq!(report["errors"][0]["code"], "people_count");
    }

    #[test]
    fn apply_patch_returns_profile_and_report() {
        let base = Profile::example().to_json_pretty();
        let patch = r#"[{"op": "set", "path": "/portfolio/target", "value": {"us_stocks": 60, "crypto": 40}}]"#;
        let out = parse(&api::apply_patch(&base, patch, TODAY).unwrap());
        assert_eq!(out["profile"]["portfolio"]["target"]["crypto"], 40.0);
        assert_eq!(out["warnings"][0]["code"], "crypto_heavy");
        let err = api::apply_patch(
            &base,
            r#"[{"op": "remove", "path": "/events/@nope"}]"#,
            TODAY,
        )
        .unwrap_err();
        assert!(err.contains("buy_home"), "{err}");
        assert!(api::apply_patch(&base, "{}", TODAY)
            .unwrap_err()
            .contains("JSON array"));
    }

    #[test]
    fn migrate_schema_levers_and_engine() {
        let v1 = include_str!("../../../examples/v1/dual-income-family.json");
        let out = parse(&api::migrate_v1(v1, TODAY).unwrap());
        assert_eq!(out["profile"]["schema_version"], 2);
        assert!(out["notes"].as_array().unwrap().len() > 5);

        let schema = parse(&api::schema());
        assert_eq!(schema["title"], "Profile");

        let example = Profile::example().to_json_pretty();
        assert!(
            parse(&api::levers(&example).unwrap())
                .as_array()
                .unwrap()
                .len()
                >= 6
        );

        let mut engine = api::Engine::new(&example, TODAY).unwrap();
        assert_eq!(engine.horizon_years(), 65);
        assert_eq!(engine.warnings_json(), "[]");
        assert!(engine.run_chunk(0, 10).is_err());
        let err = api::Engine::new("{}", TODAY).err().unwrap();
        assert_eq!(parse(&err)["errors"][0]["code"], "people_count");
    }

    #[test]
    fn merge_chunks_round_trips() {
        let a = Accumulator::new(10).to_bytes();
        let merged = api::merge_chunks(&a, &a).unwrap();
        assert_eq!(
            Accumulator::from_bytes(&merged).unwrap(),
            Accumulator::new(10)
        );
        assert!(api::merge_chunks(&a, &[1, 2, 3]).is_err());
    }
}
