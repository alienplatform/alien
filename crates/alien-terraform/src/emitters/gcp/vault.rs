//! GCP Vault — Secret Manager namespace.
//!
//! Secret Manager is project-scoped; the vault is realized as a name
//! prefix that secrets carry. ImportData ships the prefix so the
//! controller can list / fetch / put secrets without an extra cloud
//! lookup.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, Result, Vault};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpVaultEmitter;

impl TfEmitter for GcpVaultEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Ok(TfFragment::empty())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "secretPrefix",
                expr::template(format!("${{var.stack_name}}-{}", vault.id())),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(Some(expr::object([
            ("service", Expression::String("secret-manager".to_string())),
            (
                "vaultPrefix",
                expr::template(format!("${{var.stack_name}}-{}", vault.id())),
            ),
        ])))
    }
}
