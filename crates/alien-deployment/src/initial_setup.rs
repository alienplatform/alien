use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    InitialSetupAuthority, ResourceLifecycle, ResourceStatus, Stack, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::setup_scaffolding::{self, ScaffoldingProgress, SetupScaffoldingContext};
use alien_infra::{StackExecutor, StackStateExt};
use tracing::{debug, info};

/// Handle InitialSetup status (deploy setup-owned Frozen resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Under direct setup, advances the setup scaffolding of runtime-owned resources
/// 3. Executes one deployment step for Frozen resources
/// 4. Updates stack state with the result
/// 5. Transitions to Provisioning when Frozen resources are deployed and scaffolding is done
///
/// Note: Stack settings are set during Pending phase and should not change mid-deployment.
pub async fn handle_initial_setup(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling InitialSetup status");

    // Clone current first before moving any fields
    let current_cloned = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for initial setup".to_string(),
        })
    })?;

    // Get runtime metadata (must exist from Pending phase). Mutable because
    // sync_secrets_to_vault updates the last-synced hash.
    let mut runtime_metadata = current.runtime_metadata.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with prepared stack required for initial setup".to_string(),
        })
    })?;

    // Use the prepared stack from Pending phase (already mutated)
    let mut target_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Prepared stack not found in runtime metadata".to_string(),
        })
    })?;

    // Inject all environment variables — plain AND secrets.
    //
    // Worker wrappers that consume vault pointers receive the secrets vault as
    // a dependency from SecretsVaultMutation. Native-projected workloads do
    // not need workload vault access. Secret values are synced below, between
    // the step where the vault becomes Running and the step where Workers can
    // consume it.
    crate::helpers::inject_environment_variables(&mut target_stack, &config, current.platform)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(
            &mut target_stack,
            monitoring,
            current.platform,
        )?;
    }

    // Sync secrets to vault if the vault is already Running (from a previous
    // step). The executor checks dependencies against the pre-step state, so a
    // vault that became Running in step N won't unblock dependents until step
    // N+1 — giving us this window to sync secrets before any function starts.
    let vault_is_running = stack_state
        .resources
        .get("secrets")
        .map(|r| r.status == ResourceStatus::Running)
        .unwrap_or(false);

    if vault_is_running {
        let synced = crate::helpers::sync_secrets_to_vault(
            &target_stack,
            &stack_state,
            &client_config,
            &config,
            &mut runtime_metadata,
        )
        .await?;

        if synced {
            info!("Secrets synced to vault during InitialSetup");
        }
    }

    // Deploy setup-owned resources during initial setup. Live resources are
    // created later in Provisioning using the permissions granted by setup.
    info!("Deploying frozen resources in initial setup");
    let client_config_for_scaffolding = client_config.clone();
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .service_provider(service_provider.clone())
        .initial_setup_authority(runtime_metadata.initial_setup_authority)
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .step_running_resources(false)
        // Resumed setup may contain unfinished Live work; only runtime may advance it.
        .step_out_of_scope_resources(false)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for initial setup".to_string(),
        })?;

    let scaffolding = match runtime_metadata.initial_setup_authority {
        InitialSetupAuthority::DirectSetup => {
            reconcile_setup_scaffolding(
                &target_stack,
                &stack_state,
                &client_config_for_scaffolding,
                service_provider.as_ref(),
                &mut runtime_metadata,
            )
            .await?
        }
        // An imported setup created the scaffolding itself, from the template.
        InitialSetupAuthority::ImportedHandoff => ScaffoldingProgress::Done,
    };

    let step_result = match runtime_metadata.initial_setup_authority {
        InitialSetupAuthority::DirectSetup => executor.step(stack_state).await,
        InitialSetupAuthority::ImportedHandoff => executor.continue_imported(stack_state).await,
    }
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to execute deployment step".to_string(),
    })?;

    // Compute status only for Frozen resources. A stack with no Frozen
    // resources can hand off immediately to Provisioning.
    let stack_status = compute_lifecycle_status(
        &target_stack,
        &step_result.next_state,
        ResourceLifecycle::Frozen,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to compute initial setup status".to_string(),
    })?;

    let result = if stack_status == StackStatus::Running && scaffolding == ScaffoldingProgress::Done
    {
        info!("Initial setup complete (frozen resources deployed), transitioning to Provisioning");

        // Debug: log all resources in stack state to diagnose external binding persistence
        for (res_id, res_state) in &step_result.next_state.resources {
            debug!(
                resource_id = %res_id,
                status = ?res_state.status,
                has_outputs = res_state.outputs.is_some(),
                lifecycle = ?res_state.lifecycle,
                "InitialSetup complete: resource in stack state"
            );
        }

        // Note: Cross-account access setup happens in the manager after this step
        // The manager has access to the artifact registry binding

        let mut next = current_cloned;
        next.status = DeploymentStatus::Provisioning;
        next.stack_state = Some(step_result.next_state);
        next.error = None;
        next.runtime_metadata = Some(runtime_metadata);

        // Add a short delay before starting Provisioning to allow AWS IAM inline
        // policies (applied during InitialSetup via ApplyingResourcePermissions) to
        // propagate. IAM eventual consistency can take up to ~60s, but typically
        // settles within 10s. Without this delay, Provisioning may start immediately
        // and hit AccessDenied on the management role for newly-attached policies.
        DeploymentStepResult {
            state: next,
            suggested_delay_ms: Some(10_000),
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        }
    } else if stack_status == StackStatus::Failure {
        info!("Initial setup failed");

        let mut next_state = step_result.next_state;

        let failed_resources: Vec<(String, String)> = next_state
            .resources
            .values()
            .filter(|r| {
                r.lifecycle == Some(ResourceLifecycle::Frozen)
                    && matches!(
                        r.status,
                        alien_core::ResourceStatus::ProvisionFailed
                            | alien_core::ResourceStatus::UpdateFailed
                            | alien_core::ResourceStatus::DeleteFailed
                            | alien_core::ResourceStatus::RefreshFailed
                    )
            })
            .map(|r| (r.config.id().to_string(), r.resource_type.clone()))
            .collect();

        let failed_refs: Vec<(&str, &str)> = failed_resources
            .iter()
            .map(|(id, t)| (id.as_str(), t.as_str()))
            .collect();

        crate::helpers::interrupt_in_progress_resources(
            &mut next_state,
            &failed_refs,
            Some(ResourceLifecycle::Frozen),
        );

        let mut next = current_cloned;
        next.status = DeploymentStatus::InitialSetupFailed;
        next.stack_state = Some(next_state);
        next.error = None;
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        }
    } else {
        // Frozen resources may all be Running while scaffolding is not, and a step with no
        // delay of its own would then re-enter setup at once, ahead of IAM's read-after-write.
        let suggested_delay_ms = match scaffolding {
            ScaffoldingProgress::InProgress => step_result
                .suggested_delay_ms
                .or(Some(SCAFFOLDING_POLL_DELAY_MS)),
            ScaffoldingProgress::Done => step_result.suggested_delay_ms,
        };
        // Still in progress — log which Frozen resources are not yet running.
        let non_running =
            non_running_resources_for_lifecycle(&target_stack, &step_result.next_state);
        info!(
            "Initial setup in progress. Non-running resources: [{}]",
            non_running.join(", ")
        );

        let mut next = current_cloned;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            suggested_delay_ms,
            update_heartbeat: false,
            heartbeats: step_result.heartbeats,
            observed_inventory_batches: vec![],
        }
    };

    Ok(result)
}

