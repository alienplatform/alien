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

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, remote_bindings_role_label, required_label,
        resource_prefix_template, service_account_principal_id, setup_execution_role_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, AzureServiceBusNamespace, ErrorData, PermissionProfile, PermissionSet,
    PermissionSetReference, Queue, RemoteBindings, Result, Worker, WorkerTrigger,
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
        emit_profile_grants(ctx, &mut fragment, label)?;
        emit_remote_access(ctx, &mut fragment, label)?;
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
                WorkerTrigger::Queue { queue, .. }
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

/// Grants that permission profiles hold on this queue, for example a Worker identity's
/// `queue/data-read`. Setup creates the queue, and an imported queue starts out Ready, so
/// its controller never applies them at runtime; setup must, as it does for Storage.
fn emit_profile_grants(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
) -> Result<()> {
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = queue_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        let Some(principal_id) = service_account_principal_id(ctx, profile_name) else {
            continue;
        };
        for permission_set_ref in refs {
            let permission_set = resolve_permission_set(permission_set_ref, ctx.resource_id)?;
            if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none()
            {
                continue;
            }
            let grant_plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context(label).with_resource_name(format!(
                        "${{local.resource_prefix}}-{}",
                        ctx.resource_id
                    )),
                )
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure queue grants for '{}'",
                        permission_set.id
                    ),
                })?;
            for (index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let role_index = grant_plan
                            .custom_roles
                            .iter()
                            .position(|role| &role.key == key)
                            .ok_or_else(|| {
                                AlienError::new(ErrorData::GenericError {
                                    message: format!(
                                        "Azure queue permission set '{}' references missing custom role '{key}'",
                                        permission_set.id
                                    ),
                                })
                            })?;
                        let role_label = setup_execution_role_label(
                            profile_name,
                            &binding.role_name,
                            role_index,
                        );
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };
                let role_label = sanitize_label(&binding.role_name);
                fragment.resource_blocks.push(resource_block(
                    "azurerm_role_assignment",
                    &format!("{label}_{role_label}_{profile_name}_assignment_{index}"),
                    [
                        attr(
                            "name",
                            expr::raw(format!(
                                "uuidv5(\"oid\", \"deployment:azure:queue-role-assign:${{local.resource_prefix}}:{}:{role_label}:{profile_name}:{index}\")",
                                ctx.resource_id
                            )),
                        ),
                        // A resource-scoped grant is on this queue; the permission set's
                        // scope template assumes a per-queue namespace that setup does not use.
                        attr(
                            "scope",
                            expr::traversal(["azurerm_servicebus_queue", label, "id"]),
                        ),
                        attr("role_definition_id", role_definition_id),
                        attr("principal_id", principal_id.clone()),
                    ],
                ));
            }
        }
    }
    Ok(())
}

fn queue_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    let mut refs: Vec<&PermissionSetReference> = Vec::new();
    let resource_refs = profile.0.get(resource_id).into_iter().flatten();
    let wildcard_refs = profile
        .0
        .get("*")
        .into_iter()
        .flatten()
        .filter(|permission_ref| permission_ref.id().starts_with("queue/"));
    for permission_ref in resource_refs.chain(wildcard_refs) {
        if !refs.iter().any(|seen| seen.id() == permission_ref.id()) {
            refs.push(permission_ref);
        }
    }
    refs
}

fn resolve_permission_set(
    permission_set_ref: &PermissionSetReference,
    resource_id: &str,
) -> Result<PermissionSet> {
    permission_set_ref
        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "permission set '{}' referenced by Azure queue '{}' was not found",
                    permission_set_ref.id(),
                    resource_id
                ),
            })
        })
}

fn sanitize_label(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn emit_remote_access(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str) -> Result<()> {
    let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(ctx.resource)
    else {
        return Ok(());
    };
    let access_label = ctx
        .stack
        .resources()
        .find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "Remote Queue requires its setup-owned access identity".to_string(),
            })
        })?;
    let permission_set = alien_permissions::get_permission_set(definition.permission_set)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Remote Queue permission set {} is not registered",
                    definition.permission_set
                ),
            })
        })?;
    let context = permission_context(label)
        .with_resource_name(format!("${{azurerm_servicebus_queue.{label}.name}}"));
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "Generate remote Queue table grants".to_string(),
        })?;
    for (index, binding) in plan.bindings.iter().enumerate() {
        let role_id = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                expr::template(role_definition_id.clone())
            }
            AzureRoleDefinitionRef::Custom { key } => {
                let role_index = plan
                    .custom_roles
                    .iter()
                    .position(|role| &role.key == key)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: "Missing remote Queue custom role".to_string(),
                        })
                    })?;
                let role_label = remote_bindings_role_label(&binding.role_name, role_index);
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
                    "scope",
                    expr::traversal(["azurerm_servicebus_queue", label, "id"]),
                ),
                attr("role_definition_id", role_id),
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
