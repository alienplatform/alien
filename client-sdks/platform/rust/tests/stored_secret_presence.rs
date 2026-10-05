// Acquisition models are generated only by the full SDK.
#![cfg(feature = "full-api")]

use alien_platform_api::types::{DeploymentConfig, SyncAcquireResponseDeployment};
use serde_json::{Value, json};

fn config() -> Value {
    json!({
        "environmentVariables": {
            "variables": [], "hash": "demo", "createdAt": "2026-01-01T00:00:00Z"
        },
        "inputValues": { "enableFeature": false }
    })
}

fn acquired_deployment(config: Value) -> Value {
    json!({
        "deploymentId": format!("dep_{}", "0".repeat(28)),
        "projectId": "demo",
        "deploymentGroupId": format!("dg_{}", "0".repeat(28)),
        "operationId": null,
        "attemptId": null,
        "current": { "platform": "aws", "protocolVersion": 1, "status": "running" },
        "config": config
    })
}

#[test]
fn acquired_config_retains_stored_secret_ids_without_secret_values() {
    let mut original = config();
    original["storedSecretInputIds"] = json!(["credential"]);
    let acquired: SyncAcquireResponseDeployment =
        serde_json::from_value(acquired_deployment(original.clone())).unwrap();
    let typed_config: &DeploymentConfig = &acquired.config;
    let encoded_config = serde_json::to_value(typed_config).unwrap();
    let encoded_response = serde_json::to_value(&acquired).unwrap();

    assert_eq!(encoded_response["config"], encoded_config);
    assert_eq!(
        encoded_config["storedSecretInputIds"],
        json!(["credential"])
    );
    assert_eq!(encoded_config["inputValues"], original["inputValues"]);
    assert_eq!(
        encoded_config["environmentVariables"],
        original["environmentVariables"]
    );
    let decoded_again: SyncAcquireResponseDeployment =
        serde_json::from_value(encoded_response.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded_again).unwrap(),
        encoded_response
    );
}

#[test]
fn absent_or_empty_stored_secret_ids_remain_backward_compatible() {
    for original in [config(), {
        let mut config = config();
        config["storedSecretInputIds"] = json!([]);
        config
    }] {
        let acquired: SyncAcquireResponseDeployment =
            serde_json::from_value(acquired_deployment(original.clone())).unwrap();
        let encoded = serde_json::to_value(&acquired.config).unwrap();
        // Empty optional arrays may be omitted when the generated model serializes.
        let ids: Vec<String> = serde_json::from_value(
            encoded
                .get("storedSecretInputIds")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .unwrap();
        assert!(ids.is_empty());
        assert_eq!(encoded["inputValues"], original["inputValues"]);
        assert_eq!(
            encoded["environmentVariables"],
            original["environmentVariables"]
        );
    }
}

#[test]
fn stored_secret_ids_reject_malformed_arrays_in_acquired_configs() {
    for invalid in [
        json!([42]),
        json!(["credential", false]),
        json!([null]),
        json!("credential"),
        json!({ "credential": true }),
    ] {
        let mut config = config();
        config["storedSecretInputIds"] = invalid;
        assert!(
            serde_json::from_value::<DeploymentConfig>(config.clone()).is_err(),
            "malformed presence must fail config decoding"
        );
        assert!(
            serde_json::from_value::<SyncAcquireResponseDeployment>(acquired_deployment(config))
                .is_err(),
            "malformed presence must fail acquisition decoding"
        );
    }
}
