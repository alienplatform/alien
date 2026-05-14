//! ArtifactRegistry emitter — local registry placeholder for on-prem charts.

use crate::emitter::{HelmEmitter, HelmFragment, InfrastructureValue};
use alien_core::{import::EmitContext, Result};
use indexmap::indexmap;

#[derive(Debug, Default)]
pub struct ArtifactRegistryEmitter;

impl HelmEmitter for ArtifactRegistryEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "artifact_registry".to_string(),
                service: "local".to_string(),
                fields: indexmap! {
                    "registryUrl".to_string() => "registry.example.com".to_string(),
                    "dataDir".to_string() => "null".to_string(),
                },
            }),
        )
    }
}
