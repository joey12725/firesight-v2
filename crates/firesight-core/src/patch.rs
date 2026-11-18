//! Profile patches: small edits addressed by path.
//!
//! A patch is a list of operations `{ "op": "set" | "remove" | "append", "path", "value" }`.
//! `path` is a JSON pointer (RFC 6901) with one extension: a segment addressing an
//! array element may be `@<id>`, which selects the element whose `"id"` equals `<id>`.
//! Ids survive reordering, so stored scenarios and AI-written edits stay valid when
//! the list changes.
//!
//! ```text
//! { "op": "set",    "path": "/incomes/@alex_job/kind/salary", "value": 140000 }
//! { "op": "append", "path": "/spending/items", "value": { "id": "boat", "amount": 300 } }
//! { "op": "remove", "path": "/events/@kitchen" }
//! ```
//!
//! Error messages are written for a language model that will read them and retry:
//! they name the operation, the full path, the exact segment that failed, and the
//! ids or keys that do exist at that level.
//!
//! Operations apply in order to a copy; if any fails, nothing is changed.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Profile;

/// One edit to a profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatchOp {
    /// Replace the value at `path`, or add it if the parent is an object and the key
    /// is absent. On an array, the segment `-` appends.
    Set {
        /// JSON pointer to the value, for example "/retirement/trigger" or
        /// "/accounts/@brokerage/balance".
        path: String,
        /// The new value.
        value: Value,
    },
    /// Delete an array element, or reset an object field to its default by removing it.
    Remove {
        /// JSON pointer to the element or field, for example "/debts/@car_loan".
        path: String,
    },
    /// Add an element to the end of the array at `path`.
    Append {
        /// JSON pointer to an array, for example "/events".
        path: String,
        /// The element to add. If it has an "id", the id must not already be used in
        /// that array.
        value: Value,
    },
}

impl PatchOp {
    /// Shorthand for a `set` operation.
    pub fn set(path: impl Into<String>, value: impl Into<Value>) -> Self {
        PatchOp::Set {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Shorthand for a `remove` operation.
    pub fn remove(path: impl Into<String>) -> Self {
        PatchOp::Remove { path: path.into() }
    }

    /// Shorthand for an `append` operation.
    pub fn append(path: impl Into<String>, value: impl Into<Value>) -> Self {
        PatchOp::Append {
            path: path.into(),
            value: value.into(),
        }
    }

    /// The path the operation addresses.
    pub fn path(&self) -> &str {
        match self {
            PatchOp::Set { path, .. } | PatchOp::Remove { path } | PatchOp::Append { path, .. } => {
                path
            }
        }
    }

    fn name(&self) -> &'static str {
        match self {
            PatchOp::Set { .. } => "set",
            PatchOp::Remove { .. } => "remove",
            PatchOp::Append { .. } => "append",
        }
    }
}

/// Why a patch could not be applied.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PatchError {
    /// An operation failed. `op_index` is its position in the patch.
    #[error("patch operation {op_index} ({op} {path:?}) failed: {reason}")]
    Op {
        /// Zero-based index of the failing operation.
        op_index: usize,
        /// "set", "remove" or "append".
        op: &'static str,
        /// The operation's full path.
        path: String,
        /// What went wrong and what would have been valid.
        reason: Box<PathFailure>,
    },
    /// Every operation applied, but the result is not a valid profile document
    /// (wrong type, unknown field, unknown enum variant).
    #[error("the patched profile does not match the schema: {message}")]
    Schema {
        /// Serde's message, prefixed with the path of the offending value.
        message: String,
    },
}

