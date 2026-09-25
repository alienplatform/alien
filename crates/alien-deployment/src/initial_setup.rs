use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    InitialSetupAuthority, ResourceLifecycle, ResourceStatus, Stack, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::setup_scaffolding::{
    self, ScaffoldingProgress, SeedContext, SetupScaffoldingContext,
};
use alien_infra::{ImporterRegistry, StackExecutor, StackStateExt};
use tracing::{debug, info};

/// Handle InitialSetup status (deploy setup-owned Frozen resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Under direct setup, advances the setup scaffolding of runtime-owned resources
/// 3. Executes one deployment step for Frozen resources, then seeds the scaffolded resources'
///    controller state once their scaffolding is done
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
        match crate::helpers::sync_secrets_to_vault(
            &target_stack,
            &stack_state,
            &client_config,
            &config,
            &mut runtime_metadata,
        )
        .await
        {
            Ok(true) => info!("Secrets synced to vault during InitialSetup"),
            Ok(false) => {}
            Err(error) => {
                return Ok(failed_keeping_record(
                    current_cloned,
                    stack_state,
                    runtime_metadata,
                    error,
                ))
            }
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
            match reconcile_setup_scaffolding(
                &target_stack,
                &stack_state,
                &client_config_for_scaffolding,
                service_provider.as_ref(),
                &mut runtime_metadata,
            )
            .await
            {
                Ok(progress) => progress,
                Err(error) => {
                    return Ok(failed_keeping_record(
                        current_cloned,
                        stack_state,
                        runtime_metadata,
                        error,
                    ))
                }
            }
        }
        // An imported setup created the scaffolding itself, from the template.
        InitialSetupAuthority::ImportedHandoff => ScaffoldingProgress::Done,
    };

    let pre_step_state = stack_state.clone();
    let mut step_result = match match runtime_metadata.initial_setup_authority {
        InitialSetupAuthority::DirectSetup => executor.step(stack_state).await,
        InitialSetupAuthority::ImportedHandoff => executor.continue_imported(stack_state).await,
    }
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to execute deployment step".to_string(),
    }) {
        Ok(step_result) => step_result,
        Err(error) => {
            return Ok(failed_keeping_record(
                current_cloned,
                pre_step_state,
                runtime_metadata,
                error,
            ))
        }
    };

    // The handoff below requires Done, so no runtime controller starts from an unseeded state.
    if runtime_metadata.initial_setup_authority == InitialSetupAuthority::DirectSetup
        && scaffolding == ScaffoldingProgress::Done
    {
        if let Err(error) = seed_setup_scaffolding(
            &target_stack,
            &mut step_result.next_state,
            &config,
            &client_config_for_scaffolding,
            service_provider.as_ref(),
            &runtime_metadata,
        ) {
            return Ok(failed_keeping_record(
                current_cloned,
                step_result.next_state,
                runtime_metadata,
                error,
            ));
        }
    }

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

/// Setup scaffolding and secret sync record what they create in `runtime_metadata` as they go, and
/// the runner's failure path would persist the state from before the step, dropping those records.
/// So once either has run, a step fails itself with the record it holds.
fn failed_keeping_record(
    current: DeploymentState,
    stack_state: StackState,
    runtime_metadata: alien_core::RuntimeMetadata,
    error: AlienError<ErrorData>,
) -> DeploymentStepResult {
    let mut next = current;
    next.status = DeploymentStatus::InitialSetupFailed;
    next.stack_state = Some(stack_state);
    next.error = Some(error.into_generic());
    next.retry_requested = false;
    next.runtime_metadata = Some(runtime_metadata);
    DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    }
}

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
        stack_state,
        &mut runtime_metadata.setup_scaffolding,
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create setup scaffolding".to_string(),
    })
}

