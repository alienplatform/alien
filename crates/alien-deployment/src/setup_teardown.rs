use std::{sync::Arc, time::Duration};

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, InitialSetupAuthority,
    ResourceLifecycle, RuntimeMetadata, SetupScaffolding, StackState, StackStatus,
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

    // Checked before anything is checkpointed, so a retry of a broken record keeps its error.
    if state.stack_state.is_none() {
        return Err(AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for setup teardown".to_string(),
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
                    "Setup scaffolding teardown did not complete within {} steps; still waiting on {}",
                    policy.max_steps,
                    outstanding_scaffolding(state)
                ),
            });
            fail_setup_teardown(deployment_id, state, config, transport, error).await?;
            return Ok(Some(RunnerResult {
                loop_result: LoopResult {
                    stop_reason: LoopStopReason::BudgetExceeded,
                    outcome: LoopOutcome::Failure,
                    final_status: state.status,
                },
                steps_executed: scaffolding_steps,
            }));
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

    if let Err(error) = delete_synced_vault_secrets(state, config, client_config).await {
        fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
        return Err(error);
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

/// The next object each sandbox's scaffolding teardown waits on. The record drops each object once
/// it is gone, so this is what AWS has not released.
fn outstanding_scaffolding(state: &DeploymentState) -> String {
    let outstanding: Vec<String> = state
        .runtime_metadata
        .iter()
        .flat_map(|metadata| &metadata.setup_scaffolding)
        .map(|(resource_id, scaffolding)| match scaffolding {
            SetupScaffolding::AwsSandbox { image_arn: Some(image), .. } => {
                format!("sandbox '{resource_id}': MicroVM image '{image}'")
            }
            SetupScaffolding::AwsSandbox { egress: Some(egress), build_role_name, image_arn: None } => {
                match (&egress.connector_request, &egress.connector_arn, &egress.security_group_id) {
                    (Some(request), _, _) => format!(
                        "sandbox '{resource_id}': Cloud Control request '{request}' for its network connector"
                    ),
                    (None, Some(connector), _) => {
                        format!("sandbox '{resource_id}': network connector '{connector}'")
                    }
                    (None, None, Some(group)) => format!(
                        "sandbox '{resource_id}': security group '{group}', which network interfaces still hold"
                    ),
                    (None, None, None) => format!(
                        "sandbox '{resource_id}': roles '{}' and '{build_role_name}'",
                        egress.operator_role_name
                    ),
                }
            }
            SetupScaffolding::AwsSandbox { egress: None, build_role_name, image_arn: None } => {
                format!("sandbox '{resource_id}': role '{build_role_name}'")
            }
        })
        .collect();
    if outstanding.is_empty() {
        "nothing recorded".to_string()
    } else {
        outstanding.join("; ")
    }
}

/// How long to wait before asking again whether AWS has released what holds a scaffolding object.
const SCAFFOLDING_TEARDOWN_DELAY_MS: u64 = 15_000;

/// Only a direct setup scaffolds; an imported setup's template owns and removes its own. A stack
/// that scaffolds anything counts even with an empty record, which a lost checkpoint can leave.
pub(crate) fn has_setup_scaffolding(
    runtime_metadata: Option<&RuntimeMetadata>,
    platform: alien_core::Platform,
) -> bool {
    runtime_metadata.is_some_and(|metadata| {
        metadata.initial_setup_authority == InitialSetupAuthority::DirectSetup
            && (!metadata.setup_scaffolding.is_empty()
                || metadata
                    .prepared_stack
                    .as_ref()
                    .is_some_and(|stack| setup_scaffolding::scaffolds_any(stack, platform)))
    })
}

async fn teardown_setup_scaffolding(
    state: &mut DeploymentState,
    client_config: &ClientConfig,
    service_provider: &dyn alien_infra::PlatformServiceProvider,
) -> Result<ScaffoldingProgress> {
    let platform = state.platform;
    if !has_setup_scaffolding(state.runtime_metadata.as_ref(), platform) {
        return Ok(ScaffoldingProgress::Done);
    }
    let Some(runtime_metadata) = state.runtime_metadata.as_mut() else {
        return Ok(ScaffoldingProgress::Done);
    };
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
    if let Some(stack) = runtime_metadata.prepared_stack.as_ref() {
        setup_scaffolding::recover_unrecorded(
            &ctx,
            stack,
            platform,
            &mut runtime_metadata.setup_scaffolding,
        )
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to look for setup scaffolding the record does not hold".to_string(),
        })?;
    }
    setup_scaffolding::teardown(&ctx, &mut runtime_metadata.setup_scaffolding)
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to delete setup scaffolding".to_string(),
        })
}

