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
fn preserves_inline_union_branches_constraints_and_annotations() {
    for keyword in ["anyOf", "oneOf"] {
        let union = json!({
            keyword: [
                { "type": "string", "minLength": 3, "enum": ["alpha", "beta"] },
                { "type": "integer", "minimum": 1, "maximum": 8 },
                { "type": "boolean" }
            ],
            "description": "A constrained value.",
            "default": "alpha"
        });
        let mut distinct = union.clone();
        distinct[keyword][1]["maximum"] = json!(9);
        let document = json!({
            "paths": {
                "/values": {
                    "get": {
                        "operationId": "values",
                        "responses": {
                            "200": {
                                "description": "ok",
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/Values" }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Values": {
                        "type": "object",
                        "properties": {
                            "first": union.clone(),
                            "second": union.clone(),
                            "distinct": distinct.clone()
                        },
                        "required": ["first", "second"]
                    }
                }
            }
        });

        let filtered = openapi_filter::filter_openapi(&document, &["values"]).unwrap();
        let properties = &filtered["components"]["schemas"]["Values"]["properties"];
        assert_eq!(properties["first"], union);
        assert_eq!(properties["second"], union);
        assert_eq!(properties["distinct"], distinct);
        assert_eq!(
            filtered["components"]["schemas"]["Values"]["required"],
            json!(["first", "second"])
        );
        assert_eq!(
            filtered,
            openapi_filter::filter_openapi(&document, &["values"]).unwrap()
        );
    }
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
fn removes_additional_properties_false_from_schemas_but_not_from_literal_data() {
    let strict = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "nested": {
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "additionalProperties": false
            },
            "labels": { "type": "object", "additionalProperties": { "type": "string" } }
        },
        "additionalProperties": false
    });
    let literal = json!({ "additionalProperties": false });
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
                                    "properties": { "body": { "type": "string" } },
                                    "additionalProperties": false
                                },
                                "example": literal.clone()
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Strict" }
                                }
                            }
                        },
                        "default": {
                            "description": "error",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": { "code": { "type": "string" } },
                                        "additionalProperties": false
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
                "Strict": {
                    "allOf": [strict],
                    "example": literal.clone()
                }
            }
        }
    });

    for opened in [
        openapi_filter::filter_openapi(&document, &["kept"]).unwrap(),
        openapi_filter::normalize_openapi(&document).unwrap(),
    ] {
        let mut strict_schemas = Vec::new();
        collect_strict_schemas(&opened, "", &mut strict_schemas);
        assert!(strict_schemas.is_empty(), "{strict_schemas:?}");
        assert_eq!(
            opened
                .pointer("/components/schemas/Strict/example")
                .unwrap(),
            &literal
        );
        assert_eq!(
            opened
                .pointer("/paths/~1a/post/requestBody/content/application~1json/example")
                .unwrap(),
            &literal
        );
        let labels = opened
            .pointer("/components/schemas/Strict/allOf/0/properties/labels/additionalProperties")
            .unwrap();
        assert_eq!(labels, &json!({ "type": "string" }));
    }
}

