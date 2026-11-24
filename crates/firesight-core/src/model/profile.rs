//! The root of the schema.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    Account, Assumptions, Debt, DebtPolicy, Household, Income, LifeEvent, PortfolioPolicy,
    Property, RetirementPlan, SimulationSettings, Spending, WhenContext, YearMonth,
};
use crate::MAX_PEOPLE;

/// The schema version written by this build.
pub const SCHEMA_VERSION: u32 = 2;

/// Everything FIRESight knows about a household: who they are, what they earn, spend,
/// own and owe, what they plan, and the assumptions used to simulate their future.
/// All money is in today's dollars and all rates are annual decimals (0.04 = 4%).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    /// Version of the profile format. Always 2.
    pub schema_version: u32,
    /// A name for this plan, for example "Baseline" or "Move to Lisbon".
    pub name: String,
    /// Free-text notes about the plan. Not used by the simulation.
    pub description: String,
    /// The month the simulation starts and the date at which all "today's dollars"
    /// amounts are valued, as "YYYY-MM". Leave empty to use the current month.
    pub start: Option<YearMonth>,
    /// The people in the plan and where they live.
    pub household: Household,
    /// Jobs, self-employment, pensions and other income. Social Security is set on
    /// each person and rental income on each property.
    pub incomes: Vec<Income>,
    /// The budget and how spending behaves over time.
    pub spending: Spending,
    /// Bank, brokerage and retirement accounts.
    pub accounts: Vec<Account>,
    /// How savings are invested.
    pub portfolio: PortfolioPolicy,
    /// Homes and rental properties owned today.
    pub properties: Vec<Property>,
    /// Loans and credit balances other than mortgages.
    pub debts: Vec<Debt>,
    /// How debts are paid down beyond the required payments.
    pub debt_policy: DebtPolicy,
    /// Planned and possible future events: purchases, moves, windfalls, career changes.
    pub events: Vec<LifeEvent>,
    /// When to retire and how to draw down savings.
    pub retirement: RetirementPlan,
    /// Assumptions about markets, taxes and lifespans.
    pub assumptions: Assumptions,
    /// Number of simulated paths, random seed and horizon.
    pub simulation: SimulationSettings,
}

impl Default for Profile {
    /// An empty plan with default policies and assumptions. It has no people, so it
    /// does not validate until at least one person is added.
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: String::new(),
            description: String::new(),
            start: None,
            household: Household::default(),
            incomes: Vec::new(),
            spending: Spending::default(),
            accounts: Vec::new(),
            portfolio: PortfolioPolicy::default(),
            properties: Vec::new(),
            debts: Vec::new(),
            debt_policy: DebtPolicy::default(),
            events: Vec::new(),
            retirement: RetirementPlan::default(),
            assumptions: Assumptions::default(),
            simulation: SimulationSettings::default(),
        }
    }
}

impl Profile {
    /// Parses a profile from JSON, reporting the path of the offending field on error
    /// (for example `incomes[0].kind: unknown field ...`).
    pub fn from_json(json: &str) -> Result<Profile, String> {
        let de = &mut serde_json::Deserializer::from_str(json);
        serde_path_to_error::deserialize(de).map_err(|e| format_path_error(&e))
    }

    /// Parses a profile from an already-parsed JSON value, with the same error format
    /// as [`Profile::from_json`].
    pub fn from_value(value: &serde_json::Value) -> Result<Profile, String> {
        serde_path_to_error::deserialize(value).map_err(|e| format_path_error(&e))
    }

    /// Serialises to pretty-printed JSON with every field present.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Profile serialisation cannot fail")
    }

    /// The JSON Schema of the profile format, derived from the Rust types.
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Profile))
            .expect("schema serialisation cannot fail")
    }

    /// A minimal valid profile for a new user: one 30-year-old, default policies and
    /// assumptions, nothing else. The UI's "start from scratch" document.
    pub fn starter(today: YearMonth) -> Profile {
        Profile {
            name: "My plan".into(),
            start: Some(today),
            household: Household {
                people: vec![super::Person {
                    id: "you".into(),
                    name: "You".into(),
                    birth: today - 30 * 12,
                    ..super::Person::default()
                }],
                ..Household::default()
            },
            ..Profile::default()
        }
    }

    /// A copy with `start` filled in. Core has no clock, so the caller (wasm, CLI)
    /// supplies the current month when the profile leaves it empty. The engine only
    /// accepts profiles with a resolved start.
    pub fn with_start(mut self, today: YearMonth) -> Profile {
        self.start.get_or_insert(today);
        self
    }

    /// Context for resolving [`super::When`] values before retirement is known.
    /// Returns `None` if `start` is unset.
    pub fn when_context(&self) -> Option<WhenContext> {
        let start = self.start?;
        let mut births = [None; MAX_PEOPLE];
        for (slot, person) in births.iter_mut().zip(&self.household.people) {
            *slot = Some(person.birth);
        }
        Some(WhenContext {
            start,
            births,
            retirement: None,
        })
    }

    /// Total budget per year in today's dollars, counting every item regardless of its
    /// start and end dates. A rough figure for lints and summaries, not the simulation.
    pub fn annual_budget(&self) -> f64 {
        self.spending.items.iter().map(|i| i.annual_amount()).sum()
    }

    /// Total budget per year once retired, in today's dollars (same caveats).
    pub fn annual_budget_in_retirement(&self) -> f64 {
        self.spending
            .items
            .iter()
            .map(|i| i.annual_amount_in_retirement())
            .sum()
    }
}

