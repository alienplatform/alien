use std::{sync::Arc, time::Duration};

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, InitialSetupAuthority,
    ResourceLifecycle, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::setup_scaffolding::{self, ScaffoldingProgress, SetupScaffoldingContext};
use alien_infra::{state_utils::StackStateExt, StackExecutor};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::{
    loop_contract::{LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    runner::{run_with_lease_renewal, DelayStrategy, RunnerPolicy, RunnerResult},
    transport::DeploymentLoopTransport,
    ErrorData, Result,
};

/// Run privileged setup teardown after runtime cleanup reached the handoff
/// point.
///
/// This is intentionally separate from the normal deployment step machine:
/// managers and agents stop at `TeardownRequired`, while setup-authority
/// callers such as the CLI can continue with their own credentials.
pub async fn run_setup_teardown_after_handoff(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<Option<RunnerResult>> {
    run_with_lease_renewal(
        deployment_id,
        transport,
        run_setup_teardown_after_handoff_inner(
            state,
            config,
            client_config,
            deployment_id,
            policy,
            transport,
            service_provider,
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_setup_teardown_after_handoff_inner(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<Option<RunnerResult>> {
    if !matches!(
        state.status,
        DeploymentStatus::TeardownRequired | DeploymentStatus::TeardownFailed
    ) {
        return Ok(None);
    }

    if policy.operation != LoopOperation::Delete {
        return Err(AlienError::new(ErrorData::MissingConfiguration {
            message: "Setup teardown can only run during delete operations".to_string(),
        }));
    }

    info!(deployment_id = %deployment_id, "Starting setup-owned teardown");
    state.status = DeploymentStatus::TeardownRequired;
    state.error = None;

    let service_provider = service_provider
        .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));

    // Frozen teardown waits for this: the network cannot go while the scaffolding's group and
    // connector still hold interfaces in its subnets.
    let mut scaffolding_steps = 0;
    loop {
        scaffolding_steps += 1;
        let progress =
            match teardown_setup_scaffolding(state, client_config, service_provider.as_ref()).await
            {
                Ok(progress) => progress,
                Err(error) => {
                    fail_setup_teardown(deployment_id, state, config, transport, error.clone())
                        .await?;
                    return Err(error);
                }
            };
        if progress == ScaffoldingProgress::Done {
            checkpoint_setup_teardown_state(
                deployment_id,
                state,
                config,
                transport,
                None,
                Vec::new(),
            )
            .await?;
            break;
        }
        if scaffolding_steps >= policy.max_steps {
            let error = AlienError::new(ErrorData::StackExecutionFailed {
                message: format!(
                    "Setup scaffolding teardown did not complete within {} steps",
                    policy.max_steps
                ),
            });
            fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
            return Err(error);
        }
        checkpoint_setup_teardown_state(
            deployment_id,
            state,
            config,
            transport,
            Some(SCAFFOLDING_TEARDOWN_DELAY_MS),
            Vec::new(),
        )
        .await?;
        if policy.delay_strategy == DelayStrategy::Yield {
            return Ok(Some(RunnerResult {
                loop_result: LoopResult {
                    stop_reason: LoopStopReason::Delayed,
                    outcome: LoopOutcome::Neutral,
                    final_status: state.status,
                },
                steps_executed: scaffolding_steps,
            }));
        }
        sleep(Duration::from_millis(SCAFFOLDING_TEARDOWN_DELAY_MS)).await;
    }

    let mut stack_state = state.stack_state.take().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for setup teardown".to_string(),
        })
    })?;

    let prepared = match stack_state
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare setup-owned resources for teardown".to_string(),
        }) {
        Ok(prepared) => prepared,
        Err(error) => {
            state.stack_state = Some(stack_state);
            fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
            return Err(error);
        }
    };
    info!(
        deployment_id = %deployment_id,
        prepared_count = prepared.len(),
        prepared = ?prepared,
        "Prepared setup-owned resources for teardown"
    );

    state.stack_state = Some(stack_state);
    checkpoint_setup_teardown_state(deployment_id, state, config, transport, None, Vec::new())
        .await?;

    let executor = StackExecutor::for_deletion_with_service_provider(
        client_config.clone(),
        config,
        service_provider,
        Some(vec![ResourceLifecycle::Frozen]),
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create stack executor for setup teardown".to_string(),
    })?;

    for step_count in 1..=policy.max_steps {
        let status = setup_teardown_status(state.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Stack state required during setup teardown".to_string(),
            })
        })?)?;

        match status {
            StackStatus::Deleted => {
                state.status = DeploymentStatus::Deleted;
                state.error = None;
                checkpoint_setup_teardown_state(
                    deployment_id,
                    state,
                    config,
                    transport,
                    None,
                    Vec::new(),
                )
                .await?;
                return Ok(Some(RunnerResult {
                    loop_result: LoopResult {
                        stop_reason: LoopStopReason::Deleted,
                        outcome: LoopOutcome::Success,
                        final_status: state.status,
                    },
                    steps_executed: step_count - 1,
                }));
            }
            StackStatus::Failure => {
                let error = AlienError::new(ErrorData::StackExecutionFailed {
                    message: "Setup-owned resource teardown failed".to_string(),
                });
                fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
                return Err(error);
            }
            StackStatus::Pending | StackStatus::InProgress | StackStatus::Running => {}
        }

        // Keep the last checkpoint in `state` while the cloud operation is in flight. Lease loss
        // cancels this future; retaining the checkpoint lets the next owner resume safely even if
        // cancellation happens after the provider has started a long-running deletion.
        let current_stack_state = state.stack_state.clone().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Stack state required for setup teardown step".to_string(),
            })
        })?;
        let step_result = match executor.step(current_stack_state).await.context(
            ErrorData::StackExecutionFailed {
                message: "Failed to execute setup teardown step".to_string(),
            },
        ) {
            Ok(step_result) => step_result,
            Err(error) => {
                fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
                return Err(error);
            }
        };
        let suggested_delay_ms = step_result.suggested_delay_ms;
        let heartbeats = step_result.heartbeats.clone();
        state.stack_state = Some(step_result.next_state);

        checkpoint_setup_teardown_state(
            deployment_id,
            state,
            config,
            transport,
            suggested_delay_ms,
            heartbeats,
        )
        .await?;

        if let Some(delay_ms) = suggested_delay_ms {
            if policy.delay_strategy == DelayStrategy::Yield {
                debug!(
                    deployment_id = %deployment_id,
                    delay_ms,
                    "Setup teardown step requested a delay; yielding"
                );
                return Ok(Some(RunnerResult {
                    loop_result: LoopResult {
                        stop_reason: LoopStopReason::Delayed,
                        outcome: LoopOutcome::Neutral,
                        final_status: state.status,
                    },
                    steps_executed: step_count,
                }));
            }
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let error = AlienError::new(ErrorData::StackExecutionFailed {
        message: format!(
            "Setup-owned resource teardown did not complete within {} steps",
            policy.max_steps
        ),
    });
    fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;

    Ok(Some(RunnerResult {
        loop_result: LoopResult {
            stop_reason: LoopStopReason::BudgetExceeded,
            outcome: LoopOutcome::Failure,
            final_status: state.status,
        },
        steps_executed: policy.max_steps,
    }))
}

