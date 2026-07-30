//! GCP AI — Vertex AI inference gateway.
//!
//! Vertex AI is a project-level service with no per-stack resource to
//! provision. The emitter returns an empty fragment and carries the project
//! and location in the import ref so the controller can reconstruct the
//! Vertex AI endpoint without a cloud round-trip.
//!
//! The `aiplatform.googleapis.com` API enablement is handled by the
//! `GcpServiceActivationEmitter` when the preflight injects a
//! `ServiceActivation` for that API. The `ai/invoke` custom IAM role (predict
//! only) is emitted by `GcpServiceAccountEmitter` when a permission profile
//! references `ai/invoke`.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, Ai, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAiEmitter;

impl TfEmitter for GcpAiEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        Ok(TfFragment::empty())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            ("location", expr::raw("var.gcp_region")),
        ]))
    }
}
