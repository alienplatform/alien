use super::*;

#[test]
fn runtime_values_include_valid_agent_encryption_key() {
    let values = runtime_values().expect("runtime values should build");
    let key = values
        .pointer("/encryption/key")
        .and_then(Value::as_str)
        .expect("runtime encryption key should be present");

    assert_eq!(key.len(), 64);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn runtime_values_use_exact_operator_image() {
    temp_env::with_var(
        "ALIEN_TEST_OVERRIDE_OPERATOR_IMAGE",
        Some("ghcr.io/alienplatform/alien-operator:test-head"),
        || {
            let values =
                runtime_values().expect("runtime values should use the requested operator image");
            let yaml = to_helm_values_yaml(&serde_json::json!({
                "runtime": values,
            }))
            .expect("runtime values should render as Helm values");
            let rendered: Value =
                serde_yaml::from_str(&yaml).expect("rendered Helm values should parse");

            assert_eq!(
                rendered.pointer("/runtime/image/repository"),
                Some(&Value::from("ghcr.io/alienplatform/alien-operator"))
            );
            assert_eq!(
                rendered.pointer("/runtime/image/tag"),
                Some(&Value::from("test-head"))
            );
            assert_eq!(
                rendered.pointer("/runtime/image/pullPolicy"),
                Some(&Value::from("IfNotPresent"))
            );
        },
    );
}

#[test]
fn runtime_values_preserve_existing_pod_labels() {
    let mut values = serde_json::json!({
        "runtime": {
            "podLabels": {
                "azure.workload.identity/use": "true"
            }
        }
    });
    let values_object = values.as_object_mut().expect("values object");
    merge_runtime_values(
        values_object,
        serde_json::json!({
            "image": {
                "repository": "ghcr.io/alienplatform/alien-operator",
                "tag": "test",
                "pullPolicy": "IfNotPresent"
            },
            "encryption": {
                "key": "abcd"
            }
        }),
    )
    .expect("runtime values should merge");

    assert_eq!(
        values.pointer("/runtime/podLabels/azure.workload.identity~1use"),
        Some(&Value::from("true"))
    );
    assert_eq!(
        values.pointer("/runtime/image/tag"),
        Some(&Value::from("test"))
    );
}

#[test]
fn manager_fetch_values_keep_chart_service_routes() {
    let mut values = serde_json::json!({
        "serviceAccounts": {},
        "stackSettings": {},
    });
    let values_object = values.as_object_mut().expect("values object");
    let chart_values = serde_json::json!({
        "services": {
            "alien-rs-worker": {
                "type": "clusterIp",
                "port": 80,
                "targetPort": 8080,
                "component": "worker",
            },
        },
    });

    merge_chart_service_values(values_object, &chart_values).expect("chart services should merge");

    assert_eq!(
        values.pointer("/services/alien-rs-worker/targetPort"),
        Some(&Value::from(8080))
    );
    assert_eq!(
        values.pointer("/services/alien-rs-worker/component"),
        Some(&Value::from("worker"))
    );
}

#[test]
fn manager_fetch_values_merge_empty_service_map_from_setup() {
    let mut values = serde_json::json!({
        "services": {},
    });
    let values_object = values.as_object_mut().expect("values object");
    let chart_values = serde_json::json!({
        "services": {
            "alien-rs-worker": {
                "type": "clusterIp",
                "port": 80,
                "targetPort": 8080,
                "component": "worker",
            },
        },
    });

    merge_chart_service_values(values_object, &chart_values)
        .expect("chart services should merge into an empty setup map");

    assert_eq!(
        values.pointer("/services/alien-rs-worker/port"),
        Some(&Value::from(80))
    );
}

#[test]
fn manager_fetch_values_preserve_service_overrides() {
    let mut values = serde_json::json!({
        "services": {
            "alien-rs-worker": {
                "type": "nodePort",
                "port": 8081,
            },
        },
    });
    let values_object = values.as_object_mut().expect("values object");
    let chart_values = serde_json::json!({
        "services": {
            "alien-rs-worker": {
                "type": "clusterIp",
                "port": 80,
                "targetPort": 8080,
                "component": "worker",
            },
        },
    });

    merge_chart_service_values(values_object, &chart_values)
        .expect("chart defaults should merge under explicit overrides");

    assert_eq!(
        values.pointer("/services/alien-rs-worker/type"),
        Some(&Value::from("nodePort"))
    );
    assert_eq!(
        values.pointer("/services/alien-rs-worker/port"),
        Some(&Value::from(8081))
    );
    assert_eq!(
        values.pointer("/services/alien-rs-worker/targetPort"),
        Some(&Value::from(8080))
    );
}
