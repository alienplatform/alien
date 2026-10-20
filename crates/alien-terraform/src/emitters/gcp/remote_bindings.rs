use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label, service_account_id_template},
    expr,
};
use alien_core::{import::EmitContext, RemoteBindings, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpRemoteBindingsEmitter;

impl TfEmitter for GcpRemoteBindingsEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteBindings>(ctx, RemoteBindings::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block("google_service_account", label, [
            attr("project", expr::raw("var.gcp_project")),
            attr("account_id", service_account_id_template(label)),
            attr("display_name", expr::template("${local.deployment_name}: application access".to_string())),
            attr("description", expr::template("Data-plane identity for explicitly published resources in ${local.deployment_name}.".to_string())),
        ]));
        super::federation::emit(&mut fragment, label, &RemoteBindings::RESOURCE_TYPE);

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "federation",
                super::federation::import_ref(label, &RemoteBindings::RESOURCE_TYPE),
            ),
            ("projectId", expr::raw("var.gcp_project")),
            (
                "serviceAccountEmail",
                expr::traversal(["google_service_account", label, "email"]),
            ),
            (
                "serviceAccountUniqueId",
                expr::traversal(["google_service_account", label, "unique_id"]),
            ),
        ]))
    }
}
