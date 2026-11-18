//! What a custom binding is checked against: its type's outputs schema, read as JSON Schema, with
//! a field the schema doesn't name refused, and any keyword the check can't read refusing the
//! binding instead of being skipped.

use alien_core::{ExternalBinding, ResourceTypeDescriptor};
use serde_json::{json, Value};

fn descriptor(outputs: Value) -> ResourceTypeDescriptor {
    serde_json::from_value(json!({
        "lifecycles": ["frozen"],
        "holdsData": false,
        "setup": { "cloudformation": [], "terraform": [], "helm": ["kubernetes"], "importers": [] },
        "controllerPlatforms": [],
        "permissionSets": [],
        "link": { "permissionSets": [] },
        "schemas": { "config": {}, "outputs": outputs }
    }))
    .unwrap()
}

fn check(outputs: Value, fields: Value) -> Result<(), String> {
    let mut binding = json!({ "type": "acme/dns-zone", "service": "route53" });
    binding
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    let ExternalBinding::Custom(custom) =
        serde_json::from_value::<ExternalBinding>(binding).unwrap()
    else {
        panic!("not custom")
    };
    custom.validate(&descriptor(outputs))
}

/// Each binding breaks its schema the way JSON Schema reads it.
#[test]
fn a_binding_that_breaks_its_outputs_schema_is_refused() {
    for (case, outputs, fields, problem) in [
        (
            "a $ref into $defs",
            json!({ "type": "object", "required": ["zoneId"], "properties": { "zoneId": { "type": "string" }, "ttl": { "$ref": "#/$defs/Ttl" } }, "$defs": { "Ttl": { "type": "integer" } } }),
            json!({ "zoneId": "Z1", "ttl": "sixty" }),
            "'ttl' is string, where the outputs schema says integer",
        ),
        (
            "an enum",
            json!({ "type": "object", "properties": { "zoneId": { "type": "string", "enum": ["Z1"] } } }),
            json!({ "zoneId": "Z2" }),
            "'zoneId' is \"Z2\", which is not one of [\"Z1\"]",
        ),
        (
            "a nested object's required field",
            json!({ "type": "object", "properties": { "zone": { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } } } }),
            json!({ "zone": {} }),
            "'zone' has no 'id'",
        ),
        (
            "an additionalProperties schema",
            json!({ "type": "object", "properties": { "zoneId": { "type": "string" } }, "additionalProperties": { "type": "string" } }),
            json!({ "zoneId": "Z1", "extra": 5 }),
            "'extra' is number, where the outputs schema says string",
        ),
        (
            "utoipa's Option<Struct>, a oneOf of null and a $ref",
            json!({ "type": "object", "properties": { "zone": { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Z" }] } }, "$defs": { "Z": { "type": "object" } } }),
            json!({ "zone": 7 }),
            "'zone' matches none of its alternatives",
        ),
        (
            "a const",
            json!({ "type": "object", "properties": { "kind": { "const": "zone" } } }),
            json!({ "kind": "record" }),
            "'kind' is \"record\", where the outputs schema says \"zone\"",
        ),
        (
            "an array's items",
            json!({ "type": "object", "properties": { "servers": { "type": "array", "items": { "type": "string" } } } }),
            json!({ "servers": ["ns1", 2] }),
            "'servers[1]' is number, where the outputs schema says string",
        ),
        (
            "a field the schema doesn't name",
            json!({ "type": "object", "properties": { "zoneId": { "type": "string" } } }),
            json!({ "zoneId": "Z1", "zoneID": "Z1" }),
            "'zoneID' is not one of the outputs its schema names",
        ),
        (
            "a bound Rust's types imply",
            json!({ "type": "object", "properties": { "ttl": { "type": "integer", "format": "int32", "minimum": 0 } } }),
            json!({ "ttl": -1 }),
            "'ttl' is -1, where the outputs schema says minimum 0",
        ),
    ] {
        let error = check(outputs, fields).unwrap_err();
        assert!(error.contains(problem), "{case}: {error}");
    }
}

/// A keyword the check doesn't read refuses the binding, whatever its value.
#[test]
fn a_schema_keyword_the_check_cannot_read_refuses_the_binding() {
    for (outputs, keyword) in [
        (
            json!({ "type": "object", "properties": { "zoneId": { "type": "string", "not": { "const": "x" } } } }),
            "'not'",
        ),
        (
            json!({ "type": "object", "if": { "required": ["a"] } }),
            "'if'",
        ),
        (
            json!({ "type": "object", "properties": { "zone": { "$ref": "https://example.com/zone.json" } } }),
            "the reference",
        ),
        (
            json!({ "type": "object", "properties": { "zone": { "type": "string" }, "zoneId": { "type": "string", "uniqueItems": true } } }),
            "'uniqueItems'",
        ),
        (
            json!({ "type": "object", "properties": ["zoneId"] }),
            "'properties' that is not an object",
        ),
        (
            json!({ "type": "object", "required": "zoneId" }),
            "'required' that is not a list of names",
        ),
        (
            json!({ "type": "object", "required": [7] }),
            "'required' that is not a list of names",
        ),
    ] {
        let error = check(outputs, json!({ "zoneId": "Z1", "zone": "z" })).unwrap_err();
        assert!(
            error.contains(keyword) && error.contains("which Alien can't check a binding against"),
            "{error}"
        );
    }
}

/// A malformed `properties` or `required` refuses the binding even where the value is not an
/// object.
#[test]
fn a_malformed_object_schema_refuses_a_value_of_any_type() {
    for (outputs, keyword) in [
        (
            json!({ "type": "object", "properties": { "n": { "properties": ["x"] } } }),
            "'properties' that is not an object",
        ),
        (
            json!({ "type": "object", "properties": { "n": { "required": "x" } } }),
            "'required' that is not a list of names",
        ),
    ] {
        let error = check(outputs, json!({ "n": 4 })).unwrap_err();
        assert!(
            error.contains(keyword) && error.contains("which Alien can't check a binding against"),
            "{error}"
        );
    }
}

/// `#[serde(flatten)]` writes an `allOf` of the flattened struct and the struct's own fields. A
/// field one branch names is named for all of them; a field none names is still refused.
#[test]
fn a_flattened_outputs_schema_names_the_fields_of_every_branch() {
    let outputs = json!({
        "allOf": [
            { "$ref": "#/$defs/Common" },
            { "type": "object", "required": ["zoneId"], "properties": { "zoneId": { "type": "string" } } }
        ],
        "$defs": { "Common": { "type": "object", "required": ["region"], "properties": { "region": { "type": "string" } } } }
    });
    check(
        outputs.clone(),
        json!({ "zoneId": "Z1", "region": "us-east-1" }),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let error = check(
        outputs.clone(),
        json!({ "zoneId": "Z1", "region": "us-east-1", "zoneID": "Z1" }),
    )
    .unwrap_err();
    assert!(
        error.contains("'zoneID' is not one of the outputs its schema names"),
        "{error}"
    );
    let error = check(outputs, json!({ "zoneId": "Z1", "region": 7 })).unwrap_err();
    assert!(
        error.contains("'region' is number, where the outputs schema says string"),
        "{error}"
    );
}

#[test]
fn a_binding_that_matches_its_outputs_schema_is_accepted() {
    for (outputs, fields) in [
        // An integer is any number without a fraction, as JSON Schema counts it.
        (
            json!({ "type": "object", "properties": { "ttl": { "type": "integer" } } }),
            json!({ "ttl": 1.0 }),
        ),
        // With no properties named, any field is an output.
        (
            json!({ "type": "object" }),
            json!({ "anything": { "secretRef": { "name": "n", "key": "k" } } }),
        ),
        (
            json!({ "type": "object", "properties": { "zone": { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Z" }] } }, "$defs": { "Z": { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } } } }),
            json!({ "zone": { "id": "Z1" } }),
        ),
        (
            json!({ "type": "object", "properties": { "zone": { "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/Z" }] } }, "$defs": { "Z": { "type": "object" } } }),
            json!({ "zone": null }),
        ),
    ] {
        check(outputs.clone(), fields.clone())
            .unwrap_or_else(|error| panic!("{outputs} {fields}: {error}"));
    }
}

/// Only a tag of the form `<extension>/<type>` reads as custom, and a custom binding names its
/// service.
#[test]
fn what_reads_as_a_custom_binding() {
    let read = |raw: &str| {
        serde_json::from_str::<ExternalBinding>(raw).map(|b| b.binding_type().to_string())
    };
    assert_eq!(
        read(r#"{"type":"acme/a/b","service":"s3"}"#).unwrap(),
        "acme/a/b"
    );
    for raw in [
        r#"{"type":"acme/dns-zone","zoneId":"Z1"}"#,
        r#"{"type":"acme/dns-zone","service":7,"zoneId":"Z1"}"#,
        r#"{"type":"Storage","service":"s3","bucketName":"b"}"#,
        r#"{"type":"experimental/x","service":"s3"}"#,
        r#"{"type":"/x","service":"s3"}"#,
        r#"{"type":"acme/","service":"s3"}"#,
        r#"{"service":"s3","bucketName":"b"}"#,
    ] {
        assert!(read(raw).is_err(), "{raw} read as {:?}", read(raw));
    }
}
