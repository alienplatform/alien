use alien_core::{
    AwsManagementConfig, Container, ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, Key, ManagementConfig, PersistentStorage, Platform, Postgres,
    ResourceLifecycle, ResourceRef, ResourceSpec, ResourceStatus, Stack, StackResourceState,
    StackSettings, StackState, Storage, VolumeBackups,
};
use alien_preflights::runner::PreflightRunner;

fn release(key: Option<&str>, allow_existing: bool) -> Stack {
    let mut database = Postgres::new("database".to_string()).build();
    let mut files = Storage::new("files".to_string()).build();
    let selected = key.map(|key| ResourceRef::new(Key::RESOURCE_TYPE, key));
    database.encryption_key = selected.clone();
    database.allow_unencrypted_existing = allow_existing;
    files.encryption_key = selected.clone();
    files.allow_unencrypted_existing = allow_existing;
    Stack::new("dedicated-environment".to_string())
        .add(
            Key::new("data-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Key::new("other-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(database, ResourceLifecycle::Frozen)
        .add(files, ResourceLifecycle::Frozen)
        .add(
            Container::new("cache".to_string())
                .code(ContainerCode::Image {
                    image: "redis:7".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "0.5".to_string(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "512Mi".to_string(),
                })
                .permissions("execution".to_string())
                .stateful(true)
                .persistent_storage(PersistentStorage {
                    size: "10Gi".to_string(),
                    mount_path: "/data".to_string(),
                    encryption_key: selected,
                    allow_unencrypted_existing: allow_existing,
                    backups: VolumeBackups::default(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn config() -> DeploymentConfig {
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

fn installed_state(stack: &Stack, status: ResourceStatus) -> StackState {
    let mut state = StackState::new(Platform::Aws);
    for (id, entry) in &stack.resources {
        let mut resource = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.dependencies.clone(),
        );
        resource.status = status;
        state.resources.insert(id.clone(), resource);
    }
    state
}

async fn prepare(stack: Stack, state: &StackState) -> Stack {
    PreflightRunner::new()
        .apply_mutations(stack, state, &config())
        .await
        .expect("preparation succeeds")
}

#[tokio::test]
async fn keyless_installations_keep_updating_when_a_release_adds_encryption() {
    let installed = prepare(release(None, false), &StackState::new(Platform::Aws)).await;
    for status in [
        ResourceStatus::Running,
        ResourceStatus::RefreshFailed,
        ResourceStatus::UpdateFailed,
        ResourceStatus::ProvisionFailed,
    ] {
        let state = installed_state(&installed, status);
        let mut source = release(Some("data-key"), true);
        source.resources["cache"]
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .code = ContainerCode::Image {
            image: "redis:8".to_string(),
        };
        let next = prepare(source, &state).await;
        let summary = PreflightRunner::new()
            .run_compatibility_checks(&installed, &next, &config(), Platform::Aws)
            .await
            .unwrap();
        assert!(
            summary.success,
            "keyless update must require no setup: {summary:?}"
        );
        for id in ["database", "files", "cache"] {
            assert!(!next.resources[id]
                .config
                .get_dependencies()
                .contains(&ResourceRef::new(Key::RESOURCE_TYPE, "data-key")));
            installed.resources[id]
                .config
                .validate_update(&next.resources[id].config)
                .expect("key remains absent");
        }
        assert_eq!(
            installed.permissions, next.permissions,
            "no new encryption permissions for installed keyless resources"
        );
    }
}

#[tokio::test]
async fn fresh_and_not_yet_created_resources_receive_the_declared_key() {
    for state in [
        StackState::new(Platform::Aws),
        installed_state(&release(None, false), ResourceStatus::Pending),
        installed_state(&release(None, false), ResourceStatus::Deleted),
    ] {
        let next = prepare(release(Some("data-key"), true), &state).await;
        for id in ["database", "files", "cache"] {
            assert!(next.resources[id]
                .config
                .get_dependencies()
                .contains(&ResourceRef::new(Key::RESOURCE_TYPE, "data-key")));
        }
    }
    let mut state = installed_state(&release(None, false), ResourceStatus::Running);
    state.resources.remove("files");
    let next = prepare(release(Some("data-key"), true), &state).await;
    assert!(next.resources["files"]
        .config
        .downcast_ref::<Storage>()
        .unwrap()
        .encryption_key
        .is_some());
    assert!(next.resources["database"]
        .config
        .downcast_ref::<Postgres>()
        .unwrap()
        .encryption_key
        .is_none());
}

#[tokio::test]
async fn existing_encrypted_resources_still_reject_key_removal_and_replacement() {
    let installed = prepare(
        release(Some("data-key"), false),
        &StackState::new(Platform::Aws),
    )
    .await;
    let state = installed_state(&installed, ResourceStatus::Running);
    for key in [None, Some("data-key"), Some("other-key")] {
        let next = prepare(release(key, true), &state).await;
        for id in ["database", "files", "cache"] {
            let result = installed.resources[id]
                .config
                .validate_update(&next.resources[id].config);
            if key == Some("data-key") {
                result.expect("same key remains valid");
            } else {
                result.expect_err("existing encrypted data cannot change keys");
            }
        }
    }
}

#[tokio::test]
async fn preserving_keyless_resources_requires_explicit_release_policy() {
    let installed = prepare(release(None, false), &StackState::new(Platform::Aws)).await;
    let next = prepare(
        release(Some("data-key"), false),
        &installed_state(&installed, ResourceStatus::Running),
    )
    .await;
    let summary = PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &config(), Platform::Aws)
        .await
        .unwrap();
    assert!(
        !summary.success,
        "without opt-in the existing frozen/key checks still reject the change"
    );
}
