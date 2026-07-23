//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Kv, LifecycleRule, PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage, Vault, Worker, WorkerCode, WorkerTrigger,
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
