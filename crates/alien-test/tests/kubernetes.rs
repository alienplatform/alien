//! Kubernetes pull E2E tests against an existing cluster.
//!
//! Unlike the distribution tests (which provision their own EKS/GKE/AKS
//! clusters via Terraform/CloudFormation + Helm), these tests install the
//! generated Helm agent into the cluster of the ambient kubeconfig. They
//! require:
//! - a reachable cluster (`KUBECONFIG` or `~/.kube/config`), and
//! - at least one cloud platform configured in `.env.test` for the image
//!   registry the built app is pushed to.
//!
//! They compile everywhere but only run in the CI cloud workflows (or on a
//! developer machine with a cluster + registry credentials).
//!
//! Run a single test:
//!   cargo nextest run -p alien-test --test kubernetes pull_kubernetes_command_routing_ts

use alien_core::Platform;
use alien_test::{DeploymentModel, TestApp};
use test_context::test_context;

mod common;
use common::e2e_test_context;

// ---------------------------------------------------------------------------
// Comprehensive TypeScript worker on Kubernetes
// ---------------------------------------------------------------------------
//
// Direct-entrypoint + bindings proof for a K8s workload: the pod's process
// serves HTTP and exercises storage/kv/vault through in-process bindings, and
// answers commands leased by its own receiver (pull model).

e2e_test_context!(
    K8sPullTypeScript,
    Platform::Kubernetes,
    DeploymentModel::Pull,
    TestApp::ComprehensiveTs
);

#[test_context(K8sPullTypeScript)]
#[tokio::test]
async fn pull_kubernetes_comprehensive_ts(ctx: &mut K8sPullTypeScript) {
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
// Target-scoped command routing on Kubernetes (Worker + Daemon)
// ---------------------------------------------------------------------------
//
// Same routing proofs as the Local variant, on Kubernetes: the Daemon pod is
// a direct entrypoint (no runtime wrapper), leases only its own commands, and
// both resources register the same command names disambiguated by target.

e2e_test_context!(
    K8sPullCommandRouting,
    Platform::Kubernetes,
    DeploymentModel::Pull,
    TestApp::CommandRoutingTs
);

#[test_context(K8sPullCommandRouting)]
#[tokio::test]
async fn pull_kubernetes_command_routing_ts(ctx: &mut K8sPullCommandRouting) {
    common::routing::check_command_routing(&ctx.ctx.deployment)
        .await
        .expect("command routing checks failed");
}