/// Hands each scaffolded resource the facts a template setup would have registered for it.
fn seed_setup_scaffolding(
    target_stack: &Stack,
    stack_state: &mut StackState,
    config: &DeploymentConfig,
    client_config: &alien_core::ClientConfig,
    service_provider: &dyn alien_infra::PlatformServiceProvider,
    runtime_metadata: &alien_core::RuntimeMetadata,
) -> Result<()> {
    let seeds = setup_scaffolding::seeds(
        &SetupScaffoldingContext {
            client_config,
            service_provider,
            resource_prefix: &stack_state.resource_prefix,
        },
        target_stack,
        stack_state,
        &runtime_metadata.setup_scaffolding,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to resolve setup scaffolding seeds".to_string(),
    })?;
    let registry = ImporterRegistry::built_in();
    setup_scaffolding::apply_seeds(
        &SeedContext {
            registry: &registry,
            stack_settings: &config.stack_settings,
            management_config: config.management_config.as_ref(),
        },
        target_stack,
        stack_state,
        seeds,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to seed resources from setup scaffolding".to_string(),
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
    use alien_aws_clients::iam::{
        CreateRoleResponse, CreateRoleResult, GetRoleResponse, GetRoleResult,
        ListAttachedRolePoliciesResponse, ListAttachedRolePoliciesResult, ListRolePoliciesResponse,
        ListRolePoliciesResult, MockIamApi, PolicyNames,
    };
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageResponse, MicrovmImage, MicrovmImageVersion, MockLambdaMicrovmsApi,
    };
    use alien_aws_clients::AwsClientConfigExt as _;
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
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
    const BUNDLE_URI: &str = "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip";

    fn live_sandbox_setup(authority: InitialSetupAuthority) -> DeploymentState {
        sandbox_setup(ResourceLifecycle::Live, authority)
    }

    fn sandbox_setup(
        lifecycle: ResourceLifecycle,
        authority: InitialSetupAuthority,
    ) -> DeploymentState {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: BUNDLE_URI.to_string(),
            })
            .egress(alien_core::SandboxEgress::Allow)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .preview_ports(vec![8080])
            .build();
        let stack = Stack::new("test".to_string())
            .add(sandbox, lifecycle)
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
            tags: Some(alien_aws_clients::iam::Tags {
                member: alien_core::setup_resource_tags("test", "agents", "sandbox")
                    .into_iter()
                    .map(|(key, value)| alien_aws_clients::iam::Tag { key, value })
                    .collect(),
            }),
            role_last_used: None,
        }
    }

    /// The build role as this step created it, before or after its policy landed.
    fn present_role() -> MockIamApi {
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
        present.expect_get_role_policy().returning(|_, policy| {
            Err(alien_error::AlienError::new(
                alien_aws_clients::ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Resource".to_string(),
                    resource_name: policy.to_string(),
                },
            ))
        });
        present
            .expect_put_role_policy()
            .withf(|role, policy, _| role == BUILD_ROLE && policy == "sandbox-image-build")
            .times(1)
            .returning(|_, _, _| Ok(()));
        present
    }

    /// The build role as a finished setup left it: its policy already reads back as setup's.
    fn ready_role() -> MockIamApi {
        let policy = serde_json::to_value(
            alien_core::sandbox_build_role::SandboxBuildRole::builder()
                .sandbox_id("agents")
                .partition("aws")
                .account_id("123456789012")
                .region("us-east-1")
                .bundle_uri(BUNDLE_URI)
                .runtime_built(true)
                .build()
                .policy()
                .unwrap(),
        )
        .unwrap();
        let mut ready = MockIamApi::new();
        ready.expect_get_role().returning(|_| {
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: created_role(),
                },
            })
        });
        ready.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec!["sandbox-image-build".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        ready.expect_list_attached_role_policies().returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        ready.expect_get_role_policy().returning(move |role, name| {
            Ok(alien_aws_clients::iam::GetRolePolicyResponse {
                get_role_policy_result: alien_aws_clients::iam::GetRolePolicyResult {
                    role_name: role.to_string(),
                    policy_name: name.to_string(),
                    policy_document: policy.to_string(),
                },
            })
        });
        ready.expect_put_role_policy().never();
        ready
    }

    /// A Live sandbox is the only resource, so Frozen setup is finished at once; the build
    /// role is what holds the handoff until it exists and carries its policy.
    #[tokio::test]
    async fn direct_setup_hands_off_only_once_the_build_role_is_ready() {
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
                egress: None,
                image_arn: None,
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

        assert!(
            !first
                .state
                .stack_state
                .as_ref()
                .unwrap()
                .resources
                .contains_key("agents"),
            "no controller state before its build role is ready"
        );

        let second = handle_initial_setup(
            first.state,
            config(),
            aws_client_config(),
            with_iam(present_role()),
        )
        .await
        .unwrap();
        assert_eq!(
            second.state.status,
            DeploymentStatus::InitialSetup,
            "the pass that applies the build policy does not also hand off"
        );

        let third = handle_initial_setup(
            second.state,
            config(),
            aws_client_config(),
            with_iam(ready_role()),
        )
        .await
        .unwrap();
        assert_eq!(third.state.status, DeploymentStatus::Provisioning);
        let seeded = &third.state.stack_state.as_ref().unwrap().resources["agents"];
        assert_eq!(seeded.status, ResourceStatus::Provisioning);
        let controller = seeded.internal_state.as_ref().unwrap();
        assert_eq!(controller["state"], "creatingImage");
        assert_eq!(
            controller["buildRoleArn"],
            format!("arn:aws:iam::123456789012:role/{BUILD_ROLE}")
        );
        assert_eq!(controller["bundleUri"], BUNDLE_URI);
        assert_eq!(controller["allowEgress"], true);
        assert_eq!(controller["previewPorts"], serde_json::json!([8080]));
        assert_eq!(
            third.state.runtime_metadata.unwrap().setup_scaffolding,
            recorded
        );
    }

    /// A role the step refuses to adopt must stop setup, not be read as done: handing off would
    /// let the controller pass that role to a build running a customer's Dockerfile.
    #[tokio::test]
    async fn a_refused_build_role_stops_setup_instead_of_handing_off() {
        let mut foreign = MockIamApi::new();
        foreign.expect_get_role().returning(|_| {
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: created_role(),
                },
            })
        });
        foreign.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec!["admin".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        foreign.expect_list_attached_role_policies().returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        foreign.expect_put_role_policy().never();

        let result = handle_initial_setup(
            live_sandbox_setup(InitialSetupAuthority::DirectSetup),
            config(),
            aws_client_config(),
            with_iam(foreign),
        )
        .await
        .expect("a refusal is a failed setup, not a lost step");
        assert_eq!(
            result.state.status,
            DeploymentStatus::InitialSetupFailed,
            "a refused role must not let setup reach Provisioning"
        );
        let error = result.state.error.expect("the refusal is recorded");
        assert!(
            format!("{error:?}").contains("SETUP_SCAFFOLDING_NOT_ADOPTABLE"),
            "the refusal surfaces as itself: {error:?}"
        );
    }

    /// One step creates the first sandbox's build role and is then refused the second's. The
    /// created role must stay in the persisted record, or nothing owns it until a teardown
    /// happens to recover it.
    #[tokio::test]
    async fn a_refused_step_keeps_what_it_created_in_the_record() {
        let second = alien_core::Sandbox::new("zeta".to_string())
            .code(alien_core::SandboxCode::Image {
                image: BUNDLE_URI.to_string(),
            })
            .egress(alien_core::SandboxEgress::Allow)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let mut state = live_sandbox_setup(InitialSetupAuthority::DirectSetup);
        let metadata = state.runtime_metadata.as_mut().unwrap();
        let stack = metadata.prepared_stack.take().unwrap();
        metadata.prepared_stack = Some(
            Stack::new("test".to_string())
                .add(
                    stack.resources["agents"]
                        .config
                        .downcast_ref::<alien_core::Sandbox>()
                        .unwrap()
                        .clone(),
                    ResourceLifecycle::Live,
                )
                .add(second, ResourceLifecycle::Live)
                .build(),
        );

        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|name| {
            if name == BUILD_ROLE {
                return Err(alien_error::AlienError::new(
                    alien_aws_clients::ErrorData::RemoteResourceNotFound {
                        resource_type: "IAM Resource".to_string(),
                        resource_name: name.to_string(),
                    },
                ));
            }
            // Someone else's role under the second sandbox's name: its ARN is not that name's.
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: created_role(),
                },
            })
        });
        iam.expect_create_role()
            .withf(|request| request.role_name == BUILD_ROLE)
            .times(1)
            .returning(|_| {
                Ok(CreateRoleResponse {
                    create_role_result: CreateRoleResult {
                        role: created_role(),
                    },
                })
            });
        iam.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        iam.expect_list_attached_role_policies().returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });

        let result = handle_initial_setup(state, config(), aws_client_config(), with_iam(iam))
            .await
            .expect("a refusal is a failed setup, not a lost step");

        assert_eq!(result.state.status, DeploymentStatus::InitialSetupFailed);
        assert!(
            format!("{:?}", result.state.error).contains("SETUP_SCAFFOLDING_NOT_ADOPTABLE"),
            "{:?}",
            result.state.error
        );
        assert_eq!(
            result.state.runtime_metadata.unwrap().setup_scaffolding,
            BTreeMap::from([(
                "agents".to_string(),
                SetupScaffolding::AwsSandbox {
                    build_role_name: BUILD_ROLE.to_string(),
                    egress: None,
                    image_arn: None,
                },
            )]),
        );
    }

    /// Scaffolding creates the build role, then the Frozen step fails on unreadable state. The
    /// role must stay recorded, or nothing owns it.
    #[tokio::test]
    async fn a_failed_frozen_step_keeps_the_scaffolding_it_follows() {
        let mut state = live_sandbox_setup(InitialSetupAuthority::DirectSetup);
        let mut unreadable = alien_core::StackResourceState::new_pending(
            "storage".to_string(),
            alien_core::Resource::new(Storage::new("store".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        unreadable.internal_state = Some(serde_json::json!({}));
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("store".to_string(), unreadable);

        let mut absent = MockIamApi::new();
        absent.expect_get_role().returning(|name| {
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

        let result = handle_initial_setup(state, config(), aws_client_config(), with_iam(absent))
            .await
            .expect("a failed step is a failed setup, not a lost step");

        assert_eq!(result.state.status, DeploymentStatus::InitialSetupFailed);
        assert!(
            format!("{:?}", result.state.error).contains("Failed to execute deployment step"),
            "{:?}",
            result.state.error
        );
        assert_eq!(
            result.state.runtime_metadata.unwrap().setup_scaffolding,
            BTreeMap::from([(
                "agents".to_string(),
                SetupScaffolding::AwsSandbox {
                    build_role_name: BUILD_ROLE.to_string(),
                    egress: None,
                    image_arn: None,
                },
            )]),
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

    const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";

    /// A template's setup built the Frozen image and registered it; handing off makes no cloud
    /// call, and a provider with no expectations panics on any.
    #[tokio::test]
    async fn imported_setup_neither_scaffolds_nor_builds_a_frozen_sandbox() {
        let mut deployment = sandbox_setup(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::ImportedHandoff,
        );
        let stack = deployment
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .clone()
            .unwrap();
        let mut registered = alien_infra::ImporterRegistry::built_in()
            .run(
                &alien_core::Sandbox::RESOURCE_TYPE,
                Platform::Aws,
                serde_json::json!({
                    "imageIdentifier": IMAGE_ARN,
                    "imageArn": IMAGE_ARN,
                    "imageVersion": "1.0",
                    "allowEgress": true,
                    "previewPorts": [8080],
                }),
                &alien_core::import::ImportContext {
                    resource_id: "agents",
                    platform: Platform::Aws,
                    region: "us-east-1",
                    stack_settings: &StackSettings::default(),
                    management_config: None,
                    resource: &stack.resources["agents"],
                },
            )
            .unwrap();
        registered.controller_platform = Some(Platform::Aws);
        let before = serde_json::to_value(&registered).unwrap();
        deployment
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("agents".to_string(), registered);

        let result = handle_initial_setup(
            deployment,
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
        assert_eq!(
            serde_json::to_value(&result.state.stack_state.unwrap().resources["agents"]).unwrap(),
            before,
            "the registered image is taken as it is"
        );
    }

    /// The binding a linked workload is handed, from the controller state as persisted.
    fn published_binding(sandbox: &alien_core::StackResourceState) -> serde_json::Value {
        sandbox
            .get_internal_controller()
            .unwrap()
            .expect("a sandbox has controller state")
            .get_binding_params()
            .unwrap()
            .expect("a sandbox with an ACTIVE image publishes a binding")
    }

    async fn load_binding(binding: &serde_json::Value) -> std::result::Result<(), String> {
        let env = std::collections::HashMap::from([
            (
                alien_core::ENV_ALIEN_DEPLOYMENT_TYPE.to_string(),
                Platform::Aws.as_str().to_string(),
            ),
            ("AWS_REGION".to_string(), "us-east-1".to_string()),
            ("AWS_ACCOUNT_ID".to_string(), "123456789012".to_string()),
            ("AWS_ACCESS_KEY_ID".to_string(), "test".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "test".to_string()),
            ("ALIEN_AGENTS_BINDING".to_string(), binding.to_string()),
        ]);
        BindingsProvider::from_env(env)
            .await
            .map_err(|error| error.to_string())?
            .load_sandbox("agents")
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn building_microvms() -> Arc<MockPlatformServiceProvider> {
        let built = Arc::new(std::sync::Mutex::new(false));
        let probe = built.clone();
        let mut microvms = MockLambdaMicrovmsApi::new();
        microvms.expect_get_microvm_image().returning(move |_| {
            if !*probe.lock().unwrap() {
                return Err(alien_error::AlienError::new(
                    alien_aws_clients::ErrorData::RemoteResourceNotFound {
                        resource_type: "Microvm".to_string(),
                        resource_name: "test-agents".to_string(),
                    },
                ));
            }
            Ok(MicrovmImage {
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        microvms
            .expect_create_microvm_image()
            .withf(|request| {
                request.build_role_arn == format!("arn:aws:iam::123456789012:role/{BUILD_ROLE}")
                    && request.code_artifact.uri == BUNDLE_URI
            })
            .times(1)
            .returning(move |_| {
                *built.lock().unwrap() = true;
                Ok(CreateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: Some("test-agents".to_string()),
                    state: Some("CREATING".to_string()),
                    image_version: Some("1.0".to_string()),
                })
            });
        microvms
            .expect_get_microvm_image_version()
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("SUCCESSFUL".to_string()),
                    status: Some("ACTIVE".to_string()),
                    state_reason: None,
                })
            });
        let microvms = Arc::new(microvms);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(microvms.clone()));
        Arc::new(provider)
    }

    /// Runtime provisioning continues from the seed rather than from a controller's defaults,
    /// which on this path would publish a binding with no connector and no open egress.
    #[tokio::test]
    async fn a_directly_set_up_sandbox_provisions_to_a_binding_the_runtime_loads() {
        let mut deployment = live_sandbox_setup(InitialSetupAuthority::DirectSetup);
        deployment
            .runtime_metadata
            .as_mut()
            .unwrap()
            .setup_scaffolding = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: None,
                image_arn: None,
            },
        )]);
        let mut state = handle_initial_setup(
            deployment,
            config(),
            aws_client_config(),
            with_iam(ready_role()),
        )
        .await
        .unwrap()
        .state;
        assert_eq!(state.status, DeploymentStatus::Provisioning);

        let provider = building_microvms();
        for _ in 0..20 {
            if state.status != DeploymentStatus::Provisioning {
                break;
            }
            state = crate::provisioning::handle_provisioning(
                state,
                config(),
                aws_client_config(),
                provider.clone(),
            )
            .await
            .unwrap()
            .state;
        }
        assert_eq!(state.status, DeploymentStatus::Running);

        let sandbox = &state.stack_state.as_ref().unwrap().resources["agents"];
        assert_eq!(sandbox.status, ResourceStatus::Running);
        let binding = &published_binding(sandbox);
        load_binding(binding)
            .await
            .unwrap_or_else(|error| panic!("the binding must load: {error}\n{binding}"));
        assert_eq!(binding["allowEgress"], true);
        assert_eq!(binding["previewPorts"], serde_json::json!([8080]));
    }

    /// A sandbox as a direct deploy left it before setup seeded one: serving, but with none of
    /// the registration's egress or preview facts.
    async fn with_serving_sandbox(mut deployment: DeploymentState) -> DeploymentState {
        let stack = deployment
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .clone()
            .unwrap();
        let mut serving = alien_infra::ImporterRegistry::built_in()
            .run(
                &alien_core::Sandbox::RESOURCE_TYPE,
                Platform::Aws,
                serde_json::json!({
                    "imageIdentifier": IMAGE_ARN,
                    "imageArn": IMAGE_ARN,
                    "imageVersion": "1.0",
                }),
                &alien_core::import::ImportContext {
                    resource_id: "agents",
                    platform: Platform::Aws,
                    region: "us-east-1",
                    stack_settings: &StackSettings::default(),
                    management_config: None,
                    resource: &stack.resources["agents"],
                },
            )
            .unwrap();
        serving.controller_platform = Some(Platform::Aws);
        assert!(
            load_binding(&published_binding(&serving)).await.is_err(),
            "the fixture must start from the binding the runtime refuses"
        );
        deployment
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("agents".to_string(), serving);
        deployment
            .runtime_metadata
            .as_mut()
            .unwrap()
            .setup_scaffolding = BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: None,
                image_arn: None,
            },
        )]);
        deployment
    }

    /// Setup runs again on a refresh. Registering the sandbox afresh would reset it to its create
    /// state and withdraw the binding of an image that is serving.
    #[tokio::test]
    async fn a_second_setup_pass_keeps_a_serving_sandbox_and_corrects_its_facts() {
        let result = handle_initial_setup(
            with_serving_sandbox(live_sandbox_setup(InitialSetupAuthority::DirectSetup)).await,
            config(),
            aws_client_config(),
            with_iam(ready_role()),
        )
        .await
        .unwrap();

        assert_eq!(result.state.status, DeploymentStatus::Provisioning);
        let sandbox = &result.state.stack_state.as_ref().unwrap().resources["agents"];
        assert_eq!(sandbox.status, ResourceStatus::Running);
        let controller = sandbox.internal_state.as_ref().unwrap();
        assert_eq!(controller["state"], "ready");
        assert_eq!(controller["imageArn"], IMAGE_ARN);
        assert_eq!(controller["activeVersion"], "1.0");
        assert_eq!(controller["allowEgress"], true);
        assert_eq!(controller["previewPorts"], serde_json::json!([8080]));
        let binding = &published_binding(sandbox);
        load_binding(binding)
            .await
            .unwrap_or_else(|error| panic!("the corrected binding must load: {error}\n{binding}"));
        assert_eq!(binding["imageVersion"], "1.0");
    }

    /// A seed that cannot be applied must stop setup: handing off would start the runtime
    /// controller from a state setup never registered.
    #[tokio::test]
    async fn a_seed_that_cannot_be_applied_stops_setup_before_handoff() {
        let mut deployment =
            with_serving_sandbox(live_sandbox_setup(InitialSetupAuthority::DirectSetup)).await;
        let sandbox = deployment
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .get_mut("agents")
            .unwrap();
        sandbox.internal_state.as_mut().unwrap()["state"] = serde_json::json!("noSuchState");

        let result = handle_initial_setup(
            deployment,
            config(),
            aws_client_config(),
            with_iam(ready_role()),
        )
        .await
        .expect("a seed failure is a failed setup, not a lost step");
        assert_eq!(
            result.state.status,
            DeploymentStatus::InitialSetupFailed,
            "setup must not hand off an unseeded sandbox"
        );
        assert!(
            format!("{:?}", result.state.error).contains("the existing state cannot take the seed"),
            "the seed's own failure surfaces: {:?}",
            result.state.error
        );
        assert_eq!(
            result.state.runtime_metadata.unwrap().setup_scaffolding["agents"],
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: None,
                image_arn: None,
            },
            "the build role this step verified stays recorded"
        );
    }

    /// The build role and the MicroVM image as AWS holds them, and every mutating call in order.
    #[derive(Default)]
    struct FakeAws {
        role: bool,
        policy: Option<String>,
        image: bool,
        /// The newest version a create or roll minted.
        version: Option<String>,
        building_polls: u32,
        log: Vec<String>,
    }

    fn not_found(name: &str) -> alien_error::AlienError<alien_aws_clients::ErrorData> {
        alien_error::AlienError::new(alien_aws_clients::ErrorData::RemoteResourceNotFound {
            resource_type: "test".to_string(),
            resource_name: name.to_string(),
        })
    }

    /// IAM and MicroVMs over one [`FakeAws`]. A MicroVMs client may be fetched only once
    /// `microvms` is set, so a build that starts early fails the test.
    fn fake_aws(
        cloud: &Arc<std::sync::Mutex<FakeAws>>,
        microvms: bool,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut iam = MockIamApi::new();
        let c = cloud.clone();
        iam.expect_get_role().returning(move |name| {
            if !c.lock().unwrap().role {
                return Err(not_found(name));
            }
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: created_role(),
                },
            })
        });
        let c = cloud.clone();
        iam.expect_create_role().returning(move |request| {
            let mut cloud = c.lock().unwrap();
            cloud.role = true;
            cloud
                .log
                .push(format!("iam:CreateRole {}", request.role_name));
            Ok(CreateRoleResponse {
                create_role_result: CreateRoleResult {
                    role: created_role(),
                },
            })
        });
        let c = cloud.clone();
        iam.expect_list_role_policies().returning(move |_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: c.lock().unwrap().policy.as_ref().map(|_| PolicyNames {
                        member: vec!["sandbox-image-build".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        iam.expect_list_attached_role_policies().returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        let c = cloud.clone();
        iam.expect_get_role_policy().returning(move |role, name| {
            let policy = c
                .lock()
                .unwrap()
                .policy
                .clone()
                .ok_or_else(|| not_found(name))?;
            Ok(alien_aws_clients::iam::GetRolePolicyResponse {
                get_role_policy_result: alien_aws_clients::iam::GetRolePolicyResult {
                    role_name: role.to_string(),
                    policy_name: name.to_string(),
                    policy_document: policy,
                },
            })
        });
        let c = cloud.clone();
        iam.expect_put_role_policy()
            .returning(move |role, _, document| {
                let mut cloud = c.lock().unwrap();
                cloud.policy = Some(document.to_string());
                cloud.log.push(format!("iam:PutRolePolicy {role}"));
                Ok(())
            });

        let mut lambda = MockLambdaMicrovmsApi::new();
        let c = cloud.clone();
        lambda.expect_get_microvm_image().returning(move |_| {
            if !c.lock().unwrap().image {
                return Err(not_found("test-agents"));
            }
            Ok(MicrovmImage {
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: c.lock().unwrap().version.clone(),
                state: Some("CREATED".to_string()),
            })
        });
        let c = cloud.clone();
        lambda
            .expect_create_microvm_image()
            .returning(move |request| {
                let mut cloud = c.lock().unwrap();
                cloud.image = true;
                cloud.version = Some("1.0".to_string());
                cloud.log.push(format!(
                    "lambda:CreateMicrovmImage {} {}",
                    request.build_role_arn, request.code_artifact.uri
                ));
                Ok(CreateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: Some("test-agents".to_string()),
                    state: Some("CREATING".to_string()),
                    image_version: Some("1.0".to_string()),
                })
            });
        let c = cloud.clone();
        lambda
            .expect_get_microvm_image_version()
            .returning(move |_, version| {
                let mut cloud = c.lock().unwrap();
                let active = cloud.building_polls == 0;
                cloud.building_polls = cloud.building_polls.saturating_sub(1);
                Ok(MicrovmImageVersion {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some(version.to_string()),
                    state: Some(if active { "SUCCESSFUL" } else { "CREATING" }.to_string()),
                    status: active.then(|| "ACTIVE".to_string()),
                    state_reason: None,
                })
            });
        let c = cloud.clone();
        lambda
            .expect_update_microvm_image()
            .returning(move |_, request| {
                let mut cloud = c.lock().unwrap();
                cloud.version = Some("2.0".to_string());
                cloud.log.push(format!(
                    "lambda:UpdateMicrovmImage {} {}",
                    request.build_role_arn, request.code_artifact.uri
                ));
                Ok(
                    alien_aws_clients::lambda_microvms::UpdateMicrovmImageResponse {
                        image_arn: Some(IMAGE_ARN.to_string()),
                        name: Some("test-agents".to_string()),
                        state: Some("UPDATING".to_string()),
                        image_version: Some("2.0".to_string()),
                    },
                )
            });

        let iam = Arc::new(iam);
        let lambda = Arc::new(lambda);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        if microvms {
            provider
                .expect_get_aws_microvms_client()
                .returning(move |_| Ok(lambda.clone()));
        }
        Arc::new(provider)
    }

    /// Setup passes until the handoff, which each Frozen sandbox scenario must reach.
    async fn run_setup(
        mut state: DeploymentState,
        cloud: &Arc<std::sync::Mutex<FakeAws>>,
    ) -> DeploymentState {
        for _ in 0..12 {
            if state.status != DeploymentStatus::InitialSetup {
                break;
            }
            state =
                handle_initial_setup(state, config(), aws_client_config(), fake_aws(cloud, true))
                    .await
                    .unwrap()
                    .state;
        }
        assert_eq!(
            state.status,
            DeploymentStatus::Provisioning,
            "{:?}",
            state.error
        );
        state
    }

    /// Setup run again for a release with a new bundle: the role is regranted to the new object
    /// before the image is rolled onto it, once, and the binding moves only when the roll serves.
    #[tokio::test]
    async fn a_setup_rerun_rolls_a_frozen_sandbox_onto_its_new_bundle_once() {
        const NEXT_BUNDLE: &str = "s3://acme-artifacts/sandbox-bundle/0ddba11/bundle.zip";
        let cloud = Arc::new(std::sync::Mutex::new(FakeAws::default()));
        let mut state = run_setup(
            sandbox_setup(
                ResourceLifecycle::Frozen,
                InitialSetupAuthority::DirectSetup,
            ),
            &cloud,
        )
        .await;
        {
            let mut cloud = cloud.lock().unwrap();
            cloud.log.clear();
            cloud.building_polls = 2;
        }

        let metadata = state.runtime_metadata.as_mut().unwrap();
        let stack = metadata.prepared_stack.as_mut().unwrap();
        let entry = stack.resources.get_mut("agents").unwrap();
        let mut sandbox = entry
            .config
            .downcast_ref::<alien_core::Sandbox>()
            .unwrap()
            .clone();
        sandbox.code = alien_core::SandboxCode::Image {
            image: NEXT_BUNDLE.to_string(),
        };
        entry.config = alien_core::Resource::new(sandbox);
        state.status = DeploymentStatus::InitialSetup;

        let state = run_setup(state, &cloud).await;

        assert_eq!(
            cloud.lock().unwrap().log,
            vec![
                format!("iam:PutRolePolicy {BUILD_ROLE}"),
                format!(
                    "lambda:UpdateMicrovmImage arn:aws:iam::123456789012:role/{BUILD_ROLE} \
                     {NEXT_BUNDLE}"
                ),
            ]
        );
        let sandbox = &state.stack_state.as_ref().unwrap().resources["agents"];
        assert_eq!(sandbox.status, ResourceStatus::Running);
        let binding = &published_binding(sandbox);
        assert_eq!(binding["imageVersion"], "2.0", "the rolled version serves");
        load_binding(binding)
            .await
            .unwrap_or_else(|error| panic!("the binding must load: {error}\n{binding}"));
    }

    /// A Frozen sandbox is built by setup, as the templates build it: the build role first, then
    /// the image, under setup's credentials, and setup hands off only once the image serves.
    #[tokio::test]
    async fn direct_setup_builds_a_frozen_sandbox_into_a_binding_the_runtime_loads() {
        let cloud = Arc::new(std::sync::Mutex::new(FakeAws {
            building_polls: 2,
            ..Default::default()
        }));
        let mut state = sandbox_setup(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
        );

        let first = handle_initial_setup(
            state,
            config(),
            aws_client_config(),
            fake_aws(&cloud, false),
        )
        .await
        .unwrap();
        assert_eq!(first.state.status, DeploymentStatus::InitialSetup);
        assert!(
            first.state.error.is_none(),
            "a sandbox waiting on its build role is not a failure: {:?}",
            first.state.error
        );
        state = run_setup(first.state, &cloud).await;

        let (log, policy) = {
            let cloud = cloud.lock().unwrap();
            (cloud.log.clone(), cloud.policy.clone())
        };
        assert_eq!(
            log,
            vec![
                format!("iam:CreateRole {BUILD_ROLE}"),
                format!("iam:PutRolePolicy {BUILD_ROLE}"),
                format!(
                    "lambda:CreateMicrovmImage arn:aws:iam::123456789012:role/{BUILD_ROLE} \
                     {BUNDLE_URI}"
                ),
            ],
            "the role and its policy land before the one build"
        );
        let frozen_policy = serde_json::to_value(
            alien_core::sandbox_build_role::SandboxBuildRole::builder()
                .sandbox_id("agents")
                .partition("aws")
                .account_id("123456789012")
                .region("us-east-1")
                .bundle_uri(BUNDLE_URI)
                .runtime_built(false)
                .build()
                .policy()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(policy.as_deref().unwrap()).unwrap(),
            frozen_policy,
            "a Frozen image is built once, so its role reads only the one bundle object"
        );

        let sandbox = &state.stack_state.as_ref().unwrap().resources["agents"];
        assert_eq!(sandbox.status, ResourceStatus::Running);
        let binding = &published_binding(sandbox);
        load_binding(binding)
            .await
            .unwrap_or_else(|error| panic!("the binding must load: {error}\n{binding}"));
        assert_eq!(binding["allowEgress"], true);
        assert_eq!(binding["previewPorts"], serde_json::json!([8080]));
        assert_eq!(
            state.runtime_metadata.unwrap().setup_scaffolding["agents"],
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: None,
                image_arn: Some(IMAGE_ARN.to_string()),
            },
            "setup records the image it built, so its teardown deletes it"
        );
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

    #[tokio::test]
    async fn a_secret_sync_that_fails_keeps_the_names_it_attempted() {
        let dir = tempfile::TempDir::new().unwrap();
        let not_a_dir = dir.path().join("not-a-dir");
        std::fs::write(&not_a_dir, b"").unwrap();
        let mut vault = alien_core::StackResourceState::builder()
            .resource_type(alien_core::Vault::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(
                alien_core::Vault::new("secrets".to_string()).build(),
            ))
            .lifecycle(ResourceLifecycle::Frozen)
            .build();
        vault.remote_binding_params = Some(
            serde_json::to_value(alien_core::bindings::VaultBinding::local(
                "secrets",
                not_a_dir.to_string_lossy(),
            ))
            .unwrap(),
        );
        let mut stack_state = StackState::new(Platform::Test);
        stack_state.resources.insert("secrets".to_string(), vault);
        let worker = alien_core::Worker::new("worker".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let prepared = Stack::new("test".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();
        let mut config = config();
        config.environment_variables.variables = vec![alien_core::EnvironmentVariable {
            name: "API_TOKEN".to_string(),
            value: "secret".to_string(),
            var_type: alien_core::EnvironmentVariableType::Secret,
            target_resources: None,
        }];
        let state = DeploymentState {
            status: DeploymentStatus::InitialSetup,
            platform: Platform::Test,
            current_release: None,
            target_release: None,
            stack_state: Some(stack_state),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(prepared),
                initial_setup_authority: InitialSetupAuthority::DirectSetup,
                ..Default::default()
            }),
        };

        let failed = handle_initial_setup(
            state,
            config,
            ClientConfig::Test,
            Arc::new(MockPlatformServiceProvider::new()),
        )
        .await
        .expect("the step fails itself with the record it holds")
        .state;

        assert_eq!(failed.status, DeploymentStatus::InitialSetupFailed);
        let metadata = failed.runtime_metadata.unwrap();
        assert_eq!(metadata.last_synced_secret_names, vec!["API_TOKEN"]);
        assert!(metadata.last_synced_env_vars_hash.is_none());
    }
}
