//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::{custom_resource_registration, render_built_ins, render_built_ins_template};
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Kv, LifecycleRule, ManagementPermissions, PermissionProfile, Queue, RemoteStackManagement,
    ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings, Storage, Vault, Worker,
    WorkerCode, WorkerTrigger,
};

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
    let storage_ref = ResourceRef::new(Storage::RESOURCE_TYPE, "files");
    let stack = Stack::new("remote-storage".to_string())
        .management(ManagementPermissions::override_(
            PermissionProfile::new().resource("files", ["storage/remote-data-write"]),
        ))
        .add_with_remote_access(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
            vec![storage_ref.clone()],
        )
        .add_with_dependencies(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
            vec![storage_ref],
        )
        .build();

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
        .find(|resource| resource.resource_type == "AWS::IAM::Role")
        .expect("management role");
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

    assert!(management_role
        .depends_on
        .contains(&storage_bucket.logical_id));
    assert!(!management_role
        .depends_on
        .contains(&storage_grant.logical_id));
    assert!(storage_grant
        .depends_on
        .contains(&management_role.logical_id));
    assert!(queue.depends_on.contains(&storage_grant.logical_id));

    let grant_properties =
        serde_json::to_value(&storage_grant.properties).expect("serialize storage grant");
    assert_eq!(
        grant_properties["Roles"],
        serde_json::json!([{ "Ref": management_role.logical_id }]),
        "setup must attach the exact storage grant to the management role"
    );
    assert_eq!(
        grant_properties["PolicyDocument"]["Statement"][0]["Action"],
        serde_json::json!([
            "s3:ListBucket",
            "s3:GetObject",
            "s3:PutObject",
            "s3:DeleteObject"
        ])
    );
    assert_eq!(
        grant_properties["PolicyDocument"]["Statement"][0]["Resource"],
        serde_json::json!([
            { "Fn::GetAtt": [storage_bucket.logical_id, "Arn"] },
            {
                "Fn::Sub": format!(
                    "arn:${{AWS::Partition}}:s3:::${{{}}}/*",
                    storage_bucket.logical_id
                )
            }
        ]),
        "setup must scope remote binding access to the concrete bucket"
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

    let read_policy = &template["Resources"]["JobsExecutionSaRoleQueuePermission00"];
    let write_policy = &template["Resources"]["JobsExecutionSaRoleQueuePermission01"];
    assert_eq!(read_policy["Type"], "AWS::IAM::Policy");
    assert_eq!(write_policy["Type"], "AWS::IAM::Policy");

    let read_actions = read_policy["Properties"]["PolicyDocument"]["Statement"][0]["Action"]
        .as_array()
        .expect("read statement should list actions");
    assert!(read_actions.contains(&serde_json::json!("sqs:ReceiveMessage")));
    let write_actions = write_policy["Properties"]["PolicyDocument"]["Statement"][0]["Action"]
        .as_array()
        .expect("write statement should list actions");
    assert!(write_actions.contains(&serde_json::json!("sqs:DeleteMessage")));

    // Statements must be pinned to the queue ARN: the physical queue name is
    // CloudFormation-generated, so a name-pattern binding would never match.
    for policy in [read_policy, write_policy] {
        assert_eq!(
            policy["Properties"]["PolicyDocument"]["Statement"][0]["Resource"]["Fn::GetAtt"],
            serde_json::json!(["Jobs", "Arn"])
        );
        assert_eq!(
            policy["Properties"]["Roles"][0]["Ref"],
            serde_json::json!("ExecutionSaRole")
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
fn aws_vault_permissions_include_vault_logical_id() {
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

    assert!(yaml.contains("SecretsExecutionSaRoleVaultPermission00"));
    assert!(yaml.contains("ProviderKeysExecutionSaRoleVaultPermission00"));
}
