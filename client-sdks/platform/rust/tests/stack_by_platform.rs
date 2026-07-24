use alien_platform_api::types::StackByPlatform;
use serde_json::json;

#[test]
fn deserializes_platform_stacks_without_dropping_them() {
    let stack: StackByPlatform = serde_json::from_value(json!({
        "aws": {
            "id": "example",
            "resources": []
        }
    }))
    .expect("a release stack returned by the platform API should deserialize");

    assert_eq!(
        stack.aws,
        Some(json!({
            "id": "example",
            "resources": []
        }))
    );
}
