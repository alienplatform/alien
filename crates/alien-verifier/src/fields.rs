//! Field tables: how the verifier checks a cloud write the controller built.
//!
//! The controller hands the verifier the exact request it would send the cloud on an unprotected
//! install. The verifier parses it into the cloud client's own typed request, refusing any field
//! the type doesn't know, then checks every field against one table per cloud write. Each row
//! names a field and one rule: the field must match the signed value, equal what setup fixed,
//! be empty, or is free (it can't change what code runs or who can call it). A field no row
//! names is refused, so a controller that starts sending a new field fails loudly until the
//! field has a row.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

/// A check of one field's value; `Err` says how it differs.
pub type Check<'a> = Box<dyn Fn(&Value) -> std::result::Result<(), String> + 'a>;

/// What one field of a write must be.
pub enum Rule<'a> {
    /// What the signed release, its config or the install decide.
    Signed(Check<'a>),
    /// Exactly what setup fixed for this install.
    Setup(Value),
    /// Nothing: absent, null, or an empty string, list or object.
    Empty,
    /// Anything: size, speed or cost, which a control plane can already harm by not deploying.
    Free,
}

impl Rule<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Rule::Signed(_) => "must match the signed release",
            Rule::Setup(_) => "must be what setup fixed",
            Rule::Empty => "must be empty",
            Rule::Free => "free",
        }
    }
}

/// One row: a field, by JSON pointer into the write, and its rule. The rule covers the whole
/// value at that pointer, nested fields included.
pub struct Row<'a> {
    pub field: &'static str,
    pub rule: Rule<'a>,
}

/// A row that checks the field's value with `check`.
pub fn signed<'a>(
    field: &'static str,
    check: impl Fn(&Value) -> std::result::Result<(), String> + 'a,
) -> Row<'a> {
    Row {
        field,
        rule: Rule::Signed(Box::new(check)),
    }
}

/// A row for a field setup fixed to `value`.
pub fn setup<'a>(field: &'static str, value: Value) -> Row<'a> {
    Row {
        field,
        rule: Rule::Setup(value),
    }
}

/// A row for a field that must be empty.
pub fn empty<'a>(field: &'static str) -> Row<'a> {
    Row {
        field,
        rule: Rule::Empty,
    }
}

/// A row for a free field.
pub fn free<'a>(field: &'static str) -> Row<'a> {
    Row {
        field,
        rule: Rule::Free,
    }
}

fn refused(reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused { reason })
}

/// Whether `value` carries nothing: null, an empty string, list or object.
pub fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

/// `write` as the client's typed request `T`. A field `T` doesn't know is refused: parsing
/// would drop it, so what reaches the cloud would differ from what was checked.
pub fn parse_strict<T: DeserializeOwned + Serialize>(operation: &str, write: &Value) -> Result<T> {
    let typed: T = serde_json::from_value(write.clone())
        .map_err(|error| refused(format!("the {operation} request does not parse: {error}")))?;
    let back = serde_json::to_value(&typed).map_err(|error| {
        refused(format!(
            "the {operation} request does not serialize: {error}"
        ))
    })?;
    if let Some(field) = dropped(write, &back, String::new()) {
        return Err(refused(format!(
            "the {operation} request has field {field}, which the verifier doesn't know"
        )));
    }
    Ok(typed)
}

/// The first field of `sent` that `parsed` lost (a null counts as absent).
fn dropped(sent: &Value, parsed: &Value, at: String) -> Option<String> {
    match (sent, parsed) {
        (Value::Object(sent), Value::Object(parsed)) => sent.iter().find_map(|(key, value)| {
            let path = format!("{at}/{key}");
            match parsed.get(key) {
                None if value.is_null() => None,
                None => Some(path),
                Some(kept) => dropped(value, kept, path),
            }
        }),
        (Value::Array(sent), Value::Array(parsed)) => sent
            .iter()
            .zip(parsed)
            .enumerate()
            .find_map(|(index, (value, kept))| dropped(value, kept, format!("{at}/{index}"))),
        _ => None,
    }
}

/// Check every field of `write` (an `operation` request) against `rows`. A field in no row is
/// refused, and so is a field that breaks its row; the reason names the row.
pub fn check(operation: &str, write: &Value, rows: &[Row<'_>]) -> Result<()> {
    walk(operation, write, "", rows)
}

fn walk(operation: &str, value: &Value, at: &str, rows: &[Row<'_>]) -> Result<()> {
    let Value::Object(fields) = value else {
        return Err(refused(format!(
            "the {operation} request's field {at} is not an object"
        )));
    };
    for (key, value) in fields {
        let path = format!("{at}/{key}");
        if let Some(row) = rows.iter().find(|row| row.field == path) {
            let broken = match &row.rule {
                Rule::Signed(check) => check(value).err(),
                Rule::Setup(expected) => {
                    (value != expected).then(|| format!("it is {value}, setup fixed {expected}"))
                }
                Rule::Empty => (!is_empty(value)).then(|| format!("it is {value}")),
                Rule::Free => None,
            };
            if let Some(reason) = broken {
                return Err(refused(format!(
                    "{operation} field {path} {}: {reason}",
                    row.rule.kind()
                )));
            }
        } else if rows
            .iter()
            .any(|row| row.field.starts_with(&format!("{path}/")))
        {
            if !value.is_null() {
                walk(operation, value, &path, rows)?;
            }
        } else if !value.is_null() {
            return Err(refused(format!(
                "{operation} field {path} is in no row of the verifier's table"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Request {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        inner: Option<Inner>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Inner {
        size: u32,
    }

    #[test]
    fn a_field_the_typed_request_drops_is_refused() {
        parse_strict::<Request>("Op", &json!({"Name": "a", "Inner": {"Size": 1}})).unwrap();
        parse_strict::<Request>("Op", &json!({"Name": "a", "Inner": null})).unwrap();
        let error = parse_strict::<Request>("Op", &json!({"Name": "a", "Layers": ["x"]}))
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("field /Layers, which the verifier doesn't know"),
            "{error}"
        );
        let error = parse_strict::<Request>(
            "Op",
            &json!({"Name": "a", "Inner": {"Size": 1, "Extra": 2}}),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("field /Inner/Extra"), "{error}");
    }

    fn rows<'a>() -> Vec<Row<'a>> {
        vec![
            signed("/Name", |value| {
                (value == "acme-api")
                    .then_some(())
                    .ok_or_else(|| format!("it is {value}"))
            }),
            setup("/Inner/Size", json!(1)),
            empty("/Layers"),
            free("/Memory"),
        ]
    }

    #[test]
    fn every_field_is_checked_against_its_row() {
        check(
            "Op",
            &json!({"Name": "acme-api", "Inner": {"Size": 1}, "Layers": [], "Memory": 9}),
            &rows(),
        )
        .unwrap();
        for (write, says) in [
            (
                json!({"Name": "other"}),
                "field /Name must match the signed release",
            ),
            (
                json!({"Inner": {"Size": 2}}),
                "field /Inner/Size must be what setup fixed",
            ),
            (json!({"Layers": ["x"]}), "field /Layers must be empty"),
            (json!({"Code": "x"}), "field /Code is in no row"),
            (
                json!({"Inner": {"Other": 1}}),
                "field /Inner/Other is in no row",
            ),
        ] {
            let error = check("Op", &write, &rows()).unwrap_err().to_string();
            assert!(error.contains(says), "{write}: {error}");
        }
    }
}
