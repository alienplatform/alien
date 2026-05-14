//! Pull model E2E tests.
//!
//! Local pull deployments run the agent locally, either as a foreground
//! process or an installed OS service.
//!
//! Cleanup is guaranteed by `test-context`: even if a binding check panics,
//! `teardown()` runs and deletes the deployment plus local agent process or
//! service.
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test pull pull_local_rust
//!
//! Run all pull tests:
//!   cargo nextest run -p alien-test --test pull

use alien_core::Platform;
use alien_test::{DeploymentModel, Language};
use test_context::test_context;

mod common;
use common::e2e_test_context;

// ---------------------------------------------------------------------------
// Local
// ---------------------------------------------------------------------------

e2e_test_context!(
    LocalPullRust,
    Platform::Local,
    DeploymentModel::Pull,
    Language::Rust
);

#[test_context(LocalPullRust)]
#[tokio::test]
async fn pull_local_rust(ctx: &mut LocalPullRust) {
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
    LocalPullTypeScript,
    Platform::Local,
    DeploymentModel::Pull,
    Language::TypeScript
);

#[test_context(LocalPullTypeScript)]
#[tokio::test]
async fn pull_local_typescript(ctx: &mut LocalPullTypeScript) {
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
