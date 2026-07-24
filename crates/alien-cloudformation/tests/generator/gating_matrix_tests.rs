//! The gating matrix: every registered CloudFormation emitter is either
//! refused by the gateability policy or proven to render gated.
//!
//! Same contract as the Terraform matrix: registering a new emitter for a
//! gateable type fails this test until a gated fixture exists, so a type can
//! never become gateable without a validated gated render.

use super::helpers::{
    custom_resource_registration, gate_input, registration_payload, render_built_ins_template,
    resolve, try_render_built_ins, Declined,
};
use alien_cloudformation::{CfRegistry, CloudFormationTarget};
use alien_core::{Kv, Platform, Queue, ResourceLifecycle, Stack, StackSettings, Storage, Vault};
use std::collections::HashMap;

fn gated_fixture(resource_type: &str) -> Option<Stack> {
    let base = || {
        Stack::new("matrix-stack".to_string()).inputs(vec![gate_input(
            "fixtureEnabled",
            "Enable the fixture resource",
            "Whether to create the gated matrix fixture.",
        )])
    };
    let stack = match resource_type {
        "kv" => base().add_enabled_when(
            Kv::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        "storage" => base().add_enabled_when(
            Storage::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        "queue" => base().add_enabled_when(
            Queue::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        "vault" => base().add_enabled_when(
            Vault::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        _ => return None,
    };
    Some(stack.build())
}

/// Rendered, linted, and resolved with the gate declined: the fixture must
/// leave no registration entry, and every resource the fixture contributed
/// must carry the gate's condition.
fn assert_gated_render(resource_type: &str, stack: &Stack) {
    let (template, _yaml) = render_built_ins_template(
        stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        &format!("gating matrix {resource_type}"),
    );

    let condition_name = "InputFixtureEnabledIsTrue";
    assert!(
        template.conditions.contains_key(condition_name),
        "{resource_type}: the gate condition should be declared"
    );

    let unconditional: Vec<&str> = template
        .resources
        .iter()
        .filter(|(logical_id, resource)| {
            resource.condition.is_none()
                && logical_id.to_ascii_lowercase().contains("fixture")
        })
        .map(|(logical_id, _)| logical_id.as_str())
        .collect();
    assert!(
        unconditional.is_empty(),
        "{resource_type}: every resource the fixture contributed should carry the gate's \
         condition, but these are unconditional: {unconditional:?}"
    );

    let payload = registration_payload(&template);
    let declined = resolve(
        &payload,
        &HashMap::from([(condition_name, false)]),
        Declined::Removed,
    )
    .expect("registration payload should survive resolution");
    let text = serde_json::to_string(&declined).expect("resolved payload serializes");
    assert!(
        !text.contains("\"fixture\""),
        "{resource_type}: a declined fixture must leave no registration entry:\n{text}"
    );
}

#[test]
fn every_registered_emitter_is_policy_refused_or_renders_gated() {
    let registry = CfRegistry::built_in();
    let mut allowed_without_fixture = Vec::new();

    for (resource_type, platform) in registry.registered_keys() {
        if platform != Platform::Aws {
            continue;
        }
        if alien_core::gate_refusal(resource_type, "matrix-fixture").is_some() {
            continue;
        }
        match gated_fixture(resource_type) {
            Some(stack) => assert_gated_render(resource_type, &stack),
            None => allowed_without_fixture.push(resource_type.to_string()),
        }
    }

    assert!(
        allowed_without_fixture.is_empty(),
        "these registered CloudFormation emitters belong to policy-allowed types but have no \
         gated fixture; add one to this matrix (or refuse the type in \
         alien_core::gateability) before the type silently becomes gateable: \
         {allowed_without_fixture:?}"
    );
}

/// Vault gates purely through the generator — its emitter never carried
/// gating code. On AWS the vault's secrets are created lazily at runtime by
/// name prefix, so a granting-profile-free vault contributes no template
/// resources at all; its gated render is exactly the registration splice,
/// asserted through both deploy-time answers and locked by a snapshot.
#[test]
fn a_gated_vault_renders_conditionally() {
    let stack = gated_fixture("vault").expect("vault fixture");
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "gated vault stack",
    );
    assert_gated_render("vault", &stack);

    let payload = registration_payload(&template);
    let accepted = resolve(
        &payload,
        &HashMap::from([("InputFixtureEnabledIsTrue", true)]),
        Declined::Removed,
    )
    .expect("payload resolves");
    let text = serde_json::to_string(&accepted).expect("resolved payload serializes");
    assert!(
        text.contains("\"fixture\""),
        "an accepted vault must keep its registration entry:\n{text}"
    );
    insta::assert_snapshot!("enabled_gated_vault", yaml);
}

/// The render-side policy check: a gate the policy refuses fails without
/// preflights ever running, naming type and resource.
#[test]
fn a_gate_on_a_policy_refused_type_fails_at_render() {
    let stack = Stack::new("matrix-stack".to_string())
        .inputs(vec![gate_input(
            "emailEnabled",
            "Enable email",
            "Whether to create the email resource.",
        )])
        .add_enabled_when(
            alien_core::Email::new("mailer".to_string()).build(),
            ResourceLifecycle::Frozen,
            "emailEnabled",
        )
        .build();

    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "gated email stack",
    )
    .expect_err("the policy should refuse a gated email at render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(error.message.contains("email"), "{}", error.message);
    assert!(error.message.contains("mailer"), "{}", error.message);
}

/// Distinct ids can sanitize to the same CloudFormation parameter logical id;
/// a silent overwrite would make both inputs read one parameter, so
/// generation must refuse.
#[test]
fn inputs_colliding_after_normalization_are_refused() {
    let stack = Stack::new("matrix-stack".to_string())
        .inputs(vec![
            gate_input("fooBar", "Input a", "First input."),
            gate_input("foo_bar", "Input b", "Second input."),
        ])
        .add(
            Kv::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "colliding inputs",
    )
    .expect_err("colliding parameter names must refuse to render");
    assert!(error.message.contains("InputFooBar"), "{}", error.message);
    assert!(
        !error.message.contains("  "),
        "the message should render without space runs: {}",
        error.message
    );
}
