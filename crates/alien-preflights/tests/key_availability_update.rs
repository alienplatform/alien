//! An AWS deployment whose setup predates the Key availability probe keeps updating.
//!
//! Preparation now grants `key/availability` on every AWS Key. A deployment set up before that
//! saved a prepared stack without it, and its first update compares that saved stack with a newly
//! prepared one. These tests run the real mutations and the update compatibility checks, as an
//! update does.

use alien_core::{
    permissions::{ManagementPermissions, PermissionSetReference},
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Key, Kv,
    ManagementConfig, Platform, ResourceLifecycle, ResourceRef, Stack, StackSettings, StackState,
    Storage,
};
use alien_preflights::runner::PreflightRunner;

const KEY_AVAILABILITY: &str = "key/availability";

/// S3 encrypted with a customer Key, plus a Live table the release can change freely.
fn release() -> Stack {
    Stack::new("test-stack".to_string())
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("customer-data".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/alien-management".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build()
}

async fn prepare(stack: Stack) -> Stack {
    PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(Platform::Aws), &deployment_config())
        .await
        .expect("the stack should prepare")
}

fn key_grants(prepared: &Stack) -> Vec<String> {
    prepared
        .management()
        .profile()
        .and_then(|profile| profile.0.get("customer-key"))
        .map(|grants| grants.iter().map(|grant| grant.id().to_string()).collect())
        .unwrap_or_default()
}

/// The same prepared stack as a setup from before the availability probe saved it.
fn saved_before_key_availability(prepared: &Stack) -> Stack {
    let mut saved = prepared.clone();
    let ManagementPermissions::Extend(profile) = &mut saved.permissions.management else {
        panic!("a stack with a Key prepares an Extend management profile");
    };
    let grants = profile
        .0
        .get_mut("customer-key")
        .expect("the Key has its own management scope");
    grants.retain(|grant| grant != &PermissionSetReference::from_name(KEY_AVAILABILITY));
    assert!(
        !grants.is_empty(),
        "the Key kept its metadata grant before the probe existed"
    );
    saved
}

#[tokio::test]
async fn an_update_without_key_changes_does_not_require_setup() {
    let installed = saved_before_key_availability(&prepare(release()).await);
    let next = prepare(release()).await;
    assert!(
        key_grants(&next).iter().any(|id| id == KEY_AVAILABILITY),
        "this release grants the probe: {:?}",
        key_grants(&next)
    );

    let summary = PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &deployment_config(), Platform::Aws)
        .await
        .expect("compatibility checks should run");
    let errors: Vec<_> = summary
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect();
    assert!(
        summary.success,
        "the update must not need setup: {errors:?}"
    );
}

#[tokio::test]
async fn the_probe_does_not_carry_another_key_grant_in_with_it() {
    let installed = saved_before_key_availability(&prepare(release()).await);
    let mut next = prepare(release()).await;
    let ManagementPermissions::Extend(profile) = &mut next.permissions.management else {
        panic!("a stack with a Key prepares an Extend management profile");
    };
    profile
        .0
        .get_mut("customer-key")
        .expect("the Key has its own management scope")
        .push(PermissionSetReference::from_name("key/native-volume"));

    let summary = PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &deployment_config(), Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(
        !summary.success,
        "a new volume-encryption grant needs setup"
    );
}
