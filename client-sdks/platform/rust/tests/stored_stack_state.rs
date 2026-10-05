use alien_platform_api::types::DeploymentStackState;
use serde_json::{json, Value};

fn stored_state(lifecycle: Value, controller_platform: Value) -> Value {
    json!({
        "platform": "aws",
        "resourcePrefix": "demo",
        "resources": {
            "archive": {
                "type": "demo-resource",
                "config": {
                    "id": "archive",
                    "type": "demo-resource",
                    "settings": { "nested": [null, true, 42, "value"] },
                    "futureOption": true
                },
                "status": "running",
                "lifecycle": lifecycle,
                "controllerPlatform": controller_platform
            }
        }
    })
}

#[test]
fn stored_resource_scalar_enums_and_opaque_config_survive_sdk_decoding() {
    for lifecycle in [json!("frozen"), json!("live"), Value::Null] {
        for platform in [json!("aws"), json!("local"), Value::Null] {
            let original = stored_state(lifecycle.clone(), platform.clone());
            let decoded: Option<DeploymentStackState> = serde_json::from_value(original.clone())
                .expect(
                    "stored resource metadata should decode before resource-specific validation",
                );
            let encoded = serde_json::to_value(decoded).unwrap();
            let resource = &encoded["resources"]["archive"];
            assert_eq!(
                resource["config"],
                original["resources"]["archive"]["config"]
            );
            assert_eq!(resource["type"], "demo-resource");
            assert_eq!(resource["status"], "running");
            // Optional null fields may serialize as absent; both have null lookup values.
            assert_eq!(resource["lifecycle"], lifecycle);
            assert_eq!(resource["controllerPlatform"], platform);
        }
    }
}

#[test]
fn stored_resource_scalar_enums_reject_malformed_values() {
    for field in ["lifecycle", "controllerPlatform"] {
        for invalid in [
            json!("unknown"),
            json!({}),
            json!(true),
            json!(42),
            json!([]),
        ] {
            let mut state = stored_state(json!("frozen"), json!("aws"));
            state["resources"]["archive"][field] = invalid;
            assert!(
                serde_json::from_value::<Option<DeploymentStackState>>(state).is_err(),
                "invalid {field} must fail"
            );
        }
    }
}

#[test]
fn empty_or_null_stored_stack_state_remains_readable() {
    for stack in [
        Value::Null,
        json!({ "platform": "aws", "resourcePrefix": "demo", "resources": {} }),
    ] {
        let state: Option<DeploymentStackState> = serde_json::from_value(stack.clone()).unwrap();
        let encoded = serde_json::to_value(state).unwrap();
        assert_eq!(encoded, stack);
    }
}
