//! Per-resource CloudFormation emitter trait.
//!
//! Each built-in resource (storage, queue, kv, function, \u2026) implements
//! `CfEmitter` for `Platform::Aws`. Plugins register additional implementations
//! against the same `CfRegistry`. Emitters return CFN-native [`CfResource`] /
//! [`CfExpression`] directly \u2014 there is no intermediate IR.

use crate::template::{CfExpression, CfResource};
use alien_core::{import::EmitContext, Result};

/// Generator-side trait that emits raw CloudFormation resources plus the
/// expression that resolves to this resource's typed `ImportData` at apply time.
pub trait CfEmitter: Send + Sync {
    /// Emit the raw `AWS::*` resources that back this stack resource. The
    /// generator merges them into the template body.
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>>;

    /// Emit an expression that resolves to this resource's typed `ImportData`
    /// at apply time (typically a `Fn::GetAtt` / `Ref` object). Embedded into
    /// the auto-import payload by the generator.
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression>;
}
