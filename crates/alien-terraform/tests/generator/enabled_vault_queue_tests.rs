//! `.enabled(input)` for `Queue` and `Vault`.
//!
//! `enabled_tests.rs` covers the mechanics on `Kv`. These two resource types
//! add the shapes `Kv` never had: a queue that owns more than one block and
//! grants IAM against them, and a vault that on two of three clouds owns no
//! block at all. The recurring failure mode is a self-reference that keeps
//! rendering after its resource became a list — valid HCL, rejected by
//! `terraform validate` — so every gated scenario here is linted.

use super::helpers::{
    assert_terraform_valid, assert_ungated_registration_list_is_a_plain_array, gate_input,
    normalized, render,
};
use alien_core::{
    AzureResourceGroup, AzureServiceBusNamespace, PermissionProfile, Queue, ResourceLifecycle,
    ServiceAccount, Stack, StackBuilder, StackInputDefinition, StackSettings, Vault,
};
use alien_terraform::TerraformTarget;

const QUEUE_GATE: &str = "count = var.input_queue_enabled ? 1 : 0";
const VAULT_GATE: &str = "count = var.input_vault_enabled ? 1 : 0";

fn gate_inputs() -> Vec<StackInputDefinition> {
    vec![
        gate_input("queueEnabled", "queue", "Whether to create the queue."),
        gate_input("vaultEnabled", "vault", "Whether to create the vault."),
    ]
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
/// `queue/data-write` is predefined roles only and renders no custom role, so
/// the gate-exclusion test below needs this fixture to have anything to check.
fn custom_role_queue_stack(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string()).inputs(gate_inputs()),
        "jobs",
        &["queue/management"],
    );
    add_queue(builder, gated).build()
}

/// Azure realises the queue inside a shared Service Bus namespace, and the
/// vault beside a resource group. The rebuild preflight injects both at
/// runtime; neither is gated, only what the tests add on top.
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

/// `secrets` is the reserved deployment secrets vault — it is wired to Workers
/// and compute clusters automatically after validation, so the preflight refuses
/// to gate it. A vault whose gate these tests assert on needs an id a customer
/// could actually use.
const VAULT_ID: &str = "app-tokens";

fn vault_stack(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string()).inputs(gate_inputs()),
        VAULT_ID,
        &["vault/data-read"],
    );
    add_vault(builder, gated).build()
}

/// `vault/data-write` grants a raw permission on GCP, which renders as a
/// project-wide custom role beside the binding. `vault/data-read` on its own
/// resolves to predefined roles, so it never exercises that path.
fn vault_stack_with_custom_role(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string()).inputs(gate_inputs()),
        VAULT_ID,
        &["vault/data-read", "vault/data-write"],
    );
    add_vault(builder, gated).build()
}

fn azure_vault_stack(gated: bool) -> Stack {
    let builder = permissioned(
        Stack::new("gated-stack".to_string())
            .inputs(gate_inputs())
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            ),
        VAULT_ID,
        &["vault/data-read"],
    );
    add_vault(builder, gated).build()
}

fn add_queue(builder: StackBuilder, gated: bool) -> StackBuilder {
    let queue = Queue::new("jobs".to_string()).build();
    if gated {
        builder.add_enabled_when(queue, ResourceLifecycle::Frozen, "queueEnabled")
    } else {
        builder.add(queue, ResourceLifecycle::Frozen)
    }
}

