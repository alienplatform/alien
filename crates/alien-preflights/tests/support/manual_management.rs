use alien_core::{
    permissions::ManagementPermissions, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, Platform, ResourceLifecycle, SetupManagement, Stack, StackSettings,
    StackState, Storage, Vault, Worker, WorkerCode,
};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, management_identity_global_refs, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use serde_json::json;
use std::collections::BTreeMap;

pub async fn compile_manual_management(
    override_profile: bool,
) -> BTreeMap<String, Vec<AwsIamStatement>> {
    let mut stack = Stack::new("sample".to_string())
        .add(
            Storage::new("uploads".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("jobs".to_string())
                .code(WorkerCode::Image {
                    image: "test:latest".to_string(),
                })
                .permissions("default".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let profile = serde_json::from_value(json!({
        "*": ["worker/provision", "container/provision"],
        "uploads": ["storage/data-read"],
        "secrets": ["vault/data-write"]
    }))
    .unwrap();
    stack.permissions.management = if override_profile {
        ManagementPermissions::Override(profile)
    } else {
        ManagementPermissions::Extend(profile)
    };
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings {
            setup_management: SetupManagement::Manual,
            ..StackSettings::default()
        })
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let stack = ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .unwrap();
    let profile = stack.management().profile().unwrap();
    let context = PermissionContext::new()
        .with_stack_prefix("sample")
        .with_aws_region("us-east-1")
        .with_aws_account_id("123456789012")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut policies = BTreeMap::new();
    let global = management_identity_global_refs(stack.resources.values(), profile)
        .into_iter()
        .flat_map(|reference| {
            let set = reference
                .resolve(|id| get_permission_set(id).cloned())
                .unwrap();
            generator
                .generate_policy(&set, BindingTarget::Stack, &context)
                .unwrap()
                .statement
        })
        .collect();
    policies.insert("*".to_string(), global);
    for (scope, refs) in &profile.0 {
        if scope == "*" {
            continue;
        }
        assert!(
            stack.resources.contains_key(scope),
            "unknown scope: {scope}"
        );
        let scoped = context
            .clone()
            .with_resource_id(scope.clone())
            .with_resource_name(format!("sample-{scope}"));
        let statements = refs
            .iter()
            .flat_map(|reference| {
                let set = reference
                    .resolve(|id| get_permission_set(id).cloned())
                    .unwrap();
                generator
                    .generate_policy(&set, BindingTarget::Resource, &scoped)
                    .unwrap()
                    .statement
            })
            .collect();
        policies.insert(scope.clone(), statements);
    }
    policies
}
