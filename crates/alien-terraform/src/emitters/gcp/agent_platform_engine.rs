//! GCP Agent Platform reasoning engine — the parent a sandbox's sessions hang under.
//!
//! Emitted only for a Frozen engine. A resource-level IAM binding is refused on a scope whose
//! resource does not exist yet, so a remote grant on the engine is placeable only by the same
//! stack that creates it; a Live engine belongs to its controller and setup renders nothing.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, labels, required_label, resource_prefix_template},
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, GcpAgentPlatformEngine, ResourceLifecycle, Result,
};
use alien_error::AlienError;
use hcl::expr::Expression;

/// The `google-beta` resource backing an engine. Named once: the emitter, the provider gate in
/// the generator and the import ref must all address the same block.
pub const ENGINE_RESOURCE: &str = "google_vertex_ai_reasoning_engine";

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAgentPlatformEngineEmitter;

impl TfEmitter for GcpAgentPlatformEngineEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let engine =
            downcast::<GcpAgentPlatformEngine>(ctx, GcpAgentPlatformEngine::RESOURCE_TYPE)?;
        if ctx.resource.lifecycle != ResourceLifecycle::Frozen {
            return Ok(TfFragment::default());
        }
        let label = required_label(ctx)?;

        Ok(TfFragment::default().with_resource(resource_block(
            ENGINE_RESOURCE,
            label,
            [
                // The engine type lives only in google-beta, so the block names that provider
                // rather than inheriting the module's `google`.
                attr("provider", expr::raw("google-beta")),
                attr("region", expr::raw("var.gcp_region")),
                // The same name the Live controller gives an engine it creates, so an operator
                // reads one convention whoever made it.
                attr("display_name", resource_prefix_template(engine.id())),
                attr("labels", labels(ctx, "sandbox-engine")),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let engine =
            downcast::<GcpAgentPlatformEngine>(ctx, GcpAgentPlatformEngine::RESOURCE_TYPE)?;
        if ctx.resource.lifecycle != ResourceLifecycle::Frozen {
            // A Live engine's id is assigned by Vertex at create time, so there is nothing setup
            // could register but a guess the template controller would then create sessions under.
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: format!("terraform register engine '{}'", engine.id()),
                reason: "a Live reasoning engine is created by its controller after apply, so \
                         setup has no id to register. Declare the sandbox as frozen"
                    .to_string(),
            }));
        }
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "engineId",
            expr::traversal([ENGINE_RESOURCE, label, "name"]),
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{PermissionsConfig, Stack, StackSettings};
    use hcl::structure::{Block, Structure};
    use indexmap::IndexMap;

    fn stack_with_engine(lifecycle: ResourceLifecycle) -> Stack {
        Stack::new("acme".to_string())
            .permissions(PermissionsConfig::new())
            .add(
                GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                lifecycle,
            )
            .build()
    }

    fn with_context<T>(lifecycle: ResourceLifecycle, run: impl FnOnce(&EmitContext<'_>) -> T) -> T {
        let stack = stack_with_engine(lifecycle);
        let resource = stack
            .resources
            .get("agents-engine")
            .expect("the engine is in the stack");
        let names = IndexMap::from([("agents-engine".to_string(), "agents_engine".to_string())]);
        let settings = StackSettings::default();
        let ctx = EmitContext {
            stack: &stack,
            resource,
            resource_id: "agents-engine",
            platform: alien_core::Platform::Gcp,
            targets_kubernetes: false,
            stack_settings: &settings,
            names: &names,
        };
        run(&ctx)
    }

    fn attribute(block: &Block, key: &str) -> String {
        block
            .body
            .iter()
            .find_map(|structure| match structure {
                Structure::Attribute(attribute) if attribute.key.as_str() == key => {
                    Some(attribute.expr.to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the block carries no '{key}' attribute: {block:?}"))
    }

    /// Setup owns a Frozen engine and nothing else. The block is what a remote grant is scoped to,
    /// and naming `google-beta` is what makes the type resolvable at all.
    #[test]
    fn a_frozen_engine_renders_a_google_beta_resource() {
        let fragment = with_context(ResourceLifecycle::Frozen, |ctx| {
            GcpAgentPlatformEngineEmitter
                .emit(ctx)
                .expect("a frozen engine renders")
        });

        assert_eq!(fragment.resource_blocks.len(), 1);
        let block = &fragment.resource_blocks[0];
        let labels: Vec<&str> = block.labels.iter().map(|label| label.as_str()).collect();
        assert_eq!(labels, vec![ENGINE_RESOURCE, "agents_engine"]);
        assert_eq!(attribute(block, "provider"), "google-beta");
        assert_eq!(attribute(block, "region"), "var.gcp_region");
        assert_eq!(
            attribute(block, "display_name"),
            "\"${local.resource_prefix}-agents-engine\""
        );
    }

    /// A Live engine is created by its controller after apply. Setup emitting one would create a
    /// second engine the controller never learns about, and `terraform destroy` would then remove
    /// the parent of sessions the controller still believes it owns.
    #[test]
    fn a_live_engine_renders_nothing_and_registers_nothing() {
        let fragment = with_context(ResourceLifecycle::Live, |ctx| {
            GcpAgentPlatformEngineEmitter
                .emit(ctx)
                .expect("a live engine renders an empty fragment")
        });
        assert!(
            fragment.resource_blocks.is_empty(),
            "setup renders no block for a controller-owned engine"
        );

        let error = with_context(ResourceLifecycle::Live, |ctx| {
            GcpAgentPlatformEngineEmitter
                .emit_import_ref(ctx)
                .expect_err("a live engine has no id setup could register")
        });
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
        assert!(
            error.to_string().contains("agents-engine"),
            "names the engine: {error}"
        );
    }

    /// The registered id is the one Vertex assigned, read off the created resource. A derived
    /// name would register an engine that does not exist, and every session under it would 404.
    #[test]
    fn the_import_ref_reads_the_id_off_the_created_engine() {
        let rendered = with_context(ResourceLifecycle::Frozen, |ctx| {
            GcpAgentPlatformEngineEmitter
                .emit_import_ref(ctx)
                .expect("a frozen engine registers")
                .to_string()
        });

        assert!(
            rendered.contains(&format!("{ENGINE_RESOURCE}.agents_engine.name")),
            "{rendered}"
        );
    }
}
