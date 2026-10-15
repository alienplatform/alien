use alien_core::{ErrorData, Resource};
use serde_json::{json, Value};

fn resource(kind: &str, key: Option<&str>) -> Resource {
    let mut config = match kind {
        "container" => json!({
            "type": "container", "id": "cache", "links": [], "ports": [],
            "code": {"type": "image", "image": "redis:7"},
            "cpu": {"min": "0.5", "desired": "0.5"},
            "memory": {"min": "512Mi", "desired": "512Mi"},
            "permissions": "execution", "stateful": true,
            "persistentStorage": {"size": "10Gi", "mountPath": "/data"}
        }),
        _ => json!({"type": kind, "id": "data"}),
    };
    if let Some(key) = key {
        let storage = if kind == "container" {
            &mut config["persistentStorage"]
        } else {
            &mut config
        };
        storage["encryptionKey"] = json!({"type": "key", "id": key});
    }
    serde_json::from_value(config).expect("valid resource configuration")
}

#[test]
fn existing_data_resources_reject_key_addition_removal_and_replacement() {
    for kind in ["postgres", "storage", "container"] {
        for before in [None, Some("first"), Some("second")] {
            for after in [None, Some("first"), Some("second")] {
                let original = resource(kind, before);
                let desired = resource(kind, after);
                let result = original.validate_update(&desired);
                if before == after {
                    result.expect("unchanged encryption must remain updateable");
                } else {
                    let error = result
                        .expect_err("must not silently change the key protecting existing data");
                    assert!(matches!(
                        error.error,
                        Some(ErrorData::InvalidResourceUpdate { .. })
                    ));
                }
            }
        }
    }
}

#[test]
fn encrypted_volume_allows_image_updates_with_the_same_key() {
    let original = resource("container", Some("first"));
    let mut desired: Value = serde_json::to_value(&original).unwrap();
    desired["code"]["image"] = json!("redis:8");
    original
        .validate_update(&serde_json::from_value(desired).unwrap())
        .expect("an image update must preserve the volume encryption selection");
}
