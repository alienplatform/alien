//! Push model E2E tests.
//!
//! Push deploys to managed cloud compute (Lambda, Cloud Run, Container Apps).
//! Each test starts an in-process manager, pushes a release, creates a
//! deployment, verifies all supported bindings, checks commands via
//! platform-native push (InvokeFunction, Pub/Sub, Service Bus), and destroys.
//!
//! Cleanup is guaranteed by `test-context`: even if a binding check panics,
//! `teardown()` runs and destroys the deployment + cloud resources.
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test push push_aws_rust
//!
//! Run all push tests:
//!   cargo nextest run -p alien-test --test push

use alien_core::Platform;
use alien_test::{DeploymentModel, Language};
use test_context::test_context;

mod common;
use common::e2e_test_context;

// ---------------------------------------------------------------------------
// AWS
// ---------------------------------------------------------------------------

e2e_test_context!(
    AwsPushRust,
    Platform::Aws,
    DeploymentModel::Push,
    Language::Rust
);

#[test_context(AwsPushRust)]
#[tokio::test]
async fn push_aws_rust(ctx: &mut AwsPushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    AwsPushTypeScript,
    Platform::Aws,
    DeploymentModel::Push,
    Language::TypeScript
);

#[test_context(AwsPushTypeScript)]
#[tokio::test]
async fn push_aws_typescript(ctx: &mut AwsPushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// GCP
// ---------------------------------------------------------------------------

e2e_test_context!(
    GcpPushRust,
    Platform::Gcp,
    DeploymentModel::Push,
    Language::Rust
);

#[test_context(GcpPushRust)]
#[tokio::test]
async fn push_gcp_rust(ctx: &mut GcpPushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    GcpPushTypeScript,
    Platform::Gcp,
    DeploymentModel::Push,
    Language::TypeScript
);

#[test_context(GcpPushTypeScript)]
#[tokio::test]
async fn push_gcp_typescript(ctx: &mut GcpPushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

e2e_test_context!(
    AzurePushRust,
    Platform::Azure,
    DeploymentModel::Push,
    Language::Rust
);

#[test_context(AzurePushRust)]
#[tokio::test]
async fn push_azure_rust(ctx: &mut AzurePushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    AzurePushTypeScript,
    Platform::Azure,
    DeploymentModel::Push,
    Language::TypeScript
);

#[test_context(AzurePushTypeScript)]
#[tokio::test]
async fn push_azure_typescript(ctx: &mut AzurePushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.language,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

// Note: Local push tests are not included because the local platform only
// supports pull (container) model through the manager pipeline. Push (function)
// model on local requires `alien dev` which bypasses the deployment pipeline.
