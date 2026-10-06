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
        remote_bindings_role_label, remote_stack_management_label, required_label,
        sanitize_role_label, service_account_principal_id, setup_execution_role_label,
        setup_management_role_label, supports_azure_resource_binding, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSet, PermissionSetReference,
    RemoteBindings, Result, Sandbox, SandboxEgress,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;
use std::collections::HashSet;

/// The preview API version the sandbox group is created at. Must match the ARM and data-plane
/// clients: ARM still answers older previews, so a version that drifts here fails as a response
/// mismatch, not a rejected request.
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
///
/// Normalized the way every other Azure resource names itself: a deployer's prefix may carry
/// underscores and uppercase Azure rejects, and this is the form `PERMISSIONS.md` states as the
/// grant's scope, which a security team approves the grant from.
fn sandbox_group(ctx: &EmitContext<'_>) -> Expression {
    expr::raw(sandbox_group_expression(ctx.resource_id))
}

/// The same name as an interpolation, for a permission scope built as a string.
fn sandbox_group_name(ctx: &EmitContext<'_>) -> String {
    format!("${{{}}}", sandbox_group_expression(ctx.resource_id))
}

fn sandbox_group_expression(resource_id: &str) -> String {
    format!("replace(lower(\"${{local.resource_prefix}}-{resource_id}\"), \"_\", \"-\")")
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
                // The azapi provider's bundled schema index doesn't carry this type yet, so validate
                // fails with "can't be found" though ARM creates it fine. Only this client-side
                // pre-check is disabled; remove once azapi's index catches up.
                attr("schema_validation_enabled", Expression::Bool(false)),
            ],
        ));

        emit_remote_access(ctx, label, &mut fragment)?;
        emit_image_management(ctx, label, &mut fragment)?;
        emit_workload_access(ctx, label, &mut fragment)?;
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
        // The declared value as is: the provider resolves a registry image to the disk image the
        // controller built from it.
        let disk_image = sandbox.azure_image()?.as_str().to_string();
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

        if let Some(seconds) = sandbox.lifecycle.idle_pause_seconds {
            fields.push((
                "idlePauseSeconds",
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
/// Scoped to the group this emitter just created, and nothing wider: the role is a data-plane
/// one covering `sandboxGroups/*` on whatever it's scoped to, so a resource-group scope would
/// hand a remote caller every sibling sandbox in the deployment.
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

    let context = permission_context(label).with_resource_name(sandbox_group_name(ctx));
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

/// Lets the stack's management identity build and delete this sandbox's disk images, on its
/// group only. The role definition is rendered with the other setup-owned management roles; this
/// adds the assignment, which has to follow the group it names.
fn emit_image_management(
    ctx: &EmitContext<'_>,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    const IMAGES: &str = "sandbox/images";
    let granted = ctx
        .stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(ctx.resource_id))
        // By registry name, never by `id()`: an inline set carries whatever id its author typed.
        .is_some_and(|refs| {
            refs.iter().any(|reference| {
                matches!(reference, alien_core::permissions::PermissionSetReference::Name(name) if name == IMAGES)
            })
        });
    // No management resource means no remote manager identity: the deploying credentials run the
    // controller, so there is no member to bind.
    let Some(management_label) = granted
        .then(|| remote_stack_management_label(ctx))
        .flatten()
    else {
        return Ok(());
    };
    let permission_set = alien_permissions::get_permission_set(IMAGES).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("{IMAGES} permission set is not registered"),
        })
    })?;

    let context = permission_context(label).with_resource_name(sandbox_group_name(ctx));
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "failed to generate Azure sandbox image permissions".to_string(),
        })?;

    // Each assignment below hard-codes the group scope, so any other binding count is refused
    // rather than rendered at a scope the set did not declare.
    if plan.bindings.len() != 1 {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "{IMAGES} must bind exactly once, on the sandbox group; it binds {} times",
                plan.bindings.len()
            ),
        }));
    }
    for (index, binding) in plan.bindings.iter().enumerate() {
        let AzureRoleDefinitionRef::Custom { key } = &binding.role_definition else {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "{IMAGES} must use a custom role: no predefined one is narrow enough"
                ),
            }));
        };
        let custom_index = plan
            .custom_roles
            .iter()
            .position(|role| &role.key == key)
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("missing generated Azure role '{key}'"),
                })
            })?;
        let role_label = setup_management_role_label(&binding.role_name, custom_index);
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_images_{index}"),
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"oid\", \"deployment:azure:sandbox-images:${{local.resource_prefix}}:{label}:{index}\")"
                    )),
                ),
                // The created group, which orders the assignment after it as the remote grant's is.
                attr("scope", expr::traversal(["azapi_resource", label, "id"])),
                attr(
                    "role_definition_id",
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ]),
                ),
                attr(
                    "principal_id",
                    expr::traversal([
                        "azurerm_user_assigned_identity",
                        management_label,
                        "principal_id",
                    ]),
                ),
            ],
        ));
    }

    Ok(())
}

