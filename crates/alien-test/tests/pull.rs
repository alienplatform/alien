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
//!   cargo nextest run -p alien-test --test pull pull_local_comprehensive_rust
//!
//! Run all pull tests:
//!   cargo nextest run -p alien-test --test pull

use alien_core::Platform;
use alien_test::{DeploymentModel, TestApp};
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
    TestApp::ComprehensiveRust
);

#[test_context(LocalPullRust)]
#[tokio::test]
async fn pull_local_comprehensive_rust(ctx: &mut LocalPullRust) {
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
    LocalPullTypeScript,
    Platform::Local,
    DeploymentModel::Pull,
    TestApp::ComprehensiveTs
);

#[test_context(LocalPullTypeScript)]
#[tokio::test]
async fn pull_local_comprehensive_ts(ctx: &mut LocalPullTypeScript) {
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
// Local: target-scoped command routing (Worker + Daemon, overlapping names)
// ---------------------------------------------------------------------------
//
// One deployment, TWO command-capable resources that both register `status`:
// Worker `api` (push) and Daemon `indexer-daemon` (pull receiver, runtime-less
// under direct supervision). Also covers the Local Daemon direct entrypoint:
// the daemon process must come up, lease its own commands (and only its own),
// and use in-process bindings.

e2e_test_context!(
    LocalPullCommandRouting,
    Platform::Local,
    DeploymentModel::Pull,
    TestApp::CommandRoutingTs
);

#[test_context(LocalPullCommandRouting)]
#[tokio::test]
async fn pull_local_command_routing_ts(ctx: &mut LocalPullCommandRouting) {
    common::routing::check_command_routing(&ctx.ctx.deployment)
        .await
        .expect("command routing checks failed");
}

// ---------------------------------------------------------------------------
// Local: Rust SOURCE Container (runtime-less, pull receiver, direct bindings)
// ---------------------------------------------------------------------------
//
// The Rust twin of the TS Daemon coverage above: a source-built Rust
// Container whose compiled binary is the image entrypoint (no runtime
// wrapper), using `alien_bindings::Bindings::from_env` for a direct KV
// binding and `alien_commands::Receiver` for target-scoped pull commands.
// This is the only test that executes the Rust receiver against a real
// command server from inside a container.

e2e_test_context!(
    LocalPullContainerRust,
    Platform::Local,
    DeploymentModel::Pull,
    TestApp::ContainerRust
);

#[test_context(LocalPullContainerRust)]
#[tokio::test]
async fn pull_local_container_rust(ctx: &mut LocalPullContainerRust) {
    common::container::check_container_status(&ctx.ctx.deployment)
        .await
        .expect("container status check failed");
}

// ---------------------------------------------------------------------------
// Local: TypeScript Container + Rust Daemon, direct bindings and commands
// ---------------------------------------------------------------------------
//
// Both source-built processes start directly, use a KV through their native
// in-process binding libraries, and own their command receiver loops. They
// deliberately register the same command name so target identity is required.

e2e_test_context!(
    LocalPullRuntimeLessMixed,
    Platform::Local,
    DeploymentModel::Pull,
    TestApp::RuntimeLessMixed
);

#[test_context(LocalPullRuntimeLessMixed)]
#[tokio::test]
async fn pull_local_runtime_less_mixed(ctx: &mut LocalPullRuntimeLessMixed) {
    common::runtime_less::check_mixed_runtime_less(&ctx.ctx.deployment)
        .await
        .expect("mixed runtime-less checks failed");
}
