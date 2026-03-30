//! Pull model E2E tests.
//!
//! Pull deployments run the alien-agent container which pulls work from the
//! manager. For Kubernetes, the agent is installed via Helm. For local, the
//! agent runs in Docker.
//!
//! Cloud platform (AWS/GCP/Azure) pull tests run the alien-agent as a Docker
//! container with target account credentials. The agent deploys serverless
//! functions directly (no cross-account IAM) — same stack as push mode but
//! driven by the agent instead of the manager.
//!
//! Cleanup is guaranteed by `test-context`: even if a binding check panics,
//! `teardown()` runs and destroys the deployment + agent.
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test pull pull_kubernetes_rust
//!
//! Run all pull tests:
//!   cargo nextest run -p alien-test --test pull

use alien_core::Platform;
use alien_test::{DeploymentModel, Language};
use test_context::test_context;

mod common;
use common::e2e_test_context;

// ---------------------------------------------------------------------------
// AWS (Docker agent with target credentials)
// ---------------------------------------------------------------------------

e2e_test_context!(AwsPullRust, Platform::Aws, DeploymentModel::Pull, Language::Rust);

#[test_context(AwsPullRust)]
#[tokio::test]
async fn pull_aws_rust(ctx: &mut AwsPullRust) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(AwsPullTypeScript, Platform::Aws, DeploymentModel::Pull, Language::TypeScript);

#[test_context(AwsPullTypeScript)]
#[tokio::test]
async fn pull_aws_typescript(ctx: &mut AwsPullTypeScript) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// GCP (Docker agent with target credentials)
// ---------------------------------------------------------------------------

e2e_test_context!(GcpPullRust, Platform::Gcp, DeploymentModel::Pull, Language::Rust);

#[test_context(GcpPullRust)]
#[tokio::test]
async fn pull_gcp_rust(ctx: &mut GcpPullRust) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(GcpPullTypeScript, Platform::Gcp, DeploymentModel::Pull, Language::TypeScript);

#[test_context(GcpPullTypeScript)]
#[tokio::test]
async fn pull_gcp_typescript(ctx: &mut GcpPullTypeScript) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// Azure (Docker agent with target credentials)
// ---------------------------------------------------------------------------

e2e_test_context!(AzurePullRust, Platform::Azure, DeploymentModel::Pull, Language::Rust);

#[test_context(AzurePullRust)]
#[tokio::test]
async fn pull_azure_rust(ctx: &mut AzurePullRust) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(AzurePullTypeScript, Platform::Azure, DeploymentModel::Pull, Language::TypeScript);

#[test_context(AzurePullTypeScript)]
#[tokio::test]
async fn pull_azure_typescript(ctx: &mut AzurePullTypeScript) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// Kubernetes (Helm install)
// ---------------------------------------------------------------------------

e2e_test_context!(K8sPullRust, Platform::Kubernetes, DeploymentModel::Pull, Language::Rust);

#[test_context(K8sPullRust)]
#[tokio::test]
async fn pull_kubernetes_rust(ctx: &mut K8sPullRust) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(K8sPullTypeScript, Platform::Kubernetes, DeploymentModel::Pull, Language::TypeScript);

#[test_context(K8sPullTypeScript)]
#[tokio::test]
async fn pull_kubernetes_typescript(ctx: &mut K8sPullTypeScript) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// Local (Docker containers)
// ---------------------------------------------------------------------------

e2e_test_context!(LocalPullRust, Platform::Local, DeploymentModel::Pull, Language::Rust);

#[test_context(LocalPullRust)]
#[tokio::test]
async fn pull_local_rust(ctx: &mut LocalPullRust) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(LocalPullTypeScript, Platform::Local, DeploymentModel::Pull, Language::TypeScript);

#[test_context(LocalPullTypeScript)]
#[tokio::test]
async fn pull_local_typescript(ctx: &mut LocalPullTypeScript) {
    common::runner::check_all_bindings(&ctx.ctx.deployment, ctx.ctx.platform, ctx.ctx.model)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}
