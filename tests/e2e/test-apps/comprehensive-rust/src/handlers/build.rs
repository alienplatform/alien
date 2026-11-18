use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use std::collections::HashMap;
use std::time::Duration;
use tracing::info;
use utoipa::path;

use crate::{
    models::{
        AppState, BuildIdentityCheckRequest, BuildIdentityCheckResponse, BuildTestConfig,
        BuildTestRequest, BuildTestResponse,
    },
    ErrorData, Result,
};
use alien_core::{BuildConfig, BuildStatus, ComputeType};
use alien_error::{AlienError, Context, ContextError};

/// Test build operations
#[utoipa::path(
    post,
    path = "/build-test/{binding_name}",
    tag = "builds",
    params(
        ("binding_name" = String, Path, description = "Name of the build binding to test")
    ),
    request_body = Option<BuildTestRequest>,
    responses(
        (status = 200, description = "Build test completed", body = BuildTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Build operation failed", body = AlienError),
    ),
    operation_id = "test_build",
    summary = "Test build operations",
    description = "Performs comprehensive testing of build operations including start build, status monitoring, and stop build"
)]
pub async fn test_build(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
    Json(request): Json<Option<BuildTestRequest>>,
) -> Result<Json<BuildTestResponse>> {
    info!(%binding_name, "Received build test request");

    let build_instance = app_state
        .internal_bindings
        .load_build(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // Create build configuration from request or use defaults
    let build_config = create_build_config(request);

    // 1. Start build operation
    let build_execution = build_instance.start_build(build_config).await.context(
        ErrorData::BuildOperationFailed {
            operation: "start_build".to_string(),
        },
    )?;

    // 2. Get build status operation with polling
    let mut final_status = BuildStatus::Queued;
    let mut status_checks = 0;
    const MAX_STATUS_CHECKS: usize = 30; // Max 30 checks (about 5 minutes with 10s intervals)

    loop {
        let execution = build_instance
            .get_build_status(&build_execution.id)
            .await
            .context(ErrorData::BuildOperationFailed {
                operation: "get_build_status".to_string(),
            })?;

        status_checks += 1;
        final_status = execution.status.clone();

        match execution.status {
            BuildStatus::Succeeded => {
                break;
            }
            BuildStatus::Failed | BuildStatus::Cancelled | BuildStatus::TimedOut => {
                return Err(AlienError::new(ErrorData::BuildOperationFailed {
                    operation: format!("build_failed_with_status_{:?}", execution.status),
                }));
            }
            BuildStatus::Queued | BuildStatus::Running => {
                if status_checks >= MAX_STATUS_CHECKS {
                    return Err(AlienError::new(ErrorData::BuildOperationFailed {
                        operation: "build_timeout".to_string(),
                    }));
                }
                // Wait before next status check
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }

    // 3. Optional: Stop build test (if it's still running for some reason)
    if matches!(final_status, BuildStatus::Queued | BuildStatus::Running) {
        build_instance
            .stop_build(&build_execution.id)
            .await
            .context(ErrorData::BuildOperationFailed {
                operation: "stop_build".to_string(),
            })?;
    }

    Ok(Json(BuildTestResponse {
        binding_name,
        execution_id: build_execution.id,
        final_status: format!("{:?}", final_status),
        success: true,
    }))
}

/// Create build configuration from request or use sensible defaults
fn create_build_config(request: Option<BuildTestRequest>) -> BuildConfig {
    let config = request.and_then(|req| req.config).unwrap_or_default();

    BuildConfig {
        script: config.script.unwrap_or_else(|| {
            "echo 'Test build started'; echo 'Build environment test'; echo 'Test build completed'".to_string()
        }),
        environment: config.environment.unwrap_or_else(|| {
            let mut env = HashMap::new();
            env.insert("BUILD_TEST_VAR".to_string(), "test_value".to_string());
            env.insert("BUILD_TIMESTAMP".to_string(), Utc::now().timestamp().to_string());
            env
        }),
        image: config.image.unwrap_or_else(|| "ubuntu:20.04".to_string()),
        monitoring: None,
        timeout_seconds: config.timeout_seconds.unwrap_or(300), // 5 minutes
        compute_type: config.compute_type.unwrap_or(ComputeType::Medium),
    }
}

impl Default for BuildTestConfig {
    fn default() -> Self {
        Self {
            script: None,
            environment: None,
            image: None,
            timeout_seconds: None,
            compute_type: None,
        }
    }
}

/// Test-only: with the grants of the build binding, try to create a job that names an identity
/// other than the build's own. The assign grant covers only the build's identity, so Azure must
/// answer 403 AuthorizationFailed. If the request is accepted, the job is deleted again.
#[utoipa::path(
    post,
    path = "/build-test/{binding_name}/other-identity",
    tag = "builds",
    params(
        ("binding_name" = String, Path, description = "Name of the Azure build binding")
    ),
    request_body = BuildIdentityCheckRequest,
    responses(
        (status = 200, description = "Check completed", body = BuildIdentityCheckResponse),
        (status = 400, description = "Binding not found or not an Azure build binding", body = AlienError),
        (status = 500, description = "Check could not run", body = AlienError),
    ),
    operation_id = "test_build_other_identity",
    summary = "Test that a build job cannot name another identity",
    description = "Test-only negative check for the narrowed Azure build grants"
)]
pub async fn test_build_other_identity(
    Path(binding_name): Path<String>,
    Json(request): Json<BuildIdentityCheckRequest>,
) -> Result<Json<BuildIdentityCheckResponse>> {
    use alien_azure_clients::{
        container_apps::{AzureContainerAppsClient, ContainerAppsApi},
        long_running_operation::OperationResult,
        models::jobs::{
            Container, Job, JobConfiguration, JobConfigurationManualTriggerConfig,
            JobConfigurationTriggerType, JobProperties, JobTemplate, ManagedServiceIdentity,
            ManagedServiceIdentityType, Parallelism, ReplicaCompletionCount,
            UserAssignedIdentities, UserAssignedIdentity,
        },
        AzureClientConfig, AzureClientConfigExt, AzureTokenCache,
    };
    use alien_core::bindings::{binding_env_var_name, BuildBinding};

    let failed = |operation: String| AlienError::new(ErrorData::BuildOperationFailed { operation });
    let raw = std::env::var(binding_env_var_name(&binding_name)).map_err(|_| {
        AlienError::new(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })
    })?;
    let BuildBinding::Aca(binding) = serde_json::from_str::<BuildBinding>(&raw)
        .map_err(|e| failed(format!("parse binding: {e}")))?
    else {
        return Err(failed("binding is not an Azure build binding".to_string()));
    };
    let environment_id = binding
        .managed_environment_id
        .into_value(&binding_name, "managed_environment_id")
        .map_err(|e| failed(format!("binding environment id: {e}")))?;
    let resource_group = binding
        .resource_group_name
        .into_value(&binding_name, "resource_group_name")
        .map_err(|e| failed(format!("binding resource group: {e}")))?;

    let config = AzureClientConfig::from_std_env()
        .await
        .map_err(|e| failed(format!("azure client config: {e}")))?;
    let client = AzureContainerAppsClient::new(
        alien_bindings::http_client::create_http_client(),
        AzureTokenCache::new(config),
    );

    let job_name = format!(
        "negcheck-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let job = Job {
        location: "eastus".to_string(),
        properties: Some(JobProperties {
            environment_id: Some(environment_id),
            configuration: Some(JobConfiguration {
                trigger_type: JobConfigurationTriggerType::Manual,
                replica_timeout: 60,
                replica_retry_limit: Some(0),
                manual_trigger_config: Some(JobConfigurationManualTriggerConfig {
                    parallelism: Some(Parallelism(1)),
                    replica_completion_count: Some(ReplicaCompletionCount(1)),
                }),
                registries: vec![],
                secrets: vec![],
                event_trigger_config: None,
                schedule_trigger_config: None,
                identity_settings: vec![],
            }),
            template: Some(JobTemplate {
                containers: vec![Container {
                    name: Some("c".to_string()),
                    image: Some("ubuntu:20.04".to_string()),
                    command: vec!["true".to_string()],
                    args: vec![],
                    env: vec![],
                    resources: None,
                    probes: vec![],
                    volume_mounts: vec![],
                }],
                init_containers: vec![],
                volumes: vec![],
            }),
            workload_profile_name: None,
            provisioning_state: None,
            event_stream_endpoint: None,
            outbound_ip_addresses: vec![],
        }),
        identity: Some(ManagedServiceIdentity {
            type_: ManagedServiceIdentityType::UserAssigned,
            user_assigned_identities: Some(UserAssignedIdentities(
                std::collections::HashMap::from([(
                    request.other_identity_id,
                    UserAssignedIdentity::default(),
                )]),
            )),
            principal_id: None,
            tenant_id: None,
        }),
        tags: Default::default(),
        id: None,
        name: None,
        type_: None,
        system_data: None,
    };

    match client
        .create_or_update_job(&resource_group, &job_name, &job)
        .await
    {
        Err(err) => {
            let refused = matches!(
                err.error,
                Some(alien_azure_clients::ErrorData::RemoteAccessDenied { .. })
            );
            Ok(Json(BuildIdentityCheckResponse {
                binding_name,
                refused,
                status: if refused { 403 } else { 500 },
                error_code: err.code.clone(),
                message: err.message.clone(),
            }))
        }
        Ok(result) => {
            // Accepted: this is the failure. Remove what was created.
            if let OperationResult::LongRunning(_) = result {
                let _ = client.get_job(&resource_group, &job_name).await;
            }
            let _ = client.delete_job(&resource_group, &job_name).await;
            Ok(Json(BuildIdentityCheckResponse {
                binding_name,
                refused: false,
                status: 0,
                error_code: String::new(),
                message: "the job naming another identity was accepted".to_string(),
            }))
        }
    }
}
