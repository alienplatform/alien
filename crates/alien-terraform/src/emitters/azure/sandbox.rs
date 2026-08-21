//! Azure Sandbox — a named group, and nothing built at setup.
//!
//! The ACA sandbox group is created by the runtime controller, idempotently by name, because a
//! group is cheap to create and pointless to hold open while no session wants one. So setup emits
//! no Azure resource here; what it owes the runtime is the three names the data plane is addressed
//! by, which the Azure client config does not carry: the group, the region that selects the
//! per-region endpoint, and the resource group the data-plane path is scoped by.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, resource_prefix_template},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxCode};
use alien_error::AlienError;
use hcl::expr::Expression;

/// Emits the Azure sandbox group's identity for the runtime to address.
#[derive(Debug, Clone, Copy, Default)]
pub struct AzureSandboxEmitter;

/// The group name the runtime controller creates and the data plane addresses.
///
/// Derived rather than emitted as a resource: both sides compute it from the same prefix and id,
/// so there is nothing to look up and nothing to keep in step. The prefix must be the resolved
/// `local.resource_prefix` — the deployer's `var.resource_prefix` defaults to empty and is
/// replaced by a generated one, so naming from the variable registers a group of `-<id>` while
/// the management grant is scoped to the real one.
fn sandbox_group(ctx: &EmitContext<'_>) -> Expression {
    resource_prefix_template(&ctx.resource_id)
}

/// The catalog image name a declaration asks for, or a refusal.
///
/// The create body names a public catalog image, so a registry reference has nowhere to go.
/// Refusing at plan time follows the AWS emitter: a reference the backend cannot honour is
/// rejected rather than quietly replaced, which is what happened before this existed — every
/// Azure session ran a stock image whatever the declaration said, with no error anywhere.
fn catalog_disk_image(sandbox: &Sandbox) -> Result<String> {
    let unsupported = |reason: String| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform emit sandbox '{}'", sandbox.id()),
            reason,
        })
    };

    match &sandbox.code {
        SandboxCode::Image { image } if image.contains('/') => Err(unsupported(format!(
            "Azure creates a sandbox from a public catalog disk image, so code.image must be a \
             catalog name such as 'ubuntu', not the registry reference '{image}'"
        ))),
        SandboxCode::Image { image } => Ok(image.clone()),
        SandboxCode::Source { .. } => Err(unsupported(
            "Azure creates a sandbox from a prebuilt catalog disk image and cannot build one \
             from source"
                .to_string(),
        )),
    }
}

impl TfEmitter for AzureSandboxEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // Deliberately empty: see the module note. A group created here would sit idle until a
        // session asked for one, and the controller would have to reconcile against it anyway.
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            ("sandboxGroup", sandbox_group(ctx)),
            ("region", expr::raw("var.azure_location")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        let disk_image = catalog_disk_image(sandbox)?;
        Ok(Some(expr::object([
            ("service", Expression::String("sandbox-azure".to_string())),
            ("sandboxGroup", sandbox_group(ctx)),
            // The data plane is a per-region host, so the region is what selects it rather than a
            // second thing to keep in step with it.
            (
                "dataPlaneEndpoint",
                expr::template("https://management.${var.azure_location}.azuredevcompute.io"),
            ),
            ("region", expr::raw("var.azure_location")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            ("diskImage", Expression::String(disk_image)),
        ])))
    }
}
