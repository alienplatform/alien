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
//!   cargo nextest run -p alien-test --test push push_aws_comprehensive_rust
//!
//! Run all push tests:
//!   cargo nextest run -p alien-test --test push

use alien_core::Platform;
use alien_test::{DeploymentModel, TestApp};
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
    TestApp::ComprehensiveRust
);

#[test_context(AwsPushRust)]
#[tokio::test]
async fn push_aws_comprehensive_rust(ctx: &mut AwsPushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
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
    TestApp::ComprehensiveTs
);

#[test_context(AwsPushTypeScript)]
#[tokio::test]
async fn push_aws_comprehensive_ts(ctx: &mut AwsPushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    AwsPushTcpTransaction,
    Platform::Aws,
    DeploymentModel::Push,
    TestApp::TcpTransaction
);

#[test_context(AwsPushTcpTransaction)]
#[tokio::test]
async fn push_aws_tcp_transaction(ctx: &mut AwsPushTcpTransaction) {
    common::tcp::check_tcp_transaction(&ctx.ctx.deployment)
        .await
        .expect("TCP transaction checks failed");
}

// ---------------------------------------------------------------------------
// GCP
// ---------------------------------------------------------------------------

e2e_test_context!(
    GcpPushRust,
    Platform::Gcp,
    DeploymentModel::Push,
    TestApp::ComprehensiveRust
);

#[test_context(GcpPushRust)]
#[tokio::test]
async fn push_gcp_comprehensive_rust(ctx: &mut GcpPushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
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
    TestApp::ComprehensiveTs
);

#[test_context(GcpPushTypeScript)]
#[tokio::test]
async fn push_gcp_comprehensive_ts(ctx: &mut GcpPushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    GcpPushTcpTransaction,
    Platform::Gcp,
    DeploymentModel::Push,
    TestApp::TcpTransaction
);

#[test_context(GcpPushTcpTransaction)]
#[tokio::test]
async fn push_gcp_tcp_transaction(ctx: &mut GcpPushTcpTransaction) {
    common::tcp::check_tcp_transaction(&ctx.ctx.deployment)
        .await
        .expect("TCP transaction checks failed");
}

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

e2e_test_context!(
    AzurePushRust,
    Platform::Azure,
    DeploymentModel::Push,
    TestApp::ComprehensiveRust
);

#[test_context(AzurePushRust)]
#[tokio::test]
async fn push_azure_comprehensive_rust(ctx: &mut AzurePushRust) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
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
    TestApp::ComprehensiveTs
);

#[test_context(AzurePushTypeScript)]
#[tokio::test]
async fn push_azure_comprehensive_ts(ctx: &mut AzurePushTypeScript) {
    common::runner::check_all_bindings(
        &ctx.ctx.deployment,
        ctx.ctx.platform,
        ctx.ctx.model,
        ctx.ctx.app,
    )
    .await
    .expect("binding checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

e2e_test_context!(
    AzurePushTcpTransaction,
    Platform::Azure,
    DeploymentModel::Push,
    TestApp::TcpTransaction
);

#[test_context(AzurePushTcpTransaction)]
#[tokio::test]
async fn push_azure_tcp_transaction(ctx: &mut AzurePushTcpTransaction) {
    common::tcp::check_tcp_transaction(&ctx.ctx.deployment)
        .await
        .expect("TCP transaction checks failed");
}

// Note: Local push tests are not included because the local platform only
// supports pull (container) model through the manager pipeline. Push (worker)
// model on local requires `alien dev` which bypasses the deployment pipeline.
