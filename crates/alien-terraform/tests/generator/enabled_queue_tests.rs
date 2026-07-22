//! `.enabled(input)` for `Queue`.
//!
//! `enabled_tests.rs` covers the mechanics on `Kv`. A queue adds the shape `Kv`
//! never had: it owns more than one block and grants IAM against them. The
//! recurring failure mode is a self-reference that keeps rendering after its
//! resource became a list — valid HCL, rejected by `terraform validate` — so
//! every gated scenario here is linted.

use super::helpers::{
    assert_terraform_valid, assert_ungated_registration_list_is_a_plain_array,
    declared_block_types, gate_input, gated_block_types, normalized, render,
};
use alien_core::{
    AzureResourceGroup, AzureServiceBusNamespace, PermissionProfile, Queue, ResourceLifecycle,
    ServiceAccount, Stack, StackBuilder, StackInputDefinition, StackSettings,
};
use alien_terraform::TerraformTarget;

const QUEUE_GATE: &str = "count = var.input_queue_enabled ? 1 : 0";

fn gate_inputs() -> Vec<StackInputDefinition> {
    vec![gate_input(
        "queueEnabled",
        "queue",
        "Whether to create the queue.",
    )]
}

/// A gated resource has to keep rendering valid Terraform for everything that
/// points at it, and IAM is where the pointers are. A stack without a permission
/// profile emits no IAM blocks at all, so a missed index would never reach
/// `terraform validate` — which is exactly how this bug ships.
fn permissioned(builder: StackBuilder, resource_id: &str, permissions: &[&str]) -> StackBuilder {
    builder
        .permission(
            "execution",
            PermissionProfile::new().resource(resource_id, permissions.to_vec()),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn queue_stack(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string()).inputs(gate_inputs()),
        "jobs",
        &["queue/data-write"],
    );
    add_queue(builder, gated).build()
}

/// `queue/management` grants raw permissions, so the emitter defines a project
/// custom role for it — the shared block a resource's gate must never reach.
/// `queue/data-write` (used above) is only predefined roles, so it emits no
/// custom role at all: the gate-exclusion test needs this fixture to have
/// anything to check.
fn custom_role_queue_stack(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string()).inputs(gate_inputs()),
        "jobs",
        &["queue/management"],
    );
    add_queue(builder, gated).build()
}

/// Azure realises the queue inside a shared Service Bus namespace. The rebuild
/// preflight injects that namespace and its resource group at runtime; neither
/// is gated, only what the tests add on top.
fn azure_queue_stack(gated: bool) -> Stack {
    let builder = Stack::new("gated-stack".to_string())
        .inputs(gate_inputs())
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureServiceBusNamespace::new("default-service-bus-namespace".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    add_queue(builder, gated).build()
}

fn add_queue(builder: StackBuilder, gated: bool) -> StackBuilder {
    let queue = Queue::new("jobs".to_string()).build();
    if gated {
        builder.add_enabled_when(queue, ResourceLifecycle::Frozen, "queueEnabled")
    } else {
        builder.add(queue, ResourceLifecycle::Frozen)
    }
}

/// The registration list only changes shape once something in the stack is
/// gated, and a declined resource contributes no entry rather than a null one.
fn assert_gated_registration_list(main: &str, gate_variable: &str) {
    assert!(
        main.contains("deployment_resources = concat("),
        "a gated stack splices its registration list together:\n{main}"
    );
    assert!(
        main.contains(&format!("{gate_variable} ? [")) && main.contains("] : []"),
        "the gated entry must collapse to an empty list, not to null:\n{main}"
    );
    assert!(
        !main.contains(": null"),
        "no null may reach the registration payload:\n{main}"
    );
}

// ---------------------------------------------------------------- AWS queue

#[test]
fn a_gated_aws_queue_is_counted_and_indexed() {
    let module = render(
        &queue_stack(true),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"aws_sqs_queue\""),
        "the queue block is still declared:\n{main}"
    );
    assert!(
        main.contains(QUEUE_GATE),
        "the queue must be created only when the deployer says yes:\n{main}"
    );
    for attribute in ["name", "url", "arn"] {
        assert!(
            main.contains(&format!("aws_sqs_queue.jobs[0].{attribute}")),
            "every self-reference must be indexed, not just the first ({attribute}):\n{main}"
        );
    }
    assert_gated_registration_list(main, "var.input_queue_enabled");
    assert_terraform_valid(&module, "gated aws queue stack");
}

