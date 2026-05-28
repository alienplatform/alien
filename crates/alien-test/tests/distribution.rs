//! Distribution E2E tests.
//!
//! Distribution tests exercise infrastructure artifacts as the initial setup
//! path: CloudFormation for AWS and Terraform for cloud/K8s targets. They use
//! the same application-specific assertions as push/pull E2E.

use alien_test::{DistributionFlow, TestApp};
use anyhow::{anyhow, Context};
use reqwest::{Client, Response};
use serde_json::Value;
use test_context::test_context;
use tokio::time::{sleep, Duration};

mod common;

async fn check_distribution_deployment(ctx: &mut alien_test::TestContext) {
    match ctx.app {
        TestApp::ComprehensiveRust | TestApp::ComprehensiveTs => {
            common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model, ctx.app)
                .await
                .expect("binding checks failed");
            common::commands::check_commands(&ctx.deployment)
                .await
                .expect("command checks failed");
        }
        TestApp::FullStackMicroservices => check_full_stack_microservices(ctx)
            .await
            .expect("full-stack microservices checks failed"),
    }
}

fn public_url(ctx: &alien_test::TestContext) -> anyhow::Result<String> {
    Ok(ctx
        .deployment
        .url
        .as_deref()
        .context("full-stack microservices deployment did not expose a public URL")?
        .trim_end_matches('/')
        .to_string())
}

async fn expect_json(response: Response, label: &str) -> anyhow::Result<Value> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("{label} response body could not be read"))?;
    if !status.is_success() {
        return Err(anyhow!("{label} failed with HTTP {status}: {body}"));
    }
    serde_json::from_str(&body).with_context(|| format!("{label} returned invalid JSON: {body}"))
}

fn string_field<'a>(value: &'a Value, path: &[&str], label: &str) -> anyhow::Result<&'a str> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor
            .get(*segment)
            .with_context(|| format!("{label} missing field {}", path.join(".")))?;
    }
    cursor
        .as_str()
        .with_context(|| format!("{label} field {} was not a string", path.join(".")))
}

async fn check_full_stack_microservices(ctx: &alien_test::TestContext) -> anyhow::Result<()> {
    let url = public_url(ctx)?;
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let gateway_health = expect_json(
        client.get(format!("{url}/health")).send().await?,
        "gateway health",
    )
    .await?;
    if string_field(&gateway_health, &["service"], "gateway health")? != "gateway" {
        return Err(anyhow!(
            "gateway health did not identify the gateway service"
        ));
    }

    let api_health = expect_json(
        client.get(format!("{url}/api/health")).send().await?,
        "api health",
    )
    .await?;
    if string_field(&api_health, &["service"], "api health")? != "api" {
        return Err(anyhow!(
            "gateway-to-api route did not reach the api service"
        ));
    }

    let issue_payload = serde_json::json!({
        "title": "Kubernetes E2E issue",
        "body": "Created through the public gateway and stored in Postgres."
    });
    let created_issue = expect_json(
        client
            .post(format!("{url}/api/issues"))
            .json(&issue_payload)
            .send()
            .await?,
        "issue creation",
    )
    .await?;
    let issue_id = string_field(&created_issue, &["issue", "id"], "issue creation")?;

    let issues = expect_json(
        client.get(format!("{url}/api/issues")).send().await?,
        "issue list",
    )
    .await?;
    let listed = issues
        .get("issues")
        .and_then(Value::as_array)
        .context("issue list did not return an issues array")?
        .iter()
        .any(|issue| issue.get("id").and_then(Value::as_str) == Some(issue_id));
    if !listed {
        return Err(anyhow!(
            "created issue {issue_id} was not returned by the Postgres-backed issue list"
        ));
    }

    let file_content = "runtime object storage write/read from full-stack Kubernetes E2E";
    let uploaded_file = expect_json(
        client
            .post(format!("{url}/api/issues/{issue_id}/files"))
            .json(&serde_json::json!({
                "filename": "e2e.txt",
                "content": file_content
            }))
            .send()
            .await?,
        "file upload",
    )
    .await?;
    let file_id = string_field(&uploaded_file, &["file", "id"], "file upload")?;

    let fetched_file = expect_json(
        client
            .get(format!("{url}/api/files/{file_id}"))
            .send()
            .await?,
        "file download",
    )
    .await?;
    if string_field(&fetched_file, &["content"], "file download")? != file_content {
        return Err(anyhow!(
            "downloaded file content did not match the object storage upload"
        ));
    }

    expect_json(
        client
            .post(format!("{url}/api/issues/{issue_id}/process"))
            .send()
            .await?,
        "worker enqueue",
    )
    .await?;

    let mut last_issue = None;
    for _ in 0..30 {
        let issue = expect_json(
            client
                .get(format!("{url}/api/issues/{issue_id}"))
                .send()
                .await?,
            "worker status",
        )
        .await?;
        let issue_status = issue
            .get("issue")
            .and_then(|issue| issue.get("status"))
            .and_then(Value::as_str);
        let job_status = issue
            .get("job")
            .and_then(|job| job.get("status"))
            .and_then(Value::as_str);
        let artifact_key = issue
            .get("job")
            .and_then(|job| job.get("artifactKey"))
            .and_then(Value::as_str);

        if issue_status == Some("processed")
            && job_status == Some("processed")
            && artifact_key.is_some()
        {
            return Ok(());
        }

        last_issue = Some(issue);
        sleep(Duration::from_secs(2)).await;
    }

    Err(anyhow!(
        "worker did not process issue {issue_id}; last status: {}",
        last_issue
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    ))
}

