//! GCP data-layer scenarios — storage / kv / queue / vault / ai.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real Google provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    Ai, Key, Kv, LifecycleRule, ManagementPermissions, PermissionProfile, PermissionsConfig, Queue,
    RemoteBindings, RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack,
    StackSettings, Storage, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn gcp_key_package_is_valid_and_retained() {
    let mut stack = Stack::new("enterprise-key".to_string())
        .add_with_remote_access(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack
        .resources
        .get_mut("customer-key")
        .unwrap()
        .dependencies = vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("google_kms_key_ring"));
    assert!(rendered.contains("random_id.customer_key_ring_suffix.hex"));
    assert!(rendered.contains("prevent_destroy = true"));
    assert!(rendered.contains("roles/cloudkms.cryptoKeyEncrypterDecrypter"));
    let detach = module
        .get("detach-retained-keys.sh")
        .expect("retained Key detach operation");
    assert!(detach.contains("google_kms_crypto_key.customer_key"));
    assert!(detach.contains("google_kms_key_ring.customer_key_ring"));
    assert_terraform_valid(&module, "gcp_key_package");
}

#[test]
fn gcp_storage_uses_customer_managed_key() {
    let stack = Stack::new("encrypted-storage".to_string())
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("data".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("default_kms_key_name = google_kms_crypto_key.customer_key.id"));
    assert!(rendered.contains("google_storage_project_service_account"));
    assert!(rendered.contains("roles/cloudkms.cryptoKeyEncrypterDecrypter"));
    assert!(rendered.contains("depends_on"));
    assert!(rendered.contains("google_kms_crypto_key_iam_member.data_storage_encryption"));
    assert_terraform_valid(&module, "gcp_encrypted_storage");
}

#[test]
fn gcp_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_minimal", &module);
    assert_terraform_valid(&module, "gcp_storage_minimal");
}

#[test]
fn gcp_storage_with_versioning_and_lifecycle() {
    let stack = Stack::new("acme-audit".to_string())
        .add(
            Storage::new("audit".to_string())
                .versioning(true)
                .lifecycle_rules(vec![LifecycleRule {
                    days: 90,
                    prefix: Some("logs/".to_string()),
                }])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "gcp_storage_versioning_and_lifecycle");
}

#[test]
fn gcp_storage_public_read_allows_object_viewer() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_public_read", &module);
    assert_terraform_valid(&module, "gcp_storage_public_read");
}

