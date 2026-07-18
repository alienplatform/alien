//! Azure Build — `azurerm_container_registry_task`.
//!
//! Each Alien `Build` becomes one ACR Task hanging off the parent
//! `azurerm_container_registry` (preflight ensures one of those is in
//! the stack). The task uses the `MultiStep` definition with a
//! placeholder build step; the controller swaps in the real build
//! command at apply time. Environment variables surface through the
//! task's `encoded_step.values` map.
//!
//! Authentication: the parent registry has admin disabled (see
//! [`super::artifact_registry`]). The task uses the registry's
//! system-assigned identity for cross-registry push (`run_on_acr`).

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{container_registry_task_name_template, downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, ArtifactRegistry, Build, ErrorData, Result};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureBuildEmitter;

impl TfEmitter for AzureBuildEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let registry_label = parent_registry_label(ctx)?.to_string();

        // Build a YAML values document from `build.environment` so the
        // ACR Task can interpolate `{{ .Values.<KEY> }}` placeholders.
        // The runtime controller overwrites both `task_content` and
        // `value_content` at apply time; this is the minimal valid
        // pair so `terraform validate` accepts the rendered module.
        let value_content = build
            .environment
            .iter()
            .map(|(k, v)| format!("{}: \"{}\"", k, v.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join("\n");

        let mut encoded_step_body: Vec<hcl::structure::Structure> = vec![attr(
            "task_content",
            Expression::String(default_task_content().to_string()),
        )];
        if !value_content.is_empty() {
            encoded_step_body.push(attr("value_content", Expression::String(value_content)));
        }

        let body: Vec<hcl::structure::Structure> = vec![
            attr("name", container_registry_task_name_template(&build.id)),
            attr(
                "container_registry_id",
                expr::traversal(["azurerm_container_registry", &registry_label, "id"]),
            ),
            attr("is_system_task", Expression::Bool(false)),
            nested(block(
                "platform",
                [
                    attr("os", Expression::String("Linux".to_string())),
                    attr("architecture", Expression::String("amd64".to_string())),
                ],
            )),
            nested(block("encoded_step", encoded_step_body)),
            nested(block(
                "identity",
                [attr(
                    "type",
                    Expression::String("SystemAssigned".to_string()),
                )],
            )),
        ];

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_container_registry_task",
            label,
            body,
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let registry_label = parent_registry_label(ctx)?.to_string();
        let env_pairs: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Expression::String(v.clone())))
            .collect();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "registryName",
                expr::traversal(["azurerm_container_registry", &registry_label, "name"]),
            ),
            (
                "taskName",
                expr::traversal(["azurerm_container_registry_task", label, "name"]),
            ),
            (
                "buildEnvVars",
                expr::object(env_pairs.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let registry_label = parent_registry_label(ctx)?.to_string();
        let env_pairs: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Expression::String(v.clone())))
            .collect();
        Ok(Some(expr::object([
            ("service", Expression::String("aca".to_string())),
            ("managedEnvironmentId", Expression::String(String::new())),
            (
                "resourceGroupName",
                expr::raw("var.azure_resource_group_name"),
            ),
            (
                "buildEnvVars",
                expr::object(env_pairs.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
            ("managedIdentityId", Expression::Null),
            (
                "resourcePrefix",
                expr::traversal(["azurerm_container_registry", &registry_label, "name"]),
            ),
            ("monitoring", Expression::Null),
        ])))
    }
}

/// Look up the first `ArtifactRegistry` resource in the stack and
/// return its Terraform label. ACR Tasks live inside the registry, so
/// the build emitter borrows whatever registry the stack already
/// declared.
fn parent_registry_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<ArtifactRegistry>().is_some() {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    Err(AlienError::new(ErrorData::GenericError {
        message:
            "Azure Build resource requires a sibling `artifact_registry` (ACR) resource in the \
             stack — ACR Tasks must be created inside an existing registry"
                .to_string(),
    }))
}

/// Placeholder ACR Task YAML. The runtime controller's `Build` flow
/// supplies the real task definition at apply time via
/// `RunRequest.encoded_step`. The emitter just needs a syntactically
/// valid placeholder so `terraform apply` succeeds before the controller
/// drops in the real one.
fn default_task_content() -> &'static str {
    "version: v1.1.0\nsteps:\n  - cmd: echo \"alien build placeholder\"\n"
}
