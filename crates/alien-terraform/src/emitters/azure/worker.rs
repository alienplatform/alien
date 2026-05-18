//! Azure Worker — `azurerm_container_app` inside the stack's
//! Container Apps Environment.
//!
//! Mirrors `AzureWorkerController`:
//!
//! * Image-only (no source-build wiring); customers pin the image at
//!   the resource level.
//! * Public ingress maps to `external_enabled = true` with target_port
//!   8080 (the controller's default). Private maps to
//!   `external_enabled = false`, only reachable from the environment's
//!   own VNet.
//! * Workload identity binds the matching `azurerm_user_assigned_identity`
//!   when a `<profile>-sa` exists in the stack.

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        binding_string_value, container_apps_environment_binding, downcast, required_label,
        stack_name_template, tags,
    },
    emitters::worker_environment::{worker_environment, AzureWorkerEnvironmentRenderer},
    expr,
    registry::TfRegistry,
};
use alien_core::{
    import::EmitContext, AzureContainerAppsEnvironment, ErrorData, Ingress, Result, ServiceAccount,
    Worker, WorkerCode,
};
use alien_error::AlienError;
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
    Identifier,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureWorkerEmitter;

impl TfEmitter for AzureWorkerEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = TfRegistry::built_in();
        self.emit_with_registry(ctx, &registry)
    }

    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &TfRegistry,
    ) -> Result<TfFragment> {
        let function = downcast::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let WorkerCode::Image { image } = &function.code else {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_terraform_module".to_string(),
                reason: format!(
                    "worker '{}' uses source code; Terraform modules require a pre-built image",
                    function.id
                ),
            }));
        };
        let label = required_label(ctx)?;
        let (env_id, env_label) = parent_environment(ctx)?;
        let env_label = env_label.to_string();
        let client_config_label = format!("{label}_current");
        let service_account_label = sa_label_for(ctx, &function.permissions);
        let container_app_environment_id =
            if let Some(binding) = container_apps_environment_binding(ctx, env_id)? {
                binding_string_value(env_id, "resource_id", &binding.resource_id)
                    .map(Expression::String)?
            } else {
                expr::traversal(["azurerm_container_app_environment", &env_label, "id"])
            };

        // Container env-var blocks (one per K/V pair).
        let env_renderer = AzureWorkerEnvironmentRenderer {
            ctx,
            registry,
            worker_id: &function.id,
            client_config_label: &client_config_label,
            service_account_label: service_account_label.as_deref(),
        };
        let env_blocks: Vec<Structure> =
            worker_environment(function, alien_core::Platform::Azure, &env_renderer)?
                .into_iter()
                .map(|(k, v)| {
                    nested(block(
                        "env",
                        [attr("name", Expression::String(k)), attr("value", v)],
                    ))
                })
                .collect();

        let mut container_body: Vec<Structure> = vec![
            attr(
                "name",
                Expression::String(sanitize_container_name(&function.id)),
            ),
            attr("image", Expression::String(image.clone())),
            attr(
                "cpu",
                Expression::Number(azure_container_app_cpu(function.memory_mb)),
            ),
            attr(
                "memory",
                Expression::String(azure_container_app_memory(function.memory_mb)),
            ),
        ];
        container_body.extend(env_blocks);

        let template_body: Vec<Structure> = vec![
            attr("min_replicas", Expression::Number(hcl::Number::from(0i64))),
            attr("max_replicas", Expression::Number(hcl::Number::from(10i64))),
            nested(block("container", container_body)),
        ];

        let public = matches!(function.ingress, Ingress::Public);
        let ingress_body: Vec<Structure> = vec![
            attr("external_enabled", Expression::Bool(public)),
            attr(
                "target_port",
                Expression::Number(hcl::Number::from(8080i64)),
            ),
            attr("transport", Expression::String("auto".to_string())),
            nested(block(
                "traffic_weight",
                [
                    attr("latest_revision", Expression::Bool(true)),
                    attr("percentage", Expression::Number(hcl::Number::from(100i64))),
                ],
            )),
        ];

        let mut body: Vec<Structure> = vec![
            attr("name", stack_name_template(&function.id)),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("container_app_environment_id", container_app_environment_id),
            attr("revision_mode", Expression::String("Single".to_string())),
            nested(block("template", template_body)),
            nested(block("ingress", ingress_body)),
            attr("tags", tags(ctx, "worker")),
        ];

        // Bind the user-assigned identity when the worker has a
        // matching service account in the stack. AzureRM requires
        // `identity_ids` to be a list of identity resource ids; we use
        // a `set` because Terraform's identity block expects sets.
        if let Some(sa_label) = &service_account_label {
            body.push(nested(Block {
                identifier: Identifier::sanitized("identity"),
                labels: vec![],
                body: hcl::structure::Body::from(vec![
                    attr("type", Expression::String("UserAssigned".to_string())),
                    attr(
                        "identity_ids",
                        Expression::Array(vec![expr::traversal([
                            "azurerm_user_assigned_identity",
                            sa_label,
                            "id",
                        ])]),
                    ),
                ]),
            }));
        }

        let mut fragment = TfFragment::default().with_data(data_block(
            "azurerm_client_config",
            &client_config_label,
            Vec::<Structure>::new(),
        ));
        fragment
            .resource_blocks
            .push(resource_block("azurerm_container_app", label, body));
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let function = downcast::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let fqdn = if matches!(function.ingress, Ingress::Public) {
            expr::traversal(["azurerm_container_app", label, "latest_revision_fqdn"])
        } else {
            Expression::Null
        };

        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "containerAppName",
                expr::traversal(["azurerm_container_app", label, "name"]),
            ),
            ("fqdn", fqdn),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let function = downcast::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let (env_id, env_label) = parent_environment(ctx)?;
        let private_url = if let Some(binding) = container_apps_environment_binding(ctx, env_id)? {
            let default_domain =
                binding_string_value(env_id, "default_domain", &binding.default_domain)?;
            expr::template(format!(
                "https://${{azurerm_container_app.{label}.name}}.{default_domain}"
            ))
        } else {
            expr::template(format!(
                "https://${{azurerm_container_app.{label}.name}}.${{azurerm_container_app_environment.{env_label}.default_domain}}"
            ))
        };
        let mut fields = vec![
            (
                "service".to_string(),
                Expression::String("containerapp".to_string()),
            ),
            (
                "subscriptionId".to_string(),
                expr::raw("var.azure_subscription_id"),
            ),
            (
                "resourceGroupName".to_string(),
                expr::raw("var.azure_resource_group_name"),
            ),
            (
                "containerAppName".to_string(),
                expr::traversal(["azurerm_container_app", label, "name"]),
            ),
            ("privateUrl".to_string(), private_url),
        ];
        if matches!(function.ingress, Ingress::Public) {
            fields.push((
                "publicUrl".to_string(),
                expr::template(format!(
                    "https://${{azurerm_container_app.{label}.latest_revision_fqdn}}"
                )),
            ));
        }
        Ok(Some(expr::object(
            fields
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        )))
    }
}

