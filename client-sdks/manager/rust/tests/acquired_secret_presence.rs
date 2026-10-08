use alien_manager_api::types::AcquiredDeploymentResponse;
use serde_json::{json, Value};

#[test]
fn acquired_raw_config_preserves_absent_empty_and_populated_secret_presence() {
    for presence in [None, Some(json!([])), Some(json!(["credential"]))] {
        let mut config = json!({
            "environmentVariables": {
                "variables": [], "hash": "example", "createdAt": "2026-01-01T00:00:00Z"
            },
            "inputValues": { "enableFeature": false }
        });
        if let Some(ids) = presence {
            config["storedSecretInputIds"] = ids;
        }
        let original = json!({
            "deployment": {
                "id": "example",
                "deploymentConfig": config
            },
            "executionClaim": null
        });
        let acquired: AcquiredDeploymentResponse =
            serde_json::from_value(original.clone()).unwrap();
        // This boundary transports JSON; resource/config validation belongs to core.
        let raw: &Value = &acquired.deployment;
        let forwarded_config = raw.get("deploymentConfig").cloned().unwrap();
        assert_eq!(forwarded_config, original["deployment"]["deploymentConfig"]);
        let encoded = serde_json::to_value(&acquired).unwrap();
        assert_eq!(encoded["deployment"], original["deployment"]);
        assert_eq!(
            encoded["deployment"]["deploymentConfig"].get("storedSecretInputIds"),
            original["deployment"]["deploymentConfig"].get("storedSecretInputIds")
        );
        let again: AcquiredDeploymentResponse = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(again).unwrap(), encoded);
    }
}
