//! `RegistrationMode` axis — Outputs fallback / Custom Resource / Both.
//!
//! Each scenario uses the sample emitter so the snapshot stays focused
//! on the registration surface (custom resource, outputs, both).

use super::helpers::{render_sample, sample_stack, SampleResource};
use alien_cloudformation::RegistrationMode;
use alien_core::{ResourceLifecycle, ResourceRef, Stack, StackSettings};

const LAMBDA_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:function:alien-import";

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
        },
        "registration both",
    );
    insta::assert_snapshot!("registration_both", yaml);
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