fn format_path_error<E: std::fmt::Display>(e: &serde_path_to_error::Error<E>) -> String {
    let path = e.path().to_string();
    if path == "." || path.is_empty() {
        e.inner().to_string()
    } else {
        format!("{path}: {}", e.inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn empty_object_is_the_default_profile() {
        let p = Profile::from_json("{}").unwrap();
        assert_eq!(p, Profile::default());
        assert_eq!(p.schema_version, 2);
        assert_eq!(p.simulation.paths, 25_000);
        assert_eq!(
            Profile::from_json(&Profile::default().to_json_pretty()).unwrap(),
            p
        );
    }

    #[test]
    fn unknown_fields_and_bad_values_are_rejected_with_a_path() {
        let e = Profile::from_json(
            r#"{"household": {"people": [{"id": "a", "birth": "1990-01", "sexx": "male"}]}}"#,
        )
        .unwrap_err();
        assert!(e.starts_with("household.people[0]"), "{e}");
        assert!(e.contains("sexx"), "{e}");
        let e = Profile::from_json(r#"{"household": {"people": [{"id": "a"}]}}"#).unwrap_err();
        assert!(e.contains("missing field `birth`"), "{e}");
        let e = Profile::from_json(
            r#"{"incomes": [{"id": "j", "kind": {"type": "job", "salry": 1}}]}"#,
        )
        .unwrap_err();
        assert!(e.contains("salry") && e.contains("salary"), "{e}");
    }

    #[test]
    fn tagged_enums_use_type_and_plain_enums_are_strings() {
        let v = serde_json::to_value(Profile::example()).unwrap();
        assert_eq!(v["incomes"][0]["kind"]["type"], "job");
        assert_eq!(v["incomes"][0]["kind"]["growth"]["type"], "lifecycle");
        assert_eq!(v["retirement"]["trigger"]["type"], "when_funded");
        assert_eq!(v["retirement"]["early_access"], "auto");
        assert_eq!(v["household"]["people"][0]["sex"], "female");
        assert_eq!(v["events"][0]["kind"]["use"]["type"], "primary");
        assert_eq!(v["start"], "2026-11");
    }

    /// The schema is the contract with the UI and the AI tools: every object is
    /// closed, every property is documented, every tagged variant declares its tag.
    #[test]
    fn schema_is_strict_and_fully_documented() {
        fn walk(name: &str, node: &Value, problems: &mut Vec<String>) {
            let Some(obj) = node.as_object() else { return };
            if let Some(props) = obj.get("properties").and_then(Value::as_object) {
                if obj.get("additionalProperties") != Some(&Value::Bool(false)) {
                    problems.push(format!("{name}: object is not closed"));
                }
                for (key, prop) in props {
                    if key != "type" && prop.get("description").is_none() {
                        problems.push(format!("{name}.{key}: no description"));
                    }
                }
            }
            if let Some(variants) = obj.get("oneOf").and_then(Value::as_array) {
                for (i, variant) in variants.iter().enumerate() {
                    if variant.get("description").is_none() {
                        problems.push(format!("{name}[{i}]: variant has no description"));
                    }
                    if variant["type"] == "object"
                        && variant["properties"]["type"]["const"].is_null()
                    {
                        problems.push(format!("{name}[{i}]: variant has no \"type\" tag"));
                    }
                    walk(&format!("{name}[{i}]"), variant, problems);
                }
            }
        }
        let schema = Profile::json_schema();
        let mut problems = Vec::new();
        walk("Profile", &schema, &mut problems);
        let defs = schema["$defs"].as_object().unwrap();
        for (name, def) in defs {
            walk(name, def, &mut problems);
        }
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(defs.len() > 60);
        // Field order is preserved for form layout.
        let first: Vec<&String> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .take(4)
            .collect();
        assert_eq!(first, ["schema_version", "name", "description", "start"]);
    }

    #[test]
    fn starter_profile_is_valid() {
        let p = Profile::starter(YearMonth::ym(2027, 5));
        assert!(crate::validate::validate(&p).is_ok());
        assert_eq!(
            p.when_context().unwrap().births[0],
            Some(YearMonth::ym(1997, 5))
        );
    }
}
