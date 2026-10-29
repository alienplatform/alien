//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::{custom_resource_registration, render_built_ins, render_built_ins_template};
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    permissions::ManagementPermissions, AwsOpenSearch, Email, Key, Kv, LifecycleRule,
    PermissionProfile, PermissionSetReference, Queue, QueueBatchConfig, RemoteBindings,
    RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings,
    Storage, Vault, Worker, WorkerCode, WorkerTrigger,
};

#[test]
fn aws_key_template_is_valid_and_retained() {
    let mut stack = Stack::new("enterprise-key".to_string())
        .add_with_remote_access(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.permissions.management =
        alien_core::ManagementPermissions::Extend(PermissionProfile::new().resource(
            "customer-key",
            [PermissionSetReference::from_name("key/management")],
        ));
    stack
        .resources
        .get_mut("customer-key")
        .unwrap()
        .dependencies = vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];

    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        alien_cloudformation::CloudFormationTarget::Aws,
        "aws",
        "aws key",
    );
    let key = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::KMS::Key")
        .expect("KMS key should render");

    assert_eq!(key.deletion_policy.as_deref(), Some("RetainExceptOnCreate"));
    assert_eq!(key.update_replace_policy.as_deref(), Some("Retain"));
    assert!(yaml.contains("kms:Encrypt"));
    assert!(yaml.contains("kms:Decrypt"));
    assert!(yaml.contains("kms:DescribeKey"));

    let metadata_policy = template
        .resources
        .values()
        .find(|resource| resource.logical_id == "CustomerKeyManagementMetadataPolicy")
        .expect("key metadata policy should render");
    let metadata_policy_json =
        serde_json::to_value(&metadata_policy.properties).expect("serialize metadata policy");
    assert_eq!(
        metadata_policy_json["Roles"],
        serde_json::json!([{ "Ref": "ManagementRole" }])
    );
    assert_eq!(
        metadata_policy_json["PolicyDocument"]["Statement"][0]["Action"],
        serde_json::json!(["kms:DescribeKey"])
    );
    assert_eq!(
        metadata_policy_json["PolicyDocument"]["Statement"][0]["Resource"],
        serde_json::json!([{ "Fn::GetAtt": ["CustomerKey", "Arn"] }])
    );

    let cryptography_policy = template
        .resources
        .values()
        .find(|resource| resource.logical_id == "CustomerKeyRemoteCryptographyPolicy")
        .expect("key cryptography policy should render");
    let cryptography_policy_json = serde_json::to_value(&cryptography_policy.properties)
        .expect("serialize cryptography policy");
    assert_eq!(
        cryptography_policy_json["Roles"],
        serde_json::json!([{ "Ref": "AccessRole" }])
    );
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

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws encrypted storage",
    );
    assert!(yaml.contains("SSEAlgorithm: aws:kms"));
    assert!(yaml.contains("KMSMasterKeyID:"));
    assert!(yaml.contains("Fn::GetAtt:"));
    assert!(yaml.contains("kms:GenerateDataKey"));
    assert!(yaml.contains("kms:Decrypt"));
}

#[test]
fn aws_data_layer_renders_idiomatic_template() {
    let stack = Stack::new("data-layer".to_string())
        .add(
            Storage::new("assets".to_string())
                .public_read(true)
                .versioning(true)
                .lifecycle_rules(vec![
                    LifecycleRule {
                        days: 30,
                        prefix: Some("tmp/".to_string()),
                    },
                    LifecycleRule {
                        days: 365,
                        prefix: None,
                    },
                ])
                .build(),
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

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws data layer",
    );
    insta::assert_snapshot!("aws_data_layer", yaml);
}

#[test]
fn aws_storage_minimal_uses_safe_defaults() {
    let stack = Stack::new("storage-minimal".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws storage minimal",
    );
    insta::assert_snapshot!("aws_storage_minimal", yaml);
}

