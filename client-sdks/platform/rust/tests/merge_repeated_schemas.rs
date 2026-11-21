//! The complete client's schema merging. A separate test binary from `openapi_filter`:
//! merging switches the module into its merging mode for the whole process.

#[path = "../build/openapi_filter.rs"]
mod openapi_filter;

use openapi_filter::{merge_repeated_schemas_with_positions, POSITION_MARKER};
use serde_json::{json, Value};

fn document(schemas: Value) -> Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "test", "version": "1" },
        "paths": {},
        "components": { "schemas": schemas }
    })
}

fn owner() -> Value {
    json!({
        "type": "object",
        "required": ["name", "kind"],
        "properties": {
            "name": { "type": "string" },
            "kind": { "type": "string", "enum": ["user", "service"] }
        }
    })
}

fn position(schema: &Value) -> Option<&str> {
    let description = schema.get("description")?.as_str()?;
    let start = description.find(POSITION_MARKER)? + POSITION_MARKER.len();
    Some(&description[start..])
}

#[test]
fn a_repeated_object_is_shared_and_each_use_records_where_it_went() {
    let input = document(json!({
        "A": { "type": "object", "properties": { "owner": owner() } },
        "B": { "type": "object", "properties": { "owner": owner() } }
    }));

    let (merged, unmerged, positioned) = merge_repeated_schemas_with_positions(&input).unwrap();

    let shared = merged["components"]["schemas"]["A"]["properties"]["owner"]["$ref"]
        .as_str()
        .unwrap()
        .strip_prefix("#/components/schemas/")
        .unwrap()
        .to_string();
    assert_eq!(
        merged["components"]["schemas"]["B"]["properties"]["owner"]["$ref"],
        format!("#/components/schemas/{shared}")
    );
    for parent in ["A", "B"] {
        let used = &unmerged["components"]["schemas"][parent]["properties"]["owner"];
        assert_eq!(position(used), Some(format!("{shared}#/").as_str()));
        assert_eq!(
            position(&used["properties"]["kind"]),
            Some(format!("{shared}#/properties/kind").as_str()),
            "a nested enum moves with the object that holds it"
        );
        assert_eq!(
            position(&unmerged["components"]["schemas"][parent]),
            None,
            "a schema that was not replaced keeps its name and needs no position"
        );
    }
    let component = &positioned["components"]["schemas"][&shared];
    assert_eq!(position(component), Some(format!("{shared}#/").as_str()));
    assert_eq!(
        position(&component["properties"]["kind"]),
        Some(format!("{shared}#/properties/kind").as_str())
    );
    assert_eq!(
        merged["components"]["schemas"][&shared].get("description"),
        None
    );
}

#[test]
fn a_use_that_matches_an_existing_component_records_the_component_reached_through_references() {
    let input = document(json!({
        "Settings": {
            "type": "object",
            "properties": { "owner": { "$ref": "#/components/schemas/Owner" } }
        },
        "Owner": owner(),
        "Request": {
            "type": "object",
            "properties": {
                "settings": {
                    "type": "object",
                    "properties": { "owner": owner() }
                }
            }
        },
        "Response": {
            "type": "object",
            "properties": {
                "settings": {
                    "type": "object",
                    "properties": { "owner": owner() }
                }
            }
        }
    }));

    let (_, unmerged, _) = merge_repeated_schemas_with_positions(&input).unwrap();

    let used = &unmerged["components"]["schemas"]["Request"]["properties"]["settings"];
    assert!(position(used).is_some_and(|text| text.ends_with("#/")));
    assert_eq!(
        position(&used["properties"]["owner"]["properties"]["kind"]),
        Some("Owner#/properties/kind")
    );
}

#[test]
fn nullable_objects_maps_and_union_variants_stay_inline() {
    let nullable = json!({
        "type": "object",
        "nullable": true,
        "properties": { "id": { "type": "string" } }
    });
    let map = json!({
        "type": "object",
        "additionalProperties": { "type": "integer", "minimum": 0 }
    });
    let variant = json!({
        "type": "object",
        "required": ["platform"],
        "properties": { "platform": { "type": "string", "enum": ["aws"] } }
    });
    let input = document(json!({
        "A": {
            "type": "object",
            "properties": {
                "link": nullable.clone(),
                "counts": map.clone(),
                "environment": { "oneOf": [variant.clone()] }
            }
        },
        "B": {
            "type": "object",
            "properties": {
                "link": nullable.clone(),
                "counts": map.clone(),
                "environment": { "oneOf": [variant.clone()] }
            }
        }
    }));

    let (merged, _, _) = merge_repeated_schemas_with_positions(&input).unwrap();

    for parent in ["A", "B"] {
        let properties = &merged["components"]["schemas"][parent]["properties"];
        assert_eq!(properties["link"], nullable);
        assert_eq!(properties["counts"], map);
        assert_eq!(properties["environment"]["oneOf"][0], variant);
    }
}