const SCAFFOLDING_POLL_DELAY_MS: u64 = 5_000;

async fn reconcile_setup_scaffolding(
    target_stack: &Stack,
    stack_state: &StackState,
    client_config: &alien_core::ClientConfig,
    service_provider: &dyn alien_infra::PlatformServiceProvider,
    runtime_metadata: &mut alien_core::RuntimeMetadata,
) -> Result<ScaffoldingProgress> {
    let ctx = SetupScaffoldingContext {
        client_config,
        service_provider,
        resource_prefix: &stack_state.resource_prefix,
    };
    setup_scaffolding::reconcile(
        &ctx,
        target_stack,
        stack_state.platform,
        &mut runtime_metadata.setup_scaffolding,
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create setup scaffolding".to_string(),
    })
}

fn compute_lifecycle_status(
    stack: &Stack,
    stack_state: &StackState,
    lifecycle: ResourceLifecycle,
) -> alien_core::Result<StackStatus> {
    let statuses: Vec<ResourceStatus> = stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == lifecycle)
        .map(|(resource_id, _)| {
            stack_state
                .resources
                .get(resource_id)
                .map(|resource| resource.status)
                .unwrap_or(ResourceStatus::Pending)
        })
        .collect();

    if statuses.is_empty() {
        return Ok(StackStatus::Running);
    }

    StackState::compute_stack_status_from_resources(&statuses)
}

