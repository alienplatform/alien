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
use alien_core::{import::EmitContext, Result, Sandbox, SandboxEgress};
use hcl::expr::Expression;

/// Emits the Azure sandbox group's identity for the runtime to address.
#[derive(Debug, Clone, Copy, Default)]
pub struct AzureSandboxEmitter;

/// The group name the data plane is addressed by.
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

impl TfEmitter for AzureSandboxEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // Deliberately empty: see the module note. A group emitted here would sit idle until a
        // session asked for one, and it is addressed by name rather than by reference.
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
        let disk_image = sandbox.azure_catalog_image()?.to_string();
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
    use alien_core::bindings::{AzureSandboxBinding, BindingValue};
    use alien_core::SandboxCode;
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
        let resource = stack
            .resources
            .get("agents")
            .expect("the sandbox is in the stack");
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

    /// Every key the binding deserializes is a key the emitter writes.
    ///
    /// The emitter types the names by hand while the provider reads them through serde, so a
    /// rename on either side would otherwise surface as a deserialization failure at runtime.
    #[test]
    fn the_emitted_keys_are_the_ones_the_binding_deserializes() {
        let rendered = binding_with(
            SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            },
            Some(900),
        );

        let binding = AzureSandboxBinding {
            sandbox_group: BindingValue::Value("sbg".to_string()),
            data_plane_endpoint: BindingValue::Value("https://example.invalid".to_string()),
            region: BindingValue::Value("eastus".to_string()),
            resource_group: BindingValue::Value("rg".to_string()),
            egress: SandboxEgress::Allow,
            idle_suspend_seconds: Some(900),
            disk_image: BindingValue::Value("ubuntu".to_string()),
        };
        let keys = serde_json::to_value(&binding).expect("the binding serializes");

        for key in keys.as_object().expect("an object").keys() {
            assert!(
                rendered.contains(&format!("{key} = ")),
                "the emitter never writes '{key}': {rendered}"
            );
        }
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
        assert!(!undeclared.contains("idleSuspendSeconds"), "{undeclared}");
    }
}
