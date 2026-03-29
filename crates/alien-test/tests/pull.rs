//! Pull model E2E tests.
//!
//! Pull deployments run the alien-agent container which pulls work from the
//! manager. For Kubernetes, the agent is installed via Helm. For local, the
//! agent runs in Docker.
//!
//! Note: Cloud platform (AWS/GCP/Azure) pull tests are not included because
//! containers on cloud platforms require Horizon clusters for orchestration,
//! which are provisioned by the alien.dev platform. Standalone managers used
//! in E2E tests do not have Horizon infrastructure.
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test pull pull_kubernetes_rust
//!
//! Run all pull tests:
//!   cargo nextest run -p alien-test --test pull

use alien_core::Platform;
use alien_test::{DeploymentModel, Language};

mod common;

// ---------------------------------------------------------------------------
// Kubernetes (Helm install)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pull_kubernetes_rust() {
    let ctx =
        alien_test::e2e::setup(Platform::Kubernetes, DeploymentModel::Pull, Language::Rust)
            .await
            .expect("Kubernetes pull Rust E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Kubernetes pull Rust binding checks failed");
    common::commands::check_commands(&ctx.deployment)
        .await
        .expect("Kubernetes pull Rust command checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Kubernetes pull Rust destroy failed");
}

#[tokio::test]
async fn pull_kubernetes_typescript() {
    let ctx = alien_test::e2e::setup(
        Platform::Kubernetes,
        DeploymentModel::Pull,
        Language::TypeScript,
    )
    .await
    .expect("Kubernetes pull TypeScript E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Kubernetes pull TypeScript binding checks failed");
    common::commands::check_commands(&ctx.deployment)
        .await
        .expect("Kubernetes pull TypeScript command checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Kubernetes pull TypeScript destroy failed");
}

// ---------------------------------------------------------------------------
// Local (Docker containers)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pull_local_rust() {
    let ctx = alien_test::e2e::setup(Platform::Local, DeploymentModel::Pull, Language::Rust)
        .await
        .expect("Local pull Rust E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Local pull Rust binding checks failed");
    common::commands::check_commands(&ctx.deployment)
        .await
        .expect("Local pull Rust command checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Local pull Rust destroy failed");
}

#[tokio::test]
async fn pull_local_typescript() {
    let ctx =
        alien_test::e2e::setup(Platform::Local, DeploymentModel::Pull, Language::TypeScript)
            .await
            .expect("Local pull TypeScript E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Local pull TypeScript binding checks failed");
    common::commands::check_commands(&ctx.deployment)
        .await
        .expect("Local pull TypeScript command checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Local pull TypeScript destroy failed");
}
