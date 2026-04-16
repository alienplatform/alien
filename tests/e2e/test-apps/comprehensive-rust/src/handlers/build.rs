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
    models::{AppState, BuildTestConfig, BuildTestRequest, BuildTestResponse},
    ErrorData, Result,
};
use alien_core::{BuildConfig, BuildStatus, ComputeType};
use alien_error::{AlienError, Context, ContextError};
use alien_sdk::{error::ErrorData as BindingsErrorData, BindingsProvider};

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
        .ctx
        .get_bindings()
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
