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
fn gives_repeated_anonymous_objects_stable_component_identity() {
    let repeated = json!({
        "type": "object",
        "properties": {
            "message": { "type": "string" },
            "details": {
                "type": "object",
                "properties": { "code": { "type": "string" } },
                "required": ["code"]
            }
        },
        "required": ["message"]
    });
    let document = json!({
        "paths": {
            "/a": {
                "get": {
                    "operationId": "kept",
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": { "schema": repeated.clone() }
                            }
                        }
                    }
                }
            },
            "/b": {
                "get": {
                    "operationId": "keptToo",
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Envelope" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Envelope": {
                    "type": "object",
                    "properties": { "value": repeated }
                }
            }
        }
    });

    let first = openapi_filter::filter_openapi(&document, &["kept", "keptToo"]).unwrap();
    let second = openapi_filter::filter_openapi(&document, &["kept", "keptToo"]).unwrap();
    assert_eq!(first, second);

    let path_schema = first
        .pointer("/paths/~1a/get/responses/200/content/application~1json/schema")
        .unwrap();
    let nested_schema = first
        .pointer("/components/schemas/Envelope/properties/value")
        .unwrap();
    assert_eq!(path_schema, nested_schema);
    let reference = path_schema["$ref"].as_str().unwrap();
    assert!(reference.starts_with("#/components/schemas/AlienSharedObject"));

    let component_name = reference.rsplit('/').next().unwrap();
    let extracted = &first["components"]["schemas"][component_name];
    assert_eq!(extracted["type"], "object");
    assert_eq!(extracted["properties"]["message"]["type"], "string");
    assert!(extracted.get("$ref").is_none());
}

#[test]
fn deduplication_never_rewrites_object_valued_contract_data() {
    let repeated_schema = json!({
        "type": "object",
        "properties": { "value": { "type": "string" } }
    });
    let literal_object = json!({
        "type": "object",
        "properties": { "looks": "like schema data" },
        "schema": repeated_schema.clone()
    });
    let document = json!({
        "paths": {
            "/a": {
                "post": {
                    "operationId": "kept",
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "first": repeated_schema.clone(),
                                        "second": repeated_schema
                                    },
                                    "example": literal_object.clone(),
                                    "default": literal_object.clone()
                                },
                                "example": literal_object.clone()
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Anchor" },
                                    "example": literal_object.clone()
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": { "Anchor": { "type": "string" } }
        }
    });

    let filtered = openapi_filter::filter_openapi(&document, &["kept"]).unwrap();
    let request_schema = filtered
        .pointer("/paths/~1a/post/requestBody/content/application~1json/schema")
        .unwrap();
    assert_eq!(
        request_schema["properties"]["first"],
        request_schema["properties"]["second"]
    );
    assert!(request_schema["properties"]["first"].get("$ref").is_some());
    assert_eq!(request_schema["example"], literal_object);
    assert_eq!(request_schema["default"], literal_object);
    assert_eq!(
        filtered
            .pointer("/paths/~1a/post/requestBody/content/application~1json/example")
            .unwrap(),
        &literal_object
    );
    assert_eq!(
        filtered
            .pointer("/paths/~1a/post/responses/200/content/application~1json/example")
            .unwrap(),
        &literal_object
    );
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

    for pointer in [
        "/components/schemas/ConfigureModelsRequest/properties/requirements/items",
        "/paths/~1v1~1projects/post/requestBody/content/application~1json/schema/properties/gitRepository",
    ] {
        let schema = filtered.pointer(pointer).unwrap();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("$ref").is_none());
    }

    let shared_components = filtered["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| name.starts_with("AlienSharedObject"))
        .count();
    assert!(shared_components > 100);
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
