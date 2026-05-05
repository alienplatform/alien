//! Storage emitter — contributes a `s3 / gcs / azureblob` infrastructure
//! binding when the customer's K8s deployment talks to cloud storage,
//! plus a placeholder for in-cluster MinIO when on-prem.

use crate::emitter::{HelmEmitter, HelmFragment, InfrastructureValue};
use alien_core::{import::EmitContext, Result};
use indexmap::indexmap;

#[derive(Debug, Default)]
pub struct StorageEmitter;

impl HelmEmitter for StorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        let placeholder = ctx.resource_id.replace('-', "_");
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "storage".to_string(),
                service: "s3".to_string(),
                fields: indexmap! {
                    "bucketName".to_string() => format!("your-{placeholder}-bucket"),
                },
            }),
        )
    }
}
