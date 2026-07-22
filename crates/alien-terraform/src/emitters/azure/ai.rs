//! Azure AI — Azure AIServices (Cognitive Services) account.
//!
//! Mirrors `AzureAiController`: one `azurerm_cognitive_account` per `Ai`
//! resource, configured as `kind = "AIServices"` with SKU `S0` and a
//! `custom_subdomain_name` derived from the stack resource prefix and the
//! resource id.
//!
//! Resource-scoped role assignments (e.g. `Cognitive Services OpenAI User` for
//! `ai/invoke`) are emitted directly by this emitter, scoped to the cognitive
//! account, for each permission profile that references `ai/invoke` on this
//! resource. Stack-level permissions flow through `AzureServiceAccountEmitter`
//! via `stack_permission_sets`.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, required_label, resource_prefix_template,
        service_account_principal_id, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, Ai, ErrorData, PermissionSetReference, Result};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureAiEmitter;

impl TfEmitter for AzureAiEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let ai = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azurerm_cognitive_account",
            label,
            [
                attr("name", resource_prefix_template(ai.id())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("kind", Expression::String("AIServices".to_string())),
                attr("sku_name", Expression::String("S0".to_string())),
                attr("custom_subdomain_name", resource_prefix_template(ai.id())),
                attr("tags", tags(ctx, "ai")),
            ],
        ));

        // One deployment per curated model, mirroring AzureAiController's
        // DeployingModels state. capacity = 1 matches the controller's
        // DEFAULT_DEPLOYMENT_CAPACITY so the Frozen (Terraform) and Live
        // (controller) paths provision the same throughput.
        for (deployment, model, version) in alien_core::ai_catalog::azure_deployments() {
            fragment.resource_blocks.push(resource_block(
                "azurerm_cognitive_deployment",
                // The Azure deployment name keeps dots (e.g. gpt-4.1); the Terraform
                // resource label cannot, so sanitize it.
                &format!("{label}_{}", sanitize_deployment_label(deployment)),
                [
                    attr("name", Expression::String(deployment.to_string())),
                    attr(
                        "cognitive_account_id",
                        expr::traversal(["azurerm_cognitive_account", label, "id"]),
                    ),
                    nested(block(
                        "model",
                        [
                            attr("format", Expression::String("OpenAI".to_string())),
                            attr("name", Expression::String(model.to_string())),
                            attr("version", Expression::String(version.to_string())),
                        ],
                    )),
                    nested(block(
                        "sku",
                        [
                            attr("name", Expression::String("GlobalStandard".to_string())),
                            attr("capacity", expr::raw("1")),
                        ],
                    )),
                ],
            ));
        }

        emit_ai_permissions(ctx, label, &mut fragment)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "accountName",
                expr::traversal(["azurerm_cognitive_account", label, "name"]),
            ),
            (
                "endpoint",
                expr::traversal(["azurerm_cognitive_account", label, "endpoint"]),
            ),
            ("location", expr::raw("var.azure_location")),
        ]))
    }
}

/// Emit `azurerm_role_assignment` blocks for permission profiles that reference
/// `ai/invoke` (or any `ai/`-prefixed set) scoped to this resource.
fn emit_ai_permissions(
    ctx: &EmitContext<'_>,
    ai_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let Some(principal_id_expr) = service_account_principal_id(ctx, profile_name) else {
            continue;
        };

        let refs: Vec<&PermissionSetReference> = profile
            .0
            .get(ctx.resource_id)
            .map(|refs| refs.iter().collect())
            .unwrap_or_default();

        for permission_set_ref in refs {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.platforms.azure.is_none() {
                continue;
            }

            let generator = AzureRuntimePermissionsGenerator::new();
            let perm_context = permission_context(ai_label).with_resource_name(
                format!("${{local.resource_prefix}}-{}", ctx.resource_id),
            );
            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, &perm_context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure AI grants for '{}'",
                        permission_set.id
                    ),
                })?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        // Custom role definitions for resource-scoped AI grants are
                        // emitted by emit_azure_setup_resource_role_definitions.
                        let index = grant_plan
                            .custom_roles
                            .iter()
                            .position(|role| role.key == *key)
                            .ok_or_else(|| AlienError::new(ErrorData::GenericError {
                                message: format!(
                                    "custom role '{key}' not found in grant plan for Azure AI resource '{}'",
                                    ctx.resource_id
                                ),
                            }))?;
                        let role_label = crate::emitters::azure::helpers::setup_execution_role_label(
                            profile_name,
                            &binding.role_name,
                            index,
                        );
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };

                let role_label = sanitize_role_label(&binding.role_name);
                fragment.resource_blocks.push(resource_block(
                    "azurerm_role_assignment",
                    &format!("{ai_label}_{role_label}_{profile_name}_assignment_{binding_index}"),
                    [
                        attr(
                            "name",
                            expr::raw(&format!(
                                "uuidv5(\"oid\", \"deployment:azure:ai-role-assign:${{azurerm_cognitive_account.{ai_label}.id}}:{role_label}:{profile_name}:{binding_index}\")"
                            )),
                        ),
                        attr(
                            "scope",
                            expr::traversal(["azurerm_cognitive_account", ai_label, "id"]),
                        ),
                        attr("role_definition_id", role_definition_id),
                        attr("principal_id", principal_id_expr.clone()),
                    ],
                ));
            }
        }
    }

    Ok(())
}

/// Turn a model deployment name into a valid Terraform resource label (letters,
/// digits, underscores, dashes): `gpt-4.1` becomes `gpt-4_1`.
fn sanitize_deployment_label(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
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
