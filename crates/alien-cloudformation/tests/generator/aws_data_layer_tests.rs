//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::{custom_resource_registration, render_built_ins, render_built_ins_template};
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Key, Kv, LifecycleRule, PermissionProfile, PermissionSetReference, Queue, RemoteBindings,
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

    assert_eq!(key.deletion_policy.as_deref(), Some("Retain"));
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
        "kms:DescribeKey"
    );
    assert_eq!(
        metadata_policy_json["PolicyDocument"]["Statement"][0]["Resource"],
        serde_json::json!({ "Fn::GetAtt": ["CustomerKey", "Arn"] })
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
    assert_eq!(cors_rule["ExposedHeaders"], serde_json::json!(["ETag"]));
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
        RegistrationMode::OutputsFallback,
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
        &template["Outputs"]["DeploymentStackSettings"]["Value"]["Fn::ToJsonString"];
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
            PermissionProfile::new().resource("jobs", ["queue/data-read", "queue/data-write"]),
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
            PermissionProfile::new().resource("store", ["kv/data-read", "kv/data-write"]),
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
        "AWS allows ten managed policies per role"
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