#[test]
fn package_type_enums_in_responses_become_open_strings() {
    let package_type = json!({ "type": "string", "enum": ["cloudformation", "terraform"] });
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
                                            { "$ref": "#/components/schemas/CapabilityMaterialization" },
                                            { "$ref": "#/components/schemas/DeploymentLinkSetupResponse" }
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
                "CapabilityMaterialization": {
                    "type": "object",
                    "properties": {
                        "packages": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "type": package_type.clone(),
                                    "status": { "type": "string", "enum": ["ready", "failed"] }
                                }
                            }
                        }
                    }
                },
                "DeploymentLinkSetupResponse": {
                    "type": "object",
                    "properties": {
                        "visiblePackageTypes": { "type": "array", "items": package_type.clone() }
                    }
                }
            }
        }
    });

    for opened in [
        openapi_filter::filter_openapi(&document, &["kept"]).unwrap(),
        openapi_filter::normalize_openapi(&document).unwrap(),
    ] {
        let packages = "/components/schemas/CapabilityMaterialization/properties/packages/items";
        assert_eq!(
            opened
                .pointer(&format!("{packages}/properties/type"))
                .unwrap(),
            &json!({ "type": "string" })
        );
        assert_eq!(
            opened
                .pointer(&format!("{packages}/properties/status/enum"))
                .unwrap(),
            &json!(["ready", "failed"])
        );
        assert_eq!(
            opened
                .pointer("/components/schemas/DeploymentLinkSetupResponse/properties/visiblePackageTypes/items")
                .unwrap(),
            &json!({ "type": "string" })
        );
    }

    let mut moved = document;
    *moved
        .pointer_mut(
            "/components/schemas/DeploymentLinkSetupResponse/properties/visiblePackageTypes/items",
        )
        .unwrap() = json!({ "$ref": "#/components/schemas/PackageType" });
    moved["components"]["schemas"]["PackageType"] = package_type;
    for error in [
        openapi_filter::filter_openapi(&moved, &["kept"]).unwrap_err(),
        openapi_filter::normalize_openapi(&moved).unwrap_err(),
    ] {
        assert!(error.contains("visiblePackageTypes"), "{error}");
    }
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
    let normalized = openapi_filter::normalize_openapi(&document).unwrap();
    let filtered =
        openapi_filter::filter_openapi(&document, openapi_filter::REQUIRED_OPERATION_IDS).unwrap();

    assert_eq!(
        operation_ids(&filtered).len(),
        openapi_filter::REQUIRED_OPERATION_IDS.len()
    );
    // Extracting shared schemas can increase the component count while reducing
    // the graph size. Count retained source components, not extracted identities.
    let source_schemas = document["components"]["schemas"].as_object().unwrap();
    let retained_source_schemas = filtered["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| source_schemas.contains_key(*name))
        .count();
    assert!(retained_source_schemas < source_schemas.len());
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
        assert_eq!(schema, normalized.pointer(pointer).unwrap());
    }

    let compute = filtered
        .pointer(
            "/components/schemas/NewDeploymentRequest/properties/stackSettings/properties/compute",
        )
        .unwrap();
    assert!(compute.get("anyOf").is_some());
    assert!(compute.get("$ref").is_none());

    let shared_components = filtered["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| name.starts_with("AlienSharedObject"))
        .count();
    assert!(shared_components > 100);

    let mut strict_schemas = Vec::new();
    collect_strict_schemas(&filtered, "", &mut strict_schemas);
    assert!(strict_schemas.is_empty(), "{strict_schemas:?}");
    for pointer in [
        "/components/schemas/CapabilityMaterialization/properties/packages/items/properties/type",
        "/components/schemas/DeploymentLinkSetupResponse/properties/visiblePackageTypes/items",
    ] {
        let schema = filtered.pointer(pointer).unwrap();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("enum").is_none());
    }
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

fn collect_strict_schemas(value: &Value, pointer: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let literal = matches!(key.as_str(), "example" | "examples")
                    || (key == "default"
                        && !pointer.ends_with("/responses")
                        && !pointer.ends_with("/properties"));
                if literal {
                    continue;
                }
                let child_pointer = format!("{pointer}/{key}");
                if key == "additionalProperties" && child == &Value::Bool(false) {
                    found.push(child_pointer);
                } else {
                    collect_strict_schemas(child, &child_pointer, found);
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_strict_schemas(child, &format!("{pointer}/{index}"), found);
            }
        }
        _ => {}
    }
}

#[test]
fn binding_normalization_leaves_overlapping_or_optional_tags_unchanged() {
    let branch = json!({
        "type": "object", "required": ["type"],
        "properties": {"type": {"type": "string", "enum": ["storage"]}}
    });
    for other in [
        branch.clone(),
        json!({
            "type": "object",
            "properties": {"type": {"type": "string", "enum": ["queue"]}}
        }),
    ] {
        let document = json!({"components": {"schemas": {
            "ExternalBinding": {"anyOf": [branch, other]}
        }}});
        assert_eq!(
            openapi_filter::normalize_openapi(&document).unwrap(),
            document
        );
    }
}

#[test]
fn constrained_json_alternatives_are_not_simplified() {
    let union = json!({"anyOf": [
        {"$ref": "#/components/schemas/JsonValue"}, {"type": "string"}
    ]});
    let document = json!({"components": {"schemas": {
        "JsonValue": {"type": "object"}, "Value": union,
        "NullableValue": {"anyOf": [
            {"allOf": [{"$ref": "#/components/schemas/JsonValue"}], "nullable": true},
            {"type": "string"}
        ]}
    }}});
    assert_eq!(
        openapi_filter::normalize_openapi(&document).unwrap(),
        document
    );
}

