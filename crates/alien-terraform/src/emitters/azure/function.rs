//! Azure Function — `azurerm_container_app` inside the stack's
//! Container Apps Environment.
//!
//! Mirrors `AzureFunctionController`:
//!
//! * Image-only (no source-build wiring); customers pin the image at
//!   the resource level.
//! * Public ingress maps to `external_enabled = true` with target_port
//!   8080 (the controller's default). Private maps to
//!   `external_enabled = false`, only reachable from the environment's
//!   own VNet.
//! * Workload identity binds the matching `azurerm_user_assigned_identity`
//!   when a `<profile>-sa` exists in the stack — the helper resolves
//!   the principal via [`super::helpers::service_account_principal_id`].

use std::collections::BTreeMap;

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, required_label, service_account_principal_id, stack_name_template, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, AzureContainerAppsEnvironment, ErrorData, Function, FunctionCode, Ingress,
    Result,
};
use alien_error::AlienError;
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
    Identifier,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureFunctionEmitter;

impl TfEmitter for AzureFunctionEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let FunctionCode::Image { image } = &function.code else {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_terraform_module".to_string(),
                reason: format!(
                    "function '{}' uses source code; Terraform modules require a pre-built image",
                    function.id
                ),
            }));
        };
        let label = required_label(ctx)?;
        let env_label = parent_environment_label(ctx)?.to_string();

        let principal_id_expr = service_account_principal_id(ctx, &function.permissions);

        // Container env-var blocks (one per K/V pair).
        let env_blocks: Vec<Structure> = function_environment(function)
            .into_iter()
            .map(|(k, v)| {
                nested(block(
                    "env",
                    [
                        attr("name", Expression::String(k)),
                        attr("value", Expression::String(v)),
                    ],
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
                Expression::Number(
                    hcl::Number::from_f64(0.5).unwrap_or_else(|| hcl::Number::from(0)),
                ),
            ),
            attr(
                "memory",
                Expression::String(format!("{}Mi", function.memory_mb.max(128))),
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
            attr(
                "container_app_environment_id",
                expr::traversal(["azurerm_container_app_environment", &env_label, "id"]),
            ),
            attr("revision_mode", Expression::String("Single".to_string())),
            nested(block("template", template_body)),
            nested(block("ingress", ingress_body)),
            attr("tags", tags(ctx, "function")),
        ];

        // Bind the user-assigned identity when the function has a
        // matching service account in the stack. AzureRM requires
        // `identity_ids` to be a list of identity resource ids; we use
        // a `set` because Terraform's identity block expects sets.
        if let Some(_principal_id) = &principal_id_expr {
            // Resolve the SA's UAMI label by stripping `principal_id`
            // off the traversal — easier to derive from the helper's
            // output than to recompute. For simplicity, look it up
            // again by Stack iteration.
            if let Some(sa_label) = sa_label_for(ctx, &function.permissions) {
                body.push(nested(Block {
                    identifier: Identifier::sanitized("identity"),
                    labels: vec![],
                    body: hcl::structure::Body::from(vec![
                        attr("type", Expression::String("UserAssigned".to_string())),
                        attr(
                            "identity_ids",
                            Expression::Array(vec![expr::traversal([
                                "azurerm_user_assigned_identity",
                                &sa_label,
                                "id",
                            ])]),
                        ),
                    ]),
                }));
            }
        }

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_container_app",
            label,
            body,
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
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
}

fn parent_environment_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry
            .config
            .downcast_ref::<AzureContainerAppsEnvironment>()
            .is_some()
        {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    Err(AlienError::new(ErrorData::GenericError {
        message: "Azure Function resource requires a sibling `azure_container_apps_environment` \
             resource in the stack (preflight-injected as \
             `default-container-apps-environment`)"
            .to_string(),
    }))
}

fn sa_label_for(ctx: &EmitContext<'_>, profile_name: &str) -> Option<String> {
    let service_account_id = format!("{profile_name}-sa");
    ctx.name_for(&service_account_id).map(|s| s.to_string())
}

fn function_environment(function: &Function) -> BTreeMap<String, String> {
    let mut env = function
        .environment
        .clone()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    env.insert("ALIEN_TRANSPORT".to_string(), "container-app".to_string());
    env.insert("ALIEN_RUNTIME_SEND_OTLP".to_string(), "true".to_string());
    env
}

/// Container Apps container names must be lowercase alphanumeric +
/// hyphens, max 63 chars. Sanitise the function id so any `_` /
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
        out.push_str("function");
    }
    out
}
