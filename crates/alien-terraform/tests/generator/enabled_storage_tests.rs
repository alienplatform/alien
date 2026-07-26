//! `.enabled(input)` on `storage` — the multi-block case.
//!
//! `kv` renders one block, so gating it is one `count` and one indexed
//! reference. `storage` renders a bucket plus a run of configuration siblings
//! (encryption, ownership, public-access block, policy, versioning, lifecycle)
//! and the IAM policies that name the bucket. Two ways to get that wrong:
//!
//! - a sibling without `count` outlives the bucket and, at apply time, points
//!   at an element of an empty list
//! - a self-reference without `[0]` does not survive `terraform validate` at all
//!
//! Every stack below therefore turns on versioning and lifecycle rules (so the
//! optional blocks render) and grants a service account access to the bucket
//! (so the IAM policies render), then runs the real `terraform validate`.

use super::helpers::{
    assert_terraform_valid, assert_ungated_registration_list_is_a_plain_array, gate_input,
    normalized, render,
};
use alien_core::{
    AzureResourceGroup, AzureStorageAccount, LifecycleRule, PermissionProfile, ResourceLifecycle,
    ServiceAccount, Stack, StackBuilder, StackSettings, Storage,
};
use alien_terraform::TerraformTarget;

const GATE: &str = "count = var.input_files_enabled ? 1 : 0";

/// Versioning and lifecycle rules are on so the optional blocks render, and the
/// permission profile is what makes the emitter produce IAM policies naming the
/// bucket. Both are the parts a single-block resource never exercises.
fn storage() -> Storage {
    Storage::new("files".to_string())
        .versioning(true)
        .lifecycle_rules(vec![LifecycleRule {
            days: 30,
            prefix: Some("tmp/".to_string()),
        }])
        .build()
}

fn stack_base() -> StackBuilder {
    Stack::new("gated-storage".to_string())
        .inputs(vec![gate_input(
            "filesEnabled",
            "Enable the bucket",
            "Whether to create the object-storage bucket.",
        )])
        .permission(
            "app",
            PermissionProfile::new().resource(
                "files",
                ["storage/data-write", "storage/trigger-management"],
            ),
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn gated_stack() -> Stack {
    stack_base()
        .add_enabled_when(storage(), ResourceLifecycle::Frozen, "filesEnabled")
        .build()
}

fn ungated_stack() -> Stack {
    stack_base()
        .add(storage(), ResourceLifecycle::Frozen)
        .build()
}

/// Azure realises a bucket as a container inside a shared Storage account, so
/// the stack needs the auxiliary resources the preflight pipeline injects.
/// Neither of them is gated; only the container on top.
fn azure_stack_base() -> StackBuilder {
    stack_base()
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn gated_azure_stack() -> Stack {
    azure_stack_base()
        .add_enabled_when(storage(), ResourceLifecycle::Frozen, "filesEnabled")
        .build()
}

fn ungated_azure_stack() -> Stack {
    azure_stack_base()
        .add(storage(), ResourceLifecycle::Frozen)
        .build()
}

/// Count the `resource "<type>" "<label>"` headers that carry the gate, so the
/// assertion is "all of them" rather than "at least the one I looked at".
fn gated_block_types(main: &str) -> Vec<String> {
    let mut types = Vec::new();
    for (index, _) in main.match_indices("resource \"") {
        let rest = &main[index + "resource \"".len()..];
        let Some((block_type, tail)) = rest.split_once('"') else {
            continue;
        };
        // The block body runs to the next `resource "` header.
        let body_end = tail.find("resource \"").unwrap_or(tail.len());
        if tail[..body_end].contains(GATE) {
            types.push(block_type.to_string());
        }
    }
    types
}

fn declared_block_types(main: &str) -> Vec<String> {
    main.match_indices("resource \"")
        .filter_map(|(index, _)| {
            main[index + "resource \"".len()..]
                .split_once('"')
                .map(|(block_type, _)| block_type.to_string())
        })
        .collect()
}

/// The whole point: the bucket disappears together with every sibling that only
/// describes it. A sibling left behind would reference an element of an empty
/// list the moment the deployer says no.
#[test]
fn every_aws_storage_block_carries_the_gate() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    let expected = [
        "aws_s3_bucket",
        "aws_s3_bucket_server_side_encryption_configuration",
        "aws_s3_bucket_ownership_controls",
        "aws_s3_bucket_public_access_block",
        "aws_s3_bucket_policy",
        "aws_s3_bucket_versioning",
        "aws_s3_bucket_lifecycle_configuration",
    ];
    let gated = gated_block_types(main);
    for block_type in expected {
        assert!(
            gated.iter().any(|found| found == block_type),
            "`{block_type}` must be created only when the deployer says yes:\n{main}"
        );
    }

    // The IAM policy embeds the bucket name in its document, so it cannot
    // outlive the bucket either.
    assert!(
        gated.iter().any(|found| found == "aws_iam_role_policy"),
        "the bucket's IAM policy must follow the same gate:\n{main}"
    );

    assert_terraform_valid(&module, "gated aws storage stack");
}

/// The failure this guards against renders fine and only breaks at
/// `terraform validate`: one sibling reading `aws_s3_bucket.files.id` while the
/// bucket is a counted list.
#[test]
fn aws_storage_indexes_every_self_reference() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    for attribute in ["id", "bucket", "arn"] {
        assert!(
            !main.contains(&format!("aws_s3_bucket.files.{attribute}")),
            "no unindexed `{attribute}` reference may survive:\n{main}"
        );
    }
    assert!(
        main.contains("aws_s3_bucket.files[0].id"),
        "the siblings read the counted bucket by index:\n{main}"
    );
    assert!(
        main.contains("aws_s3_bucket.files[0].arn"),
        "the bucket policy reads the counted bucket by index:\n{main}"
    );
    assert!(
        main.contains("aws_s3_bucket.files[0].bucket"),
        "the import ref and IAM policy read the counted bucket by index:\n{main}"
    );
}

/// The IAM policy is attached to a role this emitter does not own. Indexing that
/// role would produce Terraform that does not validate.
#[test]
fn aws_storage_leaves_the_service_account_role_unindexed() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("aws_iam_role.app_sa.id"),
        "the role the policy attaches to is ungated, so it stays unindexed:\n{main}"
    );
    assert!(
        !main.contains("aws_iam_role.app_sa[0]"),
        "the service-account role is not counted:\n{main}"
    );
}

