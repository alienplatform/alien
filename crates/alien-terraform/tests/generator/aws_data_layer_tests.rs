//! AWS data-layer scenarios — storage / kv / queue / vault / ai.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real AWS provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    Ai, Key, Kv, LifecycleRule, PermissionProfile, Queue, RemoteBindings, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, ServiceAccount, Stack,
    StackSettings, Storage, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn aws_key_package_is_valid_and_retained() {
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

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("prevent_destroy = true"));
    assert!(rendered.contains("\"kms:Encrypt\""));
    assert!(rendered.contains("\"kms:Decrypt\""));
    assert!(rendered.contains("aws_kms_key.customer_key.arn"));
    let detach = module
        .get("detach-retained-keys.sh")
        .expect("retained Key detach operation");
    assert!(detach.contains("detach_if_present 'aws_kms_key.customer_key'"));
    assert!(module
        .get("README.md")
        .unwrap()
        .contains("terraform destroy"));
    assert_terraform_valid(&module, "aws_key_package");
}

#[cfg(unix)]
#[test]
fn retained_key_detach_operation_is_idempotent_and_narrow() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let stack = Stack::new("enterprise-key".to_string())
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let temp = tempfile::tempdir().unwrap();
    let detach_path = temp.path().join("detach-retained-keys.sh");
    fs::write(&detach_path, module.get("detach-retained-keys.sh").unwrap()).unwrap();
    fs::write(
        temp.path().join("state"),
        "aws_kms_key.customer_key\naws_iam_role.unrelated\n",
    )
    .unwrap();
    let terraform_path = temp.path().join("terraform");
    fs::write(
        &terraform_path,
        "#!/bin/sh\nset -eu\nif [ \"$1 $2\" = \"state list\" ]; then cat \"$TEST_STATE\"; exit 0; fi\nif [ \"$1 $2\" = \"state rm\" ]; then\n  grep -F -x -v \"$3\" \"$TEST_STATE\" > \"$TEST_STATE.next\"\n  mv \"$TEST_STATE.next\" \"$TEST_STATE\"\n  echo \"$3\" >> \"$TEST_LOG\"\n  exit 0\nfi\nexit 2\n",
    )
    .unwrap();
    fs::set_permissions(&terraform_path, fs::Permissions::from_mode(0o755)).unwrap();

    for _ in 0..2 {
        let path = format!(
            "{}:{}",
            temp.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let status = Command::new("sh")
            .arg(&detach_path)
            .env("ALIEN_CONFIRM_DETACH_RETAINED_KEYS", "yes")
            .env("PATH", path)
            .env("TEST_STATE", temp.path().join("state"))
            .env("TEST_LOG", temp.path().join("log"))
            .status()
            .unwrap();
        assert!(status.success());
    }

    assert_eq!(
        fs::read_to_string(temp.path().join("state")).unwrap(),
        "aws_iam_role.unrelated\n"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("log")).unwrap(),
        "aws_kms_key.customer_key\n"
    );
}

#[test]
fn aws_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_minimal", &module);
    assert_terraform_valid(&module, "aws_storage_minimal");
}

#[test]
fn aws_storage_uses_customer_managed_key() {
    let stack = Stack::new("encrypted-storage".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().resource("data", ["storage/data-write"]),
        ))
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
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("sse_algorithm     = \"aws:kms\""));
    assert!(rendered.contains("kms_master_key_id = aws_kms_key.customer_key.arn"));
    assert!(rendered.contains("kms:GenerateDataKey"));
    assert!(rendered.contains("kms:Decrypt"));
    assert_terraform_valid(&module, "aws_encrypted_storage");
}

#[test]
fn aws_storage_with_versioning_and_lifecycle() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "aws_storage_versioning_and_lifecycle");
}

#[test]
fn aws_storage_public_read_allows_get_object() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_public_read", &module);
    assert_terraform_valid(&module, "aws_storage_public_read");
}

#[test]
fn aws_byo_bucket_is_acyclic_and_valid() {
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

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_byo_bucket", &module);
    assert_terraform_valid(&module, "aws_remote_storage_management_dependencies");
}

#[test]
fn aws_kv_renders_dynamodb_table_with_pitr() {
    let stack = Stack::new("acme-kv".to_string())
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_kv_minimal", &module);
    assert_terraform_valid(&module, "aws_kv_minimal");
}

