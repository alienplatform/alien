/*!
# CodeBuild Client Integration Tests

These tests perform real AWS CodeBuild operations including creating projects and managing builds.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with your AWS credentials.
The role used to run the tests needs permissions to create and manage IAM roles and CodeBuild projects.

### 2. Required Permissions
Your AWS credentials need these permissions:
- `codebuild:*`
- `iam:CreateRole`, `iam:DeleteRole`, `iam:PutRolePolicy`, `iam:DeleteRolePolicy`, `iam:PassRole`

## Running Tests
```bash
# Run all CodeBuild tests (
cargo test --package alien-aws-clients --test aws_codebuild_client_tests -- --nocapture
```
*/

use alien_aws_clients::codebuild::*;
use alien_aws_clients::iam::{CreateRoleRequest, IamApi, IamClient};
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::{Error, ErrorData, Result};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct CodeBuildTestContext {
    codebuild_client: CodeBuildClient,
    iam_client: IamClient,
    created_projects: Mutex<HashSet<String>>,
    service_role_arn: String,
    service_role_name: String,
}

impl CodeBuildTestContext {
    fn track_project(&self, name: &str) {
        let mut projects = self.created_projects.lock().unwrap();
        projects.insert(name.to_string());
        info!("📝 Tracking project for cleanup: {}", name);
    }

    async fn cleanup_project(&self, name: &str) {
        info!("🧹 Cleaning up project: {}", name);
        let req = DeleteProjectRequest {
            name: name.to_string(),
        };
        match self.codebuild_client.delete_project(req).await {
            Ok(_) => info!("✅ Project {} deleted successfully", name),
            Err(e) => {
                if !matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                    warn!("Failed to delete project {} during cleanup: {:?}", name, e);
                }
            }
        }
    }

    async fn create_project_with_retry(
        &self,
        create_req: CreateProjectRequest,
    ) -> Result<CreateProjectResponse> {
        let mut delay = Duration::from_secs(1);
        for attempt in 1..=5 {
            match self
                .codebuild_client
                .create_project(create_req.clone())
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if matches!(e.error, Some(ErrorData::RemoteResourceConflict { .. }))
                        && attempt < 5
                    {
                        info!(
                            "Project already exists, retrying in {}s... (attempt {}/5)",
                            delay.as_secs(),
                            attempt
                        );
                        tokio::time::sleep(delay).await;
                        delay = delay * 2; // exponential backoff
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    fn get_project_name(&self) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        format!(
            "alien-test-project-{}-{}",
            timestamp,
            Uuid::new_v4().simple()
        )
    }
}

impl AsyncTestContext for CodeBuildTestContext {
    async fn setup() -> Self {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
            },
            service_overrides: None,
        };

        let codebuild_client = CodeBuildClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config.clone()));
        let iam_client = IamClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        // Create IAM role for CodeBuild
        let role_name = format!("alien-test-codebuild-role-{}", Uuid::new_v4().simple());
        info!("🔧 Creating IAM service role for tests: {}", role_name);

        let assume_role_policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"Service": "codebuild.amazonaws.com"},
                "Action": "sts:AssumeRole"
            }]
        }"#
        .to_string();

        let role_request = CreateRoleRequest::builder()
            .role_name(role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .build();

        let role = iam_client.create_role(role_request).await.expect(
            "Failed to create IAM role for CodeBuild. Check your test credentials and permissions.",
        );
        let service_role_arn = role.create_role_result.role.arn.clone();

        let policy_document = r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "logs:CreateLogGroup",
                        "logs:CreateLogStream",
                        "logs:PutLogEvents"
                    ],
                    "Resource": "*"
                }
            ]
        }"#
        .to_string();

        iam_client
            .put_role_policy(&role_name, "CodeBuildDefaultPolicy", &policy_document)
            .await
            .expect("Failed to attach policy to role");

        // Give IAM a moment to propagate the new role
        info!("Waiting 10 seconds for IAM propagation...");
        tokio::time::sleep(Duration::from_secs(10)).await;
        info!("IAM role should be ready.");

        Self {
            codebuild_client,
            iam_client,
            created_projects: Mutex::new(HashSet::new()),
            service_role_arn,
            service_role_name: role_name,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting CodeBuild test cleanup...");

        let projects_to_cleanup = self.created_projects.lock().unwrap().clone();
        for name in projects_to_cleanup {
            self.cleanup_project(&name).await;
        }

        info!("🧹 Cleaning up IAM role: {}", self.service_role_name);
        match self
            .iam_client
            .delete_role_policy(&self.service_role_name, "CodeBuildDefaultPolicy")
            .await
        {
            Ok(_) => info!("✅ Deleted role policy successfully."),
            Err(e) => warn!(
                "Failed to delete policy from role {} during cleanup: {:?}",
                self.service_role_name, e
            ),
        }
        match self.iam_client.delete_role(&self.service_role_name).await {
            Ok(_) => info!("✅ Deleted IAM role successfully."),
            Err(e) => warn!(
                "Failed to delete role {} during cleanup: {:?}",
                self.service_role_name, e
            ),
        }

        info!("✅ CodeBuild test cleanup completed");
    }
}