fn non_running_resources_for_lifecycle(stack: &Stack, stack_state: &StackState) -> Vec<String> {
    stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
        .filter_map(|(resource_id, _)| {
            let status = stack_state
                .resources
                .get(resource_id)
                .map(|resource| resource.status)
                .unwrap_or(ResourceStatus::Pending);

            (status != ResourceStatus::Running).then(|| format!("{resource_id}({status:?})"))
        })
        .collect()
}

/// Handle InitialSetupFailed status - retry failed resources and transition back to InitialSetup
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Retries failed Frozen resources without changing Live runtime state
/// 3. Transitions back to InitialSetup status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_initial_setup_failed(
    current: DeploymentState,
    _target_stack: Stack,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling InitialSetupFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in InitialSetupFailed status");
        return Ok(DeploymentStepResult {
            state: current,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    info!("Retrying failed resources");

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for retry".to_string(),
        })
    })?;

    // Retry failed resources using alien-infra
    let retried = stack_state
        .retry_failed_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to retry failed resources".to_string(),
        })?;

    info!("Retried {} failed resources: {:?}", retried.len(), retried);

    // Transition back to InitialSetup to continue deployment
    next.status = DeploymentStatus::InitialSetup;
    next.stack_state = Some(stack_state);
    next.error = None;
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::iam::MockIamApi;
    use alien_core::{
        ClientConfig, EnvironmentVariablesSnapshot, Platform, RuntimeMetadata, SetupScaffolding,
        StackSettings, Storage,
    };
    use alien_infra::{
        DefaultPlatformServiceProvider, MockPlatformServiceProvider, StackResourceStateExt,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    async fn setup_with_unfinished_live(status: ResourceStatus) -> DeploymentState {
        let live = Storage::new("live".to_string()).build();
        let stack = Stack::new("test".to_string())
            .add(live.clone(), ResourceLifecycle::Live)
            .build();
        let config = config();
        let executor = StackExecutor::builder(&stack, ClientConfig::Test)
            .deployment_config(&config)
            .build()
            .unwrap();
        let mut state = executor
            .step(StackState::new(Platform::Test))
            .await
            .unwrap()
            .next_state;
        assert_eq!(state.resources["live"].status, ResourceStatus::Provisioning);
        if status != ResourceStatus::Provisioning {
            for _ in 0..4 {
                state = executor.step(state).await.unwrap().next_state;
            }
            assert_eq!(state.resources["live"].status, ResourceStatus::Running);
            let resource = state.resources.get_mut("live").unwrap();
            let mut controller = resource.get_internal_controller().unwrap().unwrap();
            match status {
                ResourceStatus::Updating => controller.transition_to_update().unwrap(),
                ResourceStatus::Deleting => controller.transition_to_delete_start().unwrap(),
                _ => panic!("unsupported fixture status"),
            }
            resource.status = controller.get_status();
            resource.set_internal_controller(Some(controller)).unwrap();
        }
        assert_eq!(state.resources["live"].status, status);
        let mut target = Stack::new("test".to_string())
            .add(
                Storage::new("frozen".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        if status != ResourceStatus::Deleting {
            target
                .resources
                .insert("live".to_string(), stack.resources["live"].clone());
        }
        DeploymentState {
            status: DeploymentStatus::InitialSetup,
            platform: Platform::Test,
            current_release: None,
            target_release: None,
            stack_state: Some(state),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(target),
                initial_setup_authority: InitialSetupAuthority::DirectSetup,
                ..Default::default()
            }),
        }
    }

    const BUILD_ROLE: &str = "test-agents-build";

    fn live_sandbox_setup(authority: InitialSetupAuthority) -> DeploymentState {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
            })
            .egress(alien_core::SandboxEgress::Allow)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let stack = Stack::new("test".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build();
        DeploymentState {
            status: DeploymentStatus::InitialSetup,
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
                prepared_stack: Some(stack),
                initial_setup_authority: authority,
                ..Default::default()
            }),
        }
    }

    fn aws_client_config() -> ClientConfig {
        use alien_aws_clients::AwsClientConfigExt as _;
        ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock()))
    }

    fn with_iam(iam: MockIamApi) -> Arc<MockPlatformServiceProvider> {
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        Arc::new(provider)
    }

    fn created_role() -> alien_aws_clients::iam::Role {
        let trust = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "Service": "lambda.amazonaws.com" },
                "Action": "sts:AssumeRole",
                "Condition": { "StringEquals": { "aws:SourceAccount": "123456789012" } }
            }]
        });
        alien_aws_clients::iam::Role {
            path: "/".to_string(),
            role_name: BUILD_ROLE.to_string(),
            role_id: "AROAEXAMPLE".to_string(),
            arn: format!("arn:aws:iam::123456789012:role/{BUILD_ROLE}"),
            create_date: "2026-09-23T00:00:00Z".to_string(),
            assume_role_policy_document: Some(trust.to_string()),
            description: None,
            max_session_duration: None,
            permissions_boundary: None,
            tags: None,
            role_last_used: None,
        }
    }

    /// A Live sandbox is the only resource, so Frozen setup is finished at once; the build
    /// role is what holds the handoff until it exists and carries its policy.
    #[tokio::test]
    async fn direct_setup_hands_off_only_once_the_build_role_is_ready() {
        use alien_aws_clients::iam::{
            CreateRoleResponse, CreateRoleResult, GetRoleResponse, GetRoleResult,
            ListAttachedRolePoliciesResponse, ListAttachedRolePoliciesResult,
            ListRolePoliciesResponse, ListRolePoliciesResult,
        };

        let mut absent = MockIamApi::new();
        absent.expect_get_role().times(1).returning(|name| {
            Err(alien_error::AlienError::new(
                alien_aws_clients::ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Resource".to_string(),
                    resource_name: name.to_string(),
                },
            ))
        });
        absent
            .expect_create_role()
            .withf(|request| request.role_name == BUILD_ROLE)
            .times(1)
            .returning(|_| {
                Ok(CreateRoleResponse {
                    create_role_result: CreateRoleResult {
                        role: created_role(),
                    },
                })
            });
        let first = handle_initial_setup(
            live_sandbox_setup(InitialSetupAuthority::DirectSetup),
            config(),
            aws_client_config(),
            with_iam(absent),
        )
        .await
        .unwrap();
        assert_eq!(first.state.status, DeploymentStatus::InitialSetup);
        assert_eq!(first.suggested_delay_ms, Some(SCAFFOLDING_POLL_DELAY_MS));
        let recorded = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
            },
        )]);
        assert_eq!(
            first
                .state
                .runtime_metadata
                .as_ref()
                .unwrap()
                .setup_scaffolding,
            recorded
        );

        let mut present = MockIamApi::new();
        present.expect_get_role().returning(|_| {
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: created_role(),
                },
            })
        });
        present.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        present.expect_list_attached_role_policies().returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        present
            .expect_put_role_policy()
            .withf(|role, policy, _| role == BUILD_ROLE && policy == "sandbox-image-build")
            .times(1)
            .returning(|_, _, _| Ok(()));
        let second = handle_initial_setup(
            first.state,
            config(),
            aws_client_config(),
            with_iam(present),
        )
        .await
        .unwrap();
        assert_eq!(second.state.status, DeploymentStatus::Provisioning);
        assert_eq!(
            second.state.runtime_metadata.unwrap().setup_scaffolding,
            recorded
        );
    }

    /// An imported setup's template already made the role; a provider with no expectations
    /// panics on any cloud call.
    #[tokio::test]
    async fn imported_setup_creates_no_scaffolding() {
        let result = handle_initial_setup(
            live_sandbox_setup(InitialSetupAuthority::ImportedHandoff),
            config(),
            aws_client_config(),
            Arc::new(MockPlatformServiceProvider::new()),
        )
        .await
        .unwrap();
        assert_eq!(result.state.status, DeploymentStatus::Provisioning);
        assert!(result
            .state
            .runtime_metadata
            .unwrap()
            .setup_scaffolding
            .is_empty());
    }

    #[tokio::test]
    async fn setup_preserves_unfinished_live_work_until_runtime_handoff() {
        for status in [
            ResourceStatus::Provisioning,
            ResourceStatus::Updating,
            ResourceStatus::Deleting,
        ] {
            let mut state = setup_with_unfinished_live(status).await;
            let before =
                serde_json::to_value(&state.stack_state.as_ref().unwrap().resources["live"])
                    .unwrap();
            for _ in 0..8 {
                if state.status == DeploymentStatus::Provisioning {
                    break;
                }
                state = handle_initial_setup(
                    state,
                    config(),
                    ClientConfig::Test,
                    Arc::new(DefaultPlatformServiceProvider::default()),
                )
                .await
                .unwrap()
                .state;
                assert_eq!(
                    serde_json::to_value(&state.stack_state.as_ref().unwrap().resources["live"])
                        .unwrap(),
                    before,
                    "setup advanced {status:?} Live work"
                );
            }
            assert_eq!(state.status, DeploymentStatus::Provisioning);
            assert_eq!(
                state.stack_state.as_ref().unwrap().resources["frozen"].status,
                ResourceStatus::Running
            );
            let after = crate::provisioning::handle_provisioning(
                state,
                config(),
                ClientConfig::Test,
                Arc::new(DefaultPlatformServiceProvider::default()),
            )
            .await
            .unwrap()
            .state;
            assert_ne!(
                serde_json::to_value(&after.stack_state.as_ref().unwrap().resources["live"])
                    .unwrap(),
                before,
                "runtime did not resume {status:?} Live work"
            );
        }
    }

    #[tokio::test]
    async fn setup_failure_and_retry_preserve_live_resource_state() {
        let mut state = setup_with_unfinished_live(ResourceStatus::Provisioning).await;
        let target = state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .clone()
            .unwrap();
        let mut frozen = alien_core::StackResourceState::new_pending(
            "storage".to_string(),
            target.resources["frozen"].config.clone(),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        frozen.status = ResourceStatus::ProvisionFailed;
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("frozen".to_string(), frozen);
        let before =
            serde_json::to_value(&state.stack_state.as_ref().unwrap().resources["live"]).unwrap();
        let mut failed = handle_initial_setup(
            state,
            config(),
            ClientConfig::Test,
            Arc::new(DefaultPlatformServiceProvider::default()),
        )
        .await
        .unwrap()
        .state;
        assert_eq!(failed.status, DeploymentStatus::InitialSetupFailed);
        assert_eq!(
            serde_json::to_value(&failed.stack_state.as_ref().unwrap().resources["live"]).unwrap(),
            before
        );

        // An unrelated runtime failure must retain its controller checkpoint and error through setup retry.
        let live = failed
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .get_mut("live")
            .unwrap();
        live.last_failed_state = live.internal_state.clone();
        live.status = ResourceStatus::ProvisionFailed;
        let before_retry = serde_json::to_value(&*live).unwrap();
        failed.retry_requested = true;
        let retried = handle_initial_setup_failed(
            failed,
            target,
            config(),
            ClientConfig::Test,
            Arc::new(DefaultPlatformServiceProvider::default()),
        )
        .await
        .unwrap()
        .state;
        assert_eq!(retried.status, DeploymentStatus::InitialSetup);
        assert_eq!(
            retried.stack_state.as_ref().unwrap().resources["frozen"].status,
            ResourceStatus::Pending
        );
        assert_eq!(
            serde_json::to_value(&retried.stack_state.as_ref().unwrap().resources["live"]).unwrap(),
            before_retry
        );
    }
}
