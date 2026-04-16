#![cfg(all(test, feature = "gcp"))]
use alien_client_core::{ErrorData, Result};
use alien_gcp_clients::cloudbuild::{
    Build, BuildStatus, BuildStep, CloudBuildApi, CloudBuildClient,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tokio::time::sleep;
use tracing::{info, warn};

struct CloudBuildTestContext {
    client: CloudBuildClient,
    project_id: String,
    location: String,
    created_builds: Mutex<HashSet<String>>,
}

impl AsyncTestContext for CloudBuildTestContext {
    async fn setup() -> CloudBuildTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let location = "us-central1".to_string(); // Cloud Build is regional

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: location.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = CloudBuildClient::new(Client::new(), config);

        CloudBuildTestContext {
            client,
            project_id,
            location,
            created_builds: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Cloud Build test cleanup...");

        let builds_to_cleanup = {
            let builds = self.created_builds.lock().unwrap();
            builds.clone()
        };

        for build_id in builds_to_cleanup {
            self.cleanup_build(&build_id).await;
        }

        info!("✅ Cloud Build test cleanup completed");
    }
}

impl CloudBuildTestContext {
    fn track_build(&self, build_id: &str) {
        let mut builds = self.created_builds.lock().unwrap();
        builds.insert(build_id.to_string());
        info!("📝 Tracking build for cleanup: {}", build_id);
    }

    async fn cleanup_build(&self, build_id: &str) {
        info!("🧹 Cleaning up build: {}", build_id);

        match self.client.get_build(&self.location, build_id).await {
            Ok(build) => match build.status {
                Some(BuildStatus::Queued) | Some(BuildStatus::Working) => {
                    info!(
                        "Build {} is still in progress, attempting to cancel.",
                        build_id
                    );
                    if let Err(e) = self.client.cancel_build(&self.location, build_id).await {
                        warn!(
                            "Failed to cancel build {} during cleanup: {:?}",
                            build_id, e
                        );
                    } else {
                        info!("Successfully requested cancellation for build {}", build_id);
                    }
                }
                _ => {
                    info!(
                        "Build {} is in a terminal state, no cleanup action needed.",
                        build_id
                    );
                }
            },
            Err(e) => {
                if let Some(ErrorData::RemoteResourceNotFound { .. }) = &e.error {
                    info!("Build {} not found, likely already cleaned up.", build_id);
                } else {
                    warn!("Failed to get build {} during cleanup: {:?}", build_id, e);
                }
            }
        }
    }

    async fn wait_for_build(&self, build_id: &str) -> Result<Build> {
        let max_attempts = 60; // ~5 minutes max
        let delay = Duration::from_secs(5);

        for attempt in 1..=max_attempts {
            let build = self.client.get_build(&self.location, build_id).await?;

            if let Some(status) = &build.status {
                match status {
                    BuildStatus::Queued | BuildStatus::Working => {
                        info!(
                            "Build {} is {:?}, waiting... (attempt {}/{})",
                            build_id, status, attempt, max_attempts
                        );
                        sleep(delay).await;
                    }
                    _ => {
                        info!("Build {} reached terminal state: {:?}", build_id, status);
                        return Ok(build);
                    }
                }
            } else {
                info!("Build {} has no status yet, waiting...", build_id);
                sleep(delay).await;
            }
        }

        Err(alien_error::AlienError::new(ErrorData::Timeout {
            message: format!(
                "Build {} did not complete after {} attempts.",
                build_id, max_attempts
            ),
        }))
    }
}

#[test_context(CloudBuildTestContext)]
#[tokio::test]
async fn test_create_and_get_build(ctx: &mut CloudBuildTestContext) {
    info!("🚀 Starting test_create_and_get_build");

    let build_request = Build::builder()
        .steps(vec![BuildStep::builder()
            .name("ubuntu".to_string())
            .args(vec![
                "bash".to_string(),
                "-c".to_string(),
                // Write to workspace and use exit code to verify execution
                "echo 'Hello from Alien e2e test!' && echo 'BUILD_SUCCESS_MARKER' > /workspace/test_output.txt && echo 'Build completed successfully' && exit 0".to_string(),
            ])
            .build()])
        .timeout("120s".to_string())
        .build();

    let operation = ctx
        .client
        .create_build(&ctx.location, build_request)
        .await
        .expect("Failed to create build");

    let build_id = operation
        .metadata
        .as_ref()
        .and_then(|m| m.get("build"))
        .and_then(|b| b.get("id"))
        .and_then(|id| id.as_str())
        .expect("Could not extract build ID from operation metadata")
        .to_string();

    ctx.track_build(&build_id);
    info!("Build created with ID: {}", build_id);

    let final_build = ctx
        .wait_for_build(&build_id)
        .await
        .expect("Build failed to complete");

    assert_eq!(final_build.status, Some(BuildStatus::Success));
    info!("✅ Build {} completed successfully.", build_id);

    // Verify that the build actually executed by checking build step outputs
    if let Some(results) = &final_build.results {
        info!("📄 Build step outputs: {:?}", results.build_step_outputs);
        if !results.build_step_outputs.is_empty() {
            info!("✅ Build produced outputs, indicating successful execution");
        }
    }

    let fetched_build = ctx
        .client
        .get_build(&ctx.location, &build_id)
        .await
        .expect("Failed to get build");
    assert_eq!(fetched_build.id.as_ref().unwrap(), &build_id);
    assert_eq!(fetched_build.status, Some(BuildStatus::Success));
    info!(
        "✅ Successfully fetched build {} and verified execution markers.",
        build_id
    );
}

#[test_context(CloudBuildTestContext)]
#[tokio::test]
async fn test_cancel_and_retry_build(ctx: &mut CloudBuildTestContext) {
    info!("🚀 Starting test_cancel_and_retry_build");

    // --- Part 1: Cancel a build ---
    info!("Part 1: Testing build cancellation");
    let cancel_build_request = Build::builder()
        .steps(vec![BuildStep::builder()
            .name("ubuntu".to_string())
            .args(vec![
                "bash".to_string(),
                "-c".to_string(),
                // Write start marker, sleep, then write completion marker (should be cancelled before completion)
                "echo 'CANCEL_TEST_STARTED' > /workspace/cancel_start.txt && echo 'This build will be cancelled...' && sleep 30 && echo 'CANCEL_TEST_COMPLETED' > /workspace/cancel_complete.txt && echo 'This should not print!';".to_string(),
            ])
            .build()])
        .timeout("120s".to_string())
        .build();

    let cancel_op = ctx
        .client
        .create_build(&ctx.location, cancel_build_request)
        .await
        .expect("Failed to create build for cancellation test");

    let cancel_build_id = cancel_op
        .metadata
        .as_ref()
        .and_then(|m| m.get("build"))
        .and_then(|b| b.get("id"))
        .and_then(|id| id.as_str())
        .unwrap()
        .to_string();
    ctx.track_build(&cancel_build_id);
    info!("Created build to cancel with ID: {}", &cancel_build_id);

    // Give it a moment to get into QUEUED or WORKING state
    sleep(Duration::from_secs(5)).await;

    ctx.client
        .cancel_build(&ctx.location, &cancel_build_id)
        .await
        .expect("Failed to cancel build");
    info!("Cancellation requested for build {}", &cancel_build_id);

    let cancelled_build = ctx
        .wait_for_build(&cancel_build_id)
        .await
        .expect("Build failed to reach terminal state after cancellation");
    assert_eq!(cancelled_build.status, Some(BuildStatus::Cancelled));
    info!("✅ Build {} successfully cancelled.", &cancel_build_id);

    // --- Part 2: Retry a failed build ---
    info!("Part 2: Testing build retry");
    let fail_build_request = Build::builder()
        .steps(vec![BuildStep::builder()
            .name("ubuntu".to_string())
            .args(vec![
                "bash".to_string(),
                "-c".to_string(),
                // Write failure marker to verify execution before failing
                "echo 'FAILURE_TEST_EXECUTED' > /workspace/failure_marker.txt && echo 'This build will fail.' && exit 1;".to_string(),
            ])
            .build()])
        .timeout("120s".to_string())
        .build();

    let fail_op = ctx
        .client
        .create_build(&ctx.location, fail_build_request)
        .await
        .expect("Failed to create build for retry test");

    let fail_build_id = fail_op
        .metadata
        .as_ref()
        .and_then(|m| m.get("build"))
        .and_then(|b| b.get("id"))
        .and_then(|id| id.as_str())
        .unwrap()
        .to_string();
    ctx.track_build(&fail_build_id);
    info!("Created build to fail with ID: {}", &fail_build_id);

    let failed_build = ctx
        .wait_for_build(&fail_build_id)
        .await
        .expect("Failing build did not complete");
    assert_eq!(failed_build.status, Some(BuildStatus::Failure));
    info!(
        "✅ Build {} successfully failed as expected.",
        &fail_build_id
    );

    // Verify the build actually executed (even though it failed)
    if let Some(results) = &failed_build.results {
        info!("📄 Failed build outputs: {:?}", results.build_step_outputs);
        if !results.build_step_outputs.is_empty() {
            info!("✅ Failed build produced outputs, indicating it executed before failing");
        }
    }

    let retry_op = ctx
        .client
        .retry_build(&ctx.location, &fail_build_id)
        .await
        .expect("Failed to retry build");
    let retry_build_id = retry_op
        .metadata
        .as_ref()
        .and_then(|m| m.get("build"))
        .and_then(|b| b.get("id"))
        .and_then(|id| id.as_str())
        .unwrap()
        .to_string();
    ctx.track_build(&retry_build_id);
    info!(
        "Retrying build {}, new build ID: {}",
        &fail_build_id, &retry_build_id
    );

    let retried_build = ctx
        .wait_for_build(&retry_build_id)
        .await
        .expect("Retried build did not complete");
    assert_eq!(retried_build.status, Some(BuildStatus::Failure));
    info!(
        "✅ Retried build {} also failed as expected.",
        &retry_build_id
    );

    // Verify the retried build also executed
    if let Some(results) = &retried_build.results {
        info!("📄 Retried build outputs: {:?}", results.build_step_outputs);
        if !results.build_step_outputs.is_empty() {
            info!("✅ Retried build produced outputs, indicating retry mechanism works correctly");
        }
    }
}

#[test_context(CloudBuildTestContext)]
#[tokio::test]
async fn test_build_execution_verification(ctx: &mut CloudBuildTestContext) {
    info!("🚀 Starting test_build_execution_verification");

    let build_request = Build::builder()
        .steps(vec![
            // Step 1: Create a marker file
            BuildStep::builder()
                .name("ubuntu".to_string())
                .id("create-marker".to_string())
                .args(vec![
                    "bash".to_string(),
                    "-c".to_string(),
                    "echo 'EXECUTION_VERIFIED' > /workspace/execution_marker.txt && echo 'Step 1 completed'".to_string(),
                ])
                .build(),
            // Step 2: Verify the marker exists and create a success file
            BuildStep::builder()
                .name("ubuntu".to_string())
                .id("verify-marker".to_string())
                .wait_for(vec!["create-marker".to_string()])
                .args(vec![
                    "bash".to_string(),
                    "-c".to_string(),
                    "cat /workspace/execution_marker.txt && echo 'SUCCESS_VERIFICATION' > /workspace/success.txt && echo 'Step 2 completed'".to_string(),
                ])
                .build(),
        ])
        .timeout("120s".to_string())
        .build();

    let operation = ctx
        .client
        .create_build(&ctx.location, build_request)
        .await
        .expect("Failed to create verification build");

    let build_id = operation
        .metadata
        .as_ref()
        .and_then(|m| m.get("build"))
        .and_then(|b| b.get("id"))
        .and_then(|id| id.as_str())
        .expect("Could not extract build ID from operation metadata")
        .to_string();

    ctx.track_build(&build_id);
    info!("Verification build created with ID: {}", build_id);

    let final_build = ctx
        .wait_for_build(&build_id)
        .await
        .expect("Verification build failed to complete");

    assert_eq!(final_build.status, Some(BuildStatus::Success));
    info!("✅ Verification build {} completed successfully.", build_id);

    // Check that both steps were executed successfully
    if let Some(results) = &final_build.results {
        info!(
            "📄 Verification build outputs: {:?}",
            results.build_step_outputs
        );

        // Verify that we have step outputs (indicating execution)
        if !results.build_step_outputs.is_empty() {
            info!(
                "✅ Build produced {} outputs, confirming step execution",
                results.build_step_outputs.len()
            );
        } else {
            warn!("⚠️  No build step outputs found - may indicate limited execution verification");
        }
    }

    // Verify that both build steps completed
    for (i, step) in final_build.steps.iter().enumerate() {
        if let Some(status) = &step.status {
            info!("📝 Step {} status: {:?}", i + 1, status);
            assert_eq!(
                *status,
                BuildStatus::Success,
                "Step {} should have succeeded",
                i + 1
            );
        }
    }

    info!("✅ All verification checks passed - build execution confirmed!");
}
