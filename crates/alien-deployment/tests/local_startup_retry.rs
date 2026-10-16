//! A failed first deployment must not install a new release using its old prepared stack.
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, ResourceLifecycle,
    Stack, StackSettings, Storage,
};
use alien_infra::DefaultPlatformServiceProvider;
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;

fn stack(lifecycle: ResourceLifecycle, added: bool) -> Stack {
    let builder = Stack::new("retry".to_string())
        .add(
            Storage::new("retained".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(Storage::new("blocked".to_string()).build(), lifecycle);
    if added {
        builder
            .add(
                Storage::new("added".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    } else {
        builder.build()
    }
}

fn release(id: &str, stack: Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(id.to_string()),
        version: None,
        description: None,
        stack,
    }
}

async fn drive(
    mut state: DeploymentState,
    config: &DeploymentConfig,
    client: &ClientConfig,
    services: Arc<DefaultPlatformServiceProvider>,
) -> DeploymentState {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let result = alien_deployment::step(
                state,
                config.clone(),
                client.clone(),
                Some(services.clone()),
            )
            .await
            .unwrap();
            state = result.state;
            if state.status == DeploymentStatus::Running
                || (state.status.is_failed() && !state.retry_requested)
            {
                return state;
            }
            if let Some(delay) = result.suggested_delay_ms {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }
    })
    .await
    .expect("local storage deployment must converge")
}

#[tokio::test]
async fn local_startup_retry_prepares_new_release_and_preserves_existing_data() {
    for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
        let directory = TempDir::new().unwrap();
        std::fs::create_dir(directory.path().join("storage")).unwrap();
        let blocked = directory.path().join("storage/blocked");
        std::fs::write(&blocked, "not a directory").unwrap();
        let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
        let services = Arc::new(DefaultPlatformServiceProvider::with_local_bindings(
            bindings.clone(),
        ));
        let client = ClientConfig::Local {
            state_directory: directory.path().to_string_lossy().into_owned(),
        };
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let state = DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Local,
            current_release: None,
            target_release: Some(release("old", stack(lifecycle, false))),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        };
        let failed = drive(state, &config, &client, services.clone()).await;
        assert_eq!(
            failed.status,
            if lifecycle == ResourceLifecycle::Frozen {
                DeploymentStatus::InitialSetupFailed
            } else {
                DeploymentStatus::ProvisioningFailed
            }
        );
        let kept = directory.path().join("storage/retained/value");
        std::fs::write(&kept, "preserve this data").unwrap();
        std::fs::remove_file(blocked).unwrap();
        // This is a real durable checkpoint, not a fresh resource controller.
        let mut retry: DeploymentState =
            serde_json::from_slice(&serde_json::to_vec(&failed).unwrap()).unwrap();
        retry.target_release = Some(release("new", stack(lifecycle, true)));
        retry.retry_requested = true;
        let ready = drive(retry, &config, &client, services).await;
        assert_eq!(ready.status, DeploymentStatus::Running);
        assert_eq!(
            ready.current_release.unwrap().release_id.as_deref(),
            Some("new")
        );
        assert!(
            directory.path().join("storage/added").is_dir(),
            "the new release must actually be provisioned"
        );
        assert!(directory.path().join("storage/blocked").is_dir());
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "preserve this data");
        bindings.shutdown().await;
    }
}