/// The manager deserializes every registration entry into typed import data with
/// required fields, so a declined bucket has to be absent from the list rather
/// than present with a null payload.
#[test]
fn a_declined_aws_bucket_drops_out_of_the_registration_list() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("deployment_resources = concat("),
        "a gated stack splices its registration list together:\n{main}"
    );
    assert!(
        main.contains("var.input_files_enabled ? [") && main.contains("] : []"),
        "the gated entry must collapse to an empty list, not to null:\n{main}"
    );
    assert!(
        !main.contains(": null"),
        "no null may reach the registration payload:\n{main}"
    );
}

/// Ungated stacks are untouched: opt-in means no `.enabled(...)`, so no counts
/// and no indexing.
#[test]
fn an_ungated_aws_storage_stack_is_untouched() {
    let module = render(
        &ungated_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        gated_block_types(main).is_empty(),
        "nothing is gated, so no block gains a count:\n{main}"
    );
    assert!(
        !main.contains("aws_s3_bucket.files[0]"),
        "no indexing on an ungated bucket:\n{main}"
    );
    assert!(
        main.contains("aws_s3_bucket.files.id"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
    assert_terraform_valid(&module, "ungated aws storage stack");
}

#[test]
fn every_gcp_storage_block_carries_the_gate() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    let gated = gated_block_types(main);
    assert!(
        gated.iter().any(|found| found == "google_storage_bucket"),
        "the bucket must be created only when the deployer says yes:\n{main}"
    );
    assert!(
        gated
            .iter()
            .any(|found| found == "google_storage_bucket_iam_member"),
        "a grant on the bucket cannot outlive it:\n{main}"
    );
    assert!(
        gated
            .iter()
            .any(|found| found == "google_project_iam_member"),
        "the project-scoped signBlob grant renders through the service-account \
         path, not this emitter, so it must still follow the bucket's gate:\n{main}"
    );
    assert!(
        !main.contains("google_storage_bucket.files.name"),
        "no unindexed reference may survive:\n{main}"
    );
    assert!(
        main.contains("google_storage_bucket.files[0].name"),
        "the grants read the counted bucket by index:\n{main}"
    );
    assert_terraform_valid(&module, "gated gcp storage stack");
}

/// `storage/trigger-management` grants raw GCP permissions at bucket scope, so
/// this emitter defines a custom role for it. That block already carries its own
/// `var.gcp_manage_custom_roles` count and is referenced through it, so the
/// deployer's gate must stay off it — a second `count` on one block is not
/// renderable, and re-pointing the reference is not this emitter's to do.
#[test]
fn gcp_custom_roles_keep_their_own_gate() {
    let module = render(
        &gated_stack(),
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
        !gated_block_types(main)
            .iter()
            .any(|found| found == "google_project_iam_custom_role"),
        "the custom role keeps its own count, not the deployer's:\n{main}"
    );
    assert!(
        main.contains("var.gcp_manage_custom_roles ? 1 : 0"),
        "the custom role's own count is still there:\n{main}"
    );
}

/// Two `count` attributes in one block is the way a blanket "gate everything in
/// the fragment" pass fails: it renders, and Terraform rejects it.
#[test]
fn no_block_carries_two_counts() {
    for target in [
        TerraformTarget::Aws,
        TerraformTarget::Gcp,
        TerraformTarget::Azure,
    ] {
        let stack = if matches!(target, TerraformTarget::Azure) {
            gated_azure_stack()
        } else {
            gated_stack()
        };
        let module = render(&stack, target, StackSettings::default());
        for (path, contents) in module.iter() {
            for block in contents.split("\nresource \"").skip(1) {
                // Match `count` as a token, not a prefix: a name like
                // `account = …` must not read as a count, and `terraform fmt`
                // pads the `=` for alignment so a raw `"count ="` never matches.
                let counts = block
                    .lines()
                    .filter(|line| {
                        let mut tokens = line.split_whitespace();
                        tokens.next() == Some("count") && tokens.next() == Some("=")
                    })
                    .count();
                assert!(
                    counts <= 1,
                    "{target:?} {path} declares a block with {counts} counts:\n{block}"
                );
            }
        }
    }
}

#[test]
fn every_gcp_storage_self_reference_is_indexed() {
    let module = render(
        &gated_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    for attribute in ["name", "self_link", "location"] {
        assert!(
            !main.contains(&format!("google_storage_bucket.files.{attribute}")),
            "no unindexed `{attribute}` reference may survive:\n{main}"
        );
    }
    assert!(
        main.contains("google_storage_bucket.files[0].self_link"),
        "every import-ref attribute is indexed, not just the first:\n{main}"
    );
    assert!(
        main.contains("google_storage_bucket.files[0].location"),
        "every import-ref attribute is indexed, not just the first:\n{main}"
    );
}

#[test]
fn an_ungated_gcp_storage_stack_is_untouched() {
    let module = render(
        &ungated_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("google_storage_bucket.files[0]"),
        "no indexing on an ungated bucket:\n{main}"
    );
    assert!(
        main.contains("google_storage_bucket.files.name"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
    assert_terraform_valid(&module, "ungated gcp storage stack");
}

#[test]
fn every_azure_storage_block_carries_the_gate() {
    let module = render(
        &gated_azure_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    let gated = gated_block_types(main);
    for block_type in [
        "azurerm_storage_container",
        "azurerm_storage_management_policy",
        "azurerm_role_assignment",
    ] {
        assert!(
            gated.iter().any(|found| found == block_type),
            "`{block_type}` must be created only when the deployer says yes:\n{main}"
        );
    }
    assert_terraform_valid(&module, "gated azure storage stack");
}

/// The Azure payload mixes references to the gated container with references to
/// the ungated Storage account that hosts it. Indexing the latter would produce
/// Terraform that does not validate.
#[test]
fn a_gated_azure_container_indexes_only_its_own_references() {
    let module = render(
        &gated_azure_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("azurerm_storage_container.files[0].name"),
        "references to the counted container must be indexed:\n{main}"
    );
    assert!(
        !main.contains("azurerm_storage_account.default_storage_account[0]"),
        "the parent Storage account is not counted, so it must stay unindexed:\n{main}"
    );
    assert!(
        main.contains("azurerm_storage_account.default_storage_account.name"),
        "the parent account is still read directly:\n{main}"
    );
}

#[test]
fn an_ungated_azure_storage_stack_is_untouched() {
    let module = render(
        &ungated_azure_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        gated_block_types(main).is_empty(),
        "nothing is gated, so no block gains a count:\n{main}"
    );
    assert!(
        !main.contains("azurerm_storage_container.files[0]"),
        "no indexing on an ungated container:\n{main}"
    );
    assert!(
        main.contains("azurerm_storage_container.files.name"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
    assert_terraform_valid(&module, "ungated azure storage stack");
}
