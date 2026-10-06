// The filtered SDK exposes the same heartbeat contract through resource details;
// the full SDK also sends it in reconciliation requests.
#[cfg(not(feature = "full-api"))]
use alien_platform_api::types::GetResourceDeploymentDetailResponseHeartbeatHeartbeat as Heartbeat;
#[cfg(feature = "full-api")]
use alien_platform_api::types::SyncReconcileRequestResourceHeartbeatsItem as Heartbeat;
use serde_json::{json, Value};

fn sandbox_heartbeats() -> Vec<Value> {
    [
        ("aws", json!({"backend":"awsMicrovm", "imageIdentifier":"demo-image", "imageState":"Active"})),
        ("azure", json!({"backend":"azureSandboxGroup", "sandboxGroup":"demo-group", "provisioningState":"Succeeded"})),
        ("kubernetes", json!({"backend":"kubernetesPods", "namespace":"demo", "activeSessions":2, "idlePods":1})),
        ("local", json!({"backend":"local", "activeSessions":1, "routeServing":true})),
    ].into_iter().map(|(backend, mut data)| {
        data["status"] = json!({"health":"healthy", "lifecycle":"running", "stale":false, "partial":false, "collectionIssues":[], "message":"ready"});
        json!({"backend":backend, "controllerPlatform":"aws", "data":{"resourceType":"sandbox", "data":data}, "observedAt":"2026-01-01T00:00:00Z", "raw":[], "resourceId":"demo", "resourceType":"sandbox"})
    }).collect()
}

fn without_nulls(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key, without_nulls(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

#[test]
fn sandbox_heartbeat_backends_preserve_complete_payloads() {
    for original in sandbox_heartbeats() {
        let decoded: Heartbeat = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(
            without_nulls(serde_json::to_value(decoded).unwrap()),
            original
        );
    }
}

#[test]
fn sandbox_heartbeat_tags_and_required_fields_are_validated() {
    for original in sandbox_heartbeats() {
        for pointer in ["/data/resourceType", "/data/data/backend"] {
            let mut invalid = original.clone();
            *invalid.pointer_mut(pointer).unwrap() = json!("unknown");
            assert!(
                serde_json::from_value::<Heartbeat>(invalid).is_err(),
                "invalid {pointer}"
            );
        }
        for (pointer, field) in [
            ("/data", "resourceType"),
            ("/data/data", "backend"),
            ("/data/data", "status"),
            ("/data/data/status", "stale"),
        ] {
            let mut invalid = original.clone();
            invalid
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                serde_json::from_value::<Heartbeat>(invalid).is_err(),
                "missing {pointer}/{field}"
            );
        }
    }
}
