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

/// The state the failed release left: `mgmt` and both dependents in UpdateFailed with their
/// config unchanged, each refused the grant its update needs.
async fn all_three_failed(stack: &Stack, ids: &[String; 3]) -> Result<StackState> {
    let mut state = run_to_synced(&new_executor(stack)?, new_test_state()).await?;
    // No resume: failures stay where they fall.
    let executor = executor(stack, false)?;
    // The dependents fail first, while `mgmt` is healthy; then `mgmt` fails.
    for id in &ids[1..] {
        withhold_test_worker_setup_grant(id, TestSetupGrant::Update);
        request_test_worker_update(id, true);
    }
    step_until(&executor, &mut state, 60, |s| {
        ids[1..]
            .iter()
            .all(|id| status(s, id) == ResourceStatus::UpdateFailed)
    })
    .await?;
    for id in &ids[1..] {
        request_test_worker_update(id, false);
    }
    withhold_test_worker_setup_grant(&ids[0], TestSetupGrant::Update);
    request_test_worker_update(&ids[0], true);
    step_until(&executor, &mut state, 60, |s| {
        status(s, &ids[0]) == ResourceStatus::UpdateFailed
    })
    .await?;
    request_test_worker_update(&ids[0], false);
    Ok(state)
}

/// The sequence that stuck an install: the dependency failed its update, the dependents were
/// failed behind it, and one retry resumed them all at once. Now the dependency recovers
/// first, then the dependents resume and settle.
#[tokio::test]
async fn a_retry_recovers_the_dependency_before_its_failed_dependents() -> Result<()> {
    let (stack, ids) = stack("retry-order");
    let mut state = all_three_failed(&stack, &ids).await?;

    // Setup's grant arrives for all, and one explicit retry (resume enabled) runs.
    for id in &ids {
        give_test_worker_setup_grant(id);
    }
    let executor = executor(&stack, true)?;
    let mut steps = 0;
    while !executor.is_synced(&state) {
        let before = state.clone();
        state = executor.step(state).await?.next_state;
        steps += 1;
        assert!(steps <= 60, "the stack never synced");
        for id in &ids[1..] {
            let resumed = status(&before, id) == ResourceStatus::UpdateFailed
                && status(&state, id) != ResourceStatus::UpdateFailed;
            if resumed {
                assert_eq!(
                    status(&before, &ids[0]),
                    ResourceStatus::Running,
                    "{id} was resumed while its dependency was {:?}",
                    status(&before, &ids[0])
                );
            }
        }
    }
    println!("synced after {steps} steps");
    assert_all_running(&state, &ids.each_ref().map(|id| id.as_str()));
    Ok(())
}

/// A dependency that stays failed keeps its dependents as they are, and the failure that
/// holds them is the dependency's own, still recorded on it.
#[tokio::test]
async fn a_dependency_that_stays_failed_keeps_its_dependents_unresumed() -> Result<()> {
    let (stack, ids) = stack("stays-failed");
    let mut state = all_three_failed(&stack, &ids).await?;
    let attempts: Vec<_> = ids[1..]
        .iter()
        .map(|id| state.resources[id].retry_attempt)
        .collect();

    // Only the dependents' grants arrive; `mgmt` keeps failing.
    for id in &ids[1..] {
        give_test_worker_setup_grant(id);
    }
    let executor = executor(&stack, true)?;
    for _ in 0..30 {
        state = executor.step(state).await?.next_state;
        assert_ne!(
            status(&state, &ids[0]),
            ResourceStatus::Running,
            "the dependency cannot recover while it is refused"
        );
        for id in &ids[1..] {
            assert_eq!(
                status(&state, id),
                ResourceStatus::UpdateFailed,
                "{id} must stay as it is"
            );
        }
    }
    let attempts_after: Vec<_> = ids[1..]
        .iter()
        .map(|id| state.resources[id].retry_attempt)
        .collect();
    assert_eq!(attempts, attempts_after, "a dependent was retried");
    assert!(
        state.resources[&ids[0]].error.is_some(),
        "the dependency's failure is still recorded"
    );
    assert!(!executor.is_synced(&state));
    give_test_worker_setup_grant(&ids[0]);
    Ok(())
}

/// A controller's own request for an update waits for a dependency that is failing its
/// update, and is planned once the dependency has settled.
#[tokio::test]
async fn a_requested_update_waits_for_a_failing_dependency() -> Result<()> {
    let (stack, ids) = stack("requested-waits");
    let mut state = all_three_failed(&stack, &ids).await?;
    // The dependents are settled and healthy again; only `mgmt` is failing.
    for id in &ids[1..] {
        give_test_worker_setup_grant(id);
    }
    let resume = executor(&stack, true)?;
    // `mgmt` is held failed by marking the dependents' recovery while it is refused: they
    // stay failed (see the test above), so put them back to Running as an earlier retry of
    // theirs would have, keeping `mgmt` failed.
    for id in &ids[1..] {
        state.resources.get_mut(id).unwrap().status = ResourceStatus::Running;
    }
    request_test_worker_update(&ids[1], true);
    request_test_worker_update(&ids[2], true);
    let plan = resume.plan(&state)?;
    assert!(
        plan.updates.is_empty(),
        "planned behind a failed dependency: {:?}",
        plan.updates.keys()
    );

    // The dependency recovers; once it is rolled the requests are planned.
    give_test_worker_setup_grant(&ids[0]);
    let mut state = state;
    step_until(&resume, &mut state, 60, |s| {
        status(s, &ids[0]) == ResourceStatus::Running
    })
    .await?;
    let plan = resume.plan(&state)?;
    for id in &ids[1..] {
        assert!(plan.updates.contains_key(id), "{id} is planned: {plan:?}");
    }
    request_test_worker_update(&ids[1], false);
    request_test_worker_update(&ids[2], false);
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