/// The specific failure inside a [`PatchError::Op`].
#[derive(Clone, Debug, PartialEq)]
pub enum PathFailure {
    /// The path is not a valid pointer.
    Syntax(String),
    /// An object has no such key.
    NoSuchKey {
        at: String,
        key: String,
        available: Vec<String>,
    },
    /// An array has no element with that id.
    NoSuchId {
        at: String,
        id: String,
        available: Vec<String>,
    },
    /// An array index is out of range or not a number.
    BadIndex {
        at: String,
        segment: String,
        len: usize,
        available_ids: Vec<String>,
    },
    /// The path continues through a value that has no children.
    NotAContainer {
        at: String,
        found: &'static str,
        segment: String,
    },
    /// `append` addressed something that is not an array.
    NotAnArray { at: String, found: &'static str },
    /// An appended or replaced element reuses an id.
    DuplicateId {
        at: String,
        id: String,
        available: Vec<String>,
    },
    /// The operation is not allowed on the document root.
    Root,
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}

fn show(at: &str) -> &str {
    if at.is_empty() {
        "the profile root"
    } else {
        at
    }
}

impl fmt::Display for PathFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathFailure::Syntax(msg) => write!(f, "invalid path: {msg}"),
            PathFailure::NoSuchKey { at, key, available } => write!(
                f,
                "{} has no field {key:?}. Fields there: {}",
                show(at),
                list(available)
            ),
            PathFailure::NoSuchId { at, id, available } => write!(
                f,
                "no element with id {id:?} in {}. Ids there: {}",
                show(at),
                list(available)
            ),
            PathFailure::BadIndex {
                at,
                segment,
                len,
                available_ids,
            } => write!(
                f,
                "{} is an array of {len} element(s); {segment:?} is not a valid position. \
                 Address elements as @<id> or by index 0..{len}. Ids there: {}",
                show(at),
                list(available_ids)
            ),
            PathFailure::NotAContainer { at, found, segment } if *found == "null" => write!(
                f,
                "{} is null, so {segment:?} cannot be addressed inside it. Set the whole \
                 object at {} instead",
                show(at),
                show(at)
            ),
            PathFailure::NotAContainer { at, found, segment } => write!(
                f,
                "{} is a {found}, which has no field or element {segment:?}",
                show(at)
            ),
            PathFailure::NotAnArray { at, found } => write!(
                f,
                "append needs an array, but {} is {}. Use set to change it",
                show(at),
                article(found)
            ),
            PathFailure::DuplicateId { at, id, available } => write!(
                f,
                "id {id:?} is already used in {}. Choose a different id, or use set on \
                 {}/@{id} to change the existing element. Ids there: {}",
                show(at),
                at,
                list(available)
            ),
            PathFailure::Root => {
                write!(f, "the profile root cannot be removed or appended to; give a path such as \"/events\"")
            }
        }
    }
}

