//! Azure Sandbox — a named group, and nothing built at setup.
//!
//! No sandbox controller is registered for Azure, and `create_or_update_sandbox_group` has no
//! caller, so nothing here creates the group a session lives in — it has to exist already. Setup
//! emits no Azure resource for the same reason it would not be useful to: a group is cheap to
//! create by name and pointless to hold open while no session wants one.
//!
//! What this emitter contributes is the three names the data plane is addressed by, which the
//! Azure client config does not carry: the group, the region that selects the per-region endpoint,
//! and the resource group the data-plane path is scoped by.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, resource_prefix_template},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxCode, SandboxEgress};
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

/// The declared outbound policy, in the shape the binding carries.
///
/// The sandbox is created with it rather than a setup resource enforcing it — Azure's proxy takes
/// the policy at create — so the declaration has to survive as far as the binding intact.
fn egress(sandbox: &Sandbox) -> Expression {
    match &sandbox.egress {
        SandboxEgress::Deny => expr::object([("mode", Expression::String("deny".to_string()))]),
        SandboxEgress::Allow => expr::object([("mode", Expression::String("allow".to_string()))]),
        SandboxEgress::AllowDomains { domains } => expr::object([
            ("mode", Expression::String("allowDomains".to_string())),
            (
                "domains",
                Expression::from(
                    domains
                        .iter()
                        .map(|domain| Expression::String(domain.clone()))
                        .collect::<Vec<_>>(),
                ),
            ),
        ]),
    }
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
        // A tag is the shape that gets through unnoticed: `ubuntu:24.04` has no slash, renders
        // into the customer's module, plans and applies, and fails at the first session.
        SandboxCode::Image { image }
            if image.contains('/') || image.contains(':') || image.contains('@') =>
        {
            Err(unsupported(format!(
                "Azure creates a sandbox from a public catalog disk image, so code.image must be \
                 a bare catalog name such as 'ubuntu' — '{image}' carries a registry path, tag or \
                 digest, which the data plane has nowhere to put"
            )))
        }
        SandboxCode::Image { image } => Ok(image.clone()),
        SandboxCode::Source { .. } => Err(unsupported(
            "no sandbox backend builds an image from source yet".to_string(),
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
        let mut fields = vec![
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
            ("egress", egress(sandbox)),
        ];

        if let Some(seconds) = sandbox.session.idle_suspend_seconds {
            fields.push((
                "idleSuspendSeconds",
                Expression::Number(i64::from(seconds).into()),
            ));
        }

        Ok(Some(expr::object(fields)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, SandboxSessionPolicy, Stack, StackSettings};
    use indexmap::IndexMap;

    fn binding_for(egress: SandboxEgress) -> String {
        binding_with(egress, None)
    }

    fn binding_with(egress: SandboxEgress, idle_suspend_seconds: Option<u32>) -> String {
        let stack = Stack::new("acme".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                    })
                    .egress(egress)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let resource = stack.resources.get("agents").expect("the sandbox is in the stack");
        let names = IndexMap::from([("agents".to_string(), "agents".to_string())]);
        let settings = StackSettings::default();
        let ctx = EmitContext {
            stack: &stack,
            resource,
            resource_id: "agents",
            platform: alien_core::Platform::Azure,
            targets_kubernetes: false,
            stack_settings: &settings,
            names: &names,
        };

        AzureSandboxEmitter
            .emit_binding_ref(&ctx)
            .expect("the binding renders")
            .expect("an Azure sandbox has a binding")
            .to_string()
    }

    /// The declared mode has to reach the binding, whole.
    ///
    /// Azure applies the policy at create rather than through a setup resource, so the binding is
    /// the only carrier: a mode that stops here leaves every session created under the data
    /// plane's own default, which is open. A hostname list fails twice over — the mode without the
    /// domains denies everything, and the domains without the mode are ignored.
    #[test]
    fn the_binding_carries_the_declared_egress() {
        // The key names are asserted, not just the values: `AzureSandboxBinding.egress` has no
        // serde default, so a misspelled key here is a deserialization failure on the customer's
        // cluster rather than a failure at emit.
        let denied = binding_for(SandboxEgress::Deny);
        assert!(denied.contains("egress = {"), "{denied}");
        assert!(denied.contains(r#"mode = "deny""#), "{denied}");

        let listed = binding_for(SandboxEgress::AllowDomains {
            domains: vec!["api.example.com".to_string()],
        });
        assert!(listed.contains(r#"mode = "allowDomains""#), "{listed}");
        assert!(listed.contains("domains = ["), "{listed}");
        assert!(listed.contains(r#""api.example.com""#), "{listed}");

        let open = binding_for(SandboxEgress::Allow);
        assert!(open.contains(r#"mode = "allow""#), "{open}");
    }

    /// The idle-suspend policy travels the same way, and only when it was declared.
    ///
    /// Azure takes it at create, so a number that stops at the emitter leaves the session on the
    /// service default — and an emitted zero would be a policy nobody asked for.
    #[test]
    fn the_binding_carries_a_declared_idle_suspend_and_nothing_otherwise() {
        let declared = binding_with(SandboxEgress::Allow, Some(900));
        assert!(declared.contains("idleSuspendSeconds = 900"), "{declared}");

        let undeclared = binding_with(SandboxEgress::Allow, None);
        assert!(
            !undeclared.contains("idleSuspendSeconds"),
            "{undeclared}"
        );
    }
}
