//! Lambda reservation demand uses the provider's current unreserved allocation.
use super::cloud_quotas::record;
use crate::{
    quotas::{QuotaIdentity, QuotaObservation, QuotaOutcome, QuotaResult},
    CheckResult,
};
use alien_aws_clients::{lambda::AccountSettings, LambdaApi};
use alien_core::{Platform, ResourceStatus, Stack, StackState, Worker, WorkerOutputs};

pub fn quota_identity(region: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Aws, service: "lambda".into(), provider_id: "L-B99A9384".into(),
        name: "Concurrent executions".into(), scope: region.into(),
        increase_url: format!("https://{region}.console.aws.amazon.com/servicequotas/home?region={region}#!/services/lambda/quotas/L-B99A9384"),
    }
}

/// Preserve the actual applied limit and current reserved allocation.
pub fn observation(settings: AccountSettings) -> Option<QuotaObservation> {
    let limit = settings.account_limit?;
    let total = limit.concurrent_executions?;
    let unreserved = limit.unreserved_concurrent_executions?;
    if unreserved > total {
        return None;
    }
    Some(QuotaObservation {
        used: f64::from(total - unreserved),
        limit: f64::from(total),
    })
}

fn reservation_outcome(needed: f64, observed: Option<QuotaObservation>) -> QuotaOutcome {
    let Some(QuotaObservation { used, limit }) = observed else {
        return QuotaOutcome::Unknown {
            reason: "Lambda account allocation limits are unavailable".into(),
        };
    };
    // Small-account minimums differ from the documented standard floor of 100.
    // GetAccountSettings does not expose that floor; do not invent one.
    if limit < 100.0 && needed <= limit - used {
        return QuotaOutcome::Unknown {
            reason: "Lambda does not expose the minimum unreserved concurrency for this low-limit account; AWS validates positive reservations during deployment".into(),
        };
    }
    if needed + 100.0 > limit - used {
        QuotaOutcome::Fail {
            used,
            limit,
            needed,
        }
    } else {
        QuotaOutcome::Pass {
            used,
            limit,
            needed,
        }
    }
}

