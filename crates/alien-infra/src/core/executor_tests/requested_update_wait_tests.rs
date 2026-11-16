//! A resource that must update waits for its dependencies, whoever asked for the update.
//!
//! `mgmt` is a Frozen resource that the others depend on, `frozen-dep` another Frozen resource
//! behind it and `live-dep` a Live one. The test worker's update can be requested without a
//! config change and refused until a grant is given, which makes an update fail and later
//! recover with the config unchanged: the state a retry resumes.

use super::helpers::*;
use crate::error::Result;
use crate::worker::{
    give_test_worker_setup_grant, request_test_worker_update, withhold_test_worker_setup_grant,
    TestSetupGrant,
};
use alien_core::{ResourceLifecycle, ResourceRef, ResourceStatus, Stack, StackState, Worker};

fn worker(id: &str) -> Worker {
    Worker::new(id.to_string())
        .code(alien_core::WorkerCode::Image {
            image: format!("{id}-r1"),
        })
        .permissions("execution".to_string())
        .build()
}

/// `mgmt` <- `frozen-dep` and `live-dep`, with ids made unique by `p` (the test knobs are
/// process-wide).
fn stack(p: &str) -> (Stack, [String; 3]) {
    let ids = [
        format!("{p}-mgmt"),
        format!("{p}-frozen"),
        format!("{p}-live"),
    ];
    let edge = vec![ResourceRef::new(Worker::RESOURCE_TYPE, ids[0].clone())];
    let stack = Stack::new(format!("{p}-stack"))
        .add(worker(&ids[0]), ResourceLifecycle::Frozen)
        .add_with_dependencies(worker(&ids[1]), ResourceLifecycle::Frozen, edge.clone())
        .add_with_dependencies(worker(&ids[2]), ResourceLifecycle::Live, edge)
        .build();
    (stack, ids)
}

/// An executor for an install set up with direct authority, so runtime does not leave the
/// Frozen resources to setup. `resume` is the retry that resumes unchanged failures.
fn executor(stack: &Stack, resume: bool) -> Result<crate::core::StackExecutor> {
    crate::core::StackExecutor::builder(stack, alien_core::ClientConfig::Test)
        .deployment_config(&default_deployment_config())
        .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
        .resume_unchanged_failed_resources(resume)
        .build()
}

fn status(state: &StackState, id: &str) -> ResourceStatus {
    state.resources[id].status
}

/// Steps until `done` holds, at most `max` steps. Returns the steps taken.
async fn step_until(
    executor: &crate::core::StackExecutor,
    state: &mut StackState,
    max: usize,
    done: impl Fn(&StackState) -> bool,
) -> Result<usize> {
    for taken in 0..max {
        if done(state) {
            return Ok(taken);
        }
        *state = executor.step(state.clone()).await?.next_state;
    }
    assert!(done(state), "not reached in {max} steps");
    Ok(max)
}

/// A controller's own request for an update waits for a dependency that is failing its
/// update, and is planned once the dependency has settled.
#[tokio::test]
async fn a_requested_update_waits_for_a_failing_dependency() -> Result<()> {
    let (stack, ids) = stack("requested-waits");
    // Only `mgmt` fails its update; the dependents are healthy.
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let resume = executor(&stack, false)?;
    withhold_test_worker_setup_grant(&ids[0], TestSetupGrant::Update);
    request_test_worker_update(&ids[0], true);
    step_until(&resume, &mut state, 60, |s| {
        status(s, &ids[0]) == ResourceStatus::UpdateFailed
    })
    .await?;
    request_test_worker_update(&ids[0], false);
    request_test_worker_update(&ids[1], true);
    request_test_worker_update(&ids[2], true);
    let plan = resume.plan(&state)?;
    assert!(
        plan.updates.is_empty(),
        "planned behind a failed dependency: {:?}",
        plan.updates.keys()
    );

    // The dependency recovers; no dependent starts until it is Running again, and then
    // each one is served.
    give_test_worker_setup_grant(&ids[0]);
    let resume = executor(&stack, true)?;
    for _ in 0..60 {
        let before = state.clone();
        state = resume.step(state).await?.next_state;
        for id in &ids[1..] {
            if status(&state, id) == ResourceStatus::Updating {
                request_test_worker_update(id, false);
                assert_eq!(
                    status(&before, &ids[0]),
                    ResourceStatus::Running,
                    "{id} started while its dependency was {:?}",
                    status(&before, &ids[0])
                );
            }
        }
        // The deployment never reports a failure while the dependency recovers.
        if status(&state, &ids[0]) != ResourceStatus::UpdateFailed {
            assert_ne!(stack_status(&state), alien_core::StackStatus::Failure, "{:?}", ids.iter().map(|i| status(&state, i)).collect::<Vec<_>>());
        }
    }
    request_test_worker_update(&ids[1], false);
    request_test_worker_update(&ids[2], false);
    assert!(resume.is_synced(&state));
    assert!(resume.plan(&state)?.updates.is_empty());
    Ok(())
}

