//! Vault emitter — defaults to Kubernetes Secrets for on-prem charts.

use crate::emitter::{HelmEmitter, HelmFragment, InfrastructureValue};
use alien_core::{import::EmitContext, Result};
use indexmap::indexmap;

#[derive(Debug, Default)]
pub struct VaultEmitter;

impl HelmEmitter for VaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        let placeholder = ctx.resource_id.replace('-', "_");
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "vault".to_string(),
                service: "kubernetes-secret".to_string(),
                fields: indexmap! {
                    "namespace".to_string() => "default".to_string(),
                    "vaultPrefix".to_string() => placeholder,
                },
            }),
        )
    }
}
