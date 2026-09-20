use std::collections::{BTreeSet, VecDeque};

use serde_json::Value;

pub fn retain_reachable_schemas(document: &mut Value, roots: &[String]) -> Result<(), String> {
    let schemas = document
        .pointer("/components/schemas")
        .or_else(|| document.get("definitions"))
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenAPI document has no schema component map".to_string())?;

    let mut reachable = BTreeSet::new();
    let mut queue: VecDeque<String> = roots.iter().cloned().collect();
    while let Some(name) = queue.pop_front() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        let schema = schemas
            .get(&name)
            .ok_or_else(|| format!("required Azure schema `{name}` does not exist"))?;
        collect_schema_references(schema, &mut queue);
    }

    let schemas = if document.pointer("/components/schemas").is_some() {
        document.pointer_mut("/components/schemas")
    } else {
        document.get_mut("definitions")
    }
    .and_then(Value::as_object_mut)
    .expect("schema map was validated above");
    schemas.retain(|name, _| reachable.contains(name));
    Ok(())
}

fn collect_schema_references(value: &Value, queue: &mut VecDeque<String>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_schema_references(value, queue);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_schema_references(value, queue);
            }
        }
        Value::String(value) => {
            if let Some(name) = parse_schema_reference(value) {
                queue.push_back(name);
            }
        }
        _ => {}
    }
}

fn parse_schema_reference(value: &str) -> Option<String> {
    value
        .strip_prefix("#/components/schemas/")
        .or_else(|| value.strip_prefix("#/definitions/"))
        .map(decode_json_pointer_segment)
}

fn decode_json_pointer_segment(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}
