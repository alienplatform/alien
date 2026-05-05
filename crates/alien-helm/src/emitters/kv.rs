//! KV emitter — `redis` infrastructure binding (default for K8s; cloud
//! KV bindings are wired via per-cloud overlays).

use crate::emitter::{HelmEmitter, HelmFragment, InfrastructureValue};
use alien_core::{import::EmitContext, Result};
use indexmap::indexmap;

#[derive(Debug, Default)]
pub struct KvEmitter;

impl HelmEmitter for KvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        let placeholder = ctx.resource_id.replace('-', "_");
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "kv".to_string(),
                service: "redis".to_string(),
                fields: indexmap! {
                    "connectionUrl".to_string() => format!("redis://{placeholder}.internal:6379"),
                },
            }),
        )
    }
}
