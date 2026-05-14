//! Distribution E2E tests.
//!
//! Distribution tests exercise infrastructure artifacts as the initial setup
//! path: CloudFormation for AWS and Terraform for cloud/K8s targets. They use
//! the same `comprehensive-rust` app and the same assertions as push/pull E2E.

use alien_test::{DistributionFlow, Language};
use test_context::test_context;

mod common;

async fn check_distribution_deployment(ctx: &mut alien_test::TestContext) {
    common::runner::check_all_bindings(&ctx.deployment, ctx.platform, ctx.model, ctx.language)
        .await
        .expect("binding checks failed");
    common::commands::check_commands(&ctx.deployment)
        .await
        .expect("command checks failed");
}

macro_rules! distribution_test_context {
    ($name:ident, $flow:expr, $lang:expr) => {
        struct $name {
            ctx: alien_test::TestContext,
        }

        impl test_context::AsyncTestContext for $name {
            async fn setup() -> Self {
                alien_test::e2e::init_tracing();
                let ctx = alien_test::e2e::setup_distribution($flow, $lang)
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
    Language::Rust
);

#[test_context(CloudFormationAwsPushRust)]
#[tokio::test]
async fn cloudformation_aws_push_rust(ctx: &mut CloudFormationAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// ---------------------------------------------------------------------------
// Terraform push
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformAwsPushRust,
    DistributionFlow::TerraformAwsPush,
    Language::Rust
);

#[test_context(TerraformAwsPushRust)]
#[tokio::test]
async fn terraform_aws_push_rust(ctx: &mut TerraformAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGcpPushRust,
    DistributionFlow::TerraformGcpPush,
    Language::Rust
);

#[test_context(TerraformGcpPushRust)]
#[tokio::test]
async fn terraform_gcp_push_rust(ctx: &mut TerraformGcpPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAzurePushRust,
    DistributionFlow::TerraformAzurePush,
    Language::Rust
);

#[test_context(TerraformAzurePushRust)]
#[tokio::test]
async fn terraform_azure_push_rust(ctx: &mut TerraformAzurePushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// ---------------------------------------------------------------------------
// Terraform + Helm pull
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformEksHelmPullRust,
    DistributionFlow::TerraformEksHelmPull,
    Language::Rust
);

#[test_context(TerraformEksHelmPullRust)]
#[tokio::test]
async fn terraform_eks_helm_pull_rust(ctx: &mut TerraformEksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullRust,
    DistributionFlow::TerraformGkeHelmPull,
    Language::Rust
);

#[test_context(TerraformGkeHelmPullRust)]
#[tokio::test]
async fn terraform_gke_helm_pull_rust(ctx: &mut TerraformGkeHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullRust,
    DistributionFlow::TerraformAksHelmPull,
    Language::Rust
);

#[test_context(TerraformAksHelmPullRust)]
#[tokio::test]
async fn terraform_aks_helm_pull_rust(ctx: &mut TerraformAksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformOnpremHelmPullRust,
    DistributionFlow::TerraformOnpremHelmPull,
    Language::Rust
);

#[test_context(TerraformOnpremHelmPullRust)]
#[tokio::test]
#[ignore = "on-prem Helm local-import needs a complete external binding fixture for comprehensive-rust"]
async fn terraform_onprem_helm_pull_rust(ctx: &mut TerraformOnpremHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}
