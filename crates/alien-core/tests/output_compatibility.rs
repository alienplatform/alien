use alien_core::{Key, KeyOutputs, ResourceOutputs, StackState};
use serde_json::json;

#[test]
fn persisted_stack_accepts_additive_key_observations_and_keeps_unavailability() {
    let value = json!({
        "platform": "aws",
        "resourcePrefix": "example",
        "resources": {
            "key": {
                "type": "key",
                "status": "running",
                "config": { "type": "key", "id": "key" },
                "outputs": {
                    "type": "key",
                    "available": false,
                    "fingerprint": { "provider": "aws", "key_arn": "example-key" },
                    "wrappingKeyId": "example-key",
                    "futureObservation": { "reason": "provider unavailable" }
                }
            }
        }
    });
    let stack: StackState = serde_json::from_value(value).expect("read newer stack observations");
    let outputs = stack
        .get_resource_outputs::<KeyOutputs>("key")
        .expect("key outputs");
    assert_eq!(outputs.available, Some(false));
    assert_eq!(outputs.wrapping_key_id, "example-key");
    assert!(!stack.resources["key"]
        .outputs
        .as_ref()
        .expect("outputs")
        .as_resource_outputs()
        .is_available());
}

#[test]
fn access_outputs_accept_additive_observations_without_losing_identity() {
    for value in [
        json!({
            "type": "resource-access",
            "resourceId": "example-role",
            "accessConfiguration": "example-access",
            "futureObservation": true
        }),
        json!({
            "type": "remote-stack-management",
            "managementResourceId": "example-role",
            "accessConfiguration": "example-access",
            "futureObservation": true
        }),
    ] {
        let outputs: ResourceOutputs =
            serde_json::from_value(value.clone()).expect("read newer outputs");
        let serialized = serde_json::to_value(outputs).expect("serialize outputs");
        assert_eq!(serialized["type"], value["type"]);
        assert_eq!(serialized["accessConfiguration"], "example-access");
    }
}

#[test]
fn unknown_configuration_and_invalid_known_outputs_still_fail() {
    assert!(serde_json::from_value::<Key>(json!({ "id": "key", "unknownInput": true })).is_err());
    assert!(serde_json::from_value::<ResourceOutputs>(json!({
        "type": "key",
        "fingerprint": { "provider": "aws", "key_arn": "example-key" },
        "wrappingKeyId": 42,
        "futureObservation": true
    }))
    .is_err());
}
