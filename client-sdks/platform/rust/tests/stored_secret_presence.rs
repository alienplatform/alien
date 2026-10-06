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
fn absent_and_empty_stored_secret_ids_remain_distinct() {
    for original in [config(), {
        let mut config = config();
        config["storedSecretInputIds"] = json!([]);
        config
    }] {
        let acquired: SyncAcquireResponseDeployment =
            serde_json::from_value(acquired_deployment(original.clone())).unwrap();
        let rebuilt: DeploymentConfig =
            alien_platform_api::types::builder::DeploymentConfig::from(acquired.config.clone())
                .stored_secret_input_ids(
                    original
                        .get("storedSecretInputIds")
                        .map(|value| serde_json::from_value::<Vec<String>>(value.clone()).unwrap()),
                )
                .try_into()
                .unwrap();
        let encoded = serde_json::to_value(&acquired.config).unwrap();
        assert_eq!(serde_json::to_value(rebuilt).unwrap(), encoded);
        assert_eq!(
            encoded.get("storedSecretInputIds"),
            original.get("storedSecretInputIds")
        );
        let response = serde_json::to_value(&acquired).unwrap();
        assert_eq!(
            response["config"].get("storedSecretInputIds"),
            original.get("storedSecretInputIds")
        );
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

#[test]
fn null_presence_decodes_like_legacy_omission() {
    // The API schema remains nonnullable; Rust matches core's permissive legacy decoder.
    let mut original = config();
    original["storedSecretInputIds"] = Value::Null;
    let acquired: SyncAcquireResponseDeployment =
        serde_json::from_value(acquired_deployment(original)).unwrap();
    assert!(
        serde_json::to_value(acquired.config)
            .unwrap()
            .get("storedSecretInputIds")
            .is_none()
    );
}

#[test]
fn acquired_catalog_preserves_runtime_isolation_generation_through_core() {
    for generation in [None, Some(1_u32)] {
        let mut catalog = json!({
            "channel": "stable",
            "machineImageVersion": "1",
            "horizondVersion": "1",
            "gitSha": "abc123",
            "createdAt": "2026-01-01T00:00:00Z",
            "baseImage": { "name": "linux", "version": "1" },
            "horizondArtifacts": {
                "linux-arm64": { "url": "https://example.com/arm64", "sha256": "a".repeat(64) },
                "linux-amd64": { "url": "https://example.com/amd64", "sha256": "b".repeat(64) }
            },
            "aws": { "amis": { "arm64": { "us-east-1": "ami-example" } } },
            "gcp": { "images": { "arm64": { "sourceImage": "example-image" } } },
            "azure": { "images": { "arm64": { "imageVersionId": "example-version" } } }
        });
        if let Some(generation) = generation {
            catalog["runtimeIsolationGeneration"] = json!(generation);
            for artifact in catalog["horizondArtifacts"]
                .as_object_mut()
                .unwrap()
                .values_mut()
            {
                artifact["runtimeIsolationGeneration"] = json!(generation);
            }
        }
        let expected: alien_core::HorizonMachineImage =
            serde_json::from_value(catalog.clone()).unwrap();
        let mut original = config();
        original["computeBackend"] = json!({
            "type": "horizon",
            "url": "https://example.com",
            "clusters": {},
            "horizonMachineImage": catalog
        });
        let acquired: SyncAcquireResponseDeployment =
            serde_json::from_value(acquired_deployment(original)).unwrap();
        let encoded = serde_json::to_value(acquired).unwrap();
        let core: alien_core::DeploymentConfig =
            serde_json::from_value(encoded["config"].clone()).unwrap();
        let alien_core::ComputeBackend::Horizon(backend) = core.compute_backend.unwrap();
        let actual = backend.horizon_machine_image.unwrap();
        assert_eq!(actual.runtime_isolation_generation, generation.unwrap_or(0));
        assert_eq!(actual.horizond_artifacts.len(), 2);
        for artifact in actual.horizond_artifacts.values() {
            assert_eq!(
                artifact.runtime_isolation_generation,
                generation.unwrap_or(0)
            );
        }
        assert_eq!(actual, expected);
    }
}
