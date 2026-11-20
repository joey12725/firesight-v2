//! The browser-facing API of the engine (architecture section 6).
//!
//! Everything crosses the boundary as JSON strings or byte arrays: the surface is
//! small and the Web Worker protocol stays trivial. The logic lives in [`api`] as
//! plain functions returning `Result<_, String>` so it is unit-tested natively; the
//! `#[wasm_bindgen]` items below only convert errors into thrown JS `Error`s whose
//! `message` is the error text (a JSON document where noted).
//!
//! A profile without a `start` month starts in the current month of the JS clock.
//!
//! Everything in `docs/architecture.md` section 6 is wired, plus a stateful
//! [`Merger`] (so a run of many chunks is not re-encoded on every merge),
//! `compare`, `digest` and `planPreview`.

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

/// Collects the chunks of one run. Holding the accumulator on the wasm side avoids
/// re-encoding the running total on every merge (`mergeChunks` copies the whole
/// accumulator, 10 MB at 100,000 paths, each time).
#[wasm_bindgen]
#[derive(Default)]
pub struct Merger {
    inner: api::Merger,
}

#[wasm_bindgen]
impl Merger {
    /// An empty merger.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Merger {
        Merger::default()
    }

    /// Merges one serialised chunk (the bytes `Engine.runChunk` returned). Chunks
    /// may arrive in any order; adding a path twice throws.
    pub fn add(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.inner.add(bytes).map_err(throw)
    }

    /// Number of paths merged so far.
    pub fn paths(&self) -> f64 {
        self.inner.paths() as f64
    }

    /// The `Summary` JSON of what has been merged so far.
    pub fn summarize(&mut self, profile_json: &str) -> Result<String, JsError> {
        self.inner.summarize(profile_json, today()).map_err(throw)
    }

    /// The `SummaryDigest` JSON (about 2 KB) of what has been merged so far.
    pub fn digest(&mut self, profile_json: &str) -> Result<String, JsError> {
        self.inner.digest(profile_json, today()).map_err(throw)
    }

    /// The merged accumulator, serialised (to store, or to pass to `compare`).
    pub fn bytes(&self) -> Result<Vec<u8>, JsError> {
        self.inner.bytes().map_err(throw)
    }
}

/// The paired `Comparison` JSON of two runs made with the same seed and path
/// indices: B minus A, where `a` is the baseline. `baseline_profile_json` is the
/// baseline's profile (it identifies the run; both runs must have the same horizon).
#[wasm_bindgen]
pub fn compare(a: &[u8], b: &[u8], baseline_profile_json: &str) -> Result<String, JsError> {
    api::compare(a, b, baseline_profile_json, today()).map_err(throw)
}

/// The `SummaryDigest` JSON of a serialised accumulator: the small form of the
/// summary for an LLM tool result.
#[wasm_bindgen]
pub fn digest(acc: &[u8], profile_json: &str) -> Result<String, JsError> {
    api::digest(acc, profile_json, today()).map_err(throw)
}

/// The `PlanPreview` JSON: what the plan implies before any simulation (spending
/// now and in retirement, the funded requirement today, savings rate, horizon, life
/// expectancies). Cheap enough to call on every edit. Throws the validation report
/// when the profile cannot be compiled.
#[wasm_bindgen(js_name = planPreview)]
pub fn plan_preview(profile_json: &str) -> Result<String, JsError> {
    api::plan_preview(profile_json, today()).map_err(throw)
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
    api::tax_detail(profile_json, year, inputs_json, today()).map_err(throw)
}

/// The API as plain Rust, independent of wasm-bindgen.
pub mod api {
    use firesight_core::data::Data;
    use firesight_core::model::{Profile, YearMonth};
    use firesight_core::patch::{self, PatchOp};
    use firesight_core::sim::{self, Plan, Workspace};
    use firesight_core::stats::{self, Accumulator};
    use firesight_core::tax::{self, StateCode, TaxInputs, TaxLaw};
    use firesight_core::validate::{self, Issue, Report};
    use firesight_core::{legacy, scenario};
    use serde_json::{json, Value};

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

