//! Per-resource CloudFormation emitter trait.
//!
//! Each built-in resource (storage, queue, kv, function, \u2026) implements
//! `CfEmitter` for `Platform::Aws`. Plugins register additional implementations
//! against the same `CfRegistry`. Emitters return CFN-native [`CfResource`] /
//! [`CfExpression`] directly \u2014 there is no intermediate IR.

use crate::{
    registry::CfRegistry,
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Result};

/// Generator-side trait that emits raw CloudFormation resources plus the
/// expression that resolves to this resource's typed registration data at apply time.
pub trait CfEmitter: Send + Sync {
    /// Emit the raw `AWS::*` resources that back this stack resource. The
    /// generator merges them into the template body.
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>>;

    /// Emit resources with access to the full registry. Resource emitters that
    /// need linked-resource binding references can override this while older
    /// emitters keep implementing the simpler method.
    fn emit_resources_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        _registry: &CfRegistry,
    ) -> Result<Vec<CfResource>> {
        self.emit_resources(ctx)
    }

    /// Emit an expression that resolves to this resource's typed registration
    /// data at apply time (typically a `Fn::GetAtt` / `Ref` object). Embedded
    /// into the setup registration payload by the generator.
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression>;

    /// Whether this emitter renders correctly for a resource gated by
    /// `.enabled(input)`.
    ///
    /// Opting in means the emitter leaves the generator free to stamp the gate's
    /// `Condition` onto its resources, and folds its own import ref through
    /// `Fn::If` so nothing references a resource the condition skipped. The
    /// generator refuses to render a gated resource whose emitter has not, so a
    /// half-converted emitter fails loudly instead of silently creating the
    /// resource the deployer declined.
    fn supports_enabled_when(&self) -> bool {
        false
    }

    /// Expression that resolves to this resource's runtime binding payload.
    /// Import data feeds the manager; binding data feeds user code.
    fn emit_binding_ref(&self, _ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        Ok(None)
    }
}