fn article(kind: &str) -> String {
    match kind {
        "null" => "null".to_string(),
        "object" => "an object".to_string(),
        other => format!("a {other}"),
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn ids_of(array: &[Value]) -> Vec<String> {
    array
        .iter()
        .filter_map(|e| e.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn parse_pointer(path: &str) -> Result<Vec<String>, PathFailure> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    if !path.starts_with('/') {
        return Err(PathFailure::Syntax(format!(
            "a path must start with \"/\" (for example \"/{path}\"); segments are separated by \"/\""
        )));
    }
    path[1..]
        .split('/')
        .map(|raw| {
            if raw.is_empty() {
                return Err(PathFailure::Syntax(
                    "empty segment: remove the doubled or trailing \"/\"".to_string(),
                ));
            }
            // RFC 6901 escapes: "~1" is "/", "~0" is "~", decoded in that order.
            Ok(raw.replace("~1", "/").replace("~0", "~"))
        })
        .collect()
}

fn join(at: &str, segment: &str) -> String {
    format!("{at}/{}", segment.replace('~', "~0").replace('/', "~1"))
}

/// Position of the element a segment selects in an array.
fn locate(array: &[Value], segment: &str, at: &str) -> Result<usize, PathFailure> {
    if let Some(id) = segment.strip_prefix('@') {
        return array
            .iter()
            .position(|e| e.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| PathFailure::NoSuchId {
                at: at.to_string(),
                id: id.to_string(),
                available: ids_of(array),
            });
    }
    match segment.parse::<usize>() {
        Ok(i) if i < array.len() => Ok(i),
        _ => {
            // A bare word that matches an id is almost certainly a forgotten "@".
            let ids = ids_of(array);
            if ids.iter().any(|id| id == segment) {
                return Err(PathFailure::Syntax(format!(
                    "{} is an array; to address the element with id {segment:?} write \"@{segment}\"",
                    show(at)
                )));
            }
            Err(PathFailure::BadIndex {
                at: at.to_string(),
                segment: segment.to_string(),
                len: array.len(),
                available_ids: ids,
            })
        }
    }
}

fn descend<'a>(
    mut node: &'a mut Value,
    segments: &[String],
) -> Result<(&'a mut Value, String), PathFailure> {
    let mut at = String::new();
    for segment in segments {
        node = match node {
            Value::Object(map) => {
                if !map.contains_key(segment.as_str()) {
                    return Err(PathFailure::NoSuchKey {
                        at,
                        key: segment.clone(),
                        available: map.keys().cloned().collect(),
                    });
                }
                map.get_mut(segment.as_str()).expect("checked above")
            }
            Value::Array(array) => {
                let i = locate(array, segment, &at)?;
                &mut array[i]
            }
            other => {
                return Err(PathFailure::NotAContainer {
                    at,
                    found: kind_of(other),
                    segment: segment.clone(),
                })
            }
        };
        at = join(&at, segment);
    }
    Ok((node, at))
}

fn check_new_id(
    array: &[Value],
    value: &Value,
    replacing: Option<usize>,
    at: &str,
) -> Result<(), PathFailure> {
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return Ok(());
    };
    if id.is_empty() {
        return Ok(());
    }
    let clash = array
        .iter()
        .enumerate()
        .any(|(i, e)| Some(i) != replacing && e.get("id").and_then(Value::as_str) == Some(id));
    if clash {
        return Err(PathFailure::DuplicateId {
            at: at.to_string(),
            id: id.to_string(),
            available: ids_of(array),
        });
    }
    Ok(())
}

fn apply_one(doc: &mut Value, op: &PatchOp) -> Result<(), PathFailure> {
    let segments = parse_pointer(op.path())?;
    match op {
        PatchOp::Append { value, .. } => {
            let (target, at) = descend(doc, &segments)?;
            let found = kind_of(target);
            let Value::Array(array) = target else {
                return Err(if segments.is_empty() {
                    PathFailure::Root
                } else {
                    PathFailure::NotAnArray { at, found }
                });
            };
            check_new_id(array, value, None, &at)?;
            array.push(value.clone());
            Ok(())
        }
        PatchOp::Set { value, .. } => {
            let Some((last, parents)) = segments.split_last() else {
                *doc = value.clone();
                return Ok(());
            };
            let (parent, at) = descend(doc, parents)?;
            match parent {
                Value::Object(map) => {
                    map.insert(last.clone(), value.clone());
                    Ok(())
                }
                Value::Array(array) if last == "-" => {
                    check_new_id(array, value, None, &at)?;
                    array.push(value.clone());
                    Ok(())
                }
                Value::Array(array) => {
                    let i = locate(array, last, &at)?;
                    check_new_id(array, value, Some(i), &at)?;
                    array[i] = value.clone();
                    Ok(())
                }
                other => Err(PathFailure::NotAContainer {
                    at,
                    found: kind_of(other),
                    segment: last.clone(),
                }),
            }
        }
        PatchOp::Remove { .. } => {
            let Some((last, parents)) = segments.split_last() else {
                return Err(PathFailure::Root);
            };
            let (parent, at) = descend(doc, parents)?;
            match parent {
                Value::Object(map) => {
                    // shift_remove keeps the remaining keys in order.
                    if map.shift_remove(last.as_str()).is_none() {
                        return Err(PathFailure::NoSuchKey {
                            at,
                            key: last.clone(),
                            available: map.keys().cloned().collect(),
                        });
                    }
                    Ok(())
                }
                Value::Array(array) => {
                    let i = locate(array, last, &at)?;
                    array.remove(i);
                    Ok(())
                }
                other => Err(PathFailure::NotAContainer {
                    at,
                    found: kind_of(other),
                    segment: last.clone(),
                }),
            }
        }
    }
}

/// Applies `ops` in order to a copy of `profile_json` and returns the result. The
/// input is untouched; on error nothing is applied. This layer is schema-agnostic:
/// it checks that paths resolve, not that the result is a valid profile (see
/// [`apply_typed`]).
pub fn apply(profile_json: &Value, ops: &[PatchOp]) -> Result<Value, PatchError> {
    let mut doc = profile_json.clone();
    for (op_index, op) in ops.iter().enumerate() {
        apply_one(&mut doc, op).map_err(|reason| PatchError::Op {
            op_index,
            op: op.name(),
            path: op.path().to_string(),
            reason: Box::new(reason),
        })?;
    }
    Ok(doc)
}

/// Applies a patch and deserialises the result into a [`Profile`]. A schema mismatch
/// is reported with the path of the offending value and serde's message, which lists
/// the accepted fields or variants.
pub fn apply_typed(profile_json: &Value, ops: &[PatchOp]) -> Result<Profile, PatchError> {
    let patched = apply(profile_json, ops)?;
    Profile::from_value(&patched).map_err(|message| PatchError::Schema { message })
}

/// Applies a patch to a typed profile. The profile is serialised with every field
/// present, so any schema path can be addressed.
pub fn apply_to_profile(profile: &Profile, ops: &[PatchOp]) -> Result<Profile, PatchError> {
    let json = serde_json::to_value(profile).expect("Profile serialisation cannot fail");
    apply_typed(&json, ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IncomeKind, RetirementTrigger};
    use serde_json::json;

    fn base() -> Profile {
        Profile::example()
    }

    fn err(ops: &[PatchOp]) -> String {
        apply_to_profile(&base(), ops).unwrap_err().to_string()
    }

    #[test]
    fn set_scalar_by_id_path() {
        let p = apply_to_profile(
            &base(),
            &[PatchOp::set("/incomes/@priya_job/kind/salary", 150_000)],
        )
        .unwrap();
        let IncomeKind::Job(job) = &p.incomes[0].kind else {
            panic!()
        };
        assert_eq!(job.salary, 150_000.0);
        // Everything else is untouched.
        let mut expected = base();
        let IncomeKind::Job(j) = &mut expected.incomes[0].kind else {
            panic!()
        };
        j.salary = 150_000.0;
        assert_eq!(p, expected);
    }

    #[test]
    fn set_replaces_tagged_enum() {
        let op = PatchOp::set(
            "/retirement/trigger",
            json!({"type": "at_age", "person": 0, "age": 55}),
        );
        let p = apply_to_profile(&base(), &[op]).unwrap();
        assert_eq!(
            p.retirement.trigger,
            RetirementTrigger::AtAge {
                person: 0,
                age: 55.0
            }
        );
    }

    #[test]
    fn append_and_remove_elements() {
        let ops = [
            PatchOp::append(
                "/spending/items",
                json!({"id": "boat", "name": "Boat", "category": "entertainment", "amount": 300}),
            ),
            PatchOp::remove("/events/@kitchen"),
            PatchOp::remove("/debts/0"),
        ];
        let p = apply_to_profile(&base(), &ops).unwrap();
        assert_eq!(p.spending.items.last().unwrap().id, "boat");
        assert!(p.events.iter().all(|e| e.id != "kitchen"));
        assert!(p.debts.is_empty());
    }

    #[test]
    fn remove_field_resets_to_default() {
        let p = apply_to_profile(&base(), &[PatchOp::remove("/retirement/spending_rule")]).unwrap();
        assert_eq!(
            p.retirement.spending_rule,
            crate::model::SpendingRule::ConstantReal
        );
    }

    #[test]
    fn set_dash_appends_and_index_replaces() {
        let doc = json!({"xs": [{"id": "a", "v": 1}]});
        let out = apply(
            &doc,
            &[
                PatchOp::set("/xs/-", json!({"id": "b"})),
                PatchOp::set("/xs/0/v", 2),
            ],
        )
        .unwrap();
        assert_eq!(out, json!({"xs": [{"id": "a", "v": 2}, {"id": "b"}]}));
    }

    #[test]
    fn failure_leaves_input_untouched_and_is_atomic() {
        let doc = json!({"a": 1, "xs": []});
        let ops = [PatchOp::set("/a", 2), PatchOp::remove("/nope")];
        assert!(apply(&doc, &ops).is_err());
        assert_eq!(doc, json!({"a": 1, "xs": []}));
    }

    #[test]
    fn unknown_id_lists_valid_ids() {
        let msg = err(&[PatchOp::set("/incomes/@bob_job/kind/salary", 1)]);
        assert!(msg.contains("operation 0"), "{msg}");
        assert!(msg.contains("/incomes/@bob_job/kind/salary"), "{msg}");
        assert!(
            msg.contains("no element with id \"bob_job\" in /incomes"),
            "{msg}"
        );
        assert!(msg.contains("priya_job, daniel_job"), "{msg}");
    }

    #[test]
    fn unknown_key_lists_valid_keys() {
        let msg = err(&[PatchOp::set("/retirment/bequest_target", 1)]);
        assert!(
            msg.contains("the profile root has no field \"retirment\""),
            "{msg}"
        );
        assert!(
            msg.contains("retirement") && msg.contains("household"),
            "{msg}"
        );

        let msg = err(&[PatchOp::remove("/spending/lifestyle")]);
        assert!(
            msg.contains("/spending has no field \"lifestyle\""),
            "{msg}"
        );
        assert!(msg.contains("lifestyle_creep"), "{msg}");
    }

    #[test]
    fn missing_at_sign_is_explained() {
        let msg = err(&[PatchOp::remove("/events/kitchen")]);
        assert!(msg.contains("write \"@kitchen\""), "{msg}");
        let msg = err(&[PatchOp::remove("/events/99")]);
        assert!(
            msg.contains("array of 4 element(s)") && msg.contains("buy_home"),
            "{msg}"
        );
    }

    #[test]
    fn null_parent_is_explained() {
        let msg = err(&[PatchOp::set(
            "/incomes/@daniel_job/kind/bonus/target_fraction",
            0.1,
        )]);
        assert!(
            msg.contains("/incomes/@daniel_job/kind/bonus is null"),
            "{msg}"
        );
        assert!(msg.contains("Set the whole object"), "{msg}");
    }

    #[test]
    fn append_errors() {
        let msg = err(&[PatchOp::append("/retirement", json!({}))]);
        assert!(
            msg.contains("append needs an array, but /retirement is an object"),
            "{msg}"
        );
        let msg = err(&[PatchOp::append("/accounts", json!({"id": "hsa"}))]);
        assert!(
            msg.contains("id \"hsa\" is already used in /accounts"),
            "{msg}"
        );
        assert!(msg.contains("priya_401k"), "{msg}");
    }

    #[test]
    fn pointer_syntax_errors() {
        let msg = err(&[PatchOp::set("retirement/bequest_target", 1)]);
        assert!(msg.contains("must start with \"/\""), "{msg}");
        let msg = err(&[PatchOp::set("/retirement//x", 1)]);
        assert!(msg.contains("empty segment"), "{msg}");
        assert!(err(&[PatchOp::remove("")]).contains("root"));
    }

    #[test]
    fn schema_errors_name_the_path_and_the_choices() {
        let msg = err(&[PatchOp::set("/spending/lifestyle_crep", 0.5)]);
        assert!(msg.contains("does not match the schema"), "{msg}");
        assert!(
            msg.contains("spending") && msg.contains("lifestyle_crep"),
            "{msg}"
        );
        assert!(msg.contains("lifestyle_creep"), "{msg}");

        let msg = err(&[PatchOp::set(
            "/retirement/trigger",
            json!({"type": "at_some_point"}),
        )]);
        assert!(msg.contains("retirement.trigger"), "{msg}");
        assert!(
            msg.contains("when_funded") && msg.contains("at_age"),
            "{msg}"
        );

        let msg = err(&[PatchOp::set("/household/people/@priya/birth", "March 1989")]);
        assert!(
            msg.contains("household.people[0].birth") && msg.contains("YYYY-MM"),
            "{msg}"
        );
    }

    #[test]
    fn escapes_follow_rfc_6901() {
        let doc = json!({"a/b": {"c~d": 1}});
        let out = apply(&doc, &[PatchOp::set("/a~1b/c~0d", 2)]).unwrap();
        assert_eq!(out, json!({"a/b": {"c~d": 2}}));
    }

    #[test]
    fn ops_deserialise_from_the_documented_shape() {
        let ops: Vec<PatchOp> = serde_json::from_value(json!([
            {"op": "set", "path": "/name", "value": "x"},
            {"op": "remove", "path": "/events/@a"},
            {"op": "append", "path": "/events", "value": {"id": "b"}}
        ]))
        .unwrap();
        assert_eq!(ops[0], PatchOp::set("/name", "x"));
        assert_eq!(ops[1], PatchOp::remove("/events/@a"));
        assert!(serde_json::from_value::<PatchOp>(json!({"op": "move", "path": "/a"})).is_err());
    }
}
