//! GCP ArtifactRegistry — Docker repository in Artifact Registry.
//!
//! One Docker-format repository plus two service-accounts (pull-only +
//! push), each granted IAM at the repository scope only — never project-
//! wide. Service-accounts use 30-char Google-account-id constraints and
//! impersonation flows via `roles/iam.serviceAccountUser` upstream.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, labels, required_label, sanitize_label_value, service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, ArtifactRegistry, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpArtifactRegistryEmitter;

impl TfEmitter for GcpArtifactRegistryEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = downcast::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let repo_id = sanitize_label_value(&format!("{}-{}", "stack", registry.id));
        fragment.resource_blocks.push(resource_block(
            "google_artifact_registry_repository",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("location", expr::raw("var.gcp_region")),
                attr(
                    "repository_id",
                    expr::template(format!("${{var.stack_name}}-{}", registry.id)),
                ),
                attr("format", Expression::String("DOCKER".to_string())),
                attr(
                    "description",
                    expr::template(format!("Alien {} container artifact registry", registry.id)),
                ),
                attr("labels", labels(ctx, "artifact-registry")),
            ],
        ));
        let _ = repo_id;

        // Pull + Push service accounts.
        for (suffix, role, sa_label) in [
            (
                "pull",
                "roles/artifactregistry.reader",
                format!("{label}_pull"),
            ),
            (
                "push",
                "roles/artifactregistry.writer",
                format!("{label}_push"),
            ),
        ] {
            let id_template = expr::raw(format!(
                "substr(replace(\"${{var.stack_name}}-{label}-{suffix}\", \"_\", \"-\"), 0, 30)"
            ));
            fragment.resource_blocks.push(resource_block(
                "google_service_account",
                &sa_label,
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr("account_id", id_template),
                    attr(
                        "display_name",
                        expr::template(format!(
                            "Alien {} artifact registry {} identity",
                            registry.id, suffix
                        )),
                    ),
                ],
            ));

            let member = service_account_member_for_label(&sa_label);
            fragment.resource_blocks.push(resource_block(
                "google_artifact_registry_repository_iam_member",
                &format!("{sa_label}_iam"),
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr(
                        "location",
                        expr::traversal(["google_artifact_registry_repository", label, "location"]),
                    ),
                    attr(
                        "repository",
                        expr::traversal(["google_artifact_registry_repository", label, "name"]),
                    ),
                    attr("role", Expression::String(role.to_string())),
                    attr("member", member),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let pull = format!("{label}_pull");
        let push = format!("{label}_push");
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            ("region", expr::raw("var.gcp_region")),
            (
                "repositoryId",
                expr::traversal(["google_artifact_registry_repository", label, "name"]),
            ),
            (
                "repositoryName",
                expr::traversal(["google_artifact_registry_repository", label, "id"]),
            ),
            (
                "registryEndpoint",
                expr::template(format!(
                    "${{var.gcp_region}}-docker.pkg.dev/${{var.gcp_project}}/${{google_artifact_registry_repository.{label}.name}}"
                )),
            ),
            (
                "pullServiceAccountEmail",
                expr::traversal(["google_service_account", &pull, "email"]),
            ),
            (
                "pushServiceAccountEmail",
                expr::traversal(["google_service_account", &push, "email"]),
            ),
        ]))
    }
}
