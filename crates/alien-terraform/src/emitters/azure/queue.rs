//! Azure Queue — `azurerm_servicebus_queue` inside the stack's
//! `azurerm_servicebus_namespace`.
//!
//! Mirrors `AzureQueueController`:
//!
//! * Queue name = `${local.resource_prefix}-{id}`, matching
//!   [`super::helpers::resource_prefix_template`].
//! * Default lock duration / TTL / partitioning — the controller leaves
//!   them at provider defaults too; rebuild stays consistent.
//! * Parent `azurerm_servicebus_namespace` is preflight-injected as
//!   `default-service-bus-namespace`. The auxiliary
//!   [`super::service_bus_namespace::AzureServiceBusNamespaceEmitter`]
//!   realises it.
//!
//! Resource-scoped `azurerm_role_assignment` blocks for permission
//! profiles that reference this queue are emitted alongside it; wildcard
//! grants stay on the service-account identity via `stack_permission_sets`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, required_label, resource_prefix_template,
        service_account_principal_id, setup_execution_role_label, setup_management_role_label,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{
    import::EmitContext, AzureServiceBusNamespace, ErrorData, PermissionProfile,
    PermissionSetReference, Queue, RemoteStackManagement, Result, Worker, WorkerTrigger,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureQueueEmitter;

impl TfEmitter for AzureQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();

        let lock_duration = lock_duration_for(ctx);

        let q = resource_block(
            "azurerm_servicebus_queue",
            label,
            [
                attr("name", resource_prefix_template(queue.id())),
                attr(
                    "namespace_id",
                    expr::traversal(["azurerm_servicebus_namespace", &parent_label, "id"]),
                ),
                attr("partitioning_enabled", Expression::Bool(false)),
                attr(
                    "lock_duration",
                    Expression::String(format!("PT{}S", lock_duration)),
                ),
                attr(
                    "default_message_ttl",
                    Expression::String("P14D".to_string()),
                ),
                attr(
                    "max_delivery_count",
                    Expression::Number(hcl::Number::from(10i64)),
                ),
                attr(
                    "dead_lettering_on_message_expiration",
                    Expression::Bool(true),
                ),
            ],
        );

        let mut fragment = TfFragment::default().with_resource(q);
        emit_queue_permissions(ctx, queue, label, &mut fragment)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "namespaceName",
                expr::traversal(["azurerm_servicebus_namespace", &parent_label, "name"]),
            ),
            (
                "queueName",
                expr::traversal(["azurerm_servicebus_queue", label, "name"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();
        Ok(Some(expr::object([
            ("service", Expression::String("servicebus".to_string())),
            (
                "namespace",
                expr::traversal(["azurerm_servicebus_namespace", &parent_label, "name"]),
            ),
            (
                "queueName",
                expr::traversal(["azurerm_servicebus_queue", label, "name"]),
            ),
        ])))
    }
}

fn emit_queue_permissions(
    ctx: &EmitContext<'_>,
    queue: &Queue,
    queue_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, permission_set_refs) in queue_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("queue/")
                || permission_set.id.ends_with("/provision")
                || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let gate_count =
                permission_gate_count(ctx, &profile_name, &permission_set.id, &[ctx.resource_id])?;
            let grant_plan = queue_grant_plan(queue_label, &permission_set)?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index =
                            custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
                        let role_label =
                            setup_execution_role_label(&profile_name, &binding.role_name, index);
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };
                emit_role_assignment(
                    fragment,
                    queue.id(),
                    queue_label,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    role_definition_id,
                    principal_id_expr.clone(),
                    gate_count.clone(),
                );
            }
        }
    }

    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };

    let principal_id_expr = expr::traversal([
        "azurerm_user_assigned_identity",
        management_label,
        "principal_id",
    ]);

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if !permission_set.id.starts_with("queue/")
            || permission_set.id.ends_with("/provision")
            || permission_set.platforms.azure.is_none()
        {
            continue;
        }

        let grant_plan = queue_grant_plan(queue_label, &permission_set)?;

        for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    expr::template(role_definition_id.clone())
                }
                AzureRoleDefinitionRef::Custom { key } => {
                    let index =
                        custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
                    let role_label = setup_management_role_label(&binding.role_name, index);
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ])
                }
            };
            emit_role_assignment(
                fragment,
                queue.id(),
                queue_label,
                "management",
                binding_index,
                &binding.role_name,
                role_definition_id,
                principal_id_expr.clone(),
                None,
            );
        }
    }

    Ok(())
}

