use super::*;
use alien_core::{ResourceRef, ResourceStatus};
use alien_infra::{
    give_test_worker_setup_grant, test_worker_configs_deployed, test_worker_deletes_issued,
    withhold_test_worker_setup_grant, StackResourceStateExt, TestSetupGrant,
};

#[derive(Clone, Copy)]
enum RecoveryTarget {
    NewRelease,
    NewDependency,
    LegacyRejection,
    NeverConfirm,
    CurrentWriter,
    NoRetry,
    SameRelease,
    Removed,
    Denied,
}

#[tokio::test]
async fn retry_new_release_supersedes_legacy_interrupted_update() {
    exercise_recovery(RecoveryTarget::NewRelease).await;
}

#[tokio::test]
async fn retry_new_release_waits_for_dependency_before_starting_replacement_update() {
    exercise_recovery(RecoveryTarget::NewDependency).await;
}

#[tokio::test]
async fn retry_recovers_legacy_transition_rejection_without_original_interruption_error() {
    exercise_recovery(RecoveryTarget::LegacyRejection).await;
}

#[tokio::test]
async fn corrective_release_supersedes_an_interrupted_wait_that_can_never_complete() {
    exercise_recovery(RecoveryTarget::NeverConfirm).await;
}

#[tokio::test]
async fn interruption_records_terminal_controller_and_preserves_retry_checkpoint() {
    exercise_recovery(RecoveryTarget::CurrentWriter).await;
}

#[tokio::test]
async fn failed_record_never_steps_its_runnable_checkpoint_during_refresh() {
    exercise_recovery(RecoveryTarget::NoRetry).await;
}

#[tokio::test]
async fn retry_same_release_finishes_interrupted_updates_without_duplicate_writes() {
    exercise_recovery(RecoveryTarget::SameRelease).await;
}

#[tokio::test]
async fn retry_removes_interrupted_resource_without_finishing_its_old_update() {
    exercise_recovery(RecoveryTarget::Removed).await;
}

#[tokio::test]
async fn retry_preserves_a_real_failure_during_resumed_update() {
    exercise_recovery(RecoveryTarget::Denied).await;
}