fn parent_environment<'a>(ctx: &EmitContext<'a>) -> Result<(&'a str, &'a str)> {
    for (id, entry) in ctx.stack.resources() {
        if entry
            .config
            .downcast_ref::<AzureContainerAppsEnvironment>()
            .is_some()
        {
            if let Some(label) = ctx.name_for(id) {
                return Ok((id, label));
            }
        }
    }
    Err(AlienError::new(ErrorData::GenericError {
        message: "Azure Worker resource requires a sibling `azure_container_apps_environment` \
             resource in the stack (preflight-injected as \
             `default-container-apps-environment`)"
            .to_string(),
    }))
}

fn sa_label_for(ctx: &EmitContext<'_>, profile_name: &str) -> Option<String> {
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(&service_account_id).map(|s| s.to_string())
}

/// Container Apps container names must be lowercase alphanumeric +
/// hyphens, max 63 chars. Sanitise the worker id so any `_` /
/// uppercase isn't rejected at apply time.
fn sanitize_container_name(input: &str) -> String {
    let mut out: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if out.len() > 63 {
        out.truncate(63);
    }
    if out.is_empty() {
        out.push_str("worker");
    }
    out
}

fn azure_container_app_memory(memory_mb: u32) -> String {
    let memory_gi = azure_container_app_memory_gi(memory_mb);
    let rounded = (memory_gi * 100.0).round() / 100.0;
    if rounded.fract().abs() < f64::EPSILON {
        format!("{rounded:.0}Gi")
    } else if (rounded * 10.0).fract().abs() < f64::EPSILON {
        format!("{rounded:.1}Gi")
    } else {
        format!("{rounded:.2}Gi")
    }
}

fn azure_container_app_cpu(memory_mb: u32) -> hcl::Number {
    hcl::Number::from_f64(azure_container_app_memory_gi(memory_mb) / 2.0)
        .unwrap_or_else(|| hcl::Number::from(0))
}

fn azure_container_app_memory_gi(memory_mb: u32) -> f64 {
    f64::from(memory_mb.max(512)) / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_container_app_resources_from_memory() {
        assert_eq!(azure_container_app_memory(256), "0.5Gi");
        assert_eq!(
            azure_container_app_cpu(256),
            hcl::Number::from_f64(0.25).unwrap()
        );
        assert_eq!(azure_container_app_memory(512), "0.5Gi");
        assert_eq!(
            azure_container_app_cpu(512),
            hcl::Number::from_f64(0.25).unwrap()
        );
        assert_eq!(azure_container_app_memory(1536), "1.5Gi");
        assert_eq!(
            azure_container_app_cpu(1536),
            hcl::Number::from_f64(0.75).unwrap()
        );
        assert_eq!(azure_container_app_memory(2048), "2Gi");
        assert_eq!(
            azure_container_app_cpu(2048),
            hcl::Number::from_f64(1.0).unwrap()
        );
    }
}
