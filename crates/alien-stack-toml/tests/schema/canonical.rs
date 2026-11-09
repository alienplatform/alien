//! `schema/canonical.json`: the alien-core types an `alien.toml` is read through.

use std::collections::BTreeSet;

use alien_core::stack_authoring::{AuthoredInput, AuthoredPool, AUTHORABLE_TYPES};
use alien_core::{ComputeCluster, ManagementPermissions, OperationsConfig, PermissionsConfig, Platform};
use serde_json::{Map, Value};
use utoipa::OpenApi;

/// Every type the resolver reads besides the resource types in the registry.
const EXTRA_ROOTS: &[&str] = &[
    "AuthoredInput",
    "AuthoredPool",
    "ComputeCluster",
    "ManagementPermissions",
    "OperationsConfig",
    "PermissionsConfig",
    "Platform",
];

#[derive(OpenApi)]
#[openapi(components(schemas(
    alien_core::Container,
    alien_core::Daemon,
    alien_core::Worker,
    alien_core::Build,
    alien_core::Storage,
    alien_core::Kv,
    alien_core::Queue,
    alien_core::Vault,
    alien_core::Key,
    alien_core::Postgres,
    alien_core::Email,
    alien_core::Ai,
    alien_core::Sandbox,
    alien_core::ArtifactRegistry,
    alien_core::ServiceAccount,
    alien_core::AwsOpenSearch,
    AuthoredInput,
    AuthoredPool,
    ComputeCluster,
    ManagementPermissions,
    OperationsConfig,
    PermissionsConfig,
    Platform,
)))]
struct Types;

/// All component schemas of the OpenAPI document, with their descriptions.
pub fn components() -> Map<String, Value> {
    let document = serde_json::to_value(Types::openapi()).expect("the OpenAPI document serializes");
    document["components"]["schemas"]
        .as_object()
        .expect("the document has component schemas")
        .clone()
}

/// The roots plus every schema they reach.
pub fn reachable(components: &Map<String, Value>) -> BTreeSet<String> {
    let mut roots: Vec<String> = AUTHORABLE_TYPES
        .iter()
        .map(|ty| ty.schema.to_string())
        .chain(EXTRA_ROOTS.iter().map(|name| name.to_string()))
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(name) = roots.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let schema = components
            .get(&name)
            .unwrap_or_else(|| panic!("schema '{name}' is not in the OpenAPI document; add its type to `Types`"));
        references(schema, &mut roots);
    }
    seen
}

fn references(schema: &Value, found: &mut Vec<String>) {
    match schema {
        Value::Object(object) => {
            for (key, value) in object {
                match (key.as_str(), value) {
                    ("$ref", Value::String(target)) => {
                        found.push(target.rsplit('/').next().unwrap_or(target).to_string());
                    }
                    _ => references(value, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| references(item, found)),
        _ => {}
    }
}

/// What the resolver needs of a schema: its shape, without documentation or bounds.
fn strip(schema: &Value) -> Value {
    const KEPT: &[&str] = &[
        "type",
        "properties",
        "required",
        "items",
        "additionalProperties",
        "oneOf",
        "allOf",
        "enum",
        "$ref",
    ];
    match schema {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| KEPT.contains(&key.as_str()))
                .map(|(key, value)| {
                    let value = match key.as_str() {
                        "properties" => Value::Object(
                            value
                                .as_object()
                                .expect("properties is an object")
                                .iter()
                                .map(|(name, property)| (name.clone(), strip(property)))
                                .collect(),
                        ),
                        _ => strip(value),
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip).collect()),
        other => other.clone(),
    }
}

/// Sorts object keys so the output does not depend on serde_json's `preserve_order`, which
/// another crate in the build may switch on.
pub fn sorted(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(entries.into_iter().map(|(key, value)| (key, sorted(value))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

pub fn generate() -> String {
    let components = components();
    let schemas: Map<String, Value> = reachable(&components)
        .into_iter()
        .map(|name| {
            let schema = strip(&components[&name]);
            (name, schema)
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&sorted(serde_json::json!({ "components": schemas })))
        .expect("the schema serializes");
    text.push('\n');
    text
}

#[test]
fn canonical_schema_is_up_to_date() {
    crate::check_generated("canonical.json", &generate());
}