/// Grants each workload identity the `sandbox/*` sets it holds on this group.
///
/// The service-account emitter delivers a profile's `"*"` sets at the resource group, which is as
/// narrow as an Azure stack binding gets, so from `"*"` only sets with no stack binding land here:
/// `sandbox/execute`, whose role reaches inside a sandbox. Such a holder gets one assignment per
/// sandbox group in the deployment and never one at the resource group. An entry keyed by this
/// sandbox grants every resource-bound set it names on this group alone. The profile is the
/// grant; a Worker link is not checked. Custom role definitions are the setup-owned ones
/// `emit_setup_resource_role_definitions` renders for the same profile and set.
///
/// Each assignment is addressed by sandbox, profile and role, so reordering a profile or swapping
/// two sets that resolve to one role keeps the same Terraform address and Azure name: a changed
/// address would destroy and recreate an assignment Azure still holds, which it refuses.
///
/// Management never reaches this loop: the stack keeps that profile in `Stack::management()`,
/// outside `permission_profiles()`, and its sandbox grant is `emit_image_management`'s.
fn emit_workload_access(
    ctx: &EmitContext<'_>,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let group_scope_suffix = format!(
        "/providers/Microsoft.App/sandboxGroups/{}",
        sandbox_group_name(ctx)
    );
    let context = permission_context(label).with_resource_name(sandbox_group_name(ctx));
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let Some(principal_id) = service_account_principal_id(ctx, profile_name) else {
            continue;
        };
        // `sandbox/execute` and `sandbox/remote-execute` both resolve to the data-plane role, and
        // Azure refuses a second assignment of one role to one principal at one scope.
        let mut seen_roles = HashSet::new();
        for (name, stack_wide) in sandbox_permission_set_names(profile, ctx.resource_id)? {
            if name == "sandbox/provision" {
                if stack_wide {
                    continue;
                }
                // Provisioning creates and deletes the group itself; a workload identity holding
                // that on its own sandbox could replace the sandbox it runs against.
                return Err(refused(
                    name,
                    ctx.resource_id,
                    "a workload profile cannot hold provisioning rights on a sandbox group",
                ));
            }
            let permission_set = alien_permissions::get_permission_set(name).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "permission set '{name}' referenced by sandbox '{}' is not registered",
                        ctx.resource_id
                    ),
                })
            })?;
            if !supports_azure_resource_binding(permission_set)
                || (stack_wide && has_azure_stack_binding(permission_set))
            {
                continue;
            }

            let plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure sandbox grants for '{name}' on sandbox '{}'",
                        ctx.resource_id
                    ),
                })?;

            for binding in &plan.bindings {
                // Each assignment below is scoped to the created group, so a set declaring any
                // other resource scope is refused rather than rendered somewhere it did not ask for.
                if !binding.scope.ends_with(&group_scope_suffix) {
                    return Err(refused(
                        name,
                        ctx.resource_id,
                        &format!(
                            "the set binds on '{}', and this grant is placed on the sandbox group only",
                            binding.scope
                        ),
                    ));
                }
                let (role_segment, role_definition_id) = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        let guid = role_definition_id
                            .rsplit('/')
                            .next()
                            .unwrap_or(role_definition_id.as_str());
                        (
                            sanitize_role_label(guid),
                            expr::template(role_definition_id.clone()),
                        )
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let custom_index = plan
                            .custom_roles
                            .iter()
                            .position(|role| &role.key == key)
                            .ok_or_else(|| {
                                AlienError::new(ErrorData::GenericError {
                                    message: format!(
                                        "Azure sandbox permission set '{name}' on sandbox '{}' generated a binding for missing custom role '{key}'",
                                        ctx.resource_id
                                    ),
                                })
                            })?;
                        let role_label = setup_execution_role_label(
                            profile_name,
                            &binding.role_name,
                            custom_index,
                        );
                        let role_definition_id = expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ]);
                        (role_label, role_definition_id)
                    }
                };
                if !seen_roles.insert(role_segment.clone()) {
                    continue;
                }
                let profile_segment = sanitize_role_label(profile_name);
                fragment.resource_blocks.push(resource_block(
                    "azurerm_role_assignment",
                    &format!("{label}_{profile_segment}_{role_segment}"),
                    [
                        attr(
                            "name",
                            expr::raw(format!(
                                "uuidv5(\"oid\", \"deployment:azure:sandbox-workload:${{local.resource_prefix}}:{label}:{profile_name}:{role_segment}\")"
                            )),
                        ),
                        // Referencing the created group orders this assignment after it, as the
                        // remote grant does.
                        attr("scope", expr::traversal(["azapi_resource", label, "id"])),
                        attr("role_definition_id", role_definition_id),
                        attr("principal_id", principal_id.clone()),
                    ],
                ));
            }
        }
    }

    Ok(())
}

