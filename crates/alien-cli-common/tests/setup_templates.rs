//! Rendering a protected stack's setup templates the way a signed release records them.

use alien_cli_common::setup_templates::{render_setup_templates, SetupRenderOptions};
use alien_cloudformation::{CfRegistry, RegistrationMode};
use alien_core::{SetupPins, Stack, StackSigning};
use alien_helm::HelmRegistry;
use alien_signing::release::SETUP_TEMPLATES;
use alien_terraform::TfRegistry;

const FINGERPRINT: &str =
    "SHA256:9f2c3b1a77d0e5c20b19a4f36c8e2d5100112233445566778899aabbccddeeff";

const PUBLISHER: &str = "111122223333";
const VERIFIER: &str = "sha256:5d41402abc4b2a76b9719d911017c592ae9e2b6d7c5c7f2a3f1e0d9c8b7a6f50";

fn pins() -> SetupPins {
    SetupPins {
        aws_publisher: Some(PUBLISHER.to_string()),
        aws_verifier_digest: Some(VERIFIER.to_string()),
    }
}

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
    let rendered = render_setup_templates(
        &stack(),
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .registration(RegistrationMode::OutputsFallback)
            .pins(pins())
            .build(),
    )
    .await
    .expect("protected setup templates render");
    // `cfn-lint`, `terraform validate` and `helm lint` run on these in
    // scripts/setup-render-diff/lint-protected.sh.
    if let Some(dir) = std::env::var_os("SETUP_TEMPLATES_OUT") {
        for (name, template) in &rendered {
            std::fs::write(std::path::Path::new(&dir).join(name), &template.bytes)
                .expect("write rendered template");
        }
    }
    rendered
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

#[tokio::test]
async fn cloudformation_runs_the_pinned_verifier_and_limits_management() {
    let rendered = render(FINGERPRINT).await;
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
    let resources = &template["Resources"];

    let verifier = &resources["SigningVerifier"]["Properties"];
    assert_eq!(
        verifier["Code"]["ImageUri"]["Fn::Sub"].as_str(),
        Some(format!("{PUBLISHER}.dkr.ecr.${{AWS::Region}}.amazonaws.com/verifier@{VERIFIER}").as_str())
    );
    assert_eq!(verifier["FunctionName"]["Fn::Sub"].as_str(), Some("${AWS::StackName}-verifier"));
    let role = &resources["SigningVerifierRole"]["Properties"];
    assert!(role.get("PermissionsBoundary").is_none(), "the verifier's role has no boundary");

    let limits = &resources["ManagementLimits"]["Properties"];
    assert_eq!(
        limits["ManagedPolicyName"]["Fn::Sub"].as_str(),
        Some("${AWS::StackName}-management-limits")
    );
    assert_eq!(limits["Roles"][0]["Ref"].as_str(), Some("ManagementRole"));
    let sids: Vec<&str> = limits["PolicyDocument"]["Statement"]
        .as_sequence()
        .expect("statements")
        .iter()
        .map(|statement| statement["Sid"].as_str().expect("sid"))
        .collect();
    assert_eq!(
        sids,
        [
            "OnlyVerifierChangesFunctions",
            "OnlyVerifierLaunchesMachines",
            "OnlyVerifierPassesRolesToEc2",
            "NoCommandsOnMachines",
            "OnlySetupChangesTheVerifier",
            "OnlySetupChangesTheseLimits",
            "CallTheVerifier",
        ]
    );
}

#[tokio::test]
async fn a_build_without_pins_refuses_to_render_protected_aws_setup() {
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let error = render_setup_templates(
        &stack(),
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .registration(RegistrationMode::OutputsFallback)
            .pins(SetupPins::default())
            .build(),
    )
    .await
    .expect_err("no publisher, no protected AWS setup");
    let text = format!("{error:?}");
    assert!(text.contains("no machine-image publisher"), "{text}");
}
