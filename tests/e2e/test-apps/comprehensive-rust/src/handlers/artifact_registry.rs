use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use std::time::Duration;
use tracing::info;
use utoipa::path;

use crate::{
    models::{AppState, ArtifactRegistryTestRequest, ArtifactRegistryTestResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_sdk::{ArtifactRegistryPermissions, BindingsProvider};

/// Test artifact registry operations
#[utoipa::path(
    post,
    path = "/artifact-registry-test/{binding_name}",
    tag = "artifact-registry",
    params(
        ("binding_name" = String, Path, description = "Name of the artifact registry binding to test")
    ),
    request_body = Option<ArtifactRegistryTestRequest>,
    responses(
        (status = 200, description = "Artifact registry test completed", body = ArtifactRegistryTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Artifact registry operation failed", body = AlienError),
    ),
    operation_id = "test_artifact_registry",
    summary = "Test artifact registry operations",
    description = "Performs comprehensive testing of artifact registry operations including repository creation, credentials generation, Docker operations, and cleanup"
)]
pub async fn test_artifact_registry(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
    Json(request): Json<Option<ArtifactRegistryTestRequest>>,
) -> Result<Json<ArtifactRegistryTestResponse>> {
    info!(%binding_name, "Received artifact registry test request");

    let artifact_registry_instance = app_state
        .ctx
        .get_bindings()
        .load_artifact_registry(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // Generate a unique repository name for testing
    let test_repo_name = request
        .as_ref()
        .and_then(|req| req.repo_name_prefix.as_ref())
        .map(|prefix| format!("{}-{}", prefix, Utc::now().timestamp_millis()))
        .unwrap_or_else(|| format!("test-repo-{}", Utc::now().timestamp_millis()));

    let skip_docker = request
        .as_ref()
        .and_then(|req| req.skip_docker_operations)
        .unwrap_or(false);

    // 1. Create repository operation
    let create_response = artifact_registry_instance
        .create_repository(&test_repo_name)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: "create_repository".to_string(),
        })?;
    let repo_id = create_response.name.clone();

    // 2. Wait for repository to be ready
    let mut status_checks = 0;
    const MAX_STATUS_CHECKS: usize = 10;

    loop {
        match artifact_registry_instance.get_repository(&repo_id).await {
            Ok(_repository) => {
                // Repository exists and is ready
                break;
            }
            Err(e) => {
                status_checks += 1;

                // Check if it's a ResourceNotFound error (repository still creating)
                if let Some(alien_sdk::error::ErrorData::ResourceNotFound { .. }) = &e.error {
                    // Repository not found yet, it might still be creating
                    if status_checks >= MAX_STATUS_CHECKS {
                        return Err(AlienError::new(
                            ErrorData::ArtifactRegistryOperationFailed {
                                operation: "repository_creation_timeout".to_string(),
                            },
                        ));
                    }
                } else {
                    // Other error, propagate it
                    return Err(e.context(ErrorData::ArtifactRegistryOperationFailed {
                        operation: "get_repository".to_string(),
                    }));
                }
            }
        }

        // Wait before next status check if not ready yet
        if status_checks < MAX_STATUS_CHECKS {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    // 3. Generate pull credentials
    artifact_registry_instance
        .generate_credentials(&repo_id, ArtifactRegistryPermissions::Pull, Some(3600))
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: "generate_pull_credentials".to_string(),
        })?;

    // 4. Generate push-pull credentials
    artifact_registry_instance
        .generate_credentials(&repo_id, ArtifactRegistryPermissions::PushPull, Some(3600))
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: "generate_push_pull_credentials".to_string(),
        })?;

    // 5. Docker operations (if not skipped)
    if !skip_docker {
        create_test_image().await.into_alien_error().context(
            ErrorData::ArtifactRegistryOperationFailed {
                operation: "create_test_image".to_string(),
            },
        )?;
    }

    // 6. Delete repository operation (cleanup)
    artifact_registry_instance
        .delete_repository(&repo_id)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: "delete_repository".to_string(),
        })?;

    Ok(Json(ArtifactRegistryTestResponse {
        binding_name,
        repo_name: repo_id,
        success: true,
    }))
}

/// Creates a simple test Docker image with a single text file.
async fn create_test_image() -> std::result::Result<dockdash::Image, dockdash::Error> {
    use dockdash::{Arch, Image, Layer};

    // Create a simple layer with a text file
    let test_content = format!(
        "Hello from Alien test image! Created at: {}",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let layer = Layer::builder()?
        .data("/test-file.txt", test_content.as_bytes(), None)?
        .build()
        .await?;

    // Build a simple image
    let (image, _) = Image::builder()
        .from("alpine:latest") // Use a small base image
        .platform("linux", &Arch::Amd64) // Use common architecture
        .layer(layer)
        .entrypoint(vec!["cat".to_string(), "/test-file.txt".to_string()])
        .build()
        .await?;

    Ok(image)
}
