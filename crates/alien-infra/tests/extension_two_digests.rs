//! One extension at two digests, v1 in the current release and v2 in the target release.

use alien_core::{Platform, ResourceType, Stack};
use alien_infra::step_protocol::protocol::{StepRequest, StepResponse};
use alien_infra::step_protocol::{ProxyFactory, Transport, TransportFailure};
use alien_infra::{LoadedExtensions, ResourceRegistry};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct Unused;

#[async_trait::async_trait]
impl Transport for Unused {
    async fn step(
        &self,
        _request: &StepRequest,
        _timeout: Duration,
    ) -> Result<StepResponse, TransportFailure> {
        unreachable!("never stepped")
    }
}

fn release(digest: &str, types: &[&str]) -> Stack {
    let descriptor = serde_json::json!({
        "lifecycles": ["live"],
        "holdsData": false,
        "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
        "controllerPlatforms": ["aws"],
        "permissionSets": [],
        "link": { "permissionSets": [] },
        "schemas": { "config": {}, "outputs": {} }
    });
    let resource_types: serde_json::Map<String, serde_json::Value> = types
        .iter()
        .map(|t| (t.to_string(), descriptor.clone()))
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": digest,
            "resourceTypes": resource_types
        }]
    }))
    .unwrap()
}

fn loaded(endpoint: &str) -> LoadedExtensions {
    LoadedExtensions::default()
        .with_controller("acme", ProxyFactory::new(endpoint, Arc::new(Unused), None))
}

fn endpoint_of(registry: &ResourceRegistry, resource_type: &str) -> String {
    let controller = registry
        .get_controller(ResourceType::from(resource_type), Platform::Aws)
        .unwrap();
    format!("{controller:?}")
}

#[test]
fn the_target_digest_serves_its_types_and_the_current_digest_only_removed_ones() {
    let current = release("sha256:v1", &["acme/dns-zone", "acme/record"]);
    let target = release("sha256:v2", &["acme/dns-zone"]);
    let registry = ResourceRegistry::with_built_ins_and_releases(
        (&target, &loaded("endpoint-v2")),
        (&current, &loaded("endpoint-v1")),
    );
    let zone = endpoint_of(&registry, "acme/dns-zone");
    assert!(
        zone.contains("endpoint-v2") && !zone.contains("endpoint-v1"),
        "{zone}"
    );
    let record = endpoint_of(&registry, "acme/record");
    assert!(
        record.contains("endpoint-v1") && !record.contains("endpoint-v2"),
        "{record}"
    );
}