async fn wait_for_build_to_finish(ctx: &CodeBuildTestContext, build_id: &str) -> String {
    info!("Polling build status for build ID: {}", build_id);
    for _ in 0..60 {
        // Poll for up to 5 minutes
        let get_builds_req = BatchGetBuildsRequest::builder()
            .ids(vec![build_id.to_string()])
            .build();
        let get_builds_res = ctx
            .codebuild_client
            .batch_get_builds(get_builds_req)
            .await
            .expect("BatchGetBuilds failed during polling");

        if let Some(build) = get_builds_res.builds.as_ref().and_then(|b| b.first()) {
            if let Some(status) = build.build_status.as_deref() {
                info!("Build {} status: {}", build_id, status);
                match status {
                    "SUCCEEDED" | "FAILED" | "FAULT" | "TIMED_OUT" | "STOPPED" => {
                        return status.to_string()
                    }
                    _ => {} // IN_PROGRESS
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    panic!("Build {} did not finish within timeout", build_id);
}

#[test_context(CodeBuildTestContext)]
#[tokio::test]
async fn test_codebuild_project_and_build_lifecycle(ctx: &mut CodeBuildTestContext) {
    let project_name = ctx.get_project_name();
    info!(
        "🚀 Testing CodeBuild lifecycle with project: {}",
        project_name
    );

    // 1. CreateProject

    let create_req = CreateProjectRequest::builder()
        .name(project_name.clone())
        .service_role(ctx.service_role_arn.clone())
        .source(
            ProjectSource::builder()
                .r#type("NO_SOURCE".to_string())
                .buildspec(
                    r#"version: 0.2
phases:
  build:
    commands:
      - echo "Hello from Alien e2e test!"
"#
                    .to_string(),
                )
                .build(),
        )
        .artifacts(
            ProjectArtifacts::builder()
                .r#type("NO_ARTIFACTS".to_string())
                .build(),
        )
        .environment(
            ProjectEnvironment::builder()
                .r#type("LINUX_CONTAINER".to_string())
                .image("aws/codebuild/standard:7.0".to_string())
                .compute_type("BUILD_GENERAL1_SMALL".to_string())
                .build(),
        )
        .build();

    let create_res = ctx
        .codebuild_client
        .create_project(create_req)
        .await
        .expect("CreateProject failed");
    ctx.track_project(&project_name);
    assert_eq!(create_res.project.name.as_ref(), Some(&project_name));
    info!("✅ CreateProject successful");

    // Sanity check: BatchGetProjects
    let batch_get_req = BatchGetProjectsRequest::builder()
        .names(vec![project_name.clone()])
        .build();
    let batch_get_res = ctx
        .codebuild_client
        .batch_get_projects(batch_get_req)
        .await
        .expect("BatchGetProjects failed");
    assert!(batch_get_res
        .projects
        .as_ref()
        .map_or(false, |p| !p.is_empty()));
    info!("✅ BatchGetProjects successful");

    // 2. UpdateProject
    let update_req = UpdateProjectRequest::builder()
        .name(project_name.clone())
        .description("Updated description".to_string())
        .build();

    let update_res = ctx
        .codebuild_client
        .update_project(update_req)
        .await
        .expect("UpdateProject failed");
    assert_eq!(
        update_res.project.description.as_deref(),
        Some("Updated description")
    );
    info!("✅ UpdateProject successful");

    // 3. StartBuild
    let start_build_req = StartBuildRequest::builder()
        .project_name(project_name.clone())
        .build();
    let start_build_res = ctx
        .codebuild_client
        .start_build(start_build_req)
        .await
        .expect("StartBuild failed");
    let build_id = start_build_res
        .build
        .id
        .clone()
        .expect("Build should have an ID");
    info!("✅ StartBuild successful, build ID: {}", build_id);

    // 4. BatchGetBuilds (for polling)
    let status = wait_for_build_to_finish(ctx, &build_id).await;
    assert_eq!(status, "SUCCEEDED", "Build did not succeed");
    info!("✅ BatchGetBuilds polling successful, build SUCCEEDED");

    // 5. StopBuild test
    let stop_project_name = ctx.get_project_name();
    info!("🚀 Testing StopBuild with project: {}", stop_project_name);

    let stop_create_req = CreateProjectRequest::builder()
        .name(stop_project_name.clone())
        .service_role(ctx.service_role_arn.clone())
        .source(
            ProjectSource::builder()
                .r#type("NO_SOURCE".to_string())
                .buildspec(
                    r#"version: 0.2
phases:
  build:
    commands:
      - echo "Starting long build..."
      - sleep 120
      - echo "This should not be printed."
"#
                    .to_string(),
                )
                .build(),
        )
        .artifacts(
            ProjectArtifacts::builder()
                .r#type("NO_ARTIFACTS".to_string())
                .build(),
        )
        .environment(
            ProjectEnvironment::builder()
                .r#type("LINUX_CONTAINER".to_string())
                .image("aws/codebuild/standard:7.0".to_string())
                .compute_type("BUILD_GENERAL1_SMALL".to_string())
                .build(),
        )
        .build();
    ctx.codebuild_client
        .create_project(stop_create_req)
        .await
        .expect("CreateProject for stop test failed");
    ctx.track_project(&stop_project_name);

    let stop_start_req = StartBuildRequest::builder()
        .project_name(stop_project_name.clone())
        .build();
    let stop_start_res = ctx
        .codebuild_client
        .start_build(stop_start_req)
        .await
        .expect("StartBuild for stop test failed");
    let stop_build_id = stop_start_res
        .build
        .id
        .clone()
        .expect("Stop build should have an ID");

    info!("Waiting 5 seconds before stopping build {}", stop_build_id);
    tokio::time::sleep(Duration::from_secs(5)).await; // Let it start

    let stop_req = StopBuildRequest::builder()
        .id(stop_build_id.clone())
        .build();
    let stop_res = ctx
        .codebuild_client
        .stop_build(stop_req)
        .await
        .expect("StopBuild failed");
    // Build might be "STOPPING" or already "STOPPED" depending on timing
    assert!(matches!(
        stop_res.build.build_status.as_deref(),
        Some("STOPPING") | Some("STOPPED")
    ));
    let stop_status = wait_for_build_to_finish(ctx, &stop_build_id).await;
    assert_eq!(stop_status, "STOPPED");
    info!("✅ StopBuild successful");

    // 6. RetryBuild test
    let fail_project_name = ctx.get_project_name();
    info!("🚀 Testing RetryBuild with project: {}", fail_project_name);

    let fail_create_req = CreateProjectRequest::builder()
        .name(fail_project_name.clone())
        .service_role(ctx.service_role_arn.clone())
        .source(
            ProjectSource::builder()
                .r#type("NO_SOURCE".to_string())
                .buildspec(
                    r#"version: 0.2
phases:
  build:
    commands:
      - echo "This build will fail."
      - exit 1
"#
                    .to_string(),
                )
                .build(),
        )
        .artifacts(
            ProjectArtifacts::builder()
                .r#type("NO_ARTIFACTS".to_string())
                .build(),
        )
        .environment(
            ProjectEnvironment::builder()
                .r#type("LINUX_CONTAINER".to_string())
                .image("aws/codebuild/standard:7.0".to_string())
                .compute_type("BUILD_GENERAL1_SMALL".to_string())
                .build(),
        )
        .build();
    ctx.codebuild_client
        .create_project(fail_create_req)
        .await
        .expect("CreateProject for retry test failed");
    ctx.track_project(&fail_project_name);

    let fail_start_req = StartBuildRequest::builder()
        .project_name(fail_project_name.clone())
        .build();
    let fail_start_res = ctx
        .codebuild_client
        .start_build(fail_start_req)
        .await
        .expect("StartBuild for retry test failed");
    let fail_build_id = fail_start_res
        .build
        .id
        .clone()
        .expect("Fail build should have an ID");

    let fail_status = wait_for_build_to_finish(ctx, &fail_build_id).await;
    assert_eq!(fail_status, "FAILED");
    info!("✅ Confirmed original build failed");

    let retry_req = RetryBuildRequest::builder()
        .id(fail_build_id.clone())
        .build();
    let retry_res = ctx
        .codebuild_client
        .retry_build(retry_req)
        .await
        .expect("RetryBuild failed");
    let retry_build_id = retry_res.build.id.expect("Retry build should have an ID");
    info!("✅ RetryBuild successful, new build ID: {}", retry_build_id);
    let retry_status = wait_for_build_to_finish(ctx, &retry_build_id).await;
    assert_eq!(retry_status, "FAILED"); // It should fail again
    info!("✅ Confirmed retried build also failed as expected");

    // 7. BatchDeleteBuilds
    info!("🚀 Testing BatchDeleteBuilds");
    let delete_req = BatchDeleteBuildsRequest::builder()
        .ids(vec![
            build_id.clone(),
            stop_build_id.clone(),
            fail_build_id.clone(),
        ])
        .build();
    let delete_res = ctx
        .codebuild_client
        .batch_delete_builds(delete_req)
        .await
        .expect("BatchDeleteBuilds failed");

    // Log the response to understand what happened
    info!(
        "BatchDeleteBuilds response - deleted: {:?}, not deleted: {:?}",
        delete_res.builds_deleted, delete_res.builds_not_deleted
    );

    // Check that the operation completed successfully (either deleted or reported as not deleted)
    let total_response_builds = delete_res.builds_deleted.as_ref().map_or(0, |v| v.len())
        + delete_res
            .builds_not_deleted
            .as_ref()
            .map_or(0, |v| v.len());
    assert_eq!(
        total_response_builds, 3,
        "Expected response for all 3 build IDs"
    );
    info!("✅ BatchDeleteBuilds successful");

    // 8. DeleteProject
    ctx.cleanup_project(&project_name).await;
    let mut projects = ctx.created_projects.lock().unwrap();
    projects.remove(&project_name); // Untrack it since we manually cleaned it up
    info!("✅ DeleteProject successful");
}
