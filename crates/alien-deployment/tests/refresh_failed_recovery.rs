//! A deployment whose health check failed reports why, and returns to Running by itself once
//! the resource is healthy again.
mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, ResourceLifecycle,
    Stack, StackSettings, Storage,
};
use alien_infra::DefaultPlatformServiceProvider;
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;

async fn drive_until(
    mut state: DeploymentState,
    config: &DeploymentConfig,
    client: &ClientConfig,
    services: Arc<DefaultPlatformServiceProvider>,
    done: impl Fn(&DeploymentState) -> bool,
) -> DeploymentState {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let result = crate::support::step(
                state,
                config.clone(),
                client.clone(),
                Some(services.clone()),
            )
            .await
            .unwrap();
            state = result.state;
            if done(&state) {
                return state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("deployment must reach the expected status")
}

#[tokio::test]
async fn refresh_failed_reports_its_error_and_recovers_when_healthy() {
    let directory = TempDir::new().unwrap();
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
    let stack = Stack::new("recover".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build();
    let state = DeploymentState {
        status: DeploymentStatus::Pending,
        platform: Platform::Local,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("r1".to_string()),
            version: None,
            description: None,
            stack,
            signed: None,
            signer_lists: Vec::new(),
        }),
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    let running = drive_until(state, &config, &client, services.clone(), |s| {
        s.status == DeploymentStatus::Running
    })
    .await;

    // The resource stops being healthy.
    let data = directory.path().join("storage/data");
    let moved = directory.path().join("storage/data-away");
    std::fs::rename(&data, &moved).unwrap();
    let failed = drive_until(running, &config, &client, services.clone(), |s| {
        s.status == DeploymentStatus::RefreshFailed
    })
    .await;
    assert!(
        failed.error.is_some(),
        "a RefreshFailed deployment must carry its error"
    );

    // It becomes healthy again with nobody asking for a retry.
    std::fs::rename(&moved, &data).unwrap();
    assert!(!failed.retry_requested);
    let recovered = drive_until(failed, &config, &client, services, |s| {
        s.status == DeploymentStatus::Running
    })
    .await;
    assert!(recovered.error.is_none());
    bindings.shutdown().await;
}