pub async fn check(
    stack: &Stack,
    state: &StackState,
    region: &str,
    lambda: &dyn LambdaApi,
    result: &mut CheckResult,
) {
    let mut additional = 0.0;
    for (id, entry) in &stack.resources {
        let Some(worker) = entry.config.downcast_ref::<Worker>() else {
            continue;
        };
        let Some(desired) = worker.concurrency_limit else {
            continue;
        };
        if desired == 0 {
            // PutFunctionConcurrency(0) disables invocation without consuming capacity.
            continue;
        }
        // The worker controller only writes concurrency when its configured
        // limit changes. A code-only update does not restore external drift.
        if state.resources.get(id).is_some_and(|current| {
            current.status == ResourceStatus::Running
                && current
                    .config
                    .downcast_ref::<Worker>()
                    .is_some_and(|previous| previous.concurrency_limit == worker.concurrency_limit)
        }) {
            continue;
        }
        let held = match state.resources.get(id) {
            None => Some(0),
            Some(current) if current.status == ResourceStatus::Deleted => Some(0),
            Some(current)
                if current.status == ResourceStatus::Pending
                    && current.internal_state.is_none() =>
            {
                Some(0)
            }
            Some(current) => match current
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<WorkerOutputs>())
            {
                Some(outputs) if !outputs.worker_name.is_empty() => lambda
                    .get_function_concurrency(&outputs.worker_name)
                    .await
                    .ok()
                    .map(|response| response.reserved_concurrent_executions.unwrap_or(0)),
                _ => None,
            },
        };
        let Some(held) = held else {
            record(
                result,
                QuotaResult {
                    quota: quota_identity(region),
                    outcome: QuotaOutcome::Unknown {
                        reason: "could not verify an existing worker's reserved concurrency".into(),
                    },
                },
            );
            return;
        };
        // Do not credit reductions in other functions: their update order is not guaranteed.
        additional += f64::from(desired.saturating_sub(held));
    }
    if additional == 0.0 {
        return;
    }
    let observed = lambda
        .get_account_settings()
        .await
        .ok()
        .and_then(observation);
    record(
        result,
        QuotaResult {
            quota: quota_identity(region),
            outcome: reservation_outcome(additional, observed),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::lambda::{FunctionConcurrency, MockLambdaApi};
    use alien_core::{ResourceLifecycle, ResourceOutputs, StackResourceState, WorkerCode};
    #[test]
    fn low_account_floor_is_unknown_and_missing_fields_are_unknown() {
        let observed = observation(serde_json::from_str(r#"{"AccountLimit":{"ConcurrentExecutions":10,"UnreservedConcurrentExecutions":10}}"#).unwrap()).unwrap();
        assert_eq!(
            observed,
            QuotaObservation {
                used: 0.0,
                limit: 10.0
            }
        );
        assert!(matches!(
            reservation_outcome(1.0, Some(observed.clone())),
            QuotaOutcome::Unknown { .. }
        ));
        assert!(reservation_outcome(11.0, Some(observed)).blocks());
        assert!(observation(
            serde_json::from_str(r#"{"AccountLimit":{"ConcurrentExecutions":1000}}"#).unwrap()
        )
        .is_none());
        assert!(observation(serde_json::from_str(r#"{"AccountLimit":{"ConcurrentExecutions":10,"UnreservedConcurrentExecutions":11}}"#).unwrap()).is_none());
    }
    #[test]
    fn existing_reservations_and_exact_unreserved_boundary() {
        let observed = observation(serde_json::from_str(r#"{"AccountLimit":{"ConcurrentExecutions":1000,"UnreservedConcurrentExecutions":150}}"#).unwrap()).unwrap();
        assert_eq!(observed.used, 850.0);
        assert!(!reservation_outcome(50.0, Some(observed)).blocks());
        assert!(reservation_outcome(51.0, Some(observed)).blocks());
    }
    #[tokio::test]
    async fn update_uses_live_reservation_and_does_not_credit_other_reductions() {
        let mut stack = Stack::new("quota-test".into()).build();
        let mut state = StackState::new(Platform::Aws);
        for (id, desired) in [("grow", 60), ("shrink", 1)] {
            let worker = Worker::new(id.into())
                .permissions("test".into())
                .code(WorkerCode::Image {
                    image: "example:latest".into(),
                })
                .concurrency_limit(desired)
                .build();
            stack.resources.extend(
                Stack::new("fixture".into())
                    .add(worker, ResourceLifecycle::Live)
                    .build()
                    .resources,
            );
            let mut previous = stack.resources[id]
                .config
                .downcast_ref::<Worker>()
                .unwrap()
                .clone();
            previous.concurrency_limit = Some(if id == "grow" { 50 } else { 30 });
            let mut current = StackResourceState::new_pending(
                id.into(),
                alien_core::Resource::new(previous),
                None,
                vec![],
            );
            current.status = ResourceStatus::Running;
            current.outputs = Some(ResourceOutputs::new(WorkerOutputs {
                worker_name: id.into(),
                public_endpoints: Default::default(),
                private_endpoints: Default::default(),
                identifier: None,
                commands_push_target: None,
            }));
            state.resources.insert(id.into(), current);
        }
        let mut lambda = MockLambdaApi::new();
        lambda
            .expect_get_function_concurrency()
            .times(2)
            .returning(|name| {
                Ok(FunctionConcurrency {
                    reserved_concurrent_executions: Some(if name == "grow" { 50 } else { 30 }),
                })
            });
        lambda.expect_get_account_settings().times(1).returning(|| Ok(serde_json::from_str(r#"{"AccountLimit":{"ConcurrentExecutions":1000,"UnreservedConcurrentExecutions":105}}"#).unwrap()));
        let mut result = CheckResult::success();
        check(&stack, &state, "eu-west-1", &lambda, &mut result).await;
        assert!(!result.success);
        assert_eq!(result.quota_results.len(), 1);
        assert_eq!(
            result.quota_results[0].outcome,
            QuotaOutcome::Fail {
                used: 895.0,
                limit: 1000.0,
                needed: 10.0
            }
        );
    }
    #[tokio::test]
    async fn unchanged_running_reservation_needs_no_quota_reads_after_external_drift() {
        let worker = Worker::new("worker".into())
            .permissions("test".into())
            .code(WorkerCode::Image {
                image: "example:new".into(),
            })
            .concurrency_limit(60)
            .build();
        let stack = Stack::new("quota-test".into())
            .add(worker, ResourceLifecycle::Live)
            .build();
        let mut state = StackState::new(Platform::Aws);
        let mut current = StackResourceState::new_pending(
            "worker".into(),
            stack.resources["worker"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        current.outputs = Some(ResourceOutputs::new(WorkerOutputs {
            worker_name: "worker".into(),
            public_endpoints: Default::default(),
            private_endpoints: Default::default(),
            identifier: None,
            commands_push_target: None,
        }));
        state.resources.insert("worker".into(), current);
        let mut lambda = MockLambdaApi::new();
        // A code-only update does not restore a reservation changed outside Alien.
        lambda.expect_get_function_concurrency().times(0);
        lambda.expect_get_account_settings().times(0);
        let mut result = CheckResult::success();
        check(&stack, &state, "eu-west-1", &lambda, &mut result).await;
        assert!(result.success);
        assert!(result.quota_results.is_empty());
    }

    #[tokio::test]
    async fn zero_and_unreserved_workers_consume_no_capacity_and_make_no_read() {
        for reservation in [Some(0), None] {
            let mut worker = Worker::new("worker".into())
                .permissions("test".into())
                .code(WorkerCode::Image {
                    image: "example:latest".into(),
                })
                .build();
            worker.concurrency_limit = reservation;
            let stack = Stack::new("quota-test".into())
                .add(worker, ResourceLifecycle::Live)
                .build();
            let mut lambda = MockLambdaApi::new();
            // Real AWS accepts reservation zero even with applied/unreserved limits 10/10.
            lambda.expect_get_account_settings().times(0);
            lambda.expect_get_function_concurrency().times(0);
            let mut result = CheckResult::success();
            check(
                &stack,
                &StackState::new(Platform::Aws),
                "eu-west-1",
                &lambda,
                &mut result,
            )
            .await;
            assert!(result.success);
            assert!(result.quota_results.is_empty());
        }
    }
}
