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
use alien_core::{
    ownership_policy_for_resource_type, AwsOpenSearch, Email, EmailInbound, Kv, Platform, Queue,
    ResourceLifecycle, ResourceRef, Stack, StackSettings, Storage, Vault, Worker, WorkerCode,
};
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
        "email" => base().add_enabled_when(
            Email::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        "experimental/aws-opensearch" => base().add_enabled_when(
            AwsOpenSearch::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        _ => return None,
    };
    Some(stack.build())
}

/// A gated Live resource never reaches a setup template: the generator skips
/// Live lifecycles before gate handling, so setup must render as if the
/// resource were absent while still asking the deployer for the input the
/// runtime strip resolves.
fn assert_live_gate_ignored_by_setup(resource_type: &str) {
    let stack = Stack::new("matrix-stack".to_string())
        .inputs(vec![gate_input(
            "fixtureEnabled",
            "Enable the fixture resource",
            "Whether to create the gated matrix fixture.",
        )])
        .add_enabled_when(
            Worker::new("fixture".to_string())
                .permissions("fixture".to_string())
                .code(WorkerCode::Image {
                    image: "example.com/fixture:latest".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
            "fixtureEnabled",
        )
        .build();

    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        &format!("gating matrix live {resource_type}"),
    );

    assert!(
        !template.conditions.contains_key("InputFixtureEnabledIsTrue"),
        "{resource_type}: setup never declares a condition for a Live gate"
    );
    assert!(
        !template
            .resources
            .keys()
            .any(|logical_id| logical_id.to_ascii_lowercase().contains("fixture")),
        "{resource_type}: a Live resource contributes nothing to setup"
    );
    let payload = registration_payload(&template);
    let text =
        serde_json::to_string(&payload).expect("registration payload should serialize");
    assert!(
        !text.contains("\"fixture\""),
        "{resource_type}: a Live resource has no setup registration entry:\n{text}"
    );
    assert!(
        template.parameters.contains_key("InputFixtureEnabled"),
        "{resource_type}: the deployer is still asked for the input the runtime strip resolves"
    );
}

/// Rendered, linted, and resolved with the gate declined: the fixture must
/// leave no registration entry, and every resource the fixture contributed
/// must carry the gate's condition.
fn assert_gated_render(resource_type: &str, stack: &Stack) {
    // The local cfn-lint spec predates the OpenSearch `Generation` property
    // and fails the type's ungated renders too, so its matrix cell asserts
    // structure without the lint until the spec catches up.
    let template = if resource_type == "experimental/aws-opensearch" {
        try_render_built_ins(
            stack,
            StackSettings::default(),
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            &format!("gating matrix {resource_type}"),
        )
        .expect("gated render should succeed")
    } else {
        let (template, _yaml) = render_built_ins_template(
            stack,
            StackSettings::default(),
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            &format!("gating matrix {resource_type}"),
        );
        template
    };

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
        if !ownership_policy_for_resource_type(resource_type).allows_frozen() {
            assert_live_gate_ignored_by_setup(resource_type);
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

/// Vault gates purely through the generator; its emitter contains no
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
            "robotEnabled",
            "Enable the robot",
            "Whether to create the service account.",
        )])
        .add_enabled_when(
            alien_core::ServiceAccount::new("robot".to_string()).build(),
            ResourceLifecycle::Frozen,
            "robotEnabled",
        )
        .build();

    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "gated service account stack",
    )
    .expect_err("the policy should refuse a gated service account at render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(error.message.contains("service-account"), "{}", error.message);
    assert!(error.message.contains("robot"), "{}", error.message);
}

/// The first live use of the gated-contribution mechanism: Email's SES write
/// grant sits inside Storage's bucket policy, so it must follow Email's gate
/// while the bucket itself stays ungated.
#[test]
fn the_ses_inbound_grant_follows_the_email_gate() {
    let stack = Stack::new("matrix-stack".to_string())
        .inputs(vec![gate_input(
            "emailEnabled",
            "Enable email",
            "Whether to create the email resource.",
        )])
        .add(
            Storage::new("mail".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Email::new("mailer".to_string())
                .inbound(EmailInbound {
                    storage: ResourceRef {
                        resource_type: Storage::RESOURCE_TYPE.clone(),
                        id: "mail".to_string(),
                    },
                })
                .build(),
            ResourceLifecycle::Frozen,
            "emailEnabled",
        )
        .build();

    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "gated email with inbound storage",
    );

    let (policy_id, policy) = template
        .resources
        .iter()
        .find(|(_id, resource)| resource.resource_type == "AWS::S3::BucketPolicy")
        .expect("the ungated bucket should keep its policy");
    assert!(
        policy.condition.is_none(),
        "{policy_id}: the bucket policy belongs to the ungated bucket"
    );

    let document = policy
        .properties
        .get("PolicyDocument")
        .expect("bucket policy document");
    let declined = resolve(
        document,
        &HashMap::from([("InputEmailEnabledIsTrue", false)]),
        Declined::Removed,
    )
    .expect("document resolves");
    let declined_text = serde_json::to_string(&declined).expect("serializes");
    assert!(
        !declined_text.contains("ses.amazonaws.com"),
        "a declined Email must take its SES grant with it:\n{declined_text}"
    );
    assert!(
        declined_text.contains("DenyInsecureTransport"),
        "the bucket's own statements survive the decline:\n{declined_text}"
    );

    let accepted = resolve(
        document,
        &HashMap::from([("InputEmailEnabledIsTrue", true)]),
        Declined::Removed,
    )
    .expect("document resolves");
    let accepted_text = serde_json::to_string(&accepted).expect("serializes");
    assert!(
        accepted_text.contains("ses.amazonaws.com"),
        "an accepted Email keeps SES delivery working:\n{accepted_text}"
    );
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