#[test]
fn remote_storage_management_dependencies_are_acyclic() {
    let management_ref = ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management");
    let bindings_ref = ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access");
    let mut stack = Stack::new("remote-storage".to_string())
        .add_with_remote_access(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
            vec![management_ref.clone()],
        )
        .build();
    stack.resources.get_mut("files").unwrap().dependencies =
        vec![management_ref.clone(), bindings_ref.clone()];

    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        alien_cloudformation::CloudFormationTarget::Aws,
        "aws",
        "remote storage management dependencies",
    );

    let management_role = template
        .resources
        .values()
        .find(|resource| {
            resource.resource_type == "AWS::IAM::Role" && resource.logical_id == "ManagementRole"
        })
        .expect("management role");
    let access_role = template
        .resources
        .values()
        .find(|resource| {
            resource.resource_type == "AWS::IAM::Role" && resource.logical_id == "AccessRole"
        })
        .expect("access role");
    let storage_bucket = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::S3::Bucket")
        .expect("storage bucket");
    let storage_grant = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::IAM::Policy")
        .expect("storage management grant");
    let queue = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::SQS::Queue")
        .expect("unrelated storage dependent");

    assert!(!management_role
        .depends_on
        .contains(&storage_bucket.logical_id));
    assert!(storage_bucket
        .depends_on
        .contains(&management_role.logical_id));
    assert!(!storage_bucket.depends_on.contains(&access_role.logical_id));
    assert!(storage_grant.depends_on.contains(&access_role.logical_id));
    assert!(storage_grant
        .depends_on
        .contains(&storage_bucket.logical_id));
    assert!(queue.depends_on.contains(&management_role.logical_id));

    let grant_properties =
        serde_json::to_value(&storage_grant.properties).expect("serialize storage grant");
    assert_eq!(
        grant_properties["Roles"],
        serde_json::json!([{ "Ref": access_role.logical_id }]),
        "setup must attach the exact storage grant to the access role"
    );
    let grant_actions = grant_properties["PolicyDocument"]["Statement"]
        .as_array()
        .expect("storage grant must contain statements")
        .iter()
        .flat_map(|statement| {
            statement["Action"]
                .as_array()
                .expect("storage grant must list actions")
        })
        .collect::<Vec<_>>();
    assert_eq!(grant_actions.len(), 8);
    for action in [
        "s3:ListBucket",
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject",
    ] {
        assert!(
            grant_actions.iter().any(|value| **value == action),
            "storage grant must contain {action}"
        );
    }
    assert_eq!(
        grant_properties["PolicyDocument"]["Statement"][0]["Resource"],
        serde_json::json!([{ "Fn::GetAtt": [storage_bucket.logical_id, "Arn"] }]),
        "bucket-level actions must use the bucket ARN"
    );
    assert_eq!(
        grant_properties["PolicyDocument"]["Statement"][1]["Resource"],
        serde_json::json!([{
            "Fn::Sub": format!(
                "arn:${{AWS::Partition}}:s3:::${{{}}}/*",
                storage_bucket.logical_id
            )
        }]),
        "object-level actions must use the object ARN"
    );

    for setup_policy in template
        .resources
        .values()
        .filter(|resource| resource.resource_type == "AWS::IAM::ManagedPolicy")
    {
        let policy = serde_json::to_string(&setup_policy.properties)
            .expect("serialize setup management policy");
        assert!(
            !policy.contains("iam:CreatePolicy")
                && !policy.contains("iam:CreatePolicyVersion")
                && !policy.contains("iam:AttachRolePolicy"),
            "the management role must not be able to expand its own permissions"
        );
    }
}

