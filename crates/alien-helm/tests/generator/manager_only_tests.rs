//! Manager-only scenarios — stacks with no `Frozen` infrastructure
//! resources. Charts still render `examples/<target>.yaml` files for
//! IRSA / Workload Identity / Federated Identity.

use super::helpers::{assert_helm_valid, render, snapshot_chart};
use alien_core::{
    Worker, WorkerCode, Ingress, PermissionProfile, ResourceLifecycle, Stack, StackSettings,
};

#[test]
fn pure_worker_chart_emits_service_for_public_ingress() {
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("runtime".to_string())
        .ingress(Ingress::Public)
        .build();
    let stack = Stack::new("pure-fn".to_string())
        .permission(
            "runtime",
            PermissionProfile::new().global(["worker/management"]),
        )
        .add(worker, ResourceLifecycle::Live)
        .build();
    let chart = render(&stack, StackSettings::default());
    snapshot_chart("manager_only_pure_worker", &chart);
    assert_helm_valid(&chart, "manager_only_pure_worker");
}