#[test]
fn aws_queue_renders_sqs_with_managed_sse() {
    let stack = Stack::new("acme-queue".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_queue_minimal", &module);
    assert_terraform_valid(&module, "aws_queue_minimal");
}

#[test]
fn aws_vault_emits_only_import_data() {
    let stack = Stack::new("acme-vault".to_string())
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_vault_minimal", &module);
    assert_terraform_valid(&module, "aws_vault_minimal");
}

#[test]
fn aws_vault_resource_permissions_attach_to_service_account_role() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("aws_iam_role_policy\" \"execution_sa_vault_secrets_set_0\""));
    assert!(rendered.contains("ssm:GetParameter"));
    assert!(rendered.contains("parameter/${local.resource_prefix}-secrets-*"));
    assert_terraform_valid(&module, "aws_vault_service_account_permissions");
}

#[test]
fn aws_data_layer_renders_complete_stack() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_data_layer_full", &module);
    assert_terraform_valid(&module, "aws_data_layer_full");
}

#[test]
fn aws_ai_emits_only_import_data() {
    // AWS Bedrock has no per-stack cloud resource to provision. The emitter
    // returns an empty fragment so only the import metadata JSON is produced.
    let stack = Stack::new("acme-ai".to_string())
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_ai_minimal", &module);
    assert_terraform_valid(&module, "aws_ai_minimal");

    // Import metadata must carry the region so the controller can reconstruct
    // the Bedrock endpoint. The import ref appears in locals.tf.
    let locals = module.get("locals.tf").expect("locals.tf should render");
    assert!(locals.contains("region"), "import ref must carry region");
}

#[test]
fn aws_ai_invoke_permissions_attach_to_service_account_role() {
    // When a permission profile references ai/invoke, the AI emitter attaches the
    // bedrock IAM policy to the workload (service-account) role.
    let stack = Stack::new("acme-ai".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("llm", ["ai/invoke"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(
        rendered.contains("bedrock:InvokeModel"),
        "bedrock InvokeModel action must appear"
    );
    assert!(
        rendered.contains("bedrock:InvokeModelWithResponseStream"),
        "bedrock InvokeModelWithResponseStream action must appear"
    );
    assert!(
        rendered.contains("arn:aws:bedrock:*::foundation-model/*"),
        "bedrock foundation-model ARN must appear"
    );
    assert_terraform_valid(&module, "aws_ai_invoke_permissions");
}

#[test]
fn aws_remote_ai_invoke_permissions_attach_to_access_role() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("bedrock:InvokeModel"));
    assert!(rendered.contains("aws_iam_role.access.name"));
    assert_terraform_valid(&module, "aws_remote_ai_invoke_permissions");
}

/// The grant a remote caller's credentials are bounded by.
///
/// Nothing else in the module attaches it: the setup package is where the Remote Bindings
/// identity gets its policies, so without this the manager mints a session against a role that
/// carries none and every call comes back AccessDenied.
#[test]
fn aws_remote_sandbox_grants_the_access_identity_its_own_image_and_nothing_wider() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox".to_string())
        .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    let policy = rendered
        .split("resource \"aws_iam_role_policy\" \"access_agents_remote_execute\"")
        .nth(1)
        .expect("the remote grant attaches to the Remote Bindings role")
        .split("\nresource ")
        .next()
        .expect("block ends");

    for action in [
        "lambda:RunMicrovm",
        "lambda:SuspendMicrovm",
        "lambda:ResumeMicrovm",
        "lambda:TerminateMicrovm",
        "lambda:CreateMicrovmAuthToken",
        "lambda:GetMicrovm",
    ] {
        assert!(policy.contains(action), "{action} is missing:\n{policy}");
    }
    assert!(
        policy.contains("microvm-image:${local.resource_prefix}-agents"),
        "the grant must name this sandbox's image, not a stack-wide pattern:\n{policy}"
    );
    assert!(
        !policy.contains("microvm-image:${local.resource_prefix}-*"),
        "a stack-wide pattern would reach every sibling sandbox:\n{policy}"
    );
    for withheld in [
        "lambda:PassNetworkConnector",
        "iam:PassRole",
        "lambda:CreateMicrovmShellAuthToken",
    ] {
        assert!(
            !policy.contains(withheld),
            "{withheld} must stay out of the remote grant:\n{policy}"
        );
    }

    assert_terraform_valid(&module, "aws_remote_sandbox");
}