#[test]
fn remote_access_generator_fragment_is_reviewable() {
    let bindings_ref = ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access");
    let mut stack = Stack::new("customer-exports".to_string())
        .add_with_remote_access(
            Storage::new("exports".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.resources.get_mut("exports").unwrap().dependencies = vec![bindings_ref];

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "BYO Bucket",
    );
    // This exercises the generator fragment directly. Product rendering runs
    // preflight mutations first and also injects the scoped management role.
    insta::assert_snapshot!("aws_byo_bucket", yaml);
}

#[test]
fn aws_storage_emits_browser_read_cors() {
    let stack = Stack::new("storage-cors".to_string())
        .add(
            Storage::new("data".to_string())
                .cors_allowed_origins(vec![
                    "https://console.example.com".to_string(),
                    "http://localhost:3000".to_string(),
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws storage CORS",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let cors_rule =
        &template["Resources"]["Data"]["Properties"]["CorsConfiguration"]["CorsRules"][0];

    assert_eq!(cors_rule["AllowedHeaders"], serde_json::json!(["*"]));
    assert_eq!(
        cors_rule["AllowedMethods"],
        serde_json::json!(["GET", "HEAD"])
    );
    assert_eq!(
        cors_rule["AllowedOrigins"],
        serde_json::json!(["https://console.example.com", "http://localhost:3000"])
    );
    assert_eq!(
        cors_rule["ExposedHeaders"],
        serde_json::json!(["ETag", "Content-Range", "Accept-Ranges"])
    );
    assert_eq!(cors_rule["MaxAge"], 3600);
}

#[test]
fn storage_only_template_omits_custom_domain_inputs() {
    let stack = Stack::new("storage-minimal".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        super::helpers::custom_resource_registration(),
        "aws storage custom-domain inputs",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");

    assert!(template["Parameters"].get("DomainName").is_none());
    assert!(template["Parameters"].get("HostedZoneId").is_none());
    assert!(template["Parameters"].get("CertificateArn").is_none());
    assert!(template["Conditions"].get("HasDomainName").is_none());
    assert!(template["Rules"].get("CustomDomainCertificate").is_none());

    let stack_settings =
        &template["Resources"]["DeploymentRegistration"]["Properties"]["StackSettings"];
    assert!(stack_settings.get("domains").is_none());
}

#[test]
fn frozen_storage_with_live_worker_trigger_omits_setup_notification_wiring() {
    let storage = Storage::new("data".to_string()).build();
    let worker = Worker::new("processor".to_string())
        .code(WorkerCode::Image {
            image: "processor:latest".to_string(),
        })
        .permissions("execution".to_string())
        .trigger(WorkerTrigger::storage(
            &storage,
            vec!["created".to_string()],
        ))
        .build();
    let stack = Stack::new("storage-trigger".to_string())
        .add(storage, ResourceLifecycle::Frozen)
        .add(worker, ResourceLifecycle::Live)
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws frozen storage live worker trigger",
    );

    assert!(yaml.contains("Data:"));
    assert!(!yaml.contains("NotificationConfiguration"));
    assert!(!yaml.contains("ProcessorWorker"));
    assert!(!yaml.contains("StoragePermission"));
}

#[test]
fn aws_queue_resource_permissions_attach_to_service_account_role() {
    let stack = Stack::new("queue-permissions".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource(
                "jobs",
                ["queue/data-read", "queue/data-write", "queue/provision"],
            ),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws queue service account permissions",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");

    let policies = policies_for_role(&template, "ExecutionSaRole");
    assert_eq!(policies.len(), 1);
    let policy = policies[0];
    let actions = policy_actions(policy);
    assert!(actions.contains(&"sqs:ReceiveMessage"));
    assert!(actions.contains(&"sqs:DeleteMessage"));
    assert!(actions.contains(&"sqs:TagQueue"));

    // Statements must be pinned to the queue ARN: the physical queue name is
    // CloudFormation-generated, so a name-pattern binding would never match.
    for statement in policy["Properties"]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("queue policy statements")
    {
        assert_eq!(
            statement["Resource"]["Fn::GetAtt"],
            serde_json::json!(["Jobs", "Arn"])
        );
    }
}

#[test]
fn aws_queue_without_grants_emits_no_iam_policies() {
    let stack = Stack::new("queue-plain".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws queue without grants",
    );

    assert!(!yaml.contains("QueuePermission"));
}

#[test]
fn aws_kv_resource_permissions_attach_to_service_account_role() {
    let stack = Stack::new("kv-permissions".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("store", ["kv/data-read", "kv/data-write", "kv/provision"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws kv service account permissions",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");

    let policies = policies_for_role(&template, "ExecutionSaRole");
    assert_eq!(policies.len(), 1);
    let policy = policies[0];
    let actions = policy_actions(policy);
    assert!(actions.contains(&"dynamodb:GetItem"));
    assert!(actions.contains(&"dynamodb:Query"));
    assert!(actions.contains(&"dynamodb:PutItem"));
    assert!(actions.contains(&"dynamodb:TagResource"));

    // Statements must be pinned to the table ARN: the physical table name is
    // CloudFormation-generated, so a name-pattern binding would never match.
    for statement in policy["Properties"]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("kv policy statements")
    {
        assert_eq!(
            statement["Resource"]["Fn::GetAtt"],
            serde_json::json!(["Store", "Arn"])
        );
    }
}

#[test]
fn aws_kv_without_grants_emits_no_iam_policies() {
    let stack = Stack::new("kv-plain".to_string())
        .add(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws kv without grants",
    );

    assert!(!yaml.contains("KvPermission"));
}

#[test]
fn aws_vault_resource_permissions_attach_to_service_account_role() {
    let stack = Stack::new("vault-permissions".to_string())
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

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws vault service account permissions",
    );

    assert!(yaml.contains("SecretsExecutionSaRoleVaultPermission00"));
    assert!(yaml.contains("ssm:GetParameter"));
    assert!(yaml.contains("parameter/${AWS::StackName}-secrets-*"));
    assert!(yaml.contains("Ref: ExecutionSaRole"));
}

#[test]
fn aws_vault_permissions_include_every_vault_scope() {
    let stack = Stack::new("vault-permissions".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("secrets", ["vault/data-read"])
                .resource("provider-keys", ["vault/data-read"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("provider-keys".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws multiple vault service account permissions",
    );

    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let policies = policies_for_role(&template, "ExecutionSaRole");
    assert_eq!(policies.len(), 1);
    let policy = serde_json::to_string(policies[0]).expect("policy should serialize");
    assert!(policy.contains("parameter/${AWS::StackName}-secrets-*"));
    assert!(policy.contains("parameter/${AWS::StackName}-provider-keys-*"));
}

#[test]
fn many_resource_grants_use_quota_safe_managed_policies() {
    let mut stack = Stack::new("many-resource-grants".to_string())
        .permission(
            "execution",
            PermissionProfile::new().global([
                "storage/data-read",
                "storage/data-write",
                "kv/data-read",
                "kv/data-write",
            ]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        );

    for index in 0..12 {
        stack = stack.add(
            Storage::new(format!("data-{index}")).build(),
            ResourceLifecycle::Frozen,
        );
    }

    let yaml = render_built_ins(
        &stack.build(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "many AWS resource grants",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let policies = policies_for_role(&template, "ExecutionSaRole");
    assert!(!policies.is_empty());
    assert!(
        policies.len() <= 10,
        "per-resource grants should pack into a handful of managed policies"
    );
    assert!(policies
        .iter()
        .all(|policy| policy["Type"] == "AWS::IAM::ManagedPolicy"));
    assert!(role_inline_policy_documents(&template, "ExecutionSaRole").is_empty());

    let policy = serde_json::to_string(&policies).expect("policies should serialize");
    for index in 0..12 {
        assert!(
            policy.contains(&format!("Data{index}")),
            "storage {index} should remain granted"
        );
    }
}

fn policies_for_role<'a>(
    template: &'a serde_json::Value,
    role_id: &str,
) -> Vec<&'a serde_json::Value> {
    template["Resources"]
        .as_object()
        .expect("resources should be an object")
        .values()
        .filter(|resource| {
            matches!(
                resource["Type"].as_str(),
                Some("AWS::IAM::Policy" | "AWS::IAM::ManagedPolicy")
            ) && resource["Properties"]["Roles"]
                .as_array()
                .is_some_and(|roles| roles.contains(&serde_json::json!({ "Ref": role_id })))
        })
        .collect()
}

fn policy_actions(policy: &serde_json::Value) -> Vec<&str> {
    policy["Properties"]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("policy statements")
        .iter()
        .flat_map(|statement| match &statement["Action"] {
            serde_json::Value::Array(actions) => actions.iter().collect::<Vec<_>>(),
            action => vec![action],
        })
        .filter_map(serde_json::Value::as_str)
        .collect()
}

fn role_inline_policy_documents<'a>(
    template: &'a serde_json::Value,
    role_id: &str,
) -> Vec<&'a serde_json::Value> {
    let mut documents = template["Resources"][role_id]["Properties"]["Policies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|policy| &policy["PolicyDocument"])
        .collect::<Vec<_>>();
    documents.extend(
        policies_for_role(template, role_id)
            .into_iter()
            .filter(|policy| policy["Type"] == "AWS::IAM::Policy")
            .map(|policy| &policy["Properties"]["PolicyDocument"]),
    );
    documents
}

#[test]
fn aws_remote_kv_setup_passes_cloudformation_validation() {
    let mut stack = Stack::new("acme-cache".to_string())
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Kv::new("sessions".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    for id in ["cache", "sessions"] {
        stack.resources.get_mut(id).unwrap().dependencies =
            vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];
    }
    render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "remote KV setup",
    );
}

#[test]
fn aws_remote_queue_setup_passes_cloudformation_validation() {
    let mut stack = Stack::new("acme-cache".to_string())
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Queue::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Queue::new("sessions".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    for id in ["cache", "sessions"] {
        stack.resources.get_mut(id).unwrap().dependencies =
            vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];
    }
    render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "remote Queue setup",
    );
}

fn render_automatic(stack: &Stack, description: &str) -> alien_core::Result<()> {
    super::helpers::try_render_built_ins(
        stack,
        StackSettings {
            setup_management: alien_core::SetupManagement::Automatic,
            ..StackSettings::default()
        },
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
    .map(|_| ())
}

/// An automatic package installs only access and secret storage; the runtime creates the
/// rest. Resources the runtime cannot create as declared must be refused up front, so the
/// platform offers manual setup instead of a package that fails after installation.
#[test]
fn automatic_packages_refuse_resources_the_runtime_cannot_create() {
    let cases: Vec<(&str, Stack)> = vec![
        (
            "key",
            Stack::new("example".to_string())
                .add(
                    Key::new("customer-key".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build(),
        ),
        (
            "email",
            Stack::new("example".to_string())
                .add(
                    Email::new("mailer".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build(),
        ),
        (
            "opensearch",
            Stack::new("example".to_string())
                .add(
                    AwsOpenSearch::new("search".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build(),
        ),
        (
            "management permission override",
            Stack::new("example".to_string())
                .add(
                    Storage::new("files".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .management(ManagementPermissions::Override(PermissionProfile::new()))
                .build(),
        ),
        (
            "resource-scoped management permission",
            Stack::new("example".to_string())
                .add(
                    Storage::new("files".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .management(ManagementPermissions::Extend(
                    PermissionProfile::new().resource("files", ["storage/data-read"]),
                ))
                .build(),
        ),
    ];
    for (name, stack) in cases {
        let error = render_automatic(&stack, name)
            .expect_err(&format!("automatic package must refuse {name}"));
        assert_eq!(
            error.code, "AUTOMATIC_SETUP_RESOURCE_UNSUPPORTED",
            "{name}: {error}"
        );
        assert!(
            error.to_string().contains("manual setup"),
            "{name}: {error}"
        );
    }

    // Both declarations are supported by the AWS runtime, including browser CORS.
    for origins in [vec![], vec!["https://console.example.com".to_string()]] {
        let storage = Stack::new("example".to_string())
            .add(
                Storage::new("files".to_string())
                    .cors_allowed_origins(origins)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        render_automatic(&storage, "storage").expect("AWS storage is supported");
    }

    // Control: a stack-wide management extension is installed by setup like the generated grant.
    let extended = Stack::new("example".to_string())
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .management(ManagementPermissions::Extend(
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .build();
    render_automatic(&extended, "stack-wide management extension")
        .expect("stack-wide management extensions are supported");
}

#[test]
fn queue_batch_rollout_setup_preserves_visibility_and_live_ownership() {
    for enabled in [false, true] {
        let queue = Queue::new("jobs".into()).build();
        let trigger = if enabled {
            WorkerTrigger::queue_batch(
                &queue,
                QueueBatchConfig {
                    max_messages: 8,
                    max_window_seconds: 5,
                },
            )
            .unwrap()
        } else {
            WorkerTrigger::queue(&queue)
        };
        let worker = Worker::new("processor".into())
            .code(WorkerCode::Image {
                image: "registry.example.com/worker:1".into(),
            })
            .permissions("execution".into())
            .timeout_seconds(60)
            .unwrap()
            .trigger(trigger)
            .build();
        let stack = Stack::new("queue-rollout".into())
            .add(queue, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .build();
        let (template, _) = render_built_ins_template(
            &stack,
            StackSettings::default(),
            RegistrationMode::OutputsFallback,
            CloudFormationTarget::Aws,
            "aws",
            "queue batch rollout setup",
        );
        let queue = template
            .resources
            .values()
            .find(|resource| resource.resource_type == "AWS::SQS::Queue")
            .unwrap();
        assert_eq!(
            serde_json::to_value(&queue.properties["VisibilityTimeout"]).unwrap(),
            serde_json::json!(if enabled { 365 } else { 360 })
        );
        assert!(!template
            .resources
            .values()
            .any(|resource| resource.resource_type == "AWS::Lambda::Function"));
    }
}

// The queue already depends on bootstrap management policies. Metadata access
// must attach separately after queue creation; cfn-lint checks this graph.
#[test]
fn frozen_queue_metadata_policy_has_no_bootstrap_cycle() {
    for enabled in [false, true] {
        let mut profile = PermissionProfile::new().global(["worker/provision"]);
        if enabled {
            profile = profile.resource("jobs", ["queue/batch-metadata"]);
        }
        let mut stack = Stack::new("queue-metadata".into())
            .management(ManagementPermissions::Override(profile))
            .add(
                RemoteStackManagement::new("management".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
            .build();
        stack
            .resources
            .get_mut("jobs")
            .unwrap()
            .dependencies
            .push(ResourceRef::new(
                RemoteStackManagement::RESOURCE_TYPE,
                "management",
            ));
        let (template, _) = render_built_ins_template(
            &stack,
            StackSettings::default(),
            RegistrationMode::OutputsFallback,
            CloudFormationTarget::Aws,
            "aws",
            "queue metadata dependency graph",
        );
        let policies: Vec<_> = template
            .resources
            .values()
            .filter(|resource| resource.resource_type == "AWS::IAM::Policy")
            .collect();
        assert_eq!(policies.len(), usize::from(enabled));
        if enabled {
            let properties = serde_json::to_value(&policies[0].properties).unwrap();
            assert_eq!(
                properties["Roles"],
                serde_json::json!([{"Ref":"ManagementRole"}])
            );
            let statements = properties["PolicyDocument"]["Statement"]
                .as_array()
                .unwrap();
            assert_eq!(statements.len(), 1);
            assert_eq!(
                statements[0]["Action"],
                serde_json::json!(["sqs:GetQueueAttributes"])
            );
            assert_eq!(
                statements[0]["Resource"],
                serde_json::json!({"Fn::GetAtt":["Jobs","Arn"]})
            );
        }
    }
}