/// How long to wait before asking again whether AWS has released what holds a scaffolding object.
const SCAFFOLDING_TEARDOWN_DELAY_MS: u64 = 15_000;

/// Only a direct setup records scaffolding; an imported setup's template owns and removes its own.
async fn teardown_setup_scaffolding(
    state: &mut DeploymentState,
    client_config: &ClientConfig,
    service_provider: &dyn alien_infra::PlatformServiceProvider,
) -> Result<ScaffoldingProgress> {
    let Some(runtime_metadata) = state.runtime_metadata.as_mut() else {
        return Ok(ScaffoldingProgress::Done);
    };
    if runtime_metadata.initial_setup_authority != InitialSetupAuthority::DirectSetup
        || runtime_metadata.setup_scaffolding.is_empty()
    {
        return Ok(ScaffoldingProgress::Done);
    }
    let resource_prefix = state
        .stack_state
        .as_ref()
        .map(|stack_state| stack_state.resource_prefix.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Stack state required for setup teardown".to_string(),
            })
        })?;
    let ctx = SetupScaffoldingContext {
        client_config,
        service_provider,
        resource_prefix: &resource_prefix,
    };
    setup_scaffolding::teardown(&ctx, &mut runtime_metadata.setup_scaffolding)
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to delete setup scaffolding".to_string(),
        })
}