fn queue_grant_plan(
    queue_label: &str,
    permission_set: &alien_core::PermissionSet,
) -> Result<alien_permissions::generators::AzureGrantPlan> {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_context = permission_context(queue_label)
        .with_resource_name(format!("${{azurerm_servicebus_queue.{queue_label}.name}}"));
    generator
        .generate_grant_plan(permission_set, BindingTarget::Resource, &permission_context)
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform".to_string(),
            reason: format!(
                "failed to generate Azure queue grants for '{}'",
                permission_set.id
            ),
        })
}

fn custom_role_index(
    custom_roles: &[alien_permissions::generators::AzureCustomRole],
    key: &str,
    permission_set_id: &str,
) -> Result<usize> {
    custom_roles
        .iter()
        .position(|role| role.key == key)
        .ok_or_else(|| {
            AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "Terraform".to_string(),
                reason: format!(
                    "Azure queue permission set '{}' generated a binding for missing custom role '{}'",
                    permission_set_id, key
                ),
            })
        })
}

fn emit_role_assignment(
    fragment: &mut TfFragment,
    queue_id: &str,
    queue_label: &str,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    role_definition_id: Expression,
    principal_id_expr: Expression,
    gate_count: Option<Expression>,
) {
    let role_label = sanitize_role_label(role_name);
    let mut body = Vec::new();
    if let Some(gate_count) = gate_count {
        body.push(attr("count", gate_count));
    }
    body.extend([
        attr(
            "name",
            expr::raw(&format!(
                "uuidv5(\"oid\", \"deployment:azure:queue-role-assign:${{local.resource_prefix}}:{queue_id}:{role_label}:{principal_label}:{binding_index}\")"
            )),
        ),
        // Scope mirrors the runtime controller's explicit ARM scope (the
        // Service Bus queue id), not the jsonc resource-scope template.
        attr(
            "scope",
            expr::traversal(["azurerm_servicebus_queue", queue_label, "id"]),
        ),
        attr("role_definition_id", role_definition_id),
        attr("principal_id", principal_id_expr),
    ]);
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{queue_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        body,
    ));
}

fn queue_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        owners.push((profile_name.clone(), refs));
    }
    owners
}

fn management_permission_refs(ctx: &EmitContext<'_>) -> Vec<PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    resource_permission_refs(profile, ctx.resource_id)
}

fn resource_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }
    refs
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn sanitize_role_label(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn parent_namespace_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry
            .config
            .downcast_ref::<AzureServiceBusNamespace>()
            .is_some()
        {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    // Missing sibling resource is a stack-authoring mistake the developer can
    // fix, so keep it external-safe rather than an internal serialization error.
    Err(AlienError::new(ErrorData::GenericError {
        message:
            "Azure Queue resource requires a sibling `azure_service_bus_namespace` resource in \
             the stack (preflight-injected as `default-service-bus-namespace`)"
                .to_string(),
    }))
}

/// Service Bus queue lock duration must be in `[5s, 5m]`. Use the
/// max-consumer-function timeout × 2, clamped to the supported range.
fn lock_duration_for(ctx: &EmitContext<'_>) -> u32 {
    let mut max_function_timeout = 0u32;
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Worker>() else {
            continue;
        };
        if function.triggers.iter().any(|trigger| {
            matches!(
                trigger,
                WorkerTrigger::Queue { queue }
                    if queue.resource_type == Queue::RESOURCE_TYPE && queue.id == ctx.resource_id
            )
        }) {
            max_function_timeout = max_function_timeout.max(function.timeout_seconds);
        }
    }
    if max_function_timeout == 0 {
        return 30;
    }
    max_function_timeout.saturating_mul(2).clamp(5, 300)
}
