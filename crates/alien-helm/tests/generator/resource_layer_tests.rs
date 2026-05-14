//! Per-resource layer scenarios — storage / kv / queue / vault /
//! artifact-registry contributions land under
//! `infrastructure.<resource_id>` in the chart's `values.yaml`.

use super::helpers::{assert_helm_valid, render, snapshot_chart};
use alien_core::{
    ArtifactRegistry, Kv, Queue, ResourceLifecycle, Stack, StackSettings, Storage, Vault,
};

#[test]
fn data_layer_emits_infrastructure_bindings() {
    let stack = Stack::new("data-chart".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    snapshot_chart("data_layer", &chart);
    assert_helm_valid(&chart, "data_layer");
}
