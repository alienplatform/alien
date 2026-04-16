#![cfg(all(test, feature = "gcp"))]

use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::gcp::cloudscheduler::{
    CloudSchedulerApi, CloudSchedulerClient, HttpTarget, SchedulerJob, SchedulerOidcToken,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct CloudSchedulerTestContext {
    client: CloudSchedulerClient,
    project_id: String,
    location: String,
    service_account_email: String,
    created_jobs: Mutex<HashSet<String>>,
}

impl AsyncTestContext for CloudSchedulerTestContext {
    async fn setup() -> CloudSchedulerTestContext {
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

        let service_account_email = service_account_value
            .get("client_email")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'client_email' must be present in the service account JSON");

        let location = "us-central1".to_string();

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: location.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = CloudSchedulerClient::new(Client::new(), config);

        CloudSchedulerTestContext {
            client,
            project_id,
            location,
            service_account_email,
            created_jobs: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("Starting Cloud Scheduler test cleanup...");

        let jobs_to_cleanup = {
            let jobs = self.created_jobs.lock().unwrap();
            jobs.clone()
        };

        for job_name in jobs_to_cleanup {
            self.cleanup_job(&job_name).await;
        }

        info!("Cloud Scheduler test cleanup completed");
    }
}

impl CloudSchedulerTestContext {
    fn track_job(&self, job_name: &str) {
        let mut jobs = self.created_jobs.lock().unwrap();
        jobs.insert(job_name.to_string());
        info!("Tracking job for cleanup: {}", job_name);
    }

    fn untrack_job(&self, job_name: &str) {
        let mut jobs = self.created_jobs.lock().unwrap();
        jobs.remove(job_name);
        info!("Job {} successfully cleaned up and untracked", job_name);
    }

    async fn cleanup_job(&self, job_name: &str) {
        match self.client.delete_job(job_name.to_string()).await {
            Ok(_) => {
                info!("Successfully deleted job: {}", job_name);
                self.untrack_job(job_name);
            }
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(
                    "Job {} not found during cleanup (already deleted)",
                    job_name
                );
                self.untrack_job(job_name);
            }
            Err(e) => {
                warn!("Failed to delete job {}: {}", job_name, e);
            }
        }
    }

    fn generate_unique_job_id(&self) -> String {
        format!("alien-test-scheduler-{}", Uuid::new_v4().simple())
    }

    fn job_name(&self, job_id: &str) -> String {
        format!(
            "projects/{}/locations/{}/jobs/{}",
            self.project_id, self.location, job_id
        )
    }
}

#[test_context(CloudSchedulerTestContext)]
#[tokio::test]
async fn test_create_job(ctx: &CloudSchedulerTestContext) {
    info!("Starting create job test");

    let job_id = ctx.generate_unique_job_id();
    let job_name = ctx.job_name(&job_id);
    info!("Using job ID: {}", job_id);

    let job = SchedulerJob {
        name: None,
        description: Some("Test job created by alien integration tests".to_string()),
        schedule: "0 */6 * * *".to_string(),
        time_zone: Some("America/New_York".to_string()),
        http_target: Some(HttpTarget {
            uri: "https://httpbin.org/post".to_string(),
            http_method: Some("POST".to_string()),
            body: None,
            headers: None,
            oidc_token: None,
        }),
        state: None,
    };

    let created_job = ctx
        .client
        .create_job(ctx.location.clone(), job_id.clone(), job)
        .await
        .expect("Failed to create Cloud Scheduler job");

    ctx.track_job(&job_name);

    assert!(created_job.name.is_some(), "created job must have a name");
    assert!(
        created_job.name.as_ref().unwrap().contains(&job_id),
        "job name must contain the job ID"
    );
    assert_eq!(
        created_job.description,
        Some("Test job created by alien integration tests".to_string())
    );
    assert_eq!(created_job.schedule, "0 */6 * * *");
    assert_eq!(
        created_job.time_zone,
        Some("America/New_York".to_string())
    );
    assert!(created_job.http_target.is_some(), "job must have an HTTP target");

    let http_target = created_job.http_target.unwrap();
    assert_eq!(http_target.uri, "https://httpbin.org/post");
    assert_eq!(http_target.http_method, Some("POST".to_string()));

    assert!(
        created_job.state.is_some(),
        "created job must have a state"
    );
    assert_eq!(
        created_job.state.as_deref(),
        Some("ENABLED"),
        "newly created job should be in ENABLED state"
    );

    info!("Create job test completed successfully");
}

#[test_context(CloudSchedulerTestContext)]
#[tokio::test]
async fn test_get_job(ctx: &CloudSchedulerTestContext) {
    info!("Starting get job test");

    let job_id = ctx.generate_unique_job_id();
    let job_name = ctx.job_name(&job_id);
    info!("Using job ID: {}", job_id);

    let job = SchedulerJob {
        name: None,
        description: Some("Get test job".to_string()),
        schedule: "30 8 * * 1".to_string(),
        time_zone: Some("UTC".to_string()),
        http_target: Some(HttpTarget {
            uri: "https://httpbin.org/get".to_string(),
            http_method: Some("GET".to_string()),
            body: None,
            headers: None,
            oidc_token: None,
        }),
        state: None,
    };

    let created_job = ctx
        .client
        .create_job(ctx.location.clone(), job_id.clone(), job)
        .await
        .expect("Failed to create Cloud Scheduler job for get test");

    ctx.track_job(&job_name);

    let retrieved_job = ctx
        .client
        .get_job(job_name.clone())
        .await
        .expect("Failed to get Cloud Scheduler job");

    assert_eq!(created_job.name, retrieved_job.name);
    assert_eq!(created_job.description, retrieved_job.description);
    assert_eq!(created_job.schedule, retrieved_job.schedule);
    assert_eq!(created_job.time_zone, retrieved_job.time_zone);
    assert_eq!(created_job.state, retrieved_job.state);

    assert!(retrieved_job.http_target.is_some(), "retrieved job must have an HTTP target");
    let created_target = created_job.http_target.unwrap();
    let retrieved_target = retrieved_job.http_target.unwrap();
    assert_eq!(created_target.uri, retrieved_target.uri);
    assert_eq!(created_target.http_method, retrieved_target.http_method);

    info!("Get job test completed successfully");
}

#[test_context(CloudSchedulerTestContext)]
#[tokio::test]
async fn test_delete_job(ctx: &CloudSchedulerTestContext) {
    info!("Starting delete job test");

    let job_id = ctx.generate_unique_job_id();
    let job_name = ctx.job_name(&job_id);
    info!("Using job ID: {}", job_id);

    let job = SchedulerJob {
        name: None,
        description: Some("Job to be deleted".to_string()),
        schedule: "0 0 * * *".to_string(),
        time_zone: Some("UTC".to_string()),
        http_target: Some(HttpTarget {
            uri: "https://httpbin.org/post".to_string(),
            http_method: Some("POST".to_string()),
            body: None,
            headers: None,
            oidc_token: None,
        }),
        state: None,
    };

    ctx.client
        .create_job(ctx.location.clone(), job_id.clone(), job)
        .await
        .expect("Failed to create Cloud Scheduler job for delete test");

    ctx.track_job(&job_name);

    // Delete the job
    ctx.client
        .delete_job(job_name.clone())
        .await
        .expect("Failed to delete Cloud Scheduler job");

    ctx.untrack_job(&job_name);

    // Verify the job is gone
    let get_result = ctx.client.get_job(job_name.clone()).await;
    assert!(
        matches!(
            get_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        ),
        "getting a deleted job should return RemoteResourceNotFound"
    );

    info!("Delete job test completed successfully");
}

#[test_context(CloudSchedulerTestContext)]
#[tokio::test]
async fn test_delete_nonexistent_job(ctx: &CloudSchedulerTestContext) {
    info!("Starting delete nonexistent job test");

    let job_id = ctx.generate_unique_job_id();
    let job_name = ctx.job_name(&job_id);

    let delete_result = ctx.client.delete_job(job_name.clone()).await;
    assert!(
        matches!(
            delete_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        ),
        "deleting a nonexistent job should return RemoteResourceNotFound"
    );

    info!("Delete nonexistent job test completed successfully");
}

#[test_context(CloudSchedulerTestContext)]
#[tokio::test]
async fn test_create_job_with_oidc_auth(ctx: &CloudSchedulerTestContext) {
    info!("Starting create job with OIDC auth test");

    let job_id = ctx.generate_unique_job_id();
    let job_name = ctx.job_name(&job_id);
    info!("Using job ID: {}", job_id);

    let job = SchedulerJob {
        name: None,
        description: Some("Job with OIDC authentication".to_string()),
        schedule: "0 12 * * *".to_string(),
        time_zone: Some("UTC".to_string()),
        http_target: Some(HttpTarget {
            uri: "https://httpbin.org/post".to_string(),
            http_method: Some("POST".to_string()),
            body: None,
            headers: None,
            oidc_token: Some(SchedulerOidcToken {
                service_account_email: ctx.service_account_email.clone(),
                audience: Some("https://httpbin.org".to_string()),
            }),
        }),
        state: None,
    };

    let created_job = ctx
        .client
        .create_job(ctx.location.clone(), job_id.clone(), job)
        .await
        .expect("Failed to create Cloud Scheduler job with OIDC auth");

    ctx.track_job(&job_name);

    assert!(created_job.name.is_some(), "created job must have a name");
    assert!(
        created_job.name.as_ref().unwrap().contains(&job_id),
        "job name must contain the job ID"
    );
    assert_eq!(
        created_job.description,
        Some("Job with OIDC authentication".to_string())
    );
    assert_eq!(created_job.schedule, "0 12 * * *");

    assert!(created_job.http_target.is_some(), "job must have an HTTP target");
    let http_target = created_job.http_target.unwrap();
    assert_eq!(http_target.uri, "https://httpbin.org/post");
    assert_eq!(http_target.http_method, Some("POST".to_string()));

    assert!(
        http_target.oidc_token.is_some(),
        "HTTP target must have an OIDC token configuration"
    );
    let oidc_token = http_target.oidc_token.unwrap();
    assert_eq!(
        oidc_token.service_account_email,
        ctx.service_account_email,
        "OIDC token service account email must match"
    );
    assert_eq!(
        oidc_token.audience,
        Some("https://httpbin.org".to_string()),
        "OIDC token audience must match"
    );

    info!("Create job with OIDC auth test completed successfully");
}
