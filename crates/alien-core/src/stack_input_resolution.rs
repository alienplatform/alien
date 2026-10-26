//! Resolves applicable inputs before checking requiredness or portable constraints.

use std::collections::HashMap;

use alien_error::{AlienError, Context, IntoAlienError};
use serde_json::Value;

use crate::{
    is_deployer_secret_input, ErrorData, Platform, Result, StackInputDefaultValue,
    StackInputDefinition, StackInputKind,
};

/// Resolve defaults and validate only inputs that apply. Values for inactive
/// inputs are ignored, even when they would fail validation. Vault-native
/// secrets are checked in the deployer's secret store instead.
pub fn resolve_stack_input_values(
    inputs: &[StackInputDefinition],
    values: &HashMap<String, Value>,
    platform: Platform,
) -> Result<HashMap<String, Value>> {
    let mut resolved = HashMap::new();
    for input in inputs {
        if !input.applies(inputs, values, platform) {
            continue;
        }
        let value = values
            .get(&input.id)
            .filter(|value| !value.is_null())
            .cloned()
            .or_else(|| {
                input.default.as_ref().map(|default| match default {
                    StackInputDefaultValue::String(value)
                    | StackInputDefaultValue::Number(value) => Value::String(value.clone()),
                    StackInputDefaultValue::Boolean(value) => Value::Bool(*value),
                    StackInputDefaultValue::StringList(value) => serde_json::json!(value),
                })
            });
        let Some(value) = value else {
            if input.required && !is_deployer_secret_input(input) && !input.is_generated() {
                return Err(invalid(input, "a value is required"));
            }
            continue;
        };
        let value = normalize(input, value)?;
        if input.required
            && (value.as_str() == Some("") || value.as_array().is_some_and(Vec::is_empty))
        {
            return Err(invalid(input, "a non-empty value is required"));
        }
        validate(input, &value)?;
        resolved.insert(input.id.clone(), value);
    }
    Ok(resolved)
}

fn invalid(input: &StackInputDefinition, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StackInputInvalid {
        input_id: input.id.clone(),
        reason: reason.into(),
    })
}

fn normalize(input: &StackInputDefinition, value: Value) -> Result<Value> {
    match input.kind {
        StackInputKind::Boolean => crate::gate_value_as_bool(&value)
            .map(Value::Bool)
            .ok_or_else(|| invalid(input, "must be a boolean")),
        StackInputKind::Integer | StackInputKind::Number => {
            let number = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
                .filter(|number| number.is_finite())
                .ok_or_else(|| invalid(input, "must be a finite number"))?;
            if input.kind == StackInputKind::Integer && number.fract() != 0.0 {
                return Err(invalid(input, "must be an integer"));
            }
            if value.is_number() {
                return Ok(value);
            }
            let parsed = value
                .as_str()
                .and_then(|value| value.parse::<serde_json::Number>().ok());
            Ok(parsed
                .map(Value::Number)
                .unwrap_or_else(|| serde_json::json!(number)))
        }
        StackInputKind::StringList => {
            if !value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
            {
                return Err(invalid(input, "must be a list of strings"));
            }
            Ok(value)
        }
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            if !value.is_string() {
                return Err(invalid(input, "must be a string"));
            }
            Ok(value)
        }
    }
}

fn validate(input: &StackInputDefinition, value: &Value) -> Result<()> {
    let Some(validation) = &input.validation else {
        return Ok(());
    };
    if let Some(text) = value.as_str() {
        let length = text.chars().count();
        if validation
            .min_length
            .is_some_and(|min| length < min as usize)
            || validation
                .max_length
                .is_some_and(|max| length > max as usize)
        {
            return Err(invalid(input, "length is outside the declared bounds"));
        }
        if validation
            .values
            .as_ref()
            .is_some_and(|values| !values.iter().any(|candidate| candidate == text))
        {
            return Err(invalid(input, "must be one of the declared values"));
        }
        if let Some(pattern) = &validation.pattern {
            let regex = regex::Regex::new(&format!("^(?:{pattern})$"))
                .into_alien_error()
                .context(ErrorData::StackInputInvalid {
                    input_id: input.id.clone(),
                    reason: "declares an invalid pattern".into(),
                })?;
            if !regex.is_match(text) {
                return Err(invalid(input, "does not match the declared pattern"));
            }
        }
        if validation.format.as_deref() == Some("url") && url::Url::parse(text).is_err() {
            return Err(invalid(input, "must be a URL"));
        }
    }
    if let Some(number) = value.as_f64() {
        for (bound, minimum) in [(&validation.min, true), (&validation.max, false)] {
            if let Some(bound) = bound {
                let bound = bound.parse::<f64>().into_alien_error().context(
                    ErrorData::StackInputInvalid {
                        input_id: input.id.clone(),
                        reason: "declares an invalid numeric bound".into(),
                    },
                )?;
                if (minimum && number < bound) || (!minimum && number > bound) {
                    return Err(invalid(input, "number is outside the declared bounds"));
                }
            }
        }
    }
    if let Some(items) = value.as_array() {
        if validation
            .min_items
            .is_some_and(|min| items.len() < min as usize)
            || validation
                .max_items
                .is_some_and(|max| items.len() > max as usize)
        {
            return Err(invalid(input, "item count is outside the declared bounds"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Vec<StackInputDefinition> {
        serde_json::from_value(serde_json::json!([
            {"id":"bucket", "kind":"string", "required":true, "providedBy":["deployer"], "label":"Bucket", "description":"Snapshot bucket", "when":{"input":"enabled","equals":true}, "validation":{"minLength":3}},
            {"id":"enabled", "kind":"boolean", "required":true, "providedBy":["deployer"], "label":"Enabled", "description":"Enable snapshots", "default":{"type":"boolean","value":true}}
        ])).unwrap()
    }

    #[test]
    fn inactive_required_input_is_ignored_including_invalid_values() {
        let inputs = inputs();
        for value in [Value::Null, serde_json::json!(42), serde_json::json!("x")] {
            let values = HashMap::from([
                ("enabled".into(), Value::Bool(false)),
                ("bucket".into(), value),
            ]);
            assert_eq!(
                resolve_stack_input_values(&inputs, &values, Platform::Aws).unwrap(),
                HashMap::from([("enabled".into(), Value::Bool(false))])
            );
        }
    }

    #[test]
    fn active_input_is_required_and_validated_with_provided_or_default_toggle() {
        for toggle in [None, Some(Value::Bool(true))] {
            for bucket in [
                None,
                Some(serde_json::json!("")),
                Some(serde_json::json!("x")),
                Some(serde_json::json!(42)),
            ] {
                let mut values = HashMap::new();
                if let Some(value) = &toggle {
                    values.insert("enabled".into(), value.clone());
                }
                if let Some(value) = bucket {
                    values.insert("bucket".into(), value);
                }
                let error =
                    resolve_stack_input_values(&inputs(), &values, Platform::Aws).unwrap_err();
                assert_eq!(error.code, "STACK_INPUT_INVALID");
                assert!(error.message.contains("bucket"));
            }
        }
        let values = HashMap::from([("bucket".into(), serde_json::json!("snapshots"))]);
        let resolved = resolve_stack_input_values(&inputs(), &values, Platform::Aws).unwrap();
        assert_eq!(resolved["bucket"], "snapshots");
        assert_eq!(resolved["enabled"], true);
    }
}
