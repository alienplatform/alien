//! AWS data-layer scenarios — storage / kv / queue / vault / ai.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real AWS provider.

use super::helpers::{
    assert_terraform_valid, gate_input, render, role_policy_documents, snapshot_module,
};
use alien_core::{
    Ai, ArtifactRegistry, Key, Kv, LifecycleRule, Network, NetworkSettings, PermissionProfile,
    Queue, RemoteBindings, ResourceLifecycle, ResourceRef, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, ServiceAccount, Stack, StackSettings, Storage, Vault,
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
    assert!(rendered.contains("terraform_data.customer_key_key_identity.output"));
    let inventory: Vec<String> =
        serde_json::from_str(module.get("retained-data.json").unwrap()).unwrap();
    assert_eq!(inventory, vec!["aws_kms_key.customer_key"]);
    assert_terraform_valid(&module, "aws_key_package");
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
    let mut files = super::helpers::linter_files(&module);
    let policy_references: Vec<_> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| {
            let body = hcl::parse(contents).unwrap();
            body.blocks()
                .filter(|block| {
                    block.identifier.as_str() == "resource"
                        && block
                            .labels
                            .first()
                            .is_some_and(|label| label.as_str() == "aws_iam_policy")
                })
                .map(|block| format!("aws_iam_policy.{}[*].policy", block.labels[1].as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    files.insert(
        "key-policy-qualification.tf".to_string(),
        format!(
            "output \"qualification_policy_documents\" {{\n value = flatten([{}])\n}}\n",
            policy_references.join(", ")
        ),
    );
    files.insert("key-qualification.tftest.hcl".to_string(), r#"
mock_provider "aws" {
  mock_data "aws_caller_identity" { defaults = { account_id = "123456789012" } }
  mock_data "aws_region" { defaults = { region = "us-east-1" } }
  mock_resource "aws_kms_key" {
    defaults = { arn = "arn:aws:kms:us-east-1:123456789012:key/selected", id = "selected" }
  }
  mock_resource "aws_iam_policy" {
    defaults = { arn = "arn:aws:iam::123456789012:policy/mock" }
  }
}
mock_provider "random" {}
variables {
  name = "encrypted-storage"
  token = "synthetic-setup-token"
  region = "us-east-1"
}
run "storage_and_workload_use_the_selected_key" {
  command = apply
  assert {
    condition = one(one(aws_s3_bucket_server_side_encryption_configuration.data.rule).apply_server_side_encryption_by_default).kms_master_key_id == "arn:aws:kms:us-east-1:123456789012:key/selected"
    error_message = "Bucket encryption must select the intended key ARN"
  }
  assert {
    condition = alltrue([for action in ["kms:GenerateDataKey", "kms:Decrypt"] : anytrue(flatten([
      for document in output.qualification_policy_documents : [
        for statement in jsondecode(document).Statement : statement.Effect == "Allow" && contains(try(tolist(statement.Action), [statement.Action]), action) && contains(try(tolist(statement.Resource), [statement.Resource]), "arn:aws:kms:us-east-1:123456789012:key/selected")
      ]
    ]))])
    error_message = "The workload must receive encryption and decryption grants for the selected key"
  }
}
"#.to_string());
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok("storage encryption and workload grants use the selected key");
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
            bundled_from: None,
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
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

    assert!(
        rendered.contains("resource \"aws_iam_role_policy\" \"access_agents_remote_execute\""),
        "the remote grant keeps its address on the Remote Bindings role"
    );
    let policy = role_policy_documents(&module, "access")
        .iter()
        .map(|(_, document)| document.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let policy = policy.as_str();

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
    // AWS attaches its own connectors to an open-egress session and authorizes each as
    // PassNetworkConnector, so a session cannot start without it. The literal `aws` account
    // segment is what stops the scope naming a customer-declared connector.
    assert!(
        policy.contains("aws:network-connector:aws-network-connector:*"),
        "the connector grant must be scoped to AWS-managed connectors:\n{policy}"
    );
    for withheld in ["iam:PassRole", "lambda:CreateMicrovmShellAuthToken"] {
        assert!(
            !policy.contains(withheld),
            "{withheld} must stay out of the remote grant:\n{policy}"
        );
    }

    insta::assert_snapshot!(
        "aws_remote_sandbox_permissions",
        module
            .files
            .get("PERMISSIONS.md")
            .expect("permission document")
    );
    assert_terraform_valid(&module, "aws_remote_sandbox");
}

/// A sandbox that routes egress through a connector is not remotely reachable, because
/// `sandbox/remote-execute` withholds `lambda:PassNetworkConnector`. Preflight refuses such a
/// stack; the emitter agrees, so a stack that reaches here another way installs no usable grant.
#[test]
fn aws_remote_sandbox_with_restricted_egress_carries_no_grant() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox-deny".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, settings);
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("awscc_lambda_network_connector\" \"agents\"")
            && rendered.contains("\"aws_iam_role\" \"access\""),
        "the deny connector and the Remote Bindings identity must still render:\n{rendered}"
    );
    assert!(
        !rendered.contains("access_agents_remote_execute"),
        "an egress-restricted sandbox must carry no remote execute grant:\n{rendered}"
    );
    assert!(
        !rendered.contains("lambda:CreateMicrovmAuthToken"),
        "nothing in the module may mint session credentials for this sandbox:\n{rendered}"
    );
}

/// The management identity must be able to report on a remotely-bound sandbox without gaining any
/// reach into its sessions. Mirrors the CloudFormation generator's assertion, because two setup
/// emitters compiling different management semantics for one stack is the failure this pins.
#[test]
fn aws_remote_sandbox_management_role_heartbeats_without_reaching_a_session() {
    let stack = Stack::new("byo-sandbox".to_string())
        .management(alien_core::permissions::ManagementPermissions::extend(
            PermissionProfile::new().global(["sandbox/heartbeat", "sandbox/management"]),
        ))
        .add_with_remote_access(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    // Only the management role's own files: the access role legitimately carries the session
    // verbs, so a whole-module grep would assert the opposite of what this pins.
    let rendered = module
        .files
        .iter()
        .filter(|(name, _)| name.contains("management"))
        .map(|(_, body)| body.clone())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("lambda:GetMicrovmImage"),
        "the management identity must be able to read the sandbox it reports on: {rendered}"
    );
    for reaches_a_session in [
        "lambda:RunMicrovm",
        "lambda:SuspendMicrovm",
        "lambda:ResumeMicrovm",
        "lambda:TerminateMicrovm",
        "lambda:CreateMicrovmAuthToken",
    ] {
        assert!(
            !rendered.contains(reaches_a_session),
            "{reaches_a_session} reaches a session and belongs to the remote caller alone"
        );
    }
}

#[test]
fn aws_existing_key_is_pinned_before_consumers_can_change() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut files = super::helpers::linter_files(&module);
    files.insert("key.tftest.hcl".to_string(), r#"
mock_provider "aws" {
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_region" {
    defaults = { region = "us-east-1" }
  }
}

mock_provider "random" {}
variables {
  name = "key-test"
  token = "test"
  region = "us-east-1"
  customer_key_existing_key = { arn = "arn:aws:kms:us-east-1:123456789012:key/first" }
}
run "use_existing_key" {
  command = apply
  assert {
    condition = length(aws_kms_key.customer_key) == 0 && one(one(aws_s3_bucket_server_side_encryption_configuration.data.rule).apply_server_side_encryption_by_default).kms_master_key_id == var.customer_key_existing_key.arn
    error_message = "Storage must use the existing key without creating a replacement key."
  }
}
run "preserve_key" {
  command = plan
}
run "refuse_key_replacement" {
  command = plan
  variables {
    customer_key_existing_key = { arn = "arn:aws:kms:us-east-1:123456789012:key/second" }
  }
  expect_failures = [terraform_data.customer_key_key_identity]
}
"#.to_string());
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok("existing encryption key selection and immutability");
}

#[test]
fn aws_key_gate_preserves_native_and_existing_selection() {
    let stack = Stack::new("optional-key".to_string())
        .inputs(vec![gate_input(
            "keyEnabled",
            "Enable key",
            "Create the key setup.",
        )])
        .add_enabled_when(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
            "keyEnabled",
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut files = super::helpers::linter_files(&module);
    files.insert("key.tftest.hcl".to_string(), r#"
mock_provider "aws" {
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_region" {
    defaults = { region = "us-east-1" }
  }
}
mock_provider "random" {}
variables {
  name = "key-test"
  token = "test"
  region = "us-east-1"
}
run "enabled_native_key" {
  command = plan
  variables { input_key_enabled = true }
  assert {
    condition = length(aws_kms_key.customer_key) == 1 && length(terraform_data.customer_key_key_identity) == 1
    error_message = "An enabled native key must create and pin exactly one key."
  }
}
run "disabled_native_key" {
  command = plan
  variables { input_key_enabled = false }
  assert {
    condition = length(aws_kms_key.customer_key) == 0 && length(terraform_data.customer_key_key_identity) == 0
    error_message = "A disabled key must create neither the key nor its identity."
  }
}
run "enabled_existing_key" {
  command = plan
  variables {
    input_key_enabled = true
    customer_key_existing_key = { arn = "arn:aws:kms:us-east-1:123456789012:key/first" }
  }
  assert {
    condition = length(aws_kms_key.customer_key) == 0 && terraform_data.customer_key_key_identity[0].input == var.customer_key_existing_key.arn
    error_message = "An enabled existing key must pin the selected ARN without creating a key."
  }
}
run "disabled_existing_key" {
  command = plan
  variables {
    input_key_enabled = false
    customer_key_existing_key = { arn = "arn:aws:kms:us-east-1:123456789012:key/first" }
  }
  assert {
    condition = length(aws_kms_key.customer_key) == 0 && length(terraform_data.customer_key_key_identity) == 0
    error_message = "An existing key selection must not override a disabled gate."
  }
}
"#.to_string());
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok("gated native and existing encryption key plans");
}

#[test]
fn automatic_key_packages_are_refused_on_all_clouds() {
    let stack = Stack::new("example".to_string())
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    for target in [
        TerraformTarget::Aws,
        TerraformTarget::Gcp,
        TerraformTarget::Azure,
    ] {
        let error = super::helpers::try_render(
            &stack,
            target,
            StackSettings {
                setup_management: alien_core::SetupManagement::Automatic,
                ..StackSettings::default()
            },
        )
        .expect_err("automatic packages cannot hand off setup-owned keys");
        assert!(error.to_string().contains("manual setup"), "{error}");
    }
}

#[test]
fn aws_storage_browser_cors() {
    let stack = Stack::new("browser-storage".to_string())
        .add(
            Storage::new("assets".to_string())
                .cors_allowed_origins(vec![
                    "https://app.example.com".to_string(),
                    "https://preview.example.com".to_string(),
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_browser_cors", &module);
    assert_terraform_valid(&module, "aws_storage_browser_cors");
}

#[test]
fn aws_retained_data_setup() {
    for encrypted in [false, true] {
        let mut storage = Storage::new("data".to_string()).versioning(true).build();
        if encrypted {
            storage.encryption_key = Some(ResourceRef::new(Key::RESOURCE_TYPE, "key"));
        }
        let mut stack = Stack::new("retained-data".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(
                Kv::new("rows".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ArtifactRegistry::new("images".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        if encrypted {
            stack = stack.add(
                Key::new("key".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        }
        let module = render(
            &stack.build(),
            TerraformTarget::Aws,
            StackSettings::default(),
        );
        let name = if encrypted {
            "aws_retained_encrypted_data_setup"
        } else {
            "aws_retained_data_setup"
        };
        snapshot_module(name, &module);
        assert_terraform_valid(&module, name);
    }
}