#[test]
fn both_modes_preserve_constraints_when_normalizing_disjoint_nullable_enums() {
    let scalar = json!({
        "anyOf": [
            {"type": "string", "enum": ["frozen", "live"], "description": "Lifecycle"},
            {"type": "string", "nullable": true, "enum": [null]}
        ],
        "description": "Optional lifecycle", "default": null
    });
    let mut expected = scalar.clone();
    expected["oneOf"] = expected.as_object_mut().unwrap().remove("anyOf").unwrap();
    let mut overlapping = scalar.clone();
    overlapping["anyOf"][0]["nullable"] = json!(true);
    let mut constrained_null = scalar.clone();
    constrained_null["anyOf"][1]["maxLength"] = json!(0);
    let mut reference_sibling = scalar.clone();
    reference_sibling["anyOf"][0]["$ref"] = json!("#/components/schemas/Other");
    let mut existing_one_of = scalar.clone();
    existing_one_of["oneOf"] = json!([{"type": "string"}]);
    let document = json!({
        "paths": {"/state": {"get": {
            "operationId": "state", "responses": {"200": {
                "description": "ok", "content": {"application/json": {
                    "schema": {"$ref": "#/components/schemas/State"}
                }}
            }}
        }}},
        "components": {"schemas": {
            "State": {"type": "object", "properties": {
                "lifecycle": scalar, "overlapping": overlapping,
                "constrainedNull": constrained_null, "referenceSibling": reference_sibling,
                "existingOneOf": existing_one_of
            }},
            "Other": {"type": "string"}
        }}
    });
    for normalized in [
        openapi_filter::normalize_openapi(&document).unwrap(),
        openapi_filter::filter_openapi(&document, &["state"]).unwrap(),
    ] {
        let properties = &normalized["components"]["schemas"]["State"]["properties"];
        assert_eq!(properties["lifecycle"], expected);
        for name in [
            "overlapping",
            "constrainedNull",
            "referenceSibling",
            "existingOneOf",
        ] {
            assert_eq!(
                properties[name],
                document["components"]["schemas"]["State"]["properties"][name]
            );
        }
    }
}

#[test]
fn both_modes_normalize_only_proven_disjoint_required_tags() {
    let branch = |tags: Value| {
        json!({"allOf": [
            {"type": "object", "required": ["payload"], "properties": {"payload": {"type": "integer"}}},
            {"type": "object", "required": ["backend"], "properties": {"backend": {"type": "string", "enum": tags}}}
        ]})
    };
    let union = json!({
        "anyOf": [branch(json!(["first", "second"])), branch(json!(["third"]))],
        "description": "Tagged payload", "minProperties": 2,
        "example": {"anyOf": [{"type": "example"}]}
    });
    let mut expected = union.clone();
    expected["oneOf"] = expected.as_object_mut().unwrap().remove("anyOf").unwrap();
    let mut cases = serde_json::Map::new();
    cases.insert("disjoint".into(), union.clone());
    let mut overlapping = union.clone();
    overlapping["anyOf"][1] = branch(json!(["second", "third"]));
    cases.insert("overlapping".into(), overlapping);
    for (name, value) in [("optional", json!([])), ("missing", Value::Null)] {
        let mut schema = union.clone();
        schema["anyOf"][1]["allOf"][1]["required"] = value;
        cases.insert(name.into(), schema);
    }
    let mut nullable = union.clone();
    nullable["anyOf"][1]["nullable"] = json!(true);
    cases.insert("nullable".into(), nullable);
    let mut reference = union.clone();
    reference["anyOf"][1]["allOf"][1]["$ref"] = json!("#/components/schemas/Other");
    cases.insert("reference".into(), reference);
    let mut outer_reference = union.clone();
    outer_reference["$ref"] = json!("#/components/schemas/Other");
    cases.insert("outerReference".into(), outer_reference);
    let mut sibling = union;
    sibling["oneOf"] = json!([{"type": "object"}]);
    cases.insert("sibling".into(), sibling);
    let document = json!({
        "paths": {"/payload": {"post": {"operationId": "payload", "responses": {
            "200": {"description": "ok", "content": {"application/json": {
                "schema": {"allOf": [{"$ref": "#/components/schemas/Payload"}], "type": "object", "properties": cases}
            }}}
        }}}},
        "components": {"schemas": {
            "Payload": {"type": "object", "properties": cases},
            "Other": {"type": "object"}
        }}
    });
    for normalized in [
        openapi_filter::normalize_openapi(&document).unwrap(),
        openapi_filter::filter_openapi(&document, &["payload"]).unwrap(),
    ] {
        // Dereference shared anonymous objects so deduplication does not obscure
        // whether the union retained every original constraint and annotation.
        let expand = |value: &Value| dereference(value, &normalized, 0);
        let inline = &normalized["paths"]["/payload"]["post"]["responses"]["200"]["content"]
            ["application/json"]["schema"];
        assert_eq!(
            expand(&inline["properties"]),
            expand(&normalized["components"]["schemas"]["Payload"]["properties"])
        );
        let properties = &normalized["components"]["schemas"]["Payload"]["properties"];
        assert_eq!(expand(&properties["disjoint"]), expand(&expected));
        for (name, original) in &cases {
            if name != "disjoint" {
                assert_eq!(expand(&properties[name]), expand(original), "{name}");
            }
        }
    }
}

