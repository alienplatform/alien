use alien_platform_api::types::{ExternalBinding, ExternalBindings};
use serde_json::{json, Value};

#[test]
fn roundtrips_all_binding_categories_with_arbitrary_keys() {
    let bindings = json!({
        "archive/custom": {
            "type": "storage", "service": "s3", "bucketName": "demo",
            "forcePathStyle": true
        },
        "events": {
            "type": "queue", "service": "sqs", "queueUrl": "https://queue.example.test/demo"
        },
        "cache": {
            "type": "kv", "service": "redis", "connectionUrl": "redis://localhost:6379",
            "database": 2
        },
        "images": {
            "type": "artifact_registry", "service": "ecr", "repositoryPrefix": "demo",
            "pullRoleArn": "demo-pull", "pushRoleArn": "demo-push"
        },
        "secrets": {
            "type": "vault", "service": "parameter-store", "vaultPrefix": "demo"
        },
        "environment": {
            "type": "container_apps_environment", "environmentName": "demo",
            "resourceId": "demo", "resourceGroupName": "demo",
            "defaultDomain": "example.test", "staticIp": "192.0.2.1"
        },
        "database": {
            "type": "postgres", "service": "external", "host": "db.example.test",
            "port": 5432, "database": "demo", "username": "demo",
            "password": "demo-only", "sslMode": "verify-full"
        },
        "model": {
            "type": "ai", "provider": "openai",
            "apiKey": { "secretRef": { "name": "demo", "key": "api-key" } }
        }
    });
    assert_binding_roundtrip(bindings);
}

#[test]
fn roundtrips_literal_secret_reference_and_expression_binding_values() {
    for bucket in [
        json!("demo-bucket"),
        json!(null),
        json!(true),
        json!(42),
        json!(["demo", null, false]),
        json!({ "secretRef": { "name": "demo", "key": "bucket" } }),
        json!({ "Fn::Join": ["-", [{ "Ref": "BucketPrefix" }, "archive"]] }),
        json!({ "nested": { "list": [null, true, 42, "demo"] } }),
    ] {
        assert_binding_roundtrip(json!({
            "archive": { "type": "storage", "service": "s3", "bucketName": bucket }
        }));
    }
}

#[test]
fn storage_variants_reject_invalid_discriminators_and_nested_booleans() {
    for binding in [
        json!({ "service": "s3", "bucketName": "demo" }),
        json!({ "type": "storage", "service": "unknown", "bucketName": "demo" }),
        json!({ "type": "unknown", "service": "s3", "bucketName": "demo" }),
        json!({
            "type": "storage", "service": "s3", "bucketName": "demo",
            "forcePathStyle": "true"
        }),
    ] {
        assert!(serde_json::from_value::<ExternalBinding>(binding).is_err());
    }
}

fn assert_binding_roundtrip(value: Value) {
    let decoded: ExternalBindings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}