async fn fail_setup_teardown(
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    transport: &dyn DeploymentLoopTransport,
    error: AlienError<ErrorData>,
) -> Result<()> {
    state.status = DeploymentStatus::TeardownFailed;
    state.error = Some(error.into_generic());
    checkpoint_setup_teardown_state(deployment_id, state, config, transport, None, Vec::new()).await
}

async fn checkpoint_setup_teardown_state(
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    transport: &dyn DeploymentLoopTransport,
    suggested_delay_ms: Option<u64>,
    heartbeats: Vec<alien_core::ResourceHeartbeat>,
) -> Result<()> {
    match transport
        .reconcile_step(
            deployment_id,
            state,
            config,
            false,
            suggested_delay_ms,
            heartbeats,
            Vec::new(),
        )
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to checkpoint setup teardown".to_string(),
        }) {
        Ok(reconciled) => {
            if let Some(updated_state) = reconciled.state {
                *state = updated_state;
            }
            if let Some(updated_config) = reconciled.config {
                *config = updated_config;
            }
            Ok(())
        }
        Err(error) => {
            warn!(deployment_id = %deployment_id, error = %error, "Failed to checkpoint setup teardown");
            Err(error)
        }
    }
}

fn setup_teardown_status(stack_state: &StackState) -> Result<StackStatus> {
    let mut statuses = Vec::new();
    for resource in stack_state.resources.values() {
        match resource.lifecycle {
            Some(ResourceLifecycle::Frozen) => statuses.push(resource.status),
            Some(ResourceLifecycle::Live) => {}
            None => {
                return Err(AlienError::new(ErrorData::MissingConfiguration {
                    message: format!(
                        "Resource '{}' is missing lifecycle metadata required for setup teardown",
                        resource.config.id()
                    ),
                }));
            }
        }
    }

    if statuses.is_empty() {
        return Ok(StackStatus::Deleted);
    }

    StackState::compute_stack_status_from_resources(&statuses).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute setup teardown status".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::StepReconcileResult;
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
    use alien_core::{
        EnvironmentVariablesSnapshot, Platform, RuntimeMetadata, SetupScaffolding, StackSettings,
    };
    use alien_infra::MockPlatformServiceProvider;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const BUILD_ROLE: &str = "test-agents-build";

    #[derive(Default)]
    struct RecordingTransport {
        checkpoints: Mutex<Vec<DeploymentState>>,
    }

    #[async_trait::async_trait]
    impl DeploymentLoopTransport for RecordingTransport {
        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            state: &DeploymentState,
            _config: &DeploymentConfig,
            _update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, AlienError> {
            self.checkpoints.lock().unwrap().push(state.clone());
            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
    }

    fn teardown_required(authority: InitialSetupAuthority) -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::TeardownRequired,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: Some(StackState::with_resource_prefix(
                Platform::Aws,
                "test".to_string(),
            )),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            runtime_metadata: Some(RuntimeMetadata {
                initial_setup_authority: authority,
                setup_scaffolding: BTreeMap::from([(
                    "agents".to_string(),
                    SetupScaffolding::AwsSandbox {
                        build_role_name: BUILD_ROLE.to_string(),
                        egress: None,
                    },
                )]),
                ..Default::default()
            }),
        }
    }

    async fn run(
        state: &mut DeploymentState,
        provider: MockPlatformServiceProvider,
        transport: &RecordingTransport,
    ) -> Result<Option<RunnerResult>> {
        run_with(state, provider, transport, DelayStrategy::Inline).await
    }

    async fn run_with(
        state: &mut DeploymentState,
        provider: MockPlatformServiceProvider,
        transport: &RecordingTransport,
        delay_strategy: DelayStrategy,
    ) -> Result<Option<RunnerResult>> {
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        run_setup_teardown_after_handoff(
            state,
            &mut config,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            "dep_test",
            &RunnerPolicy {
                operation: LoopOperation::Delete,
                delay_strategy,
                ..Default::default()
            },
            transport,
            Some(Arc::new(provider)),
        )
        .await
    }

    #[tokio::test]
    async fn direct_teardown_deletes_the_recorded_build_role() {
        let mut iam = MockIamApi::new();
        iam.expect_delete_role_policy()
            .withf(|role, policy| role == BUILD_ROLE && policy == "sandbox-image-build")
            .times(1)
            .returning(|_, _| Ok(()));
        iam.expect_delete_role()
            .withf(|role| role == BUILD_ROLE)
            .times(1)
            .returning(|_| Ok(()));
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let transport = RecordingTransport::default();
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);

        run(&mut state, provider, &transport).await.unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert!(state.runtime_metadata.unwrap().setup_scaffolding.is_empty());
        let first = &transport.checkpoints.lock().unwrap()[0];
        assert!(
            first
                .runtime_metadata
                .as_ref()
                .unwrap()
                .setup_scaffolding
                .is_empty(),
            "the deleted role leaves the persisted record before anything else is torn down"
        );
    }

    /// A provider with no expectations panics on any cloud call.
    #[tokio::test]
    async fn imported_teardown_leaves_scaffolding_to_the_template() {
        let transport = RecordingTransport::default();
        let mut state = teardown_required(InitialSetupAuthority::ImportedHandoff);

        run(&mut state, MockPlatformServiceProvider::new(), &transport)
            .await
            .unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
    }

    /// The network's teardown would fail while the deny group still holds interfaces in its
    /// subnets, so a group AWS has not released yet stops setup teardown until a later call.
    #[tokio::test]
    async fn frozen_teardown_waits_for_a_deny_group_aws_has_not_released() {
        let mut cloudcontrol = alien_aws_clients::cloudcontrol::MockCloudControlApi::new();
        cloudcontrol.expect_list_resources().returning(|_, _| {
            Ok(alien_aws_clients::cloudcontrol::ListResourcesResponse {
                resource_descriptions: vec![],
                next_token: None,
            })
        });
        let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
        ec2.expect_delete_security_group()
            .withf(|group| group == "sg-held")
            .times(1)
            .returning(|group| {
                Err(AlienError::new(
                    alien_aws_clients::ErrorData::RemoteResourceConflict {
                        message: "DependencyViolation".to_string(),
                        resource_type: "SecurityGroup".to_string(),
                        resource_name: group.to_string(),
                    },
                ))
            });
        let (cloudcontrol, ec2) = (Arc::new(cloudcontrol), Arc::new(ec2));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_cloudcontrol_client()
            .returning(move |_| Ok(cloudcontrol.clone()));
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);
        let record = SetupScaffolding::AwsSandbox {
            build_role_name: BUILD_ROLE.to_string(),
            egress: Some(alien_core::AwsSandboxEgressScaffolding {
                operator_role_name: "test-agents-egress".to_string(),
                security_group_id: Some("sg-held".to_string()),
                connector_arn: None,
            }),
        };
        state
            .runtime_metadata
            .as_mut()
            .unwrap()
            .setup_scaffolding
            .insert("agents".to_string(), record.clone());
        let transport = RecordingTransport::default();

        let result = run_with(&mut state, provider, &transport, DelayStrategy::Yield)
            .await
            .unwrap()
            .expect("the runner reports why it stopped");

        assert_eq!(result.loop_result.stop_reason, LoopStopReason::Delayed);
        assert_eq!(state.status, DeploymentStatus::TeardownRequired);
        assert_eq!(
            state.runtime_metadata.unwrap().setup_scaffolding["agents"],
            record,
            "the group stays recorded for the next call"
        );
        let checkpoints = transport.checkpoints.lock().unwrap();
        assert!(
            checkpoints
                .iter()
                .all(|checkpoint| checkpoint.status == DeploymentStatus::TeardownRequired),
            "nothing past the scaffolding may start"
        );
    }
}