#[test]
fn an_ungated_aws_queue_is_untouched() {
    let module = render(
        &queue_stack(false),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("aws_sqs_queue.jobs[0]"),
        "no indexing on an ungated queue:\n{main}"
    );
    assert!(
        main.contains("aws_sqs_queue.jobs.arn"),
        "references stay unindexed:\n{main}"
    );
    assert!(
        !main.contains(QUEUE_GATE),
        "nothing is gated, so no count belongs in the module:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}

// ---------------------------------------------------------------- GCP queue

/// GCP is the only cloud where one Alien queue owns two blocks, and the
/// subscription points at the topic. Gating one without the other renders fine
/// and fails `terraform validate`.
#[test]
fn a_gated_gcp_queue_counts_both_of_its_blocks() {
    let module = render(
        &queue_stack(true),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert_eq!(
        main.matches(QUEUE_GATE).count(),
        5,
        "topic, subscription and the three IAM members all follow the gate:\n{main}"
    );
    assert!(
        main.contains("topic = google_pubsub_topic.jobs[0].id"),
        "the subscription points at the counted topic:\n{main}"
    );
    assert_terraform_valid(&module, "gated gcp queue stack");
}

/// The IAM members grant against the topic and the subscription by name. They
/// are the references most easily missed, because a stack without a permission
/// profile does not render them at all.
#[test]
fn a_gated_gcp_queue_leaves_no_unindexed_self_reference() {
    let module = render(
        &queue_stack(true),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("google_pubsub_topic_iam_member"),
        "the fixture must actually render queue IAM, or this test proves nothing:\n{main}"
    );
    assert!(
        main.contains("topic = google_pubsub_topic.jobs[0].name")
            && main.contains("subscription = google_pubsub_subscription.jobs[0].name"),
        "IAM members address the counted instances:\n{main}"
    );
    for block in ["google_pubsub_topic", "google_pubsub_subscription"] {
        assert!(
            !main.contains(&format!("{block}.jobs.")),
            "no attribute may be read off {block} without an index:\n{main}"
        );
    }
    assert_gated_registration_list(main, "var.input_queue_enabled");
}

/// The project-wide custom roles are shared across resources and carry their own
/// `var.gcp_manage_custom_roles` count. Tying them to one resource's gate would
/// delete roles other resources still need.
#[test]
fn a_gated_gcp_queue_leaves_shared_custom_roles_alone() {
    let module = render(
        &custom_role_queue_stack(true),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        declared_block_types(main)
            .iter()
            .any(|found| found == "google_project_iam_custom_role"),
        "the fixture must actually produce a custom role, or this proves nothing:\n{main}"
    );
    assert!(
        !gated_block_types(main, QUEUE_GATE)
            .iter()
            .any(|found| found == "google_project_iam_custom_role"),
        "a shared custom role must not follow one resource's gate:\n{main}"
    );
    assert!(
        main.contains("var.gcp_manage_custom_roles ? 1 : 0"),
        "the custom role keeps its own count:\n{main}"
    );
}

#[test]
fn an_ungated_gcp_queue_is_untouched() {
    let module = render(
        &queue_stack(false),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("google_pubsub_topic.jobs[0]")
            && !main.contains("google_pubsub_subscription.jobs[0]"),
        "no indexing on an ungated queue:\n{main}"
    );
    assert!(
        main.contains("topic = google_pubsub_topic.jobs.id"),
        "references stay unindexed:\n{main}"
    );
    assert!(
        !main.contains(QUEUE_GATE),
        "nothing is gated, so no gate count belongs in the module:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}

// -------------------------------------------------------------- Azure queue

/// The Azure payload mixes the gated queue with the ungated namespace hosting
/// it. Indexing the latter would produce Terraform that does not validate.
#[test]
fn a_gated_azure_queue_indexes_only_its_own_references() {
    let module = render(
        &azure_queue_stack(true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains(QUEUE_GATE),
        "the queue must be created only when the deployer says yes:\n{main}"
    );
    assert!(
        main.contains("azurerm_servicebus_queue.jobs[0].name"),
        "references to the counted queue must be indexed:\n{main}"
    );
    assert!(
        !main.contains("azurerm_servicebus_namespace.default_service_bus_namespace[0]"),
        "the parent namespace is not counted, so it must stay unindexed:\n{main}"
    );
    assert!(
        main.contains("azurerm_servicebus_namespace.default_service_bus_namespace.name"),
        "the parent namespace name is still read directly:\n{main}"
    );
    assert_gated_registration_list(main, "var.input_queue_enabled");
    assert_terraform_valid(&module, "gated azure queue stack");
}

#[test]
fn an_ungated_azure_queue_is_untouched() {
    let module = render(
        &azure_queue_stack(false),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("azurerm_servicebus_queue.jobs[0]"),
        "no indexing on an ungated queue:\n{main}"
    );
    assert!(
        main.contains("azurerm_servicebus_queue.jobs.name"),
        "references stay unindexed:\n{main}"
    );
    assert!(
        !main.contains(QUEUE_GATE),
        "nothing is gated, so no count belongs in the module:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}
