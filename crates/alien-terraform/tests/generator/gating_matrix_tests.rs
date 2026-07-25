//! The gating matrix: every registered Terraform emitter is either refused by
//! the gateability policy or proven to render gated.
//!
//! The walk over [`TfRegistry::registered_keys`] is what makes the generic
//! post-pass safe to extend: registering a new emitter for a gateable type
//! fails this test until a gated fixture exists, so a type can never become
//! gateable without a validated gated render.

use super::helpers::{assert_terraform_valid, gate_input, render, snapshot_module};
use alien_core::{
    ownership_policy_for_resource_type, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, Kv, Platform, Queue, ResourceLifecycle, Stack, StackBuilder,
    StackSettings, Storage, Vault, Worker, WorkerCode,
};
use alien_terraform::{TerraformTarget, TfRegistry};

/// One gated fixture stack per policy-allowed resource type with a setup
/// emitter. The resource under test is gated; auxiliary resources the type
/// needs (Azure's resource group and storage account) are not.
fn gated_fixture(resource_type: &str, platform: Platform) -> Option<Stack> {
    let base = || -> StackBuilder {
        let builder = Stack::new("matrix-stack".to_string()).inputs(vec![gate_input(
            "fixtureEnabled",
            "Enable the fixture resource",
            "Whether to create the gated matrix fixture.",
        )]);
        if platform == Platform::Azure {
            builder
                .add(
                    AzureResourceGroup::new("default-resource-group".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    AzureStorageAccount::new("default-storage-account".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
        } else {
            builder
        }
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
        "queue" => {
            let builder = if platform == Platform::Azure {
                base().add(
                    AzureServiceBusNamespace::new("default-service-bus-namespace".to_string())
                        .build(),
                    ResourceLifecycle::Frozen,
                )
            } else {
                base()
            };
            builder.add_enabled_when(
                Queue::new("fixture".to_string()).build(),
                ResourceLifecycle::Frozen,
                "fixtureEnabled",
            )
        }
        "vault" => base().add_enabled_when(
            Vault::new("fixture".to_string()).build(),
            ResourceLifecycle::Frozen,
            "fixtureEnabled",
        ),
        _ => return None,
    };
    Some(stack.build())
}

fn target_for(platform: Platform) -> Option<TerraformTarget> {
    match platform {
        Platform::Aws => Some(TerraformTarget::Aws),
        Platform::Gcp => Some(TerraformTarget::Gcp),
        Platform::Azure => Some(TerraformTarget::Azure),
        _ => None,
    }
}

/// A declined fixture must leave no trace: its registration entry is spliced
/// out and every one of its blocks carries the gate's count. Shared support
/// blocks (custom roles, role definitions) may remain, unbound.
fn assert_gated_render(resource_type: &str, platform: Platform, stack: &Stack) {
    let Some(target) = target_for(platform) else {
        return;
    };
    let module = render(stack, target, StackSettings::default());
    assert_terraform_valid(
        &module,
        &format!("gating matrix {resource_type} on {platform:?}"),
    );

    let locals = module
        .files
        .get("locals.tf")
        .expect("locals.tf should exist");
    assert!(
        locals.contains("var.input_fixture_enabled ?"),
        "{resource_type}/{platform:?}: the registration list should splice the gated entry \
         behind its input:\n{locals}"
    );

    // `terraform validate` cannot tell the accepted render from the declined
    // one — the gate variable is unknown at validate time, so `count` is
    // unknown and both branches type-check. These two assertions are what
    // actually prove the gate, per rendered block.
    let gate_count = "var.input_fixture_enabled ? 1 : 0";
    let mut gated_blocks = 0usize;
    for (file_name, contents) in module.iter() {
        if !file_name.ends_with(".tf") {
            continue;
        }
        // Leading newline so a block that opens the file still splits out.
        let padded = format!("\n{contents}");
        for block in padded.split("\nresource \"").skip(1) {
            let header = block.lines().next().unwrap_or_default();
            let counted = block.contains(gate_count);
            if counted {
                gated_blocks += 1;
            }

            // Every block that reads the gated resource's indexed address
            // must itself be gated, or the declined apply resolves an index
            // into an empty list.
            if block.contains("[0]") && !counted && !is_shared_support_block(header) {
                panic!(
                    "{resource_type}/{platform:?}: `resource \"{header}` reads a gated \
                     address but carries no gate:\n{block}"
                );
            }
        }
    }
    // Some types own no blocks at all — a vault on AWS/GCP is name-prefixed
    // inside an existing secret store, so dropping its registration entry
    // (asserted above) IS its gate. Where the render does reach into the
    // resource, though, something must be counted.
    let reaches_into_the_resource = module
        .iter()
        .any(|(name, contents)| name.ends_with(".tf") && contents.contains("[0]"));
    assert!(
        gated_blocks > 0 || !reaches_into_the_resource,
        "{resource_type}/{platform:?}: the render indexes the gated resource but no block \
         carries the gate"
    );
}

/// Blocks the design deliberately leaves ungated: shared definitions other
/// resources reference (they grant nothing on their own), and the
/// allowlisted footprintless residual.
fn is_shared_support_block(header: &str) -> bool {
    header.contains("google_project_iam_custom_role")
        || header.contains("azurerm_role_definition")
        || header.contains("random_id")
}

#[test]
fn every_registered_emitter_is_policy_refused_or_renders_gated() {
    let registry = TfRegistry::built_in();
    let mut allowed_without_fixture = Vec::new();

    for (resource_type, platform) in registry.registered_keys() {
        if alien_core::gate_refusal(resource_type, "matrix-fixture").is_some() {
            // The refused side of the matrix: the policy blocks the gate at
            // preflight AND at render (proven by
            // `a_gate_on_a_policy_refused_type_fails_at_render`).
            continue;
        }
        if !ownership_policy_for_resource_type(resource_type).allows_frozen() {
            assert_live_gate_ignored_by_setup(resource_type, platform);
            continue;
        }
        match gated_fixture(resource_type, platform) {
            Some(stack) => assert_gated_render(resource_type, platform, &stack),
            None => allowed_without_fixture.push(format!("{resource_type} ({platform:?})")),
        }
    }

    assert!(
        allowed_without_fixture.is_empty(),
        "these registered emitters belong to policy-allowed types but have no gated fixture; \
         add one to this matrix (or refuse the type in alien_core::gateability) before the \
         type silently becomes gateable: {allowed_without_fixture:?}"
    );
}

/// A gated Live resource never reaches a setup module: the generator skips
/// Live lifecycles before gate handling, so setup renders as if the resource
/// were absent while still asking the deployer for the input the runtime
/// strip resolves.
fn assert_live_gate_ignored_by_setup(resource_type: &str, platform: Platform) {
    let Some(target) = target_for(platform) else {
        return;
    };
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

    let module = render(&stack, target, StackSettings::default());
    let locals = module
        .files
        .get("locals.tf")
        .expect("locals.tf should exist");
    assert!(
        !locals.contains("var.input_fixture_enabled ?"),
        "{resource_type}/{platform:?}: setup has no gated registration entry for a Live resource:\n{locals}"
    );
    assert!(
        !locals.contains("\"fixture\""),
        "{resource_type}/{platform:?}: a Live resource has no setup registration entry:\n{locals}"
    );
    let variables = module
        .files
        .get("variables.tf")
        .expect("variables.tf should exist");
    assert!(
        variables.contains("input_fixture_enabled"),
        "{resource_type}/{platform:?}: the deployer is still asked for the input the runtime          strip resolves"
    );
}

/// Vault is the first type whose gated render exists purely through the
/// post-pass; no vault emitter contains gating code. Snapshots lock the
/// render on each cloud.
#[test]
fn a_gated_vault_renders_conditionally_on_every_cloud() {
    for (platform, name) in [
        (Platform::Aws, "enabled_gated_vault_aws"),
        (Platform::Gcp, "enabled_gated_vault_gcp"),
        (Platform::Azure, "enabled_gated_vault_azure"),
    ] {
        let stack = gated_fixture("vault", platform).expect("vault fixture");
        let target = target_for(platform).expect("cloud target");
        let module = render(&stack, target, StackSettings::default());
        assert_terraform_valid(&module, name);
        snapshot_module(name, &module);
    }
}
