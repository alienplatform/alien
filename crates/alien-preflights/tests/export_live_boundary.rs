//! Export real automatic-mode policies for scripts/validate-aws-boundary.py.
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceLifecycle,
    SetupManagement, Stack, StackSettings, StackState, Storage, Vault, Worker, WorkerCode,
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_permissions::{
    generators::{aws_stack_permissions_boundary, AwsRuntimePermissionsGenerator},
    get_permission_set, management_identity_global_refs, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use serde_json::json;
use std::{env, fs, path::PathBuf};

#[tokio::test]
#[ignore = "writes artifacts for an explicitly requested live AWS test"]
async fn export_live_boundary_policies() {
    let output = PathBuf::from(env::var("BOUNDARY_OUTPUT_DIR").expect("BOUNDARY_OUTPUT_DIR"));
    let prefix = env::var("BOUNDARY_PREFIX").expect("BOUNDARY_PREFIX");
    let account = env::var("BOUNDARY_ACCOUNT").expect("BOUNDARY_ACCOUNT");
    let region = env::var("BOUNDARY_REGION").expect("BOUNDARY_REGION");
    let stack = Stack::new("sample".to_string())
        .add(
            Storage::new("uploads".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Vault::new(alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string()).build(),
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
    let settings = StackSettings {
        setup_management: SetupManagement::Automatic,
        ..StackSettings::default()
    };
    let config = DeploymentConfig::builder()
        .stack_settings(settings)
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

    let context = PermissionContext::new()
        .with_stack_prefix(&prefix)
        .with_aws_region(&region)
        .with_aws_account_id(&account)
        .with_managing_account_id(&account)
        .with_managing_role_arn(format!("arn:aws:iam::{account}:role/{prefix}-management"))
        .with_aws_permissions_boundary_arn(format!(
            "arn:aws:iam::{account}:policy/{prefix}-deployment-management-boundary"
        ));
    let boundary = aws_stack_permissions_boundary(&stack, &context, |_, scope| {
        assert_eq!(scope, "*");
        Ok(context.clone())
    })
    .expect("boundary compiles");
    let profile = stack.management().profile().unwrap();
    assert!(profile.0.keys().all(|scope| scope == "*"));
    let statements: Vec<_> = management_identity_global_refs(stack.resources.values(), profile)
        .into_iter()
        .flat_map(|reference| {
            let set = reference
                .resolve(|name| get_permission_set(name).cloned())
                .unwrap();
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, BindingTarget::Stack, &context)
                .unwrap()
                .statement
        })
        .collect();
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("tags.json"),
        serde_json::to_vec(&json!({
            ALIEN_STACK_TAG_KEY: prefix, ALIEN_MANAGED_BY_TAG_KEY: "runtime"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("boundary.json"),
        serde_json::to_vec(&boundary).unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("management.json"),
        serde_json::to_vec(&json!({
            "Version": "2012-10-17", "Statement": statements
        }))
        .unwrap(),
    )
    .unwrap();
}
