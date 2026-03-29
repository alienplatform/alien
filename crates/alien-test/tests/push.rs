//! Push model E2E tests.
//!
//! Push deploys to managed cloud compute (Lambda, Cloud Run, Container Apps).
//! Each test starts an in-process manager, pushes a release, creates a
//! deployment, verifies all supported bindings, and destroys.
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test push push_aws_rust
//!
//! Run all push tests:
//!   cargo nextest run -p alien-test --test push

use alien_core::Platform;
use alien_test::{DeploymentModel, Language};

mod common;

// ---------------------------------------------------------------------------
// AWS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn push_aws_rust() {
    let ctx = alien_test::e2e::setup(Platform::Aws, DeploymentModel::Push, Language::Rust)
        .await
        .expect("AWS push Rust E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("AWS push Rust binding checks failed");
    // Commands use Pull model in OSS standalone — the Lambda runtime polls
    // ALIEN_COMMANDS_POLLING_URL which points to localhost, unreachable from AWS.
    // Command tests are covered by pull tests where the deployment is local.
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("AWS push Rust destroy failed");
}

#[tokio::test]
async fn push_aws_typescript() {
    let ctx = alien_test::e2e::setup(Platform::Aws, DeploymentModel::Push, Language::TypeScript)
        .await
        .expect("AWS push TypeScript E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("AWS push TypeScript binding checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("AWS push TypeScript destroy failed");
}

// ---------------------------------------------------------------------------
// GCP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn push_gcp_rust() {
    let ctx = alien_test::e2e::setup(Platform::Gcp, DeploymentModel::Push, Language::Rust)
        .await
        .expect("GCP push Rust E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("GCP push Rust binding checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("GCP push Rust destroy failed");
}

#[tokio::test]
async fn push_gcp_typescript() {
    let ctx = alien_test::e2e::setup(Platform::Gcp, DeploymentModel::Push, Language::TypeScript)
        .await
        .expect("GCP push TypeScript E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("GCP push TypeScript binding checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("GCP push TypeScript destroy failed");
}

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn push_azure_rust() {
    let ctx = alien_test::e2e::setup(Platform::Azure, DeploymentModel::Push, Language::Rust)
        .await
        .expect("Azure push Rust E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Azure push Rust binding checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Azure push Rust destroy failed");
}

#[tokio::test]
async fn push_azure_typescript() {
    let ctx =
        alien_test::e2e::setup(Platform::Azure, DeploymentModel::Push, Language::TypeScript)
            .await
            .expect("Azure push TypeScript E2E setup failed");
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model)
        .await
        .expect("Azure push TypeScript binding checks failed");
    common::lifecycle::check_destroy(&ctx.deployment)
        .await
        .expect("Azure push TypeScript destroy failed");
}

// Note: Local push tests are not included because the local platform only
// supports pull (container) model through the manager pipeline. Push (function)
// model on local requires `alien dev` which bypasses the deployment pipeline.