    fn compile(profile_json: &str, today: YearMonth) -> Result<Plan, String> {
        let profile =
            parse_profile(profile_json, today).map_err(|e| to_json(&schema_error_report(e)))?;
        Plan::compile(&profile, Data::embedded()).map_err(|report| to_json(&report))
    }

    /// A compiled profile and the worker's reusable buffers.
    pub struct Engine {
        plan: Plan,
        workspace: Workspace,
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
            let workspace = Workspace::new(&plan);
            Ok(Engine {
                plan,
                workspace,
                warnings,
            })
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
        pub fn run_chunk(&mut self, first_path: u64, count: u64) -> Result<Vec<u8>, String> {
            let mut acc = self.plan.new_accumulator();
            let seed = self.plan.seed();
            for i in first_path..first_path.saturating_add(count) {
                sim::run_path(&self.plan, seed, i, &mut self.workspace, &mut acc);
            }
            Ok(acc.to_bytes())
        }

        /// See the wasm export.
        pub fn trace(&mut self, path: u64) -> Result<String, String> {
            let seed = self.plan.seed();
            Ok(to_json(&sim::trace_path_in(
                &self.plan,
                seed,
                path,
                &mut self.workspace,
            )))
        }
    }

    /// See the wasm export.
    #[derive(Default)]
    pub struct Merger {
        acc: Option<Accumulator>,
        /// What the last `summarize` derived from its profile JSON. Every
        /// progressive summary of a run passes the same profile, so it is parsed
        /// and compiled once per run instead of once per picture.
        prepared: Option<Prepared>,
    }

    /// A profile parsed for summaries: the summary's context and, when it
    /// compiles, the plan that re-simulates the picked paths.
    struct Prepared {
        json: String,
        today: YearMonth,
        context: stats::SummaryContext,
        plan: Option<Plan>,
    }

    impl Prepared {
        fn new(profile_json: &str, today: YearMonth) -> Result<Prepared, String> {
            let (context, profile) = context_of(profile_json, today)?;
            // A profile that does not compile, or is not the one the accumulator
            // was run with, simply yields no picked paths.
            let plan = Plan::compile(&profile, Data::embedded()).ok();
            Ok(Prepared {
                json: profile_json.to_string(),
                today,
                context,
                plan,
            })
        }
    }

    impl Merger {
        /// See the wasm export. The first chunk is decoded; later ones are added
        /// to it straight from their bytes.
        pub fn add(&mut self, bytes: &[u8]) -> Result<(), String> {
            match &mut self.acc {
                Some(acc) => acc.merge_bytes(bytes).map_err(|e| e.to_string()),
                None => {
                    self.acc = Some(Accumulator::from_bytes(bytes).map_err(|e| e.to_string())?);
                    Ok(())
                }
            }
        }

        /// See the wasm export.
        pub fn paths(&self) -> u64 {
            self.acc.as_ref().map_or(0, |a| a.paths)
        }

        fn acc(&self) -> Result<&Accumulator, String> {
            self.acc
                .as_ref()
                .ok_or_else(|| "no chunk has been added yet".to_string())
        }

        /// The accumulator and the prepared profile, parsing it only when it is not
        /// the one the last call saw.
        fn parts(
            &mut self,
            profile_json: &str,
            today: YearMonth,
        ) -> Result<(&Accumulator, &Prepared), String> {
            self.acc()?;
            let fresh = self
                .prepared
                .as_ref()
                .is_some_and(|p| p.today == today && p.json == profile_json);
            if !fresh {
                self.prepared = Some(Prepared::new(profile_json, today)?);
            }
            match (&self.acc, &self.prepared) {
                (Some(acc), Some(prepared)) => Ok((acc, prepared)),
                _ => unreachable!("both are set above"),
            }
        }

        /// See the wasm export.
        pub fn summarize(&mut self, profile_json: &str, today: YearMonth) -> Result<String, String> {
            let (acc, prepared) = self.parts(profile_json, today)?;
            let mut summary = stats::Summary::from_accumulator(acc, &prepared.context);
            if let Some(plan) = &prepared.plan {
                plan.fill_pick_paths(&mut summary, acc, plan.seed());
            }
            Ok(to_json(&summary))
        }

