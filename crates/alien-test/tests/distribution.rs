//! Distribution E2E tests.
//!
//! Distribution tests exercise infrastructure artifacts as the initial setup
//! path: CloudFormation for AWS and Terraform for cloud/K8s targets. They use
//! the same application-specific assertions as push/pull E2E.

use alien_test::{DistributionFlow, TestApp};
use anyhow::{anyhow, Context};
use reqwest::{Client, Response};
use serde_json::Value;
use std::collections::BTreeSet;
use test_context::test_context;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

mod common;

async fn check_distribution_deployment(ctx: &mut alien_test::TestContext) {
    match ctx.app {
        TestApp::ComprehensiveRust | TestApp::ComprehensiveTs => {
            if let Err(error) = common::runner::check_all_bindings(
                &ctx.deployment,
                ctx.platform,
                ctx.model,
                ctx.app,
            )
            .await
            {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("binding checks failed: {error:#}");
            }
            if let Err(error) = common::commands::check_commands(&ctx.deployment).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("command checks failed: {error:#}");
            }
        }
        TestApp::FullStackMicroservices => {
            if let Err(error) = check_full_stack_microservices(ctx).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("full-stack microservices checks failed: {error:#}");
            }
        }
    }
}

async fn public_url(ctx: &mut alien_test::TestContext) -> anyhow::Result<String> {
    ctx.deployment
        .wait_for_public_url(Duration::from_secs(180))
        .await
        .map_err(|error| anyhow!("{error}"))
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

async fn expect_json_get_ready(
    client: &Client,
    url: &str,
    label: &str,
    expected_service: &str,
) -> anyhow::Result<Value> {
    let mut last_error = None;

    for attempt in 1..=60 {
        match client.get(url).send().await {
            Ok(response) => match expect_json(response, label).await {
                Ok(value) => {
                    let service = string_field(&value, &["service"], label)?;
                    if service == expected_service {
                        return Ok(value);
                    }
                    last_error = Some(anyhow!(
                        "{label} did not identify {expected_service}; got service {service}"
                    ));
                }
                Err(error) => {
                    last_error = Some(error);
                }
            },
            Err(error) => {
                last_error = Some(error.into());
            }
        }

        if attempt < 60 {
            sleep(Duration::from_secs(5)).await;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("{label} did not become ready")))
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

async fn check_full_stack_microservices(ctx: &mut alien_test::TestContext) -> anyhow::Result<()> {
    let url = public_url(ctx).await?;
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    expect_json_get_ready(
        &client,
        &format!("{url}/health"),
        "gateway health",
        "gateway",
    )
    .await?;

    expect_json_get_ready(&client, &format!("{url}/api/health"), "api health", "api").await?;

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
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::new())
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

async fn dump_kubernetes_debug(ctx: &alien_test::TestContext, error: &anyhow::Error) {
    let Some((namespace, kubeconfig, kube_context, env)) = ctx
        .distribution_cleanups
        .iter()
        .find_map(|cleanup| match cleanup {
            alien_test::distribution::DistributionArtifactCleanup::Helm {
                namespace,
                kubeconfig,
                kube_context,
                env,
                ..
            } => Some((
                namespace.as_str(),
                kubeconfig.as_deref(),
                kube_context.as_deref(),
                env.as_slice(),
            )),
            _ => None,
        })
    else {
        return;
    };

    eprintln!("\n--- Kubernetes debug for namespace {namespace}; check failure: {error:#} ---");

    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods,svc,ingress,serviceaccount,role,rolebinding",
            "-o",
            "wide",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.metadata.name}{\" serviceAccount=\"}{.spec.serviceAccountName}{\" phase=\"}{.status.phase}{\" node=\"}{.spec.nodeName}{\"\\n\"}{end}",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "serviceaccount", "-o", "yaml"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "role,rolebinding", "-o", "yaml"],
    )
    .await;
    dump_service_account_auth(namespace, kubeconfig, kube_context, env).await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["describe", "pods"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "events", "--sort-by=.lastTimestamp"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "gateway,httproute,healthcheckpolicy", "-o", "yaml"],
    )
    .await;
    dump_pod_logs(namespace, kubeconfig, kube_context, env).await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "managed-by=runtime",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "app=alien-rs-worker",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "app.kubernetes.io/name=alien-e2e-comprehensive-rust",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    eprintln!("--- End Kubernetes debug for namespace {namespace} ---\n");
}

async fn dump_pod_logs(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
) {
    let output = run_kubectl_capture(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.metadata.name}{\"\\n\"}{end}",
        ],
    )
    .await;

    for pod_name in output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let pod_ref = format!("pod/{pod_name}");
        run_kubectl_debug(
            namespace,
            kubeconfig,
            kube_context,
            env,
            &[
                "logs",
                &pod_ref,
                "--all-containers",
                "--tail=500",
                "--prefix",
            ],
        )
        .await;
    }
}

async fn dump_service_account_auth(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
) {
    let output = run_kubectl_capture(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.spec.serviceAccountName}{\"\\n\"}{end}",
        ],
    )
    .await;

    let service_accounts = output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();

    for service_account in service_accounts {
        let subject = format!("system:serviceaccount:{namespace}:{service_account}");
        for (verb, resource) in [
            ("get", "secrets"),
            ("create", "secrets"),
            ("update", "secrets"),
            ("delete", "secrets"),
            ("create", "jobs.batch"),
        ] {
            run_kubectl_debug(
                namespace,
                kubeconfig,
                kube_context,
                env,
                &["auth", "can-i", verb, resource, "--as", &subject],
            )
            .await;
        }
    }
}

async fn run_kubectl_capture(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) -> String {
    let mut cmd = kubectl_debug_command(namespace, kubeconfig, kube_context, env, args);
    match cmd.output().await {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).into(),
        Ok(output) => {
            eprintln!("$ kubectl -n {namespace} {}", args.join(" "));
            if !output.stdout.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            eprintln!("kubectl exited with {}", output.status);
            String::new()
        }
        Err(error) => {
            eprintln!(
                "failed to run kubectl -n {namespace} {}: {error}",
                args.join(" ")
            );
            String::new()
        }
    }
}

async fn run_kubectl_debug(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) {
    let mut cmd = kubectl_debug_command(namespace, kubeconfig, kube_context, env, args);

    match cmd.output().await {
        Ok(output) => {
            eprintln!("$ kubectl -n {namespace} {}", args.join(" "));
            if !output.stdout.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            if !output.status.success() {
                eprintln!("kubectl exited with {}", output.status);
            }
        }
        Err(error) => {
            eprintln!(
                "failed to run kubectl -n {namespace} {}: {error}",
                args.join(" ")
            );
        }
    }
}

fn kubectl_debug_command(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) -> Command {
    let mut cmd = Command::new("kubectl");
    cmd.args(["-n", namespace]);
    cmd.args(args);
    cmd.envs(env.iter().map(|(key, value)| (key, value)));
    if let Some(kubeconfig) = kubeconfig {
        cmd.env("KUBECONFIG", kubeconfig);
    }
    if let Some(kube_context) = kube_context {
        cmd.args(["--context", kube_context]);
    }
    cmd
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

distribution_test_context!(
    CloudFormationEksHelmPullFullStackMicroservices,
    DistributionFlow::CloudFormationEksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(CloudFormationEksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn cloudformation_eks_helm_pull_full_stack_microservices(
    ctx: &mut CloudFormationEksHelmPullFullStackMicroservices,
) {
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
