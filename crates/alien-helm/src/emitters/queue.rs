//! Queue emitter — `sqs / pubsub / servicebus` infrastructure binding.

use crate::emitter::{HelmEmitter, HelmFragment, InfrastructureValue};
use alien_core::{import::EmitContext, Result};
use indexmap::indexmap;

#[derive(Debug, Default)]
pub struct QueueEmitter;

impl HelmEmitter for QueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        let placeholder = ctx.resource_id.replace('-', "_");
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "queue".to_string(),
                service: "sqs".to_string(),
                fields: indexmap! {
                    "queueUrl".to_string() => format!(
                        "https://sqs.us-east-1.amazonaws.com/123456789012/{placeholder}"
                    ),
                },
            }),
        )
    }
}
