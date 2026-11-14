// A manager decodes a deployment's stack settings with this SDK before it runs the
// deployment. A protected deployment's signing settings must survive that, or the
// manager prepares it as if it weren't protected.
use alien_platform_api::types::DeploymentStackSettings as StackSettings;
use serde_json::json;

#[test]
fn signing_settings_survive_the_sdk() {
    let reported = json!({
        "deploymentModel": "push",
        "endpointAccess": "internet",
        "signing": {
            "fingerprint": "SHA256:af34c0c9e713ded023cd25ca41406b8400fd3d13fb5584f12357b0fe3a94b07b",
            "channel": "production"
        }
    });
    let decoded: StackSettings = serde_json::from_value(reported).unwrap();
    let settings: alien_core::StackSettings =
        serde_json::from_value(serde_json::to_value(decoded).unwrap()).unwrap();
    let signing = settings.signing.expect("the signing settings are kept");
    assert_eq!(signing.channel, "production");
    assert_eq!(
        signing.fingerprint,
        "SHA256:af34c0c9e713ded023cd25ca41406b8400fd3d13fb5584f12357b0fe3a94b07b"
    );
}
