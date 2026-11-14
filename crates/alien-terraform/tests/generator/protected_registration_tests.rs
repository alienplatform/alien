//! A protected module never self-registers through a provider of the service that hosts it:
//! that provider's code would run with the customer's credentials during `terraform apply`. It
//! reports its setup through its outputs instead, so it must be told which setup it is.

use alien_core::{Stack, StackSettings, StackSigning};
use alien_terraform::{
    generate_terraform_module, TerraformOptions, TerraformRegistration, TerraformSetupIdentity,
    TerraformTarget, TfRegistry,
};

use super::helpers::test_pins;

fn stack() -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "app",
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
    .expect("stack")
}

fn protected() -> StackSettings {
    StackSettings {
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    }
}

fn registration() -> TerraformRegistration {
    TerraformRegistration {
        provider_name: "example_app".to_string(),
        provider_source: "registry.example.com/acme/example-app".to_string(),
        provider_version: "1.0.0".to_string(),
        resource_type: "deployment".to_string(),
        release_id: None,
        setup_target: "aws".to_string(),
        setup_fingerprint: "abc".to_string(),
        setup_fingerprint_version: 1,
    }
}

fn identity() -> TerraformSetupIdentity {
    TerraformSetupIdentity {
        setup_target: "aws".to_string(),
        setup_fingerprint: "abc".to_string(),
        setup_fingerprint_version: 1,
    }
}

fn render(
    settings: StackSettings,
    registration: Option<TerraformRegistration>,
    setup_identity: Option<TerraformSetupIdentity>,
) -> alien_core::Result<alien_terraform::ModuleFiles> {
    generate_terraform_module(
        &stack(),
        TerraformTarget::Aws,
        TerraformOptions {
            setup_identity,
            pins: test_pins(),
            display_name: None,
            registry: &TfRegistry::built_in(),
            stack_settings: settings,
            registration,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
}

#[test]
fn a_protected_module_refuses_a_registration_provider() {
    let error = render(protected(), Some(registration()), None).expect_err("refused");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error
            .message
            .contains("registers through `register --import terraform`"),
        "{}",
        error.message
    );
}

#[test]
fn a_protected_module_must_know_its_setup() {
    let error = render(protected(), None, None).expect_err("refused");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error
            .message
            .contains("needs its setup target and fingerprint"),
        "{}",
        error.message
    );
}

#[test]
fn registration_and_a_separate_setup_identity_are_refused_together() {
    let error = render(
        StackSettings::default(),
        Some(registration()),
        Some(identity()),
    )
    .expect_err("refused");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
}

#[test]
fn an_unprotected_module_still_self_registers() {
    render(StackSettings::default(), Some(registration()), None).expect("renders");
}
