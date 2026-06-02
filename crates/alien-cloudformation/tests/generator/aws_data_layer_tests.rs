//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Kv, LifecycleRule, PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage, Vault,
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
