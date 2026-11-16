//! A release that fails on one resource leaves everything downstream of it on the previous
//! release, through the full deployment loop.

use super::*;
use alien_core::ResourceStatus;
use alien_infra::{
    give_test_worker_setup_grant, test_worker_configs_deployed, withhold_test_worker_setup_grant,
    TestSetupGrant,
};

fn linked_worker(id: &str, image: &str, target: &Worker) -> Worker {
    Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: image.to_string(),
        })
        .memory_mb(1024)
        .permissions("default".to_string())
        .link(target)
        .build()
}

/// The shape of a release that fails halfway: `base` <- `api` <- `runner`, where the update to
/// `base` is refused. `api` and `runner` keep the previous release; they must not be written.
#[tokio::test]
async fn a_failed_dependency_update_keeps_its_dependents_on_the_previous_release() {
    let _vault = test_vault_env().await;
    let base_id = "rollout-base";
    let api_id = "rollout-api";
    let runner_id = "rollout-runner";
    let config = create_test_config("dependent-rollout", false);

    let stack_at = |image: &str| {
        let base = image_worker(base_id, image, 1024);
        let api = linked_worker(api_id, image, &base);
        let runner = linked_worker(runner_id, image, &api);
        let mut stack = create_test_stack("dependent-rollout", base_id);
        add_live_worker(&mut stack, base);
        add_live_worker(&mut stack, api);
        add_live_worker(&mut stack, runner);
        stack
    };

    let state = run_to_completion(create_initial_state(stack_at("test:v1")), config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let writes = |id: &str| test_worker_configs_deployed(&format!("test:worker:{id}")).len();
    let before = [writes(base_id), writes(api_id), writes(runner_id)];

    withhold_test_worker_setup_grant(base_id, TestSetupGrant::Update);
    let mut state = state;
    start_update(&mut state, release_of("rel_v2", stack_at("test:v2")));
    let state = run_to_completion(state, config).await;
    give_test_worker_setup_grant(base_id);

    assert_eq!(state.status, DeploymentStatus::UpdateFailed);
    let resources = &state.stack_state.as_ref().unwrap().resources;
    assert_eq!(resources[base_id].status, ResourceStatus::UpdateFailed);
    assert_eq!(
        writes(api_id),
        before[1],
        "api was written to although the update it links to failed"
    );
    assert_eq!(
        writes(runner_id),
        before[2],
        "runner was written to although the update it links to never reached api"
    );
}
