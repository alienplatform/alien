#[path = "../build/openapi_filter.rs"]
mod openapi_filter;

use serde_json::{json, Value};

#[test]
fn retains_only_required_operations_and_transitive_components() {
    let document = json!({
        "openapi": "3.0.3",
        "info": { "title": "test", "version": "1" },
        "paths": {
            "/kept": {
                "parameters": [{ "$ref": "#/components/parameters/Tenant" }],
                "get": {
                    "operationId": "kept",
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/A" }
                                }
                            }
                        }
                    }
                },
                "post": { "operationId": "removed", "responses": {} }
            },
            "/removed": {
                "get": {
                    "operationId": "alsoRemoved",
                    "responses": {
                        "200": {
                            "description": "unused",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Unused" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "parameters": {
                "Tenant": { "name": "tenant", "in": "header", "schema": { "type": "string" } }
            },
            "schemas": {
                "A": {
                    "oneOf": [{ "$ref": "#/components/schemas/B" }],
                    "discriminator": { "mapping": { "b": "#/components/schemas/B" } }
                },
                "B": { "type": "object", "properties": { "a": { "$ref": "#/components/schemas/A" } } },
                "Mode": { "type": "string", "enum": ["one", "two"] },
                "UsesMode": {
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "nullable": true,
                            "enum": ["one", "two", null],
                            "description": "A nullable mode."
                        }
                    }
                },
                "Unused": { "type": "object" }
            },
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer" }
            }
        }
    });

    let filtered = openapi_filter::filter_openapi(&document, &["kept"]).unwrap();

    assert_eq!(operation_ids(&filtered), vec!["kept"]);
    assert!(filtered.pointer("/paths/~1kept/parameters").is_some());
    assert!(filtered.pointer("/components/parameters/Tenant").is_some());
    assert!(filtered.pointer("/components/schemas/A").is_some());
    assert!(filtered.pointer("/components/schemas/B").is_some());
    assert!(filtered.pointer("/components/schemas/Unused").is_none());
    assert!(filtered
        .pointer("/components/securitySchemes/bearer")
        .is_some());
}

#[test]
fn canonicalizes_nullable_copies_of_reachable_string_enums() {
    let document = json!({
        "paths": {
            "/a": {
                "get": {
                    "operationId": "kept",
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "allOf": [
                                            { "$ref": "#/components/schemas/UsesMode" },
                                            { "$ref": "#/components/schemas/Mode" }
                                        ]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Mode": { "type": "string", "enum": ["one", "two"] },
                "UsesMode": {
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "nullable": true,
                            "enum": ["one", "two", null],
                            "description": "A nullable mode."
                        }
                    }
                }
            }
        }
    });

    let filtered = openapi_filter::filter_openapi(&document, &["kept"]).unwrap();
    let mode = filtered
        .pointer("/components/schemas/UsesMode/properties/mode")
        .unwrap();
    assert_eq!(mode["nullable"], true);
    assert_eq!(mode["description"], "A nullable mode.");
    assert_eq!(mode["allOf"][0]["$ref"], "#/components/schemas/Mode");
    assert!(mode.get("enum").is_none());

    let normalized = openapi_filter::normalize_openapi(&document).unwrap();
    let mode = normalized
        .pointer("/components/schemas/UsesMode/properties/mode")
        .unwrap();
    assert_eq!(mode["allOf"][0]["$ref"], "#/components/schemas/Mode");
}

#[test]
fn rejects_missing_duplicate_and_unresolved_operations() {
    let duplicate = json!({
        "paths": {
            "/a": { "get": { "operationId": "same" } },
            "/b": { "post": { "operationId": "same" } }
        },
        "components": {}
    });
    assert!(openapi_filter::filter_openapi(&duplicate, &["same"])
        .unwrap_err()
        .contains("duplicated"));
    assert!(openapi_filter::filter_openapi(&duplicate, &["missing"])
        .unwrap_err()
        .contains("missing"));

    let unresolved = json!({
        "paths": {
            "/a": {
                "get": {
                    "operationId": "kept",
                    "responses": { "200": { "$ref": "#/components/responses/Missing" } }
                }
            }
        },
        "components": {}
    });
    assert!(openapi_filter::filter_openapi(&unresolved, &["kept"])
        .unwrap_err()
        .contains("unresolved"));
}

#[test]
fn real_spec_contains_every_required_operation_and_shrinks() {
    let document: Value = serde_json::from_str(include_str!("../openapi.json")).unwrap();
    let filtered =
        openapi_filter::filter_openapi(&document, openapi_filter::REQUIRED_OPERATION_IDS).unwrap();

    assert_eq!(
        operation_ids(&filtered).len(),
        openapi_filter::REQUIRED_OPERATION_IDS.len()
    );
    assert!(component_count(&filtered) < component_count(&document));
    assert!(
        serde_json::to_vec(&filtered).unwrap().len() < serde_json::to_vec(&document).unwrap().len()
    );
}

fn operation_ids(document: &Value) -> Vec<&str> {
    let mut operation_ids = Vec::new();
    for path in document["paths"].as_object().unwrap().values() {
        for operation in path.as_object().unwrap().values() {
            if let Some(operation_id) = operation.get("operationId").and_then(Value::as_str) {
                operation_ids.push(operation_id);
            }
        }
    }
    operation_ids.sort_unstable();
    operation_ids
}

fn component_count(document: &Value) -> usize {
    document["components"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_object)
        .map(serde_json::Map::len)
        .sum()
}
