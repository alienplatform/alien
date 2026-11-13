//! A newer target takes over an update that only waits in a state its controller marked
//! interruptible. Every other update keeps finishing first.

use super::*;
use alien_core::ResourceStatus;
use alien_infra::{test_worker_configs_deployed, StackResourceStateExt};

fn deployed_images(identifier: &str) -> Vec<String> {
    test_worker_configs_deployed(identifier)
        .iter()
        .map(|config| match &config.code {
            WorkerCode::Image { image } => image.clone(),
            other => panic!("expected an image worker, got {other:?}"),
        })
        .collect()
}

fn with_environment(stack: &mut Stack, id: &str, key: &str) {
    stack
        .resources
        .get_mut(id)
        .unwrap()
        .config
        .downcast_mut::<Worker>()
        .unwrap()
        .environment
        .insert(key.to_string(), "true".to_string());
}

async fn step_once(state: DeploymentState, config: &DeploymentConfig) -> DeploymentState {
    crate::support::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .unwrap()
        .state
}

/// Steps until `done` holds, failing the test if it never does.
async fn step_until(
    mut state: DeploymentState,
    config: &DeploymentConfig,
    what: &str,
    done: impl Fn(&DeploymentState) -> bool,
) -> DeploymentState {
    for _ in 0..MAX_STEPS {
        if done(&state) {
            return state;
        }
        state = step_once(state, config).await;
    }
    panic!("never reached: {what}; status {:?}", state.status);
}

fn controller_state(state: &DeploymentState, id: &str) -> Option<String> {
    let resource = state.stack_state.as_ref()?.resources.get(id)?;
    resource
        .internal_state
        .as_ref()
        .and_then(|value| value["state"].as_str().map(str::to_string))
}

async fn deployment_updating_to_v2(
    id: &str,
    wait_flag: &str,
) -> (DeploymentState, DeploymentConfig, Stack, Stack) {
    let config = create_test_config("interruptible-takeover", false);
    let mut v1 = create_test_stack("interruptible-takeover", id);
    add_live_worker(&mut v1, image_worker(id, "test:v1", 1024));
    let mut state = run_to_completion(create_initial_state(v1.clone()), config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    state.current_release = Some(release_of("rel_v1", v1.clone()));

    let mut v2 = v1.clone();
    add_live_worker(&mut v2, image_worker(id, "test:v2", 1024));
    with_environment(&mut v2, id, wait_flag);
    start_update(&mut state, release_of("rel_v2", v2.clone()));
    (state, config, v1, v2)
}

#[tokio::test]
async fn promoting_the_current_release_takes_over_an_update_waiting_in_an_interruptible_state() {
    let _vault = test_vault_env().await;
    let id = "interruptible-api";
    let identifier = format!("test:worker:{id}");
    let (state, config, v1, _v2) =
        deployment_updating_to_v2(id, "SIMULATE_UPDATE_EXTERNAL_WAIT").await;

    let state = step_until(state, &config, "the wait", |state| {
        controller_state(state, id).as_deref() == Some("updateExternalWait")
    })
    .await;
    assert_eq!(state.status, DeploymentStatus::Updating);

    // More steps change nothing: the wait does not end by itself.
    let mut state = state;
    for _ in 0..5 {
        state = step_once(state, &config).await;
    }
    assert_eq!(state.status, DeploymentStatus::Updating);

    // Promote the release that is already current, as a rollback does.
    state.target_release = Some(release_of("rel_v1", v1));
    let state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert_eq!(
        deployed_images(&identifier),
        vec!["test:v1", "test:v2", "test:v1"],
        "v1 was deployed again while the v2 update was still waiting"
    );
    let resource = &state.stack_state.as_ref().unwrap().resources[id];
    assert_eq!(resource.status, ResourceStatus::Running);
    assert_eq!(
        resource
            .config
            .downcast_ref::<Worker>()
            .map(|worker| &worker.code),
        Some(&WorkerCode::Image {
            image: "test:v1".to_string()
        })
    );
}

#[tokio::test]
async fn an_update_in_a_state_that_is_not_interruptible_still_finishes_before_the_newer_target() {
    let _vault = test_vault_env().await;
    let id = "uninterruptible-api";
    let identifier = format!("test:worker:{id}");
    let (state, config, v1, _v2) = deployment_updating_to_v2(id, "SIMULATE_UPDATE_PENDING").await;

    let mut state = step_until(state, &config, "the update polling", |state| {
        controller_state(state, id).as_deref() == Some("updateCodePolling")
    })
    .await;
    state.target_release = Some(release_of("rel_v1", v1));
    for _ in 0..5 {
        state = step_once(state, &config).await;
    }
    assert_eq!(state.status, DeploymentStatus::Updating);
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .and_then(|metadata| metadata.pending_prepared_release_id.as_deref()),
        Some("rel_v2"),
        "the update in flight keeps its release"
    );
    assert_eq!(deployed_images(&identifier), vec!["test:v1", "test:v2"]);
}