#[test]
fn externally_tagged_unions_become_one_of_only_when_branches_cannot_overlap() {
    let unit = |tag: &str| json!({"type": "string", "enum": [tag]});
    let data = |tag: &str, closed: bool| {
        let mut branch = json!({
            "type": "object", "required": [tag],
            "properties": {tag: {"type": "object"}}
        });
        if closed {
            branch["additionalProperties"] = json!(false);
        }
        branch
    };
    let cases = json!({
        // A string never matches an object, so one open object branch is safe.
        "eventState": {"anyOf": [data("failed", false), unit("none"), unit("success")]},
        "closedObjects": {"anyOf": [data("extend", true), data("override", true), unit("auto")]},
        // `{"extend": {}, "override": {}}` matches both open branches.
        "openObjects": {"anyOf": [data("extend", false), data("override", false), unit("auto")]},
        "repeatedTag": {"anyOf": [data("auto", false), unit("auto")]},
        "nullable": {"anyOf": [data("failed", false), {"type": "string", "enum": ["none"], "nullable": true}]}
    });
    let document = json!({"components": {"schemas": {
        "Union": {"type": "object", "properties": cases}
    }}});
    let normalized = openapi_filter::normalize_openapi(&document).unwrap();
    let properties = &normalized["components"]["schemas"]["Union"]["properties"];
    for name in ["eventState", "closedObjects"] {
        let mut expected = cases[name].clone();
        expected["oneOf"] = expected.as_object_mut().unwrap().remove("anyOf").unwrap();
        // Normalization later drops `additionalProperties: false` from every schema.
        for branch in expected["oneOf"].as_array_mut().unwrap() {
            branch
                .as_object_mut()
                .unwrap()
                .remove("additionalProperties");
        }
        assert_eq!(properties[name], expected, "{name}");
    }
    for name in ["openObjects", "repeatedTag", "nullable"] {
        assert_eq!(properties[name], cases[name], "{name}");
    }
}

fn dereference(value: &Value, document: &Value, depth: usize) -> Value {
    assert!(depth < 64, "fixture references must not cycle");
    match value {
        Value::Object(object) => {
            if object.len() == 1 {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    return dereference(
                        document
                            .pointer(reference.strip_prefix('#').unwrap())
                            .unwrap(),
                        document,
                        depth + 1,
                    );
                }
            }
            Value::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), dereference(value, document, depth + 1)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| dereference(value, document, depth + 1))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[test]
fn secret_presence_native_type_is_narrow_and_constraint_checked_in_both_modes() {
    let document = json!({
        "openapi": "3.0.3", "info": {"title": "test", "version": "1"},
        "paths": {"/config": {"get": {"operationId": "config", "responses": {"200": {
            "description": "config", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/DeploymentConfig"}}}
        }}}}},
        "components": {"schemas": {"DeploymentConfig": {
            "type": "object", "properties": {
                "storedSecretInputIds": {"type": "array", "items": {"type": "string"}},
                "otherIds": {"type": "array", "items": {"type": "string"}}
            }
        }}}
    });
    for full in [false, true] {
        let normalize = |value: &Value| {
            if full {
                openapi_filter::normalize_openapi(value)
            } else {
                openapi_filter::filter_openapi(value, &["config"])
            }
        };
        let normalized = normalize(&document).unwrap();
        let property = "/components/schemas/DeploymentConfig/properties/storedSecretInputIds";
        let mut expected = document.pointer(property).unwrap().clone();
        expected["x-rust-type"] = json!({"crate": "std", "version": "*", "path": "std::vec::Vec", "parameters": [{"type": "string"}]});
        assert_eq!(normalized.pointer(property), Some(&expected));
        assert_eq!(
            normalized.pointer("/components/schemas/DeploymentConfig/properties/otherIds"),
            document.pointer("/components/schemas/DeploymentConfig/properties/otherIds")
        );
        for (key, constraint) in [
            ("minItems", json!(1)),
            ("nullable", json!(true)),
            ("uniqueItems", json!(true)),
        ] {
            let mut constrained = document.clone();
            constrained.pointer_mut(property).unwrap()[key] = constraint;
            assert!(normalize(&constrained).is_err(), "{key}");
        }
        let mut required = document.clone();
        required["components"]["schemas"]["DeploymentConfig"]["required"] =
            json!(["storedSecretInputIds"]);
        assert!(normalize(&required).is_err());
    }
}