macro_rules! distribution_test_context {
    ($name:ident, $flow:expr, $app:expr) => {
        struct $name {
            ctx: alien_test::TestContext,
        }

        impl test_context::AsyncTestContext for $name {
            async fn setup() -> Self {
                alien_test::e2e::init_tracing();
                let ctx = alien_test::e2e::setup_distribution($flow, $app)
                    .await
                    .expect(concat!(stringify!($name), " setup failed"));
                Self { ctx }
            }

            async fn teardown(self) {
                self.ctx.cleanup().await;
            }
        }
    };
}

// ---------------------------------------------------------------------------
// CloudFormation
// ---------------------------------------------------------------------------

distribution_test_context!(
    CloudFormationAwsPushRust,
    DistributionFlow::CloudFormationAwsPush,
    TestApp::ComprehensiveRust
);

#[test_context(CloudFormationAwsPushRust)]
#[tokio::test]
async fn cloudformation_aws_push_comprehensive_rust(ctx: &mut CloudFormationAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// ---------------------------------------------------------------------------
// Terraform push
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformAwsPushRust,
    DistributionFlow::TerraformAwsPush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAwsPushRust)]
#[tokio::test]
async fn terraform_aws_push_comprehensive_rust(ctx: &mut TerraformAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGcpPushRust,
    DistributionFlow::TerraformGcpPush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformGcpPushRust)]
#[tokio::test]
async fn terraform_gcp_push_comprehensive_rust(ctx: &mut TerraformGcpPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAzurePushRust,
    DistributionFlow::TerraformAzurePush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAzurePushRust)]
#[tokio::test]
async fn terraform_azure_push_comprehensive_rust(ctx: &mut TerraformAzurePushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// ---------------------------------------------------------------------------
// Terraform + Helm pull
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformEksHelmPullRust,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformEksHelmPullRust)]
#[tokio::test]
async fn terraform_eks_helm_pull_comprehensive_rust(ctx: &mut TerraformEksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformEksHelmPullFullStackMicroservices,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformEksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_eks_helm_pull_full_stack_microservices(
    ctx: &mut TerraformEksHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullRust,
    DistributionFlow::TerraformGkeHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformGkeHelmPullRust)]
#[tokio::test]
async fn terraform_gke_helm_pull_comprehensive_rust(ctx: &mut TerraformGkeHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullFullStackMicroservices,
    DistributionFlow::TerraformGkeHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformGkeHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_gke_helm_pull_full_stack_microservices(
    ctx: &mut TerraformGkeHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullRust,
    DistributionFlow::TerraformAksHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAksHelmPullRust)]
#[tokio::test]
async fn terraform_aks_helm_pull_comprehensive_rust(ctx: &mut TerraformAksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullFullStackMicroservices,
    DistributionFlow::TerraformAksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformAksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_aks_helm_pull_full_stack_microservices(
    ctx: &mut TerraformAksHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformOnpremHelmPullRust,
    DistributionFlow::TerraformOnpremHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformOnpremHelmPullRust)]
#[tokio::test]
#[ignore = "on-prem Helm local-import needs a complete external binding fixture for comprehensive-rust"]
async fn terraform_onprem_helm_pull_comprehensive_rust(ctx: &mut TerraformOnpremHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}
