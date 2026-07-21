use super::helpers::{
    assert_terraform_valid, assert_ungated_registration_list_is_a_plain_array, gate_input,
    normalized, render, try_render,
};
use alien_core::{
    AzureResourceGroup, AzureStorageAccount, Kv, PermissionProfile, ResourceLifecycle,
    ServiceAccount, Stack, StackBuilder, StackSettings,
};
use alien_terraform::TerraformTarget;

fn gated_kv_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate_input(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
        )])
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build()
}

fn ungated_kv_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate_input(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
        )])
        .add(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn gated_azure_kv_stack() -> Stack {
    azure_kv_stack_base()
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build()
}

fn ungated_azure_kv_stack() -> Stack {
    azure_kv_stack_base()
        .add(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// Azure KV is realised as a table inside a shared Storage account, so the
/// stack needs the auxiliary resources the preflight pipeline injects at
/// runtime. Neither of them is gated; only the table the tests add on top.
fn azure_kv_stack_base() -> StackBuilder {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate_input(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
        )])
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

/// The point of the whole feature: the table itself is conditional, not just
/// something derived from it.
#[test]
fn a_gated_resource_renders_the_table_conditionally() {
    let module = render(
        &gated_kv_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"aws_dynamodb_table\""),
        "the table block is still declared:\n{main}"
    );
    assert!(
        main.contains("count = var.input_store_enabled ? 1 : 0"),
        "the table must be created only when the deployer says yes:\n{main}"
    );
    assert_terraform_valid(&module, "gated kv stack");
}

/// The manager deserializes every registration entry into typed import data
/// with required fields, so a declined resource has to be absent from the list
/// rather than present with a null payload.
#[test]
fn a_gated_resource_drops_out_of_the_registration_list() {
    let module = render(
        &gated_kv_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("aws_dynamodb_table.store[0].name"),
        "references to a counted resource must be indexed:\n{main}"
    );
    assert!(
        main.contains("deployment_resources = concat("),
        "a gated stack splices its registration list together:\n{main}"
    );
    assert!(
        main.contains("var.input_store_enabled ? [") && main.contains("] : []"),
        "the gated entry must collapse to an empty list, not to null:\n{main}"
    );
    assert!(
        !main.contains(": null"),
        "no null may reach the registration payload:\n{main}"
    );
}

/// An ungated stack's output must not change, or every existing deployment
/// would see a template diff on its next re-apply.
#[test]
fn an_ungated_resource_is_untouched() {
    let module = render(
        &ungated_kv_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("aws_dynamodb_table.store[0]"),
        "no indexing on an ungated table:\n{main}"
    );
    assert!(
        main.contains("aws_dynamodb_table.store.name"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}

#[test]
fn a_gated_gcp_resource_renders_the_database_conditionally() {
    let module = render(
        &gated_kv_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"google_firestore_database\""),
        "the database block is still declared:\n{main}"
    );
    assert!(
        main.contains("count = var.input_store_enabled ? 1 : 0"),
        "the database must be created only when the deployer says yes:\n{main}"
    );
    assert_terraform_valid(&module, "gated gcp kv stack");
}

#[test]
fn a_gated_gcp_resource_drops_out_of_the_registration_list() {
    let module = render(
        &gated_kv_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("google_firestore_database.store[0].name"),
        "references to a counted resource must be indexed:\n{main}"
    );
    assert!(
        main.contains("google_firestore_database.store[0].location_id"),
        "every self-reference must be indexed, not just the first:\n{main}"
    );
    assert!(
        main.contains("deployment_resources = concat("),
        "a gated stack splices its registration list together:\n{main}"
    );
    assert!(
        main.contains("var.input_store_enabled ? [") && main.contains("] : []"),
        "the gated entry must collapse to an empty list, not to null:\n{main}"
    );
    assert!(
        !main.contains(": null"),
        "no null may reach the registration payload:\n{main}"
    );
}

#[test]
fn an_ungated_gcp_resource_is_untouched() {
    let module = render(
        &ungated_kv_stack(),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("google_firestore_database.store[0]"),
        "no indexing on an ungated database:\n{main}"
    );
    assert!(
        main.contains("google_firestore_database.store.name"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}

#[test]
fn a_gated_azure_resource_renders_the_table_conditionally() {
    let module = render(
        &gated_azure_kv_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("resource \"azurerm_storage_table\""),
        "the table block is still declared:\n{main}"
    );
    assert!(
        main.contains("count = var.input_store_enabled ? 1 : 0"),
        "the table must be created only when the deployer says yes:\n{main}"
    );
    assert_terraform_valid(&module, "gated azure kv stack");
}

/// The Azure payload mixes references to the gated table with references to the
/// ungated Storage account that hosts it. Indexing the latter would produce
/// Terraform that does not validate.
#[test]
fn a_gated_azure_resource_indexes_only_its_own_references() {
    let module = render(
        &gated_azure_kv_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        main.contains("azurerm_storage_table.store[0].name"),
        "references to the counted table must be indexed:\n{main}"
    );
    assert!(
        !main.contains("azurerm_storage_account.default_storage_account[0]"),
        "the parent Storage account is not counted, so it must stay unindexed:\n{main}"
    );
    assert!(
        main.contains("azurerm_storage_account.default_storage_account.primary_table_endpoint"),
        "the parent endpoint is still read directly:\n{main}"
    );
    assert!(
        main.contains("var.input_store_enabled ? [") && main.contains("] : []"),
        "the gated entry must collapse to an empty list, not to null:\n{main}"
    );
    assert!(
        !main.contains(": null"),
        "no null may reach the registration payload:\n{main}"
    );
}

#[test]
fn an_ungated_azure_resource_is_untouched() {
    let module = render(
        &ungated_azure_kv_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let main = &normalized(&module);

    assert!(
        !main.contains("azurerm_storage_table.store[0]"),
        "no indexing on an ungated table:\n{main}"
    );
    assert!(
        main.contains("azurerm_storage_table.store.name"),
        "references stay unindexed:\n{main}"
    );
    assert_ungated_registration_list_is_a_plain_array(main);
}

/// A resource-scoped grant renders through the service account's profile loop
/// too, and on GCP the kv binding is project-wide — Firestore cannot scope IAM
/// to a table — so the binding must follow the store's gate or a decline
/// leaves project-wide data access behind.
#[test]
fn a_gated_gcp_stores_profile_grant_follows_its_gate() {
    let stack = Stack::new("gated-stack".to_string())
        .inputs(vec![gate_input(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
        )])
        .permission(
            "execution",
            PermissionProfile::new().resource("store", ["kv/data-write"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build();

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let main = normalized(&module);

    assert!(
        main.contains("roles/datastore.user"),
        "the fixture must render the data grant, or this test proves nothing:\n{main}"
    );
    for chunk in main.split("resource \"").skip(1) {
        if chunk.starts_with("google_project_iam_member") && chunk.contains("roles/datastore.user")
        {
            assert!(
                chunk.contains("count = var.input_store_enabled ? 1 : 0"),
                "an ungated copy of the store's grant would survive a decline:\n{chunk}"
            );
        }
    }
    assert_terraform_valid(&module, "gated gcp kv with a profile grant");
}

/// Rendering a gated resource through an emitter that ignores the gate would
/// create exactly what the deployer declined. ServiceAccount stays a safe
/// stand-in for "unconverted": the compile-time check forbids gating
/// framework-derived types, so its emitter never needs to convert.
#[test]
fn a_gate_on_an_unconverted_emitter_fails() {
    let stack = Stack::new("gated-stack".to_string())
        .inputs(vec![gate_input(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
        )])
        .add_enabled_when(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build();

    let error = try_render(&stack, TerraformTarget::Aws, StackSettings::default())
        .expect_err("should refuse to render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error.message.contains("service-account"),
        "the error should name the resource type: {}",
        error.message
    );
    assert!(
        error.message.contains("execution-sa"),
        "the error should name the resource: {}",
        error.message
    );
}