/// Deletes the values this deployment wrote to its `secrets` vault, which the vault's own delete
/// leaves behind. Initial setup records each name durably before writing it, and runtime cleanup
/// clears the inventory once it deleted them, so an empty inventory means nothing to delete.
async fn delete_synced_vault_secrets(
    state: &mut DeploymentState,
    config: &DeploymentConfig,
    client_config: &ClientConfig,
) -> Result<()> {
    let Some(runtime_metadata) = state.runtime_metadata.as_mut() else {
        return Ok(());
    };
    let stack_state = state.stack_state.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for vault secret deletion".to_string(),
        })
    })?;
    // Sync records a hash even for a stack with no vault, so the inventory alone is not enough.
    if !crate::helpers::has_secrets_vault(stack_state)
        || (runtime_metadata.last_synced_env_vars_hash.is_none()
            && runtime_metadata.last_synced_secret_names.is_empty())
    {
        return Ok(());
    }
    let prepared_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Prepared stack required for vault secret deletion".to_string(),
        })
    })?;
    crate::helpers::delete_deployment_vault_secrets(
        &prepared_stack,
        stack_state,
        client_config,
        config,
        runtime_metadata,
    )
    .await?;
    Ok(())
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
    use alien_core::{EnvironmentVariablesSnapshot, Platform, SetupScaffolding, StackSettings};
    use alien_infra::MockPlatformServiceProvider;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Teardown also looks for groups setup made that the record lost; these tests have none.
    fn no_other_tagged_group(ec2: &mut alien_aws_clients::ec2::MockEc2Api) {
        ec2.expect_describe_security_groups().returning(|_| {
            Ok(alien_aws_clients::ec2::DescribeSecurityGroupsResponse {
                security_group_info: None,
                next_token: None,
            })
        });
    }

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
                        image_arn: None,
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
    async fn a_record_without_stack_state_fails_before_anything_is_checkpointed() {
        let transport = RecordingTransport::default();
        let mut state = DeploymentState {
            status: DeploymentStatus::TeardownFailed,
            stack_state: None,
            error: Some(AlienError::new(alien_error::GenericError {
                message: "the earlier failure".to_string(),
            })),
            ..teardown_required(InitialSetupAuthority::DirectSetup)
        };

        assert!(
            run(&mut state, MockPlatformServiceProvider::new(), &transport)
                .await
                .is_err()
        );
        assert!(transport.checkpoints.lock().unwrap().is_empty());
        assert_eq!(state.status, DeploymentStatus::TeardownFailed);
        assert!(state.error.is_some(), "the earlier failure stays recorded");
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

    /// The record lost the role's name to a checkpoint that never landed; its setup tags still
    /// mark it as this deployment's.
    #[tokio::test]
    async fn direct_teardown_deletes_a_tagged_build_role_the_record_never_held() {
        teardown_after_a_lost_record(ResourceLifecycle::Live, MockPlatformServiceProvider::new())
            .await;
    }

    /// A Frozen sandbox's image goes with the record: recovery finds it by the name the controller
    /// gives it, and teardown deletes it once.
    #[tokio::test]
    async fn direct_teardown_deletes_the_image_of_a_frozen_sandbox_the_record_never_held() {
        let mut microvms = alien_aws_clients::lambda_microvms::MockLambdaMicrovmsApi::new();
        microvms
            .expect_get_microvm_image()
            .withf(|image| image == SANDBOX_IMAGE)
            .returning(|image| {
                Ok(alien_aws_clients::lambda_microvms::MicrovmImage {
                    image_identifier: Some(image.to_string()),
                    image_arn: Some(image.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("CREATED".to_string()),
                })
            });
        microvms
            .expect_delete_microvm_image()
            .withf(|image| image == SANDBOX_IMAGE)
            .times(1)
            .returning(|_| Ok(()));
        teardown_after_a_lost_record(ResourceLifecycle::Frozen, microvms_provider(microvms)).await;
    }

    /// Recovery runs on every teardown pass, so an image already deleted must not come back into
    /// the record and be deleted again. The mock panics on any delete.
    #[tokio::test]
    async fn a_frozen_image_already_gone_is_not_recovered() {
        let mut microvms = alien_aws_clients::lambda_microvms::MockLambdaMicrovmsApi::new();
        microvms
            .expect_get_microvm_image()
            .withf(|image| image == SANDBOX_IMAGE)
            .returning(|image| {
                Err(AlienError::new(
                    alien_aws_clients::ErrorData::RemoteResourceNotFound {
                        resource_type: "MicroVM image".to_string(),
                        resource_name: image.to_string(),
                    },
                ))
            });
        teardown_after_a_lost_record(ResourceLifecycle::Frozen, microvms_provider(microvms)).await;
    }

    fn microvms_provider(
        microvms: alien_aws_clients::lambda_microvms::MockLambdaMicrovmsApi,
    ) -> MockPlatformServiceProvider {
        let microvms = Arc::new(microvms);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(microvms.clone()));
        provider
    }

    async fn teardown_after_a_lost_record(
        lifecycle: ResourceLifecycle,
        mut provider: MockPlatformServiceProvider,
    ) {
        let mut iam = MockIamApi::new();
        iam.expect_get_role()
            .withf(|role| role == BUILD_ROLE)
            .returning(|role| {
                Ok(alien_aws_clients::iam::GetRoleResponse {
                    get_role_result: alien_aws_clients::iam::GetRoleResult {
                        role: alien_aws_clients::iam::Role {
                            path: "/".to_string(),
                            role_name: role.to_string(),
                            role_id: "AROAEXAMPLE".to_string(),
                            arn: format!("arn:aws:iam::123456789012:role/{role}"),
                            create_date: "2026-09-23T00:00:00Z".to_string(),
                            assume_role_policy_document: None,
                            description: None,
                            max_session_duration: None,
                            permissions_boundary: None,
                            tags: Some(alien_aws_clients::iam::Tags {
                                member: alien_core::setup_resource_tags(
                                    "test", "agents", "sandbox",
                                )
                                .into_iter()
                                .map(|(key, value)| alien_aws_clients::iam::Tag { key, value })
                                .collect(),
                            }),
                            role_last_used: None,
                        },
                    },
                })
            });
        iam.expect_get_role()
            .withf(|role| role == "test-agents-egress")
            .returning(|role| {
                Err(AlienError::new(
                    alien_aws_clients::ErrorData::RemoteResourceNotFound {
                        resource_type: "IAM Resource".to_string(),
                        resource_name: role.to_string(),
                    },
                ))
            });
        iam.expect_delete_role_policy()
            .withf(|role, _| role == BUILD_ROLE)
            .times(1)
            .returning(|_, _| Ok(()));
        iam.expect_delete_role()
            .withf(|role| role == BUILD_ROLE)
            .times(1)
            .returning(|_| Ok(()));
        let iam = Arc::new(iam);
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);
        let metadata = state.runtime_metadata.as_mut().unwrap();
        metadata.setup_scaffolding.clear();
        metadata.prepared_stack = Some(
            alien_core::Stack::new("acme".to_string())
                .add(
                    alien_core::Sandbox::new("agents".to_string())
                        .code(alien_core::SandboxCode::Image {
                            image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip"
                                .to_string(),
                        })
                        .egress(alien_core::SandboxEgress::Allow)
                        .lifecycle(alien_core::SandboxLifecyclePolicy {
                            max_lifetime_seconds: None,
                            idle_pause_seconds: None,
                        })
                        .build(),
                    lifecycle,
                )
                .build(),
        );

        run(&mut state, provider, &RecordingTransport::default())
            .await
            .unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert!(state.runtime_metadata.unwrap().setup_scaffolding.is_empty());
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
        no_other_tagged_group(&mut ec2);
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
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("default-network".to_string(), running_frozen_network());
        let record = SetupScaffolding::AwsSandbox {
            build_role_name: BUILD_ROLE.to_string(),
            egress: Some(alien_core::AwsSandboxEgressScaffolding {
                operator_role_name: "test-agents-egress".to_string(),
                security_group_id: Some("sg-held".to_string()),
                connector_arn: None,
                connector_request: None,
            }),
            image_arn: None,
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
            state.runtime_metadata.as_ref().unwrap().setup_scaffolding["agents"],
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
        for checkpoint in checkpoints.iter().chain([&state]) {
            let network = &checkpoint.stack_state.as_ref().unwrap().resources["default-network"];
            assert_eq!(
                network.status,
                alien_core::ResourceStatus::Running,
                "the network is not prepared for teardown while the group is held"
            );
        }
    }

    /// Both setup-teardown callers wait inline, so this budget is what ends a group AWS never
    /// releases, with the record kept for a later destroy.
    #[tokio::test(start_paused = true)]
    async fn a_group_that_is_never_released_fails_teardown_within_the_step_budget() {
        let mut cloudcontrol = alien_aws_clients::cloudcontrol::MockCloudControlApi::new();
        cloudcontrol.expect_list_resources().returning(|_, _| {
            Ok(alien_aws_clients::cloudcontrol::ListResourcesResponse {
                resource_descriptions: vec![],
                next_token: None,
            })
        });
        let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
        no_other_tagged_group(&mut ec2);
        ec2.expect_delete_security_group()
            .times(3)
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
                connector_request: None,
            }),
            image_arn: None,
        };
        state.runtime_metadata.as_mut().unwrap().setup_scaffolding =
            BTreeMap::from([("agents".to_string(), record.clone())]);
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

        let result = run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            "dep_test",
            &RunnerPolicy {
                max_steps: 3,
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Inline,
            },
            &RecordingTransport::default(),
            Some(Arc::new(provider)),
        )
        .await
        .expect("running out of steps is a failed run, not an error")
        .expect("teardown ran");

        assert_eq!(
            result.loop_result.stop_reason,
            LoopStopReason::BudgetExceeded
        );
        assert_eq!(result.loop_result.outcome, LoopOutcome::Failure);
        let error = state.error.clone().expect("the failure is recorded");
        assert!(
            error.message.contains("within 3 steps")
                && error.message.contains("security group 'sg-held'"),
            "the failure names what AWS has not released: {}",
            error.message
        );
        assert_eq!(state.status, DeploymentStatus::TeardownFailed);
        assert_eq!(
            state.runtime_metadata.unwrap().setup_scaffolding["agents"],
            record
        );
    }

    fn running_frozen_network() -> alien_core::StackResourceState {
        alien_core::StackResourceState::builder()
            .resource_type(alien_core::Network::RESOURCE_TYPE.to_string())
            .status(alien_core::ResourceStatus::Running)
            .config(alien_core::Resource::new(
                alien_core::Network::new("default-network".to_string())
                    .settings(alien_core::NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
            ))
            .lifecycle(ResourceLifecycle::Frozen)
            .build()
    }

    const SANDBOX_IMAGE: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";
    const CONNECTOR: &str = "arn:aws:lambda:us-east-1:123456789012:network-connector:nc-1";

    fn serving_live_sandbox() -> alien_core::StackResourceState {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
            })
            .egress(alien_core::SandboxEgress::Deny)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        alien_core::StackResourceState::builder()
            .resource_type(alien_core::Sandbox::RESOURCE_TYPE.to_string())
            .status(alien_core::ResourceStatus::Running)
            .config(alien_core::Resource::new(sandbox))
            .internal_state(serde_json::json!({
                "_controllerStateVersion": 1,
                "type": "AwsSandboxController",
                "state": "ready",
                "allowEgress": false,
                "egressConnectorArns": [CONNECTOR],
                "previewPorts": [],
                "imageArn": SANDBOX_IMAGE,
                "imageIdentifier": SANDBOX_IMAGE,
                "activeVersion": "1.0",
                "region": "us-east-1",
                "buildRoleArn": format!("arn:aws:iam::123456789012:role/{BUILD_ROLE}"),
                "internalStayCount": null
            }))
            .lifecycle(ResourceLifecycle::Live)
            .controller_platform(Platform::Aws)
            .build()
    }

    /// Every deleting call, from runtime cleanup and setup teardown alike, in the order made.
    fn logging_provider(log: &Arc<Mutex<Vec<String>>>) -> MockPlatformServiceProvider {
        let mut microvms = alien_aws_clients::lambda_microvms::MockLambdaMicrovmsApi::new();
        let l = log.clone();
        microvms
            .expect_delete_microvm_image()
            .returning(move |image| {
                l.lock()
                    .unwrap()
                    .push(format!("lambda:DeleteMicrovmImage {image}"));
                Ok(())
            });
        let mut cloudcontrol = alien_aws_clients::cloudcontrol::MockCloudControlApi::new();
        let connector_deleted = Arc::new(Mutex::new(false));
        let deleted = connector_deleted.clone();
        cloudcontrol
            .expect_get_resource()
            .returning(move |_, identifier| {
                if *deleted.lock().unwrap() {
                    return Err(alien_error::AlienError::new(
                        alien_aws_clients::ErrorData::RemoteResourceNotFound {
                            resource_type: "network connector".to_string(),
                            resource_name: identifier.to_string(),
                        },
                    ));
                }
                Ok(alien_aws_clients::cloudcontrol::ResourceDescription {
                    identifier: identifier.to_string(),
                    properties: Some(
                        serde_json::json!({ "Arn": identifier, "Name": "test-agents" }).to_string(),
                    ),
                })
            });
        cloudcontrol.expect_list_resources().returning(|_, _| {
            Ok(alien_aws_clients::cloudcontrol::ListResourcesResponse {
                resource_descriptions: vec![],
                next_token: None,
            })
        });
        cloudcontrol
            .expect_get_resource_request_status()
            .returning(|token| {
                Ok(alien_aws_clients::cloudcontrol::ProgressEvent {
                    type_name: None,
                    identifier: None,
                    request_token: token.to_string(),
                    operation: None,
                    operation_status: alien_aws_clients::cloudcontrol::OperationStatus::Success,
                    status_message: None,
                    error_code: None,
                })
            });
        let l = log.clone();
        let deleted = connector_deleted.clone();
        cloudcontrol
            .expect_delete_resource()
            .returning(move |_, identifier| {
                *deleted.lock().unwrap() = true;
                l.lock()
                    .unwrap()
                    .push(format!("cloudcontrol:DeleteResource {identifier}"));
                Ok(alien_aws_clients::cloudcontrol::ProgressEvent {
                    type_name: None,
                    identifier: Some(identifier.to_string()),
                    request_token: "delete".to_string(),
                    operation: None,
                    operation_status: alien_aws_clients::cloudcontrol::OperationStatus::Success,
                    status_message: None,
                    error_code: None,
                })
            });
        let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
        no_other_tagged_group(&mut ec2);
        let l = log.clone();
        ec2.expect_delete_security_group().returning(move |group| {
            l.lock()
                .unwrap()
                .push(format!("ec2:DeleteSecurityGroup {group}"));
            Ok(())
        });
        let mut iam = MockIamApi::new();
        let l = log.clone();
        iam.expect_delete_role_policy().returning(move |role, _| {
            l.lock()
                .unwrap()
                .push(format!("iam:DeleteRolePolicy {role}"));
            Ok(())
        });
        let l = log.clone();
        iam.expect_delete_role().returning(move |role| {
            l.lock().unwrap().push(format!("iam:DeleteRole {role}"));
            Ok(())
        });
        let (microvms, cloudcontrol, ec2, iam) = (
            Arc::new(microvms),
            Arc::new(cloudcontrol),
            Arc::new(ec2),
            Arc::new(iam),
        );
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(microvms.clone()));
        provider
            .expect_get_aws_cloudcontrol_client()
            .returning(move |_| Ok(cloudcontrol.clone()));
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
    }

    /// A Frozen sandbox's runtime cleanup leaves its image alone: the lifecycle check refuses it,
    /// whatever IAM a Live sibling grants. Setup deletes the image it built, before the role it was
    /// built with.
    #[tokio::test]
    async fn setup_teardown_deletes_the_image_a_frozen_sandbox_was_built_with() {
        let mut serving = serving_live_sandbox();
        serving.lifecycle = Some(ResourceLifecycle::Frozen);
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);
        state.status = DeploymentStatus::DeletePending;
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("agents".to_string(), serving);
        state.runtime_metadata.as_mut().unwrap().setup_scaffolding = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: None,
                image_arn: Some(SANDBOX_IMAGE.to_string()),
            },
        )]);
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
        let client_config = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));

        let runtime_calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut refused = alien_aws_clients::lambda_microvms::MockLambdaMicrovmsApi::new();
        let calls = runtime_calls.clone();
        refused
            .expect_delete_microvm_image()
            .returning(move |image| {
                calls.lock().unwrap().push(image.to_string());
                Err(AlienError::new(
                    alien_aws_clients::ErrorData::RemoteAccessDenied {
                        resource_type: "MicroVM image".to_string(),
                        resource_name: image.to_string(),
                    },
                ))
            });
        let refused = Arc::new(refused);
        let mut management = MockPlatformServiceProvider::new();
        management
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(refused.clone()));
        let management: Arc<dyn alien_infra::PlatformServiceProvider> = Arc::new(management);

        state = crate::deleting::handle_delete_pending(
            state,
            config.clone(),
            client_config.clone(),
            management.clone(),
        )
        .await
        .unwrap()
        .state;
        for _ in 0..10 {
            if state.status != DeploymentStatus::Deleting {
                break;
            }
            state = crate::deleting::handle_deleting(
                state,
                config.clone(),
                client_config.clone(),
                management.clone(),
            )
            .await
            .unwrap()
            .state;
        }
        assert_eq!(state.status, DeploymentStatus::TeardownRequired);
        assert!(
            runtime_calls.lock().unwrap().is_empty(),
            "runtime cleanup leaves the image to setup: {:?}",
            runtime_calls.lock().unwrap()
        );

        let log = Arc::new(Mutex::new(Vec::new()));
        run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &client_config,
            "dep_test",
            &RunnerPolicy {
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Inline,
                ..Default::default()
            },
            &RecordingTransport::default(),
            Some(Arc::new(logging_provider(&log))),
        )
        .await
        .unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert!(state.runtime_metadata.unwrap().setup_scaffolding.is_empty());
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                format!("lambda:DeleteMicrovmImage {SANDBOX_IMAGE}"),
                format!("iam:DeleteRolePolicy {BUILD_ROLE}"),
                format!("iam:DeleteRole {BUILD_ROLE}"),
            ]
        );
    }

    /// A deny sandbox's sessions place interfaces in the connector's group and its image builds
    /// assume the build role, so the sandbox goes first and the scaffolding only after it.
    #[tokio::test]
    async fn the_sandbox_is_deleted_before_setup_deletes_its_scaffolding() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let provider: Arc<dyn alien_infra::PlatformServiceProvider> =
            Arc::new(logging_provider(&log));
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);
        state.status = DeploymentStatus::DeletePending;
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("agents".to_string(), serving_live_sandbox());
        state.runtime_metadata.as_mut().unwrap().setup_scaffolding = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: Some(alien_core::AwsSandboxEgressScaffolding {
                    operator_role_name: "test-agents-egress".to_string(),
                    security_group_id: Some("sg-deny".to_string()),
                    connector_arn: Some(CONNECTOR.to_string()),
                    connector_request: None,
                }),
                image_arn: None,
            },
        )]);
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
        let client_config = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));

        state = crate::deleting::handle_delete_pending(
            state,
            config.clone(),
            client_config.clone(),
            provider.clone(),
        )
        .await
        .unwrap()
        .state;
        for _ in 0..10 {
            if state.status != DeploymentStatus::Deleting {
                break;
            }
            state = crate::deleting::handle_deleting(
                state,
                config.clone(),
                client_config.clone(),
                provider.clone(),
            )
            .await
            .unwrap()
            .state;
        }
        assert_eq!(state.status, DeploymentStatus::TeardownRequired);
        assert_eq!(
            state.stack_state.as_ref().unwrap().resources["agents"].status,
            alien_core::ResourceStatus::Deleted
        );

        let transport = RecordingTransport::default();
        run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &client_config,
            "dep_test",
            &RunnerPolicy {
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Inline,
                ..Default::default()
            },
            &transport,
            Some(provider),
        )
        .await
        .unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert!(state.runtime_metadata.unwrap().setup_scaffolding.is_empty());
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                format!("lambda:DeleteMicrovmImage {SANDBOX_IMAGE}"),
                format!("cloudcontrol:DeleteResource {CONNECTOR}"),
                "ec2:DeleteSecurityGroup sg-deny".to_string(),
                "iam:DeleteRolePolicy test-agents-egress".to_string(),
                "iam:DeleteRole test-agents-egress".to_string(),
                format!("iam:DeleteRolePolicy {BUILD_ROLE}"),
                format!("iam:DeleteRole {BUILD_ROLE}"),
            ]
        );
    }

    /// A direct setup that synced secrets and failed before the management identity existed: the
    /// vault is setup's, the record holds the inventory, and no scaffolding needs AWS.
    fn synced_vault_teardown(
        data_dir: &std::path::Path,
        synced: &[&str],
    ) -> (DeploymentState, Arc<dyn alien_bindings::traits::Vault>) {
        let mut vault = alien_core::StackResourceState::builder()
            .resource_type(alien_core::Vault::RESOURCE_TYPE.to_string())
            .status(alien_core::ResourceStatus::Running)
            .config(alien_core::Resource::new(
                alien_core::Vault::new("secrets".to_string()).build(),
            ))
            .internal_state(serde_json::json!({
                "_controllerStateVersion": 1,
                "type": "AwsVaultController",
                "state": "ready",
                "accountId": "123456789012",
                "region": "us-east-1",
                "vaultPrefix": "test-secrets"
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .controller_platform(Platform::Aws)
            .build();
        vault.remote_binding_params = Some(
            serde_json::to_value(alien_core::bindings::VaultBinding::local(
                "secrets",
                data_dir.to_string_lossy(),
            ))
            .unwrap(),
        );
        let mut state = teardown_required(InitialSetupAuthority::DirectSetup);
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("secrets".to_string(), vault);
        let metadata = state.runtime_metadata.as_mut().unwrap();
        metadata.setup_scaffolding.clear();
        metadata.prepared_stack = Some(alien_core::Stack::new("test".to_string()).build());
        metadata.last_synced_env_vars_hash = Some("synced".to_string());
        metadata.last_synced_secret_names = synced.iter().map(|name| name.to_string()).collect();
        let values = alien_bindings::providers::vault::local::LocalVault::new(
            "secrets".to_string(),
            data_dir.to_path_buf(),
        );
        (state, Arc::new(values))
    }

    async fn seed(vault: &Arc<dyn alien_bindings::traits::Vault>, names: &[&str]) {
        for name in names {
            vault.set_secret(name, "value").await.unwrap();
        }
    }

    async fn held(vault: &Arc<dyn alien_bindings::traits::Vault>, names: &[&str]) -> Vec<String> {
        let listed = vault.list_secrets().await.unwrap();
        names
            .iter()
            .filter(|name| listed.iter().any(|listed| listed == *name))
            .map(|name| name.to_string())
            .collect()
    }

    const TOKEN: &str = alien_core::ENV_ALIEN_COMMANDS_TOKEN;

    #[tokio::test]
    async fn setup_teardown_deletes_the_synced_secrets_before_the_vault() {
        let dir = tempfile::TempDir::new().unwrap();
        let (mut state, vault) = synced_vault_teardown(dir.path(), &["API_KEY", "DB_URL"]);
        seed(&vault, &["API_KEY", "DB_URL", TOKEN, "UNRELATED"]).await;
        let transport = RecordingTransport::default();

        let result = run(&mut state, MockPlatformServiceProvider::new(), &transport).await;

        {
            let checkpoints = transport.checkpoints.lock().unwrap();
            let vault_gone = checkpoints
                .iter()
                .find(|checkpoint| {
                    checkpoint.stack_state.as_ref().unwrap().resources["secrets"].status
                        == alien_core::ResourceStatus::Deleted
                })
                .expect("a checkpoint records the deleted vault");
            let inventory = vault_gone.runtime_metadata.as_ref().unwrap();
            assert!(
                inventory.last_synced_env_vars_hash.is_none()
                    && inventory.last_synced_secret_names.is_empty(),
                "the secrets go before the vault"
            );
        }
        result.unwrap();
        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert_eq!(
            held(&vault, &["API_KEY", "DB_URL", TOKEN, "UNRELATED"]).await,
            vec!["UNRELATED"]
        );
    }

    #[tokio::test]
    async fn a_synced_hash_without_a_secrets_vault_deletes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let (mut state, _) = synced_vault_teardown(dir.path(), &[]);
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .remove("secrets");
        state.runtime_metadata.as_mut().unwrap().prepared_stack = None;

        run(
            &mut state,
            MockPlatformServiceProvider::new(),
            &RecordingTransport::default(),
        )
        .await
        .unwrap();

        assert_eq!(state.status, DeploymentStatus::Deleted);
    }

    #[tokio::test]
    async fn a_recorded_inventory_without_its_prepared_stack_fails_teardown() {
        let dir = tempfile::TempDir::new().unwrap();
        let (mut state, vault) = synced_vault_teardown(dir.path(), &["API_KEY"]);
        state.runtime_metadata.as_mut().unwrap().prepared_stack = None;
        seed(&vault, &["API_KEY"]).await;

        let error = run(
            &mut state,
            MockPlatformServiceProvider::new(),
            &RecordingTransport::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "MISSING_CONFIGURATION");
        assert_eq!(state.status, DeploymentStatus::TeardownFailed);
        assert_eq!(
            state.stack_state.as_ref().unwrap().resources["secrets"].status,
            alien_core::ResourceStatus::Running
        );
        assert_eq!(held(&vault, &["API_KEY"]).await, vec!["API_KEY"]);
    }
}
