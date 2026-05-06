//! GCP Build — Cloud Build trigger driven by manual invocation.
//!
//! Cloud Build's "manual" trigger shape doesn't require a source-repo
//! webhook. Substitutions surface the controller's environment
//! variables. The build runs on the managed pool (no private worker
//! pool) — opt-in for VPC-bound builds happens at the controller level.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label, service_account_email},
    expr,
};
use alien_core::{import::EmitContext, Build, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpBuildEmitter;

impl TfEmitter for GcpBuildEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let mut substitutions: Vec<(String, Expression)> = vec![(
            "_ALIEN_BUILD_ID".to_string(),
            Expression::String(build.id.clone()),
        )];
        for (k, v) in &build.environment {
            // Cloud Build user substitution names must start with an
            // underscore and be uppercase. Sanitize aggressively.
            substitutions.push((sanitize_substitution_key(k), Expression::String(v.clone())));
        }

        let mut trigger_body: Vec<hcl::structure::Structure> = vec![
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{}", build.id)),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("location", expr::raw("var.gcp_region")),
            attr(
                "description",
                expr::template(format!("Alien {} build trigger", build.id)),
            ),
            attr(
                "substitutions",
                expr::object(substitutions.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
        ];

        // Service account binding when the build's permission profile
        // resolved to a `service-account` resource. Cloud Build accepts
        // either a project-default SA or a custom SA at the trigger
        // level.
        if let Some(email) = service_account_email(ctx, &build.permissions) {
            trigger_body.push(attr("service_account", email));
        }

        let mut steps_body: Vec<hcl::structure::Structure> = vec![nested(block(
            "step",
            [
                attr(
                    "name",
                    Expression::String("gcr.io/cloud-builders/docker".to_string()),
                ),
                attr("entrypoint", Expression::String("/bin/bash".to_string())),
                attr(
                    "args",
                    Expression::Array(vec![
                        Expression::String("-c".to_string()),
                        Expression::String(
                            "echo 'Alien build placeholder. Override via controller.'".to_string(),
                        ),
                    ]),
                ),
            ],
        ))];
        steps_body.push(nested(block(
            "options",
            [attr(
                "logging",
                Expression::String("CLOUD_LOGGING_ONLY".to_string()),
            )],
        )));
        trigger_body.push(nested(block("build", steps_body)));

        fragment.resource_blocks.push(resource_block(
            "google_cloudbuild_trigger",
            label,
            trigger_body,
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let env_pairs: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Expression::String(v.clone())))
            .collect();
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            ("region", expr::raw("var.gcp_region")),
            (
                "triggerId",
                expr::traversal(["google_cloudbuild_trigger", label, "trigger_id"]),
            ),
            (
                "triggerName",
                expr::traversal(["google_cloudbuild_trigger", label, "name"]),
            ),
            (
                "buildEnvVars",
                expr::object(env_pairs.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let env_pairs: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Expression::String(v.clone())))
            .collect();
        Ok(Some(expr::object([
            ("service", Expression::String("cloudbuild".to_string())),
            (
                "buildEnvVars",
                expr::object(env_pairs.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
            ("serviceAccount", Expression::String(String::new())),
            ("monitoring", Expression::Null),
        ])))
    }
}

fn sanitize_substitution_key(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 1);
    out.push('_');
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out
}
