#[path = "../build/schema_filter.rs"]
mod schema_filter;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::Path;

use serde_json::json;

#[test]
fn retains_transitive_schema_references_and_cycles() {
    let mut document = json!({
        "components": {
            "schemas": {
                "Root": {
                    "allOf": [
                        { "$ref": "#/components/schemas/Child" },
                        { "properties": { "mapped": { "$ref": "#/components/schemas/Mapped" } } }
                    ],
                    "discriminator": {
                        "mapping": { "child": "#/components/schemas/Child" }
                    }
                },
                "Child": {
                    "properties": {
                        "root": { "$ref": "#/components/schemas/Root" }
                    }
                },
                "Mapped": { "type": "string" },
                "Unused": { "type": "object" }
            }
        }
    });

    schema_filter::retain_reachable_schemas(&mut document, &["Root".to_string()]).unwrap();

    let schemas = document["components"]["schemas"].as_object().unwrap();
    assert_eq!(
        schemas.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["Child", "Mapped", "Root"])
    );
}

#[test]
fn supports_swagger_definitions_and_rejects_missing_roots() {
    let mut document = json!({
        "definitions": {
            "Root": { "$ref": "#/definitions/Child" },
            "Child": { "type": "object" },
            "Unused": { "type": "object" }
        }
    });
    schema_filter::retain_reachable_schemas(&mut document, &["Root".to_string()]).unwrap();
    assert_eq!(document["definitions"].as_object().unwrap().len(), 2);

    assert!(
        schema_filter::retain_reachable_schemas(&mut document, &["Missing".to_string()])
            .unwrap_err()
            .contains("does not exist")
    );
}

#[test]
fn checked_in_roots_resolve_and_shrink_the_schema_set() {
    let roots_by_spec: BTreeMap<String, Vec<String>> =
        serde_json::from_str(include_str!("../build/model_roots.json")).unwrap();
    assert_eq!(roots_by_spec.len(), 25);
    let mut original_total = 0;
    let mut retained_total = 0;
    let mut shrunk_specs = 0;

    for (spec_name, roots) in roots_by_spec {
        let spec_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("openapi")
            .join(&spec_name);
        let mut document: serde_json::Value =
            serde_json::from_reader(File::open(&spec_path).unwrap()).unwrap();
        let original_count = schema_count(&document);

        schema_filter::retain_reachable_schemas(&mut document, &roots).unwrap();

        let retained_count = schema_count(&document);
        original_total += original_count;
        retained_total += retained_count;
        shrunk_specs += usize::from(retained_count < original_count);
    }

    assert!(shrunk_specs >= 20, "only {shrunk_specs} of 25 specs shrank");
    assert!(
        retained_total * 4 < original_total * 3,
        "retained {retained_total} of {original_total} schemas"
    );
}

fn schema_count(document: &serde_json::Value) -> usize {
    document
        .pointer("/components/schemas")
        .or_else(|| document.get("definitions"))
        .and_then(serde_json::Value::as_object)
        .map_or(0, serde_json::Map::len)
}
