//! Azure Sandbox — the group setup creates, and the remote grant scoped to it.
//!
//! Setup emits the group because a role assignment cannot name a resource that does not exist:
//! Azure answers `ResourceNotFound` for a scope whose resource is absent, so a remote grant is
//! only placeable if the same template that grants also creates. `Microsoft.App/sandboxGroups`
//! gained an ARM representation at `2026-02-01-preview`, which is what makes that possible; it is
//! reached through `azapi_resource` because the AzureRM provider has no typed resource for it.
//!
//! Beside the group this emitter contributes the three names the data plane is addressed by,
//! which the Azure client config does not carry: the group, the region that selects the
//! per-region endpoint, and the resource group the data-plane path is scoped by.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, emit_remote_bindings_role_definitions, permission_context,
        remote_bindings_role_label, required_label, resource_prefix_template, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, RemoteBindings, Result, Sandbox, SandboxEgress,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

/// The preview API version the sandbox group is created at. Pinned rather than floating: it is the
/// only version the provider manifest lists, and a preview type's shape moves between them.
const SANDBOX_GROUP_TYPE: &str = "Microsoft.App/sandboxGroups@2026-02-01-preview";

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
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azapi_resource",
            label,
            [
                attr("type", Expression::String(SANDBOX_GROUP_TYPE.to_string())),
                attr("name", sandbox_group(ctx)),
                attr(
                    "parent_id",
                    expr::template(
                        "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}",
                    ),
                ),
                attr("location", expr::raw("var.azure_location")),
                // The group takes no configuration Alien expresses — sizing, egress and image are
                // all per-session and travel in the create body — so the body is the empty object
                // the API requires rather than a field this would have to keep in step.
                attr(
                    "body",
                    expr::object(Vec::<(&str, Expression)>::new()),
                ),
                attr("tags", tags(ctx, "sandbox")),
                // The azapi provider ships a bundled schema index and refuses a type it does not
                // carry — `Microsoft.App/sandboxGroups can't be found` at validate. The type is
                // real: it is in the live ARM provider manifest at this version and ARM creates
                // one. So the index lags the service, and the check being disabled here is a
                // client-side pre-check, not ARM's — which still validates the request at apply.
                attr("schema_validation_enabled", Expression::Bool(false)),
            ],
        ));

        emit_remote_access(ctx, label, &mut fragment)?;
        Ok(fragment)
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

        // The data plane takes the ceilings at create and nowhere else, so a declaration that
        // stops here is one the sandbox never hears about. Emitted only when declared: absent
        // means the data plane's own default, which is not the same as asserting a size.
        if let Some(limits) = sandbox.limits.as_ref() {
            fields.push(("cpu", Expression::String(limits.cpu.clone())));
            fields.push(("memory", Expression::String(limits.memory.clone())));
            fields.push(("disk", Expression::String(limits.disk.clone())));
        }

        Ok(Some(expr::object(fields)))
    }
}

/// Attaches this sandbox's remote grant to the stack's shared Remote Bindings identity.
///
/// Scoped to the group this emitter just created, and nothing wider. The role is a data-plane one
/// covering `sandboxGroups/*` on whatever it is scoped to, so a resource-group scope would hand a
/// remote caller every sibling sandbox in the deployment — the assignment names the group by
/// reference so the scope cannot drift from the resource.
fn emit_remote_access(ctx: &EmitContext<'_>, label: &str, fragment: &mut TfFragment) -> Result<()> {
    let (Some(definition), Some(access_label)) = (
        alien_core::remote_bindings::remote_binding_is_deliverable(ctx.resource)
            .then(|| alien_core::remote_bindings::remote_binding_for_entry(ctx.resource))
            .flatten(),
        remote_bindings_label(ctx),
    ) else {
        return Ok(());
    };
    let permission_set = alien_permissions::get_permission_set(definition.permission_set)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "{} permission set is not registered",
                    definition.permission_set
                ),
            })
        })?;

    let context = permission_context(label).with_resource_name(ctx.resource_id.to_string());
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "failed to generate Azure remote Sandbox permissions".to_string(),
        })?;

    emit_remote_bindings_role_definitions(fragment, permission_set)?;
    for (index, binding) in plan.bindings.iter().enumerate() {
        let role_definition_id = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                expr::template(role_definition_id.clone())
            }
            AzureRoleDefinitionRef::Custom { key } => {
                let custom_index = plan
                    .custom_roles
                    .iter()
                    .position(|role| &role.key == key)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: format!("missing generated Azure role '{key}'"),
                        })
                    })?;
                let role_label = remote_bindings_role_label(&binding.role_name, custom_index);
                expr::traversal([
                    "azurerm_role_definition",
                    role_label.as_str(),
                    "role_definition_resource_id",
                ])
            }
        };
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_access_{index}"),
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"oid\", \"deployment:azure:sandbox-access:${{local.resource_prefix}}:{label}:{index}\")"
                    )),
                ),
                // The created group rather than the rendered scope string: both spell the same
                // name, and referencing it is what orders the assignment after the group Azure
                // refuses to grant on before it exists.
                attr("scope", expr::traversal(["azapi_resource", label, "id"])),
                attr("role_definition_id", role_definition_id),
                attr(
                    "principal_id",
                    expr::traversal([
                        "azurerm_user_assigned_identity",
                        access_label,
                        "principal_id",
                    ]),
                ),
            ],
        ));
    }

    Ok(())
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
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
                    // Declared, so the ceilings are in the rendered binding: Azure takes them at
                    // create and nowhere else, and the key-coverage assertion below is what pins
                    // that they travel under the names the binding deserializes.
                    .limits(alien_core::SandboxLimits {
                        cpu: "1000m".to_string(),
                        memory: "2048Mi".to_string(),
                        disk: "20480Mi".to_string(),
                        max_processes: None,
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
            cpu: Some(BindingValue::Value("1000m".to_string())),
            memory: Some(BindingValue::Value("2048Mi".to_string())),
            disk: Some(BindingValue::Value("20480Mi".to_string())),
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