        /// See the wasm export.
        pub fn digest(&mut self, profile_json: &str, today: YearMonth) -> Result<String, String> {
            let (acc, prepared) = self.parts(profile_json, today)?;
            Ok(to_json(
                &stats::Summary::from_accumulator(acc, &prepared.context).digest(),
            ))
        }

        /// See the wasm export.
        pub fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(self.acc()?.to_bytes())
        }
    }

    /// The summary context of a profile, and the profile with its start month
    /// filled in.
    fn context_of(
        profile_json: &str,
        today: YearMonth,
    ) -> Result<(stats::SummaryContext, Profile), String> {
        let mut profile = parse_profile(profile_json, today)?;
        let start = profile.start.unwrap_or(today);
        let people = &profile.household.people;
        let first = people
            .first()
            .ok_or_else(|| "the profile has no people".to_string())?;
        let context = stats::SummaryContext {
            start,
            first_birth: first.birth,
            second_birth: people.get(1).map(|p| p.birth),
        };
        profile.start = Some(start);
        Ok((context, profile))
    }

    fn summary_of(
        acc: &Accumulator,
        profile_json: &str,
        today: YearMonth,
        with_pick_paths: bool,
    ) -> Result<stats::Summary, String> {
        let (context, profile) = context_of(profile_json, today)?;
        let mut summary = stats::Summary::from_accumulator(acc, &context);
        if with_pick_paths {
            // The annual series of the picked lives: those few paths simulated
            // again (computed into this summary, not a second one). A profile that
            // does not compile, or is not the one the accumulator was run with,
            // simply yields none.
            if let Ok(plan) = Plan::compile(&profile, Data::embedded()) {
                plan.fill_pick_paths(&mut summary, acc, plan.seed());
            }
        }
        Ok(summary)
    }

    /// See the wasm export.
    pub fn compare(
        a: &[u8],
        b: &[u8],
        baseline_profile_json: &str,
        today: YearMonth,
    ) -> Result<String, String> {
        // The profile is parsed so that a bad one is reported here, not later.
        parse_profile(baseline_profile_json, today)?;
        let a = Accumulator::from_bytes(a).map_err(|e| e.to_string())?;
        let b = Accumulator::from_bytes(b).map_err(|e| e.to_string())?;
        Ok(to_json(&stats::compare(&a, &b)))
    }

    /// See the wasm export.
    pub fn digest(acc: &[u8], profile_json: &str, today: YearMonth) -> Result<String, String> {
        let acc = Accumulator::from_bytes(acc).map_err(|e| e.to_string())?;
        Ok(to_json(
            &summary_of(&acc, profile_json, today, false)?.digest(),
        ))
    }

    /// See the wasm export.
    pub fn plan_preview(profile_json: &str, today: YearMonth) -> Result<String, String> {
        Ok(to_json(&compile(profile_json, today)?.preview()))
    }

    /// See the wasm export.
    pub fn merge_chunks(a: &[u8], b: &[u8]) -> Result<Vec<u8>, String> {
        let mut acc = Accumulator::from_bytes(a).map_err(|e| e.to_string())?;
        acc.merge_bytes(b).map_err(|e| e.to_string())?;
        Ok(acc.to_bytes())
    }

    /// See the wasm export.
    pub fn summarize(acc: &[u8], profile_json: &str, today: YearMonth) -> Result<String, String> {
        let acc = Accumulator::from_bytes(acc).map_err(|e| e.to_string())?;
        Ok(to_json(&summary_of(&acc, profile_json, today, true)?))
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

    /// See the wasm export. The law is indexed to `year` at 2.5% inflation a year
    /// from its base year (the published figures for the year after the base year),
    /// for the profile's state of residence and tax model.
    pub fn tax_detail(
        profile_json: &str,
        year: i32,
        inputs_json: &str,
        today: YearMonth,
    ) -> Result<String, String> {
        let profile = parse_profile(profile_json, today)?;
        let mut inputs: TaxInputs = serde_json::from_str(inputs_json)
            .map_err(|e| format!("inputs must be a TaxInputs object: {e}"))?;
        let law = TaxLaw::from_data(Data::embedded(), &profile.assumptions.taxes)
            .map_err(|e| e.to_string())?;
        let location = &profile.household.location;
        let state = if location.is_us() {
            location
                .state
                .as_deref()
                .and_then(StateCode::parse)
                .unwrap_or(StateCode::NONE)
        } else {
            StateCode::NONE
        };
        let index = if year <= law.base_year {
            tax::Indexation::base(year)
        } else {
            let published = law.published_next_year;
            let k = firesight_core::math::powi(1.025, year - law.base_year - 1);
            law.index_from_cpi(year, published.cpi_factor * k)
                .with_fpl_factor(published.fpl_factor * k)
        };
        let mut params = law.for_year(index, state);
        if !location.is_us() {
            params = params.with_foreign_rate(Some(location.foreign_tax_rate.unwrap_or(0.0)));
        }
        inputs.state = params.state.code;
        Ok(to_json(&tax::explain(&params, &inputs)))
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
        let a = engine.run_chunk(0, 40).unwrap();
        let b = engine.run_chunk(40, 40).unwrap();
        let whole = engine.run_chunk(0, 80).unwrap();
        assert_eq!(api::merge_chunks(&b, &a).unwrap(), whole);
        let mut merger = api::Merger::default();
        assert!(merger.bytes().is_err());
        merger.add(&b).unwrap();
        merger.add(&a).unwrap();
        assert_eq!(merger.paths(), 80);
        assert_eq!(merger.bytes().unwrap(), whole);
        assert!(merger.add(&a).is_err(), "a path cannot be added twice");
        let summary = parse(&merger.summarize(&example, TODAY).unwrap());
        assert_eq!(summary["paths"], 80);
        assert_eq!(
            summary,
            parse(&api::summarize(&whole, &example, TODAY).unwrap())
        );
        let digest = parse(&merger.digest(&example, TODAY).unwrap());
        assert_eq!(digest["paths"], 80);
        assert_eq!(
            digest,
            parse(&api::digest(&whole, &example, TODAY).unwrap())
        );
        let trace = parse(&engine.trace(3).unwrap());
        assert_eq!(trace["path_index"], 3);
        assert!(trace["months"].as_u64().unwrap() > 12);

        // A scenario on the same seed and paths, compared pair by pair.
        let patch =
            r#"[{"op": "set", "path": "/retirement/trigger/withdrawal_rate", "value": 0.03}]"#;
        let changed =
            parse(&api::apply_patch(&example, patch, TODAY).unwrap())["profile"].to_string();
        let other = api::Engine::new(&changed, TODAY)
            .unwrap()
            .run_chunk(0, 80)
            .unwrap();
        let cmp = parse(&api::compare(&whole, &other, &example, TODAY).unwrap());
        assert_eq!(cmp["paths"], 80);
        assert_eq!(cmp["outcomes"][0]["outcome"], "success");

        let preview = parse(&api::plan_preview(&example, TODAY).unwrap());
        assert_eq!(preview["horizon_years"], 65);
        assert!(preview["funded_requirement"].as_f64().unwrap() > 500_000.0);
        assert_eq!(preview["life_expectancy"].as_array().unwrap().len(), 2);
        assert!(api::plan_preview("{}", TODAY).is_err());

        let inputs = r#"{"filing_status": "married_joint", "adults": 2, "ages": [40, 38], "wages": [150000, 90000]}"#;
        let detail = parse(&api::tax_detail(&example, 2026, inputs, TODAY).unwrap());
        assert!(detail["result"]["total"].as_f64().unwrap() > 30_000.0);
        assert!(detail["lines"].as_array().unwrap().len() > 8);
        assert!(api::tax_detail(&example, 2026, "\"nope\"", TODAY).is_err());
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