async fn exercise_recovery(case: RecoveryTarget) {
    let _vault = test_vault_env().await;
    let suffix = match case {
        RecoveryTarget::NewRelease => "new-release",
        RecoveryTarget::NewDependency => "new-dependency",
        RecoveryTarget::LegacyRejection => "legacy-rejection",
        RecoveryTarget::NeverConfirm => "never-confirm",
        RecoveryTarget::CurrentWriter => "current-writer",
        RecoveryTarget::NoRetry => "no-retry",
        RecoveryTarget::SameRelease => "same-release",
        RecoveryTarget::Removed => "removed",
        RecoveryTarget::Denied => "denied",
    };
    let api_id = format!("interrupted-api-{suffix}");
    let dependency_id = format!("interrupted-dependency-{suffix}");
    let api_identifier = format!("test:worker:{api_id}");
    let dependency_identifier = format!("test:worker:{dependency_id}");
    let config = create_test_config("interrupted-update", false);
    let mut installed = create_test_stack("interrupted-update", &api_id);
    add_live_worker(&mut installed, image_worker(&api_id, "test:v1", 1024));
    add_live_worker(
        &mut installed,
        image_worker(&dependency_id, "test:v1", 1024),
    );
    let mut state =
        run_to_completion(create_initial_state(installed.clone()), config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    let mut interrupted = installed;
    add_live_worker(&mut interrupted, image_worker(&api_id, "test:v2", 1024));
    if matches!(case, RecoveryTarget::NeverConfirm) {
        interrupted
            .resources
            .get_mut(&api_id)
            .unwrap()
            .config
            .downcast_mut::<Worker>()
            .unwrap()
            .environment
            .insert("SIMULATE_UPDATE_PENDING".to_string(), "true".to_string());
    }
    add_live_worker(
        &mut interrupted,
        image_worker(&dependency_id, "test:v2", 1024),
    );
    // Fail a real handler while its sibling's update is polling. Deployment orchestration,
    // rather than the fixture, records the sibling's interrupted checkpoint.
    withhold_test_worker_setup_grant(&dependency_id, TestSetupGrant::Update);
    start_update(&mut state, release_of("rel_v2", interrupted.clone()));
    let mut state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);
    let resources = &state.stack_state.as_ref().unwrap().resources;
    assert_eq!(
        resources[&dependency_id].error.as_ref().unwrap().code,
        "MANAGEMENT_PERMISSION_MISSING"
    );
    let api = &resources[&api_id];
    assert_eq!(api.status, ResourceStatus::UpdateFailed);
    assert_eq!(api.error.as_ref().unwrap().code, "DEPLOYMENT_INTERRUPTED");
    assert_eq!(
        api.last_failed_state.as_ref().unwrap()["state"],
        if matches!(case, RecoveryTarget::NeverConfirm) {
            "updateCodePolling"
        } else {
            "updateConfigPolling"
        }
    );
    give_test_worker_setup_grant(&dependency_id);

    let api = state
        .stack_state
        .as_mut()
        .unwrap()
        .resources
        .get_mut(&api_id)
        .unwrap();
    if matches!(case, RecoveryTarget::CurrentWriter) {
        assert_eq!(
            api.get_internal_controller().unwrap().unwrap().get_status(),
            ResourceStatus::UpdateFailed
        );
    } else {
        // Older writers saved a failed status over the runnable interrupted checkpoint.
        api.internal_state = api.last_failed_state.clone();
    }
    if matches!(case, RecoveryTarget::LegacyRejection) {
        api.error = Some(
            api.get_internal_controller()
                .unwrap()
                .unwrap()
                .transition_to_update()
                .expect_err("the old in-flight controller rejects a new update")
                .into_generic(),
        );
        assert_ne!(api.error.as_ref().unwrap().code, "DEPLOYMENT_INTERRUPTED");
    }
    if matches!(case, RecoveryTarget::NoRetry) {
        let mut controller = api.get_internal_controller().unwrap().unwrap();
        controller.transition_to_failure();
        controller.transition_to_update().unwrap();
        api.set_internal_controller(Some(controller)).unwrap();
        let before = test_worker_configs_deployed(&api_identifier);
        let checkpoint = api.last_failed_state.clone();
        let error = api.error.clone();
        let executor =
            alien_infra::StackExecutor::new(&interrupted, ClientConfig::Test, None).unwrap();
        let refreshed = executor
            .refresh(state.stack_state.unwrap())
            .await
            .unwrap()
            .next_state;
        assert_eq!(
            test_worker_configs_deployed(&api_identifier),
            before,
            "refresh must not execute a failed record's update entry"
        );
        assert_eq!(
            refreshed.resources[&api_id].status,
            ResourceStatus::UpdateFailed
        );
        assert_eq!(refreshed.resources[&api_id].last_failed_state, checkpoint);
        assert_eq!(refreshed.resources[&api_id].error, error);
        return;
    }

    // Reload exactly the saved state, including both the actual failure and the interruption.
    state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    let mut target = interrupted;
    let corrective = matches!(
        case,
        RecoveryTarget::NewRelease
            | RecoveryTarget::NewDependency
            | RecoveryTarget::LegacyRejection
            | RecoveryTarget::NeverConfirm
            | RecoveryTarget::CurrentWriter
            | RecoveryTarget::Denied
    );
    if corrective {
        add_live_worker(&mut target, image_worker(&api_id, "test:v3", 1024));
    }
    if matches!(case, RecoveryTarget::NewDependency) {
        target
            .resources
            .get_mut(&api_id)
            .unwrap()
            .dependencies
            .push(ResourceRef::new("worker".into(), &dependency_id));
    }
    if matches!(case, RecoveryTarget::Removed) {
        target.resources.shift_remove(&api_id);
    }
    if matches!(case, RecoveryTarget::Denied) {
        withhold_test_worker_setup_grant(&api_id, TestSetupGrant::Update);
    }
    state.target_release = Some(release_of("rel_target", target));
    request_retry(&mut state);
    // run_to_completion stops at a failed state; execute the accepted retry transition first.
    state = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("retry transition should succeed")
        .state;
    assert_eq!(state.status, DeploymentStatus::UpdatePending);
    let state = if matches!(case, RecoveryTarget::NewDependency) {
        for _ in 0..MAX_STEPS {
            if matches!(
                state.status,
                DeploymentStatus::Running | DeploymentStatus::UpdateFailed
            ) {
                break;
            }
            let dependency_ready = state.stack_state.as_ref().unwrap().resources[&dependency_id]
                .status
                == ResourceStatus::Running;
            let writes = test_worker_configs_deployed(&api_identifier).len();
            state = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
                .await
                .unwrap()
                .state;
            if test_worker_configs_deployed(&api_identifier).len() > writes {
                assert!(
                    dependency_ready,
                    "the new update must not run before its dependency is ready"
                );
            }
        }
        state
    } else {
        run_to_completion(state, config).await
    };

    if matches!(case, RecoveryTarget::Denied) {
        assert_eq!(state.status, DeploymentStatus::UpdateFailed);
        let resources = &state.stack_state.as_ref().unwrap().resources;
        assert_eq!(resources[&api_id].status, ResourceStatus::UpdateFailed);
        assert_eq!(
            resources[&api_id].error.as_ref().unwrap().code,
            "MANAGEMENT_PERMISSION_MISSING"
        );
        assert!(resources[&api_id].last_failed_state.is_some());
        give_test_worker_setup_grant(&api_id);
        return;
    }
    assert_eq!(state.status, DeploymentStatus::Running, "{state:#?}");
    assert!(state.error.is_none());
    assert_eq!(
        state
            .current_release
            .as_ref()
            .unwrap()
            .release_id
            .as_deref(),
        Some("rel_target")
    );
    assert!(state.target_release.is_none());
    let resources = &state.stack_state.as_ref().unwrap().resources;
    for id in [&api_id, &dependency_id] {
        if id == &api_id && matches!(case, RecoveryTarget::Removed) {
            assert_eq!(resources[id].status, ResourceStatus::Deleted);
            assert_eq!(test_worker_deletes_issued(&api_identifier).len(), 1);
            continue;
        }
        assert_eq!(resources[id].status, ResourceStatus::Running);
        assert!(resources[id].error.is_none());
        assert!(test_worker_deletes_issued(&format!("test:worker:{id}")).is_empty());
    }
    let expected_images = if corrective {
        vec!["test:v1", "test:v2", "test:v3"]
    } else {
        vec!["test:v1", "test:v2"]
    };
    assert_eq!(
        test_worker_configs_deployed(&api_identifier)
            .into_iter()
            .map(|worker| worker.code)
            .collect::<Vec<_>>(),
        expected_images
            .into_iter()
            .map(|image| WorkerCode::Image {
                image: image.to_string()
            })
            .collect::<Vec<_>>(),
        "a corrective release supersedes the checkpoint without reissuing the older image",
    );
    assert_eq!(
        test_worker_configs_deployed(&dependency_identifier).len(),
        2
    );
}
