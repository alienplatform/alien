//! Rendering a protected stack's setup templates the way a signed release records them.

use alien_cli_common::setup_templates::{render_setup_templates, SetupRenderOptions};
use alien_cloudformation::{CfRegistry, RegistrationMode};
use alien_core::{Stack, StackSigning};
use alien_helm::HelmRegistry;
use alien_signing::release::SETUP_TEMPLATES;
use alien_terraform::TfRegistry;

const FINGERPRINT: &str =
    "SHA256:9f2c3b1a77d0e5c20b19a4f36c8e2d5100112233445566778899aabbccddeeff";

fn stack() -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "minimal",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("fixture stack parses")
}

async fn render(fingerprint: &str) -> std::collections::BTreeMap<String, alien_cli_common::setup_templates::RenderedSetupTemplate> {
    let signing = StackSigning {
        fingerprint: fingerprint.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    render_setup_templates(
        &stack(),
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .registration(RegistrationMode::OutputsFallback)
            .build(),
    )
    .await
    .expect("protected setup templates render")
}

#[tokio::test]
async fn renders_every_setup_template_the_same_way_twice() {
    let first = render(FINGERPRINT).await;
    let second = render(FINGERPRINT).await;

    assert_eq!(
        first.keys().map(String::as_str).collect::<Vec<_>>(),
        {
            let mut names = SETUP_TEMPLATES.to_vec();
            names.sort();
            names
        }
    );
    assert_eq!(first, second, "rendering is deterministic");

    let other = render(&FINGERPRINT.replace("9f2c", "0000")).await;
    for name in SETUP_TEMPLATES {
        assert_ne!(
            first[*name].setup_template().sha256,
            other[*name].setup_template().sha256,
            "{name}: the digest covers the fingerprint"
        );
    }
}

#[tokio::test]
async fn cloudformation_pins_the_fingerprint_and_leaves_it_off_the_allow_list() {
    let rendered = render(FINGERPRINT).await;
    let cloudformation = &rendered["cloudformation"];
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&cloudformation.bytes).expect("template is YAML");

    let parameter = &template["Parameters"]["SigningFingerprint"];
    assert_eq!(parameter["AllowedValues"], serde_yaml::to_value([FINGERPRINT]).unwrap());
    assert_eq!(parameter["Default"].as_str(), Some(FINGERPRINT));
    assert!(!cloudformation.allowed.iter().any(|name| name == "SigningFingerprint"));
    assert!(cloudformation.allowed.iter().any(|name| name == "ManagingRoleArn"));
    let template_parameters = template["Parameters"].as_mapping().unwrap().len();
    assert_eq!(cloudformation.allowed.len(), template_parameters - 1);

    let terraform = &rendered["terraform-aws"];
    assert!(!terraform.allowed.iter().any(|name| name == "signing_fingerprint"));
    let helm = &rendered["helm"];
    assert!(!helm.allowed.iter().any(|name| name == "signing"));
}