fn add_vault(builder: StackBuilder, gated: bool) -> StackBuilder {
    let vault = Vault::new(VAULT_ID.to_string()).build();
    if gated {
        builder.add_enabled_when(vault, ResourceLifecycle::Frozen, "vaultEnabled")
    } else {
        builder.add(vault, ResourceLifecycle::Frozen)
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
        main.contains("resource \"google_project_iam_custom_role\""),
        "the fixture must actually render a custom role, or this proves nothing:\n{main}"
    );
    let gated_types = gated_block_types(main, QUEUE_GATE);
    assert!(
        !gated_types
            .iter()
            .any(|found| found == "google_project_iam_custom_role"),
        "a shared custom role must not follow one resource's gate, found {gated_types:?}:\n{main}"
    );
    assert!(
        gated_types
            .iter()
            .any(|found| found == "google_pubsub_topic_iam_member"),
        "the bindings beside it still follow the gate, found {gated_types:?}:\n{main}"
    );
    assert!(
        main.contains("count = var.gcp_manage_custom_roles ? 1 : 0"),
        "the custom role keeps the count it already had:\n{main}"
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

// -------------------------------------------------------- AWS / GCP vault

/// Parse the module into `(type, label, body)` per `resource` block; each body
/// runs to the next `resource "` header.
fn resource_blocks(main: &str) -> Vec<(String, String, String)> {
    let mut blocks = Vec::new();
    for (index, _) in main.match_indices("resource \"") {
        let rest = &main[index + "resource \"".len()..];
        let Some((block_type, tail)) = rest.split_once('"') else {
            continue;
        };
        let Some((label, body)) = tail.trim_start().trim_start_matches('"').split_once('"') else {
            continue;
        };
        let body_end = body.find("resource \"").unwrap_or(body.len());
        blocks.push((
            block_type.to_string(),
            label.to_string(),
            body[..body_end].to_string(),
        ));
    }
    blocks
}

/// Which block types carry `gate`, so an assertion can say "exactly these"
/// rather than "at least the one I happened to look at".
fn gated_block_types(main: &str, gate: &str) -> Vec<String> {
    resource_blocks(main)
        .into_iter()
        .filter(|(_, _, body)| body.contains(gate))
        .map(|(block_type, _, _)| block_type)
        .collect()
}

/// The property that actually matters: no block granting against the declined
/// vault's namespace may survive without the gate.
///
/// A "some block carries the gate" check is not enough: two emitters render the
/// grant and the generator dedupes them only while their bodies match, so gating
/// one leaves an ungated twin holding the access.
fn assert_no_ungated_grant_over(main: &str, namespace: &str, gate: &str) {
    let leaked: Vec<String> = resource_blocks(main)
        .into_iter()
        .filter(|(_, _, body)| body.contains(namespace) && !body.contains(gate))
        .map(|(block_type, label, _)| format!("{block_type}.{label}"))
        .collect();
    assert!(
        leaked.is_empty(),
        "these blocks still grant against `{namespace}` with no gate, so declining \
         the vault would not withdraw the access: {leaked:?}\n{main}"
    );
}

/// On AWS and GCP the vault owns no block of its own — it is a name prefix, and
/// its import data is built from `local` / `var` / `data` values. So there is
/// nothing to index, and the prefix it reports must not change.
///
/// Its access policy is a different matter. The grant is a data-plane read
/// (`ssm:GetParameter`, `roles/secretmanager.secretAccessor`) over a prefix
/// wildcard, and resource ids may contain hyphens — so a declined vault `app`
/// left holding `<prefix>-app-*` can read a live sibling vault `app-config`.
/// Declining a vault has to withdraw the permission, not just the registration
/// entry, which is also what the CloudFormation generator does.
#[test]
fn a_gated_prefix_only_vault_gates_the_access_policy_it_owns() {
    for (target, name, grant_block) in [
        (TerraformTarget::Aws, "aws", "aws_iam_role_policy"),
        (TerraformTarget::Gcp, "gcp", "google_project_iam_member"),
    ] {
        let gated = render(&vault_stack(true), target, StackSettings::default());
        let gated_main = &normalized(&gated);

        assert!(
            gated_main.contains(&format!("resource \"{grant_block}\"")),
            "the fixture must actually render {name} vault IAM, or this proves nothing:\n{gated_main}"
        );
        assert!(
            gated_main.contains("parameterPrefix = \"${local.resource_prefix}-app-tokens\"")
                || gated_main.contains("secretPrefix = \"${local.resource_prefix}-app-tokens\""),
            "the {name} vault still reports its prefix when gated:\n{gated_main}"
        );

        // The vault owns no cloud block, so its grants are the only thing it can
        // gate — and nothing else in the module may pick the gate up.
        let gated_types = gated_block_types(gated_main, VAULT_GATE);
        assert!(
            !gated_types.is_empty() && gated_types.iter().all(|found| found == grant_block),
            "on {name} the gate belongs on the vault's {grant_block} grants and \
             nothing else, found {gated_types:?}:\n{gated_main}"
        );
        // Whoever emitted it, nothing may keep granting over the vault's
        // namespace once the deployer declines it.
        assert_no_ungated_grant_over(
            gated_main,
            "${local.resource_prefix}-app-tokens-",
            VAULT_GATE,
        );

        assert_gated_registration_list(gated_main, "var.input_vault_enabled");
        assert_terraform_valid(&gated, &format!("gated {name} vault stack"));
    }
}

/// The project-wide custom roles are shared across resources and carry their own
/// `var.gcp_manage_custom_roles` count. Adding one vault's gate would both
/// double-count them and delete roles the vault's siblings still hold, so only
/// the bindings beside them follow the gate.
#[test]
fn a_gated_gcp_vault_leaves_shared_custom_roles_alone() {
    let module = render(
        &vault_stack_with_custom_role(true),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"google_project_iam_custom_role\""),
        "the fixture must actually render a custom role, or this proves nothing:\n{main}"
    );
    let gated_types = gated_block_types(main, VAULT_GATE);
    assert!(
        !gated_types
            .iter()
            .any(|found| found == "google_project_iam_custom_role"),
        "a shared custom role must not follow one vault's gate, found {gated_types:?}:\n{main}"
    );
    assert!(
        gated_types
            .iter()
            .any(|found| found == "google_project_iam_member"),
        "the bindings beside it still follow the gate, found {gated_types:?}:\n{main}"
    );
    assert!(
        main.contains("count = var.gcp_manage_custom_roles ? 1 : 0"),
        "the custom role keeps the count it already had:\n{main}"
    );
    assert_terraform_valid(&module, "gated gcp vault stack with a custom role");
}

#[test]
fn an_ungated_prefix_only_vault_is_untouched() {
    for (target, name) in [(TerraformTarget::Aws, "aws"), (TerraformTarget::Gcp, "gcp")] {
        let main = &normalized(&render(
            &vault_stack(false),
            target,
            StackSettings::default(),
        ));

        assert!(
            !main.contains(VAULT_GATE),
            "nothing is gated, so no count belongs in the {name} module:\n{main}"
        );
        assert_ungated_registration_list_is_a_plain_array(main);
    }
}

// -------------------------------------------------------------- Azure vault

/// Azure is the one cloud where the vault is a real resource, and every role
/// assignment names it twice — once as `scope`, once inside the `uuidv5` seed
/// that derives the assignment's name. The seed is a raw interpolation, so it
/// is the reference most likely to be left behind.
#[test]
fn a_gated_azure_vault_indexes_both_references_in_every_role_assignment() {
    let module = render(
        &azure_vault_stack(true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"azurerm_role_assignment\""),
        "the fixture must actually render a role assignment, or this test proves nothing:\n{main}"
    );
    assert!(
        main.contains(VAULT_GATE),
        "the vault must be created only when the deployer says yes:\n{main}"
    );
    assert!(
        main.contains("scope = azurerm_key_vault.app_tokens[0].id"),
        "the assignment scopes to the counted vault:\n{main}"
    );
    assert!(
        main.contains("vault-role-assign:${azurerm_key_vault.app_tokens[0].id}"),
        "the uuidv5 seed must index too, not just the scope:\n{main}"
    );
    assert!(
        !main.contains("azurerm_key_vault.app_tokens."),
        "no attribute may be read off the vault without an index:\n{main}"
    );
    assert_gated_registration_list(main, "var.input_vault_enabled");
    assert_terraform_valid(&module, "gated azure vault stack");
}

/// The random suffix feeding the vault's name is a local value with no cloud
/// footprint. Leaving it uncounted is what keeps the vault's reference to it
/// unindexed, so the two decisions have to stay in step.
#[test]
fn a_gated_azure_vault_leaves_its_name_suffix_uncounted() {
    let module = render(
        &azure_vault_stack(true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("random_id.app_tokens_name_suffix.hex"),
        "the vault name reads the suffix directly:\n{main}"
    );
    assert!(
        !main.contains("random_id.app_tokens_name_suffix[0]"),
        "an uncounted suffix must not be indexed:\n{main}"
    );
}

#[test]
fn an_ungated_azure_vault_is_untouched() {
    let module = render(
        &azure_vault_stack(false),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("azurerm_key_vault.app_tokens[0]"),
        "no indexing on an ungated vault:\n{main}"
    );
    assert!(
        main.contains("scope = azurerm_key_vault.app_tokens.id")
            && main.contains("vault-role-assign:${azurerm_key_vault.app_tokens.id}"),
        "references stay unindexed:\n{main}"
    );
    assert!(
        !main.contains(VAULT_GATE),
        "nothing is gated, so no count belongs in the module:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}