#[test]
fn gcp_storage_remote_access_grants_exact_role_to_remote_bindings_identity() {
    let mut stack = Stack::new("acme-remote-storage".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["storage/heartbeat"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Storage::new("uploads".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.resources.get_mut("uploads").unwrap().dependencies = vec![
        ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management"),
        ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access"),
    ];
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_remote_access", &module);
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered
        .contains("google_project_iam_custom_role\" \"gcp_role_read_write_bucket_objects\""));
    assert!(rendered.contains(
        "google_storage_bucket_iam_member\" \"gcp_role_read_write_bucket_objects_uploads_access_storage_0\""
    ));
    assert!(rendered.contains("google_service_account.access.email"));
    assert!(
        rendered.contains("member = \"serviceAccount:${google_service_account.management.email}\"")
    );
    assert!(rendered.contains("\"storage.objects.get\""));
    assert!(rendered.contains("\"storage.objects.list\""));
    assert!(rendered.contains("\"storage.objects.create\""));
    assert!(rendered.contains("\"storage.objects.delete\""));
    assert!(!rendered.contains("\"iam.serviceAccounts.signBlob\""));
    assert_terraform_valid(&module, "gcp_storage_remote_access");
}

#[test]
fn gcp_byo_bucket_is_acyclic_and_valid() {
    let mut stack = Stack::new("acme-remote-storage".to_string())
        .add_with_remote_access(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.resources.get_mut("files").unwrap().dependencies =
        vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_byo_bucket", &module);
    assert_terraform_valid(&module, "gcp_remote_storage_management_dependencies");
}

#[test]
fn gcp_kv_renders_firestore_database() {
    let stack = Stack::new("acme-kv".to_string())
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_kv_minimal", &module);
    assert_terraform_valid(&module, "gcp_kv_minimal");
}

#[test]
fn gcp_queue_renders_pubsub_topic_and_subscription() {
    let stack = Stack::new("acme-queue".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_queue_minimal", &module);
    assert_terraform_valid(&module, "gcp_queue_minimal");
}

#[test]
fn gcp_queue_permission_profile_splits_topic_and_subscription_iam() {
    let stack = Stack::new("acme-queue".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().resource("jobs", ["queue/data-write"]),
        ))
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("google_pubsub_topic_iam_member"));
    assert!(rendered.contains("roles/pubsub.publisher"));
    assert!(rendered.contains("google_pubsub_subscription_iam_member"));
    assert!(rendered.contains("roles/pubsub.subscriber"));
    assert!(rendered.contains("roles/pubsub.viewer"));
    assert_terraform_valid(&module, "gcp_queue_permission_profile");
}

#[test]
fn gcp_vault_emits_only_import_data() {
    let stack = Stack::new("acme-vault".to_string())
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_vault_minimal", &module);
    assert_terraform_valid(&module, "gcp_vault_minimal");
}

#[test]
fn gcp_vault_resource_permissions_attach_to_service_account() {
    let stack = Stack::new("acme-vault".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("secrets", ["vault/data-read"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("roles/secretmanager.secretAccessor"));
    assert!(rendered.contains("google_service_account.execution_sa.email"));
    assert!(rendered.contains("local.resource_prefix}-secrets-"));
    assert_terraform_valid(&module, "gcp_vault_service_account_permissions");
}

#[test]
fn gcp_vault_management_permissions_disambiguate_iam_member_labels() {
    let stack = Stack::new("acme-vault".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("secrets", ["vault/heartbeat", "vault/management"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("secretmanager_viewer_management_secrets_vault_heartbeat_binding_0"));
    assert!(rendered.contains("secretmanager_viewer_management_secrets_vault_management_binding_0"));
    assert_terraform_valid(&module, "gcp_vault_management_permission_labels");
}

#[test]
fn gcp_data_layer_renders_complete_stack() {
    let stack = Stack::new("acme-data".to_string())
        .add(
            Storage::new("assets".to_string()).versioning(true).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_data_layer_full", &module);
    assert_terraform_valid(&module, "gcp_data_layer_full");
}

#[test]
fn gcp_ai_emits_only_import_data() {
    // GCP Vertex AI has no per-stack cloud resource to provision. The emitter
    // returns an empty fragment so only the import metadata JSON is produced.
    let stack = Stack::new("acme-ai".to_string())
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_ai_minimal", &module);
    assert_terraform_valid(&module, "gcp_ai_minimal");

    // Import metadata must carry project and location so the controller can
    // reconstruct the Vertex AI endpoint. The import ref appears in locals.tf.
    let locals = module.get("locals.tf").expect("locals.tf should render");
    assert!(
        locals.contains("projectId"),
        "import ref must carry projectId"
    );
    assert!(
        locals.contains("location"),
        "import ref must carry location"
    );
}

#[test]
fn gcp_ai_invoke_permissions_attach_to_service_account() {
    // When a permission profile references ai/invoke, the AI emitter emits a custom
    // role containing only predict permissions (not the over-broad
    // roles/aiplatform.user) and binds it to the workload service account.
    let stack = Stack::new("acme-ai".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().resource("llm", ["ai/invoke"]),
        ))
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(
        !rendered.contains("roles/aiplatform.user"),
        "roles/aiplatform.user must NOT appear in rendered output; ai/invoke uses a least-privilege custom role"
    );
    assert!(
        rendered.contains("aiplatform.endpoints.predict"),
        "aiplatform.endpoints.predict must appear in the custom role in rendered output"
    );
    assert_terraform_valid(&module, "gcp_ai_invoke_permissions");
}

#[test]
fn gcp_remote_ai_invoke_permissions_attach_to_access_identity() {
    let stack = Stack::new("remote-ai".to_string())
        .add_with_remote_access(
            Ai::new("models".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("aiplatform.endpoints.predict"));
    assert!(rendered.contains("google_service_account.access.email"));
    assert_terraform_valid(&module, "gcp_remote_ai_invoke_permissions");
}