/// A grant the setup will not render on a sandbox group. Not retryable: the stack has to change.
fn refused(permission_set_id: &str, resource_id: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: format!("grant '{permission_set_id}' on sandbox '{resource_id}'"),
        reason: reason.to_string(),
    })
}

fn has_azure_stack_binding(permission_set: &PermissionSet) -> bool {
    permission_set
        .platforms
        .azure
        .iter()
        .flatten()
        .any(|permission| permission.binding.stack.is_some())
}

/// The built-in sets the profile grants on this sandbox, each once: every set keyed by the
/// sandbox id, then the `sandbox/*` sets keyed `"*"` the keyed entry did not already name. The
/// flag marks a `"*"` entry.
///
/// Only references by name qualify. An inline set may reuse a built-in id while granting
/// something else, and the role label an id resolves to here is the built-in set's, so a keyed
/// inline set is refused; an inline set under `"*"` is the service-account emitter's and is left
/// alone. A keyed set of another resource type has no sandbox-group scope to render on.
fn sandbox_permission_set_names<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Result<Vec<(&'a str, bool)>> {
    let mut names: Vec<(&str, bool)> = Vec::new();
    for reference in profile.0.get(resource_id).into_iter().flatten() {
        let name = match reference {
            PermissionSetReference::Name(name) if name.starts_with("sandbox/") => name.as_str(),
            PermissionSetReference::Name(name) => {
                return Err(refused(
                    name,
                    resource_id,
                    "only sandbox/* permission sets can be granted on a sandbox",
                ));
            }
            PermissionSetReference::Inline(set) => {
                return Err(refused(
                    &set.id,
                    resource_id,
                    "only built-in permission sets referenced by name can be granted on a sandbox",
                ));
            }
        };
        if !names.iter().any(|(seen, _)| *seen == name) {
            names.push((name, false));
        }
    }
    for reference in profile.0.get("*").into_iter().flatten() {
        let PermissionSetReference::Name(name) = reference else {
            continue;
        };
        if name.starts_with("sandbox/") && !names.iter().any(|(seen, _)| seen == name) {
            names.push((name.as_str(), true));
        }
    }
    Ok(names)
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
    use alien_core::{ResourceLifecycle, SandboxLifecyclePolicy, Stack, StackSettings};
    use indexmap::IndexMap;

    fn binding_for(egress: SandboxEgress) -> String {
        binding_with(egress, None)
    }

    fn emit_for(lifecycle: ResourceLifecycle) -> TfFragment {
        let stack = Stack::new("acme".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                    })
                    .egress(SandboxEgress::Allow)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                lifecycle,
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

        AzureSandboxEmitter.emit(&ctx).expect("the sandbox renders")
    }

    fn binding_with(egress: SandboxEgress, idle_pause_seconds: Option<u32>) -> String {
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
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds,
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
            idle_pause_seconds: Some(900),
            disk_image: BindingValue::Value("ubuntu".to_string()),
            cpu: Some(BindingValue::Value("1000m".to_string())),
            memory: Some(BindingValue::Value("2048Mi".to_string())),
            disk: Some(BindingValue::Value("20480Mi".to_string())),
        };
        let keys = serde_json::to_value(&binding).expect("the binding serializes");

        let keys = keys.as_object().expect("an object");
        assert!(!keys.is_empty(), "the binding serializes at least one key");
        let missing: Vec<&String> = keys
            .keys()
            .filter(|key| !rendered.contains(&format!("{key} = ")))
            .collect();
        assert!(
            missing.is_empty(),
            "the emitter never writes {missing:?}: {rendered}"
        );
    }

    /// The idle-pause policy travels the same way, and only when it was declared.
    ///
    /// Azure takes it at create, so a number that stops at the emitter leaves the sandbox on the
    /// service default — and an emitted zero would be a policy nobody asked for.
    #[test]
    fn the_binding_carries_a_declared_idle_pause_and_nothing_otherwise() {
        let declared = binding_with(SandboxEgress::Allow, Some(900));
        assert!(declared.contains("idlePauseSeconds = 900"), "{declared}");

        let undeclared = binding_with(SandboxEgress::Allow, None);
        assert!(!undeclared.contains("idlePauseSeconds"), "{undeclared}");
    }
}