/// An update already running steps on whatever its dependency does: only the start waits.
#[tokio::test]
async fn an_updating_resource_keeps_stepping_while_its_dependency_fails() -> Result<()> {
    let (stack, ids) = stack("mid-flow");
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let executor = executor(&stack, true)?;
    request_test_worker_update(&ids[2], true);
    step_until(&executor, &mut state, 20, |s| {
        status(s, &ids[2]) == ResourceStatus::Updating
    })
    .await?;
    request_test_worker_update(&ids[2], false);
    state.resources.get_mut(&ids[0]).unwrap().status = ResourceStatus::UpdateFailed;
    step_until(&executor, &mut state, 20, |s| {
        status(s, &ids[2]) == ResourceStatus::Running
    })
    .await?;
    Ok(())
}

/// The status the deployment reports for the stack after a step, from the resource statuses.
fn stack_status(state: &StackState) -> alien_core::StackStatus {
    let statuses: Vec<_> = state.resources.values().map(|r| r.status).collect();
    StackState::compute_stack_status_from_resources(&statuses).unwrap()
}

/// The dependency and its dependent both ask for an update in one release. The dependent
/// starts only once the dependency has finished its own, and the run is not over (synced with
/// nothing left to plan) before the dependent's request is served.
#[tokio::test]
async fn a_deferred_request_is_served_in_the_same_run() -> Result<()> {
    let (stack, ids) = stack("served");
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let executor = crate::core::StackExecutor::builder(&stack, alien_core::ClientConfig::Test)
        .deployment_config(&default_deployment_config())
        .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
        .resume_unchanged_failed_resources(true)
        .rollout_id("rel_a".to_string())
        .build()?;
    request_test_worker_update(&ids[0], true);
    request_test_worker_update(&ids[2], true);

    let mut dependency_done = false;
    let mut served = false;
    let mut first_synced = None;
    for step in 0..60 {
        let before = state.clone();
        state = executor.step(state).await?.next_state;
        for id in [&ids[0], &ids[2]] {
            // The update clears the request, as a real one does.
            if status(&state, id) == ResourceStatus::Updating {
                request_test_worker_update(id, false);
            }
        }
        assert_ne!(
            stack_status(&state),
            alien_core::StackStatus::Failure,
            "step {step}"
        );
        dependency_done |= status(&state, &ids[0]) == ResourceStatus::Running
            && state.resources[&ids[0]].requested_update_ran.is_some();
        if status(&before, &ids[2]) != ResourceStatus::Updating
            && status(&state, &ids[2]) == ResourceStatus::Updating
        {
            served = true;
            assert!(
                dependency_done && status(&before, &ids[0]) == ResourceStatus::Running,
                "the dependent started before the dependency finished its update"
            );
        }
        if first_synced.is_none() && executor.is_synced(&state) {
            first_synced = Some(step);
            assert!(
                served || executor.plan(&state)?.updates.is_empty(),
                "the run ended with the dependent's request unserved"
            );
        }
    }
    request_test_worker_update(&ids[0], false);
    request_test_worker_update(&ids[2], false);
    assert!(served, "the dependent's request was never served");
    Ok(())
}

/// A run with no rollout id cannot tell a dependency that keeps asking from one yet to run;
/// the dependent's request does not wait for that for good.
#[tokio::test]
async fn a_request_does_not_wait_for_good_on_a_dependency_that_keeps_asking() -> Result<()> {
    let (stack, ids) = stack("keeps-asking");
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let executor = executor(&stack, true)?;
    // Never cleared: the dependency's check stays true.
    request_test_worker_update(&ids[0], true);
    request_test_worker_update(&ids[2], true);
    let mut served = false;
    for _ in 0..60 {
        state = executor.step(state).await?.next_state;
        if status(&state, &ids[2]) == ResourceStatus::Updating {
            served = true;
            request_test_worker_update(&ids[2], false);
        }
    }
    request_test_worker_update(&ids[0], false);
    request_test_worker_update(&ids[2], false);
    assert!(served, "the dependent's request waited for good");
    Ok(())
}
