//! The hosted manager forwards each Operator's observed inventory to the Platform API through
//! the full client. A Kubernetes observer tags workload samples with a resource type hint, so
//! that optional string must survive the generated types.
#![cfg(feature = "full-api")]

use alien_core::{
    HeartbeatBackend, ObservedHealth, ObservedInventoryBatch, ObservedResourceSample, Platform,
    ProviderLifecycleState, ResourceType,
};
use alien_platform_api::types::ObservedInventoryBatch as ApiObservedInventoryBatch;

fn sample(resource_type_hint: Option<ResourceType>) -> ObservedResourceSample {
    ObservedResourceSample {
        deployment_id: Some("dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
        raw_identity: "apps/v1/Deployment/demo/gateway".to_string(),
        provider_kind: "apps/v1/Deployment".to_string(),
        display_name: "gateway".to_string(),
        namespace: Some("demo".to_string()),
        region: None,
        scope: None,
        resource_type_hint,
        version: None,
        alien_resource_id: None,
        health: ObservedHealth::Healthy,
        lifecycle: ProviderLifecycleState::Running,
        message: None,
        partial: false,
        provider_stale: false,
        counts: None,
        collection_issues: Vec::new(),
        labels: Default::default(),
        attributes: Default::default(),
        raw: Vec::new(),
        images: Vec::new(),
    }
}

#[test]
fn observed_inventory_keeps_resource_type_hints() {
    let source = ObservedInventoryBatch {
        resources: vec![sample(Some(ResourceType::from("container"))), sample(None)],
        source_kind: "operator".to_string(),
        inventory_scope: "kubernetes/demo".to_string(),
        controller_platform: Platform::Kubernetes,
        backend: HeartbeatBackend::Kubernetes,
        observed_at: "2026-10-07T16:00:00Z".parse().expect("timestamp parses"),
        complete: true,
    };

    let api: ApiObservedInventoryBatch =
        serde_json::from_value(serde_json::to_value(&source).expect("batch encodes"))
            .expect("generated client decodes the batch");
    let round_tripped: ObservedInventoryBatch =
        serde_json::from_value(serde_json::to_value(&api).expect("generated batch encodes"))
            .expect("batch decodes");

    assert_eq!(round_tripped, source);
}
