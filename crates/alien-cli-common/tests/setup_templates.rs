//! Rendering a protected stack's setup templates the way a signed release records them.

use alien_cli_common::setup_templates::{
    render_setup_templates, SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::CfRegistry;
use alien_core::{SetupPins, Stack, StackSigning};
use alien_helm::HelmRegistry;
use alien_signing::release::SETUP_TEMPLATES;
use alien_terraform::TfRegistry;

const FINGERPRINT: &str =
    "SHA256:9f2c3b1a77d0e5c20b19a4f36c8e2d5100112233445566778899aabbccddeeff";

const PUBLISHER: &str = "111122223333";
const VERIFIER: &str = "verifier@sha256:5d41402abc4b2a76b9719d911017c592ae9e2b6d7c5c7f2a3f1e0d9c8b7a6f50";

fn pins() -> SetupPins {
    SetupPins {
        aws_publisher: Some(PUBLISHER.to_string()),
        aws_verifier_image: Some(VERIFIER.to_string()),
    }
}

fn hosting() -> SetupHostingOptions {
    serde_json::from_value(serde_json::json!({
        "registration": { "mode": "outputsFallback" },
        "setupFingerprint": "sha256:setup",
        "setupFingerprintVersion": 1
    }))
    .expect("hosting options parse")
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
            .hosting(hosting())
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
        Some(format!("{PUBLISHER}.dkr.ecr.${{AWS::Region}}.amazonaws.com/{VERIFIER}").as_str())
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
            "NoUnsignedMicrovms",
            "NoUnsignedBuilds",
            "OnlySetupChangesWhatRolesMayDo",
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
            .hosting(hosting())
            .pins(SetupPins::default())
            .build(),
    )
    .await
    .expect_err("no publisher, no protected AWS setup");
    let text = format!("{error:?}");
    assert!(text.contains("no machine-image publisher"), "{text}");
}

/// Hosting options come from a service the install doesn't trust. None of them may change what
/// a protected template locks: the limits, the verifier, its role and the signing settings stay
/// exactly as they are with the plainest options.
#[tokio::test]
async fn hosting_options_cannot_weaken_the_protected_locks() {
    let locked = |rendered: &std::collections::BTreeMap<
        String,
        alien_cli_common::setup_templates::RenderedSetupTemplate,
    >| {
        let template: serde_yaml::Value =
            serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
        let resources = &template["Resources"];
        let guard = resources
            .as_mapping()
            .expect("resources")
            .values()
            .filter(|resource| resource["Type"] == "AWS::IAM::ManagedPolicy")
            .flat_map(|policy| {
                policy["Properties"]["PolicyDocument"]["Statement"]
                    .as_sequence()
                    .cloned()
                    .unwrap_or_default()
            })
            .filter(|statement| statement["Sid"] == "OnlySetupChangesSigningSettings")
            .collect::<Vec<_>>();
        assert_eq!(guard.len(), 1, "one /signed/ guard on the management role");
        (
            [
                "ManagementLimits",
                "SigningVerifier",
                "SigningVerifierRole",
                "SigningFingerprintParameter",
                "SigningStateParameter",
            ]
            .map(|id| resources[id].clone()),
            guard,
            template["Parameters"]["SigningFingerprint"].clone(),
        )
    };
    let baseline = locked(&render(FINGERPRINT).await);

    let hostile = [
        serde_json::json!({
            "registration": {
                "mode": "regionalBoth",
                "lambdaArnsByRegion": {
                    "us-east-1": "arn:aws:lambda:us-east-1:999999999999:function:anything"
                },
                "callbackUrl": "https://attacker.example/"
            },
            "description": "Resources:\n  Evil: {Type: AWS::IAM::Role}",
            "managementTrust": {
                "requireExternalId": true,
                "allowedManagingRoleArns": ["arn:aws:iam::999999999999:role/anyone"]
            },
            "supportedAwsRegions": ["us-east-1"],
            "setupFingerprint": "${AWS::NoValue}",
            "setupFingerprintVersion": 4294967295u32
        }),
        serde_json::json!({
            "registration": {
                "mode": "customResource",
                "lambdaArn": "arn:aws:lambda:us-east-1:999999999999:function:anything"
            },
            "setupFingerprint": "x",
            "setupFingerprintVersion": 0
        }),
    ];
    for options in hostile {
        let hosting: SetupHostingOptions =
            serde_json::from_value(options.clone()).expect("hosting options parse");
        let signing = StackSigning {
            fingerprint: FINGERPRINT.to_string(),
            channel: "production".to_string(),
        };
        let (cloudformation, terraform, helm) =
            (CfRegistry::built_in(), TfRegistry::built_in(), HelmRegistry::built_in());
        let rendered = render_setup_templates(
            &stack(),
            SetupRenderOptions::builder()
                .signing(&signing)
                .cloudformation_registry(&cloudformation)
                .terraform_registry(&terraform)
                .helm_registry(&helm)
                .hosting(hosting)
                .pins(pins())
                .build(),
        )
        .await
        .expect("renders with hostile hosting options");
        assert_eq!(locked(&rendered), baseline, "{options}");
    }
}

#[test]
fn hosting_options_refuse_unknown_fields() {
    let error = serde_json::from_value::<SetupHostingOptions>(serde_json::json!({
        "registration": { "mode": "outputsFallback" },
        "setupFingerprint": "x",
        "setupFingerprintVersion": 1,
        "extraPolicies": []
    }))
    .expect_err("an unknown hosting option is refused");
    assert!(error.to_string().contains("extraPolicies"), "{error}");
}
