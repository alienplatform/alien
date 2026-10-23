//! `RegistrationMode` axis — Outputs fallback / Custom Resource / Both.
//!
//! Each scenario uses the sample emitter so the snapshot stays focused
//! on the registration surface (custom resource, outputs, both).

use super::helpers::{
    render_built_ins_template, render_sample, render_sample_template, sample_stack, SampleResource,
};
use alien_cloudformation::{CfExpression, CloudFormationTarget, RegistrationMode};
use alien_core::{Kv, ResourceLifecycle, ResourceRef, Stack, StackSettings};
use indexmap::indexmap;

const LAMBDA_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:function:setup-registration";
const REGISTRATION: &str = "DeploymentRegistration";
const SETUP_STARTED: &str = "DeploymentSetupStarted";

#[test]
fn registration_signals_preserve_resources_with_matching_names() {
    let stack = Stack::new("registration-names".to_string())
        .add(
            Kv::new("deployment-setup-started".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("deployment-registration".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        RegistrationMode::CustomResource {
            lambda_arn: LAMBDA_ARN.to_string(),
            callback_url: None,
        },
        CloudFormationTarget::Aws,
        "aws",
        "registration reserved resource names",
    );

    for logical_id in ["DeploymentSetupStarted2", "DeploymentRegistration2"] {
        assert_eq!(
            template.resources[logical_id].resource_type,
            "AWS::DynamoDB::Table"
        );
        assert!(template.resources[REGISTRATION]
            .depends_on
            .contains(&logical_id.to_string()));
        let registration = serde_json::to_value(&template.resources[REGISTRATION])
            .expect("registration should serialize");
        assert!(registration
            .to_string()
            .contains(&format!("\"{logical_id}\",\"Arn\"")));
    }
    assert_eq!(
        template.resources[SETUP_STARTED].resource_type,
        "AWS::CloudFormation::CustomResource"
    );
    assert_eq!(
        template.resources[REGISTRATION].resource_type,
        "AWS::CloudFormation::CustomResource"
    );
}

/// `DeploymentSetupStarted` tells the registration endpoint that the stack
/// began creating, long before the registration resource (which waits for
/// every other resource) can run. That only works if nothing orders it: no
/// `DependsOn`, no condition, and the registration must not wait for it
/// either. It carries the registration's own `ServiceToken`, `Token` and
/// `CallbackUrl` so the same endpoint can authenticate it, and nothing else.
#[test]
fn setup_started_resource_fires_at_stack_start_with_the_registration_credentials() {
    for callback_url in [None, Some("https://api.example.test".to_string())] {
        let (template, _) = render_sample_template(
            &sample_stack(),
            StackSettings::default(),
            RegistrationMode::CustomResource {
                lambda_arn: LAMBDA_ARN.to_string(),
                callback_url: callback_url.clone(),
            },
            "registration setup started",
        );

        let started = template
            .resources
            .get(SETUP_STARTED)
            .unwrap_or_else(|| panic!("{SETUP_STARTED} should be emitted ({callback_url:?})"));
        let registration = &template.resources[REGISTRATION];

        assert_eq!(started.resource_type, "AWS::CloudFormation::CustomResource");
        let mut expected_properties = indexmap! {
            "ServiceToken".to_string() => registration.properties["ServiceToken"].clone(),
            "Operation".to_string() => CfExpression::from("setup-started"),
            "Token".to_string() => CfExpression::ref_("Token"),
        };
        if let Some(callback_url) = &callback_url {
            expected_properties.insert(
                "CallbackUrl".to_string(),
                CfExpression::from(callback_url.clone()),
            );
            assert_eq!(
                registration.properties["CallbackUrl"],
                CfExpression::from(callback_url.clone()),
                "the registration should carry the same callback URL"
            );
        }
        assert_eq!(
            started.properties, expected_properties,
            "{SETUP_STARTED} properties ({callback_url:?})"
        );

        // Anything that orders the resource would delay the signal.
        assert!(
            started.depends_on.is_empty(),
            "{SETUP_STARTED} must not wait for anything: {:?}",
            started.depends_on
        );
        assert_eq!(started.condition, None);
        assert_eq!(started.deletion_policy, None);
        assert!(
            !registration.depends_on.iter().any(|id| id == SETUP_STARTED),
            "the registration must not wait for the start signal"
        );
        assert!(
            !template.outputs.values().any(|output| {
                serde_json::to_string(output)
                    .expect("output should serialize")
                    .contains(SETUP_STARTED)
            }),
            "the start signal has nothing to report through Outputs"
        );
    }

    // Without a service token there is nothing to call at stack start.
    let (template, _) = render_sample_template(
        &sample_stack(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "registration setup started fallback",
    );
    assert!(
        !template.resources.contains_key(SETUP_STARTED),
        "{SETUP_STARTED} needs a service token; Outputs fallback has none"
    );
}

#[test]
fn outputs_fallback_emits_6_standard_outputs_plus_resources() {
    let yaml = render_sample(
        &sample_stack(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "registration outputs fallback",
    );
    insta::assert_snapshot!("registration_outputs_fallback", yaml);
}

#[test]
fn custom_resource_invokes_lambda_with_resolved_payload() {
    let yaml = render_sample(
        &sample_stack(),
        StackSettings::default(),
        RegistrationMode::CustomResource {
            lambda_arn: LAMBDA_ARN.to_string(),
            callback_url: None,
        },
        "registration custom resource",
    );
    insta::assert_snapshot!("registration_custom_resource", yaml);
}

#[test]
fn both_modes_emit_lambda_plus_outputs() {
    let yaml = render_sample(
        &sample_stack(),
        StackSettings::default(),
        RegistrationMode::Both {
            lambda_arn: LAMBDA_ARN.to_string(),
            callback_url: None,
        },
        "registration both",
    );
    insta::assert_snapshot!("registration_both", yaml);
}

#[test]
fn regional_both_emits_mapping_plus_outputs() {
    let yaml = render_sample(
        &sample_stack(),
        StackSettings::default(),
        RegistrationMode::RegionalBoth {
            lambda_arns_by_region: indexmap! {
                "us-east-1".to_string() => LAMBDA_ARN.to_string(),
                "eu-west-1".to_string() => "arn:aws:lambda:eu-west-1:123456789012:function:setup-registration".to_string(),
            },
            callback_url: Some("https://api.dev.example.com".to_string()),
        },
        "registration regional both",
    );

    assert!(yaml.contains("RegionalCustomResourceServiceTokens:"));
    assert!(yaml.contains("Fn::FindInMap:"));
    assert!(yaml.contains("Ref: AWS::Region"));
    assert!(yaml.contains("CallbackUrl: https://api.dev.example.com"));
    insta::assert_snapshot!("registration_regional_both", yaml);
}

#[test]
fn resource_dependencies_emit_depends_on() {
    let stack = Stack::new("dependencies".to_string())
        .add(
            SampleResource {
                id: "base".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            SampleResource {
                id: "dependent".to_string(),
            },
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(SampleResource::RESOURCE_TYPE, "base")],
        )
        .build();

    let yaml = render_sample(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "resource dependencies",
    );
    assert!(yaml.contains("DependsOn:\n    - Base"));
}
