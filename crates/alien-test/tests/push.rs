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

/// A source-built Live sandbox set up directly: a session runs a command, only management writes
/// the image-build bundle, the bundle lives in the deployment's own bucket, and the bucket goes
/// with the deployment.
struct AwsPushSandboxSession {
    ctx: alien_test::TestContext,
    bundle_bucket: Option<String>,
}

impl test_context::AsyncTestContext for AwsPushSandboxSession {
    async fn setup() -> Self {
        alien_test::e2e::init_tracing();
        let ctx = alien_test::e2e::setup(
            Platform::Aws,
            DeploymentModel::Push,
            TestApp::SandboxSession,
        )
        .await
        .expect("AwsPushSandboxSession setup failed");
        Self {
            ctx,
            bundle_bucket: None,
        }
    }

    async fn teardown(self) {
        self.ctx
            .cleanup()
            .await
            .expect("E2E cleanup must reach a safe terminal state");
        if let Some(bucket) = self.bundle_bucket {
            common::sandbox::check_bundle_bucket_deleted(&bucket)
                .await
                .expect("the bundle bucket goes with the deployment");
        }
    }
}

#[test_context(AwsPushSandboxSession)]
#[tokio::test]
async fn push_aws_sandbox_session(ctx: &mut AwsPushSandboxSession) {
    common::sandbox::check_session_runs_a_command(&ctx.ctx)
        .await
        .expect("a sandbox session runs a command");
    common::sandbox::check_workload_cannot_write_bundles(&ctx.ctx)
        .await
        .expect("only management writes sandbox bundles");
    ctx.bundle_bucket = Some(
        common::sandbox::check_bundle_in_deployment_bucket(&ctx.ctx)
            .await
            .expect("the bundle lives in the deployment's own bucket"),
    );
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
    common::remote_bindings::check_remote_kv_ts(&ctx.ctx.deployment, ctx.ctx.platform)
        .await
        .expect("remote TypeScript KV checks failed");
    common::commands::check_commands(&ctx.ctx.deployment)
        .await
        .expect("command checks failed");
}

/// Focused live-cloud proof without unrelated binding and command checks.
#[test_context(AwsPushTypeScript)]
#[tokio::test]
async fn push_aws_remote_kv(ctx: &mut AwsPushTypeScript) {
    common::remote_bindings::check_remote_kv_ts(&ctx.ctx.deployment, ctx.ctx.platform)
        .await
        .expect("remote TypeScript KV checks failed");
}

/// Focused live-cloud proof without unrelated binding and command checks.
#[test_context(AwsPushTypeScript)]
#[tokio::test]
async fn push_aws_remote_queue(ctx: &mut AwsPushTypeScript) {
    common::remote_bindings::check_remote_queue_ts(&ctx.ctx.deployment, ctx.ctx.platform)
        .await
        .expect("remote TypeScript Queue checks failed");
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

// Note: Local push tests are not included because the local platform only
// supports pull (container) model through the manager pipeline. Push (worker)
// model on local requires `alien dev` which bypasses the deployment pipeline.

macro_rules! automatic_setup_test {
    ($context:ident, $test:ident, $platform:expr) => {
        struct $context(common::automatic_setup::AutomaticSetup);
        impl test_context::AsyncTestContext for $context {
            async fn setup() -> Self {
                Self(
                    common::automatic_setup::AutomaticSetup::new($platform)
                        .await
                        .expect("create automatic setup test"),
                )
            }
            async fn teardown(self) {
                self.0
                    .cleanup()
                    .await
                    .expect("automatic setup cleanup must finish");
            }
        }
        #[test_context($context)]
        #[tokio::test]
        async fn $test(ctx: &mut $context) {
            ctx.0.run().await.expect("automatic setup cloud lifecycle");
        }
    };
}
automatic_setup_test!(AwsAutomaticSetup, push_aws_automatic_setup, Platform::Aws);
automatic_setup_test!(GcpAutomaticSetup, push_gcp_automatic_setup, Platform::Gcp);
automatic_setup_test!(
    AzureAutomaticSetup,
    push_azure_automatic_setup,
    Platform::Azure
);

e2e_test_context!(
    AwsNotifications,
    Platform::Aws,
    DeploymentModel::Push,
    TestApp::StorageNotifications
);

#[test_context(AwsNotifications)]
#[tokio::test]
async fn push_aws_storage_notifications(ctx: &mut AwsNotifications) {
    common::storage_notifications::check_delivery(&ctx.ctx.deployment).await;
}

e2e_test_context!(
    GcpNotifications,
    Platform::Gcp,
    DeploymentModel::Push,
    TestApp::StorageNotifications
);

#[test_context(GcpNotifications)]
#[tokio::test]
async fn push_gcp_storage_notifications(ctx: &mut GcpNotifications) {
    common::storage_notifications::check_delivery(&ctx.ctx.deployment).await;
}

e2e_test_context!(
    AzureNotifications,
    Platform::Azure,
    DeploymentModel::Push,
    TestApp::StorageNotifications
);

#[test_context(AzureNotifications)]
#[tokio::test]
async fn push_azure_storage_notifications(ctx: &mut AzureNotifications) {
    common::storage_notifications::check_delivery(&ctx.ctx.deployment).await;
}
