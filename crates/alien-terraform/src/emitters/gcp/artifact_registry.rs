//! GCP ArtifactRegistry — Docker repository in Artifact Registry.
//!
//! One Docker-format repository plus two service-accounts (pull-only +
//! push), each granted IAM at the repository scope only — never project-
//! wide. Service-accounts use 30-char Google-account-id constraints and
//! impersonation flows via `roles/iam.serviceAccountUser` upstream.

use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        artifact_registry_repository_full_id_template, artifact_registry_repository_id_from_local,
        binding_label_for_role, downcast, emit_custom_roles_for_bindings, labels,
        permission_context, required_label, role_expression_for_binding,
        service_account_id_template, service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ArtifactRegistry, ErrorData, PermissionProfile, PermissionSet,
    PermissionSetReference, RemoteStackManagement, Result, ServiceAccount,
};
use alien_error::AlienError;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpArtifactRegistryEmitter;

impl TfEmitter for GcpArtifactRegistryEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = downcast::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();
        let repository_id_full_local = format!("{label}_repository_id_full");
        fragment.locals.insert(
            repository_id_full_local.clone(),
            artifact_registry_repository_full_id_template(registry.id()),
        );

        fragment.resource_blocks.push(resource_block(
            "google_artifact_registry_repository",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("location", expr::raw("var.gcp_region")),
                attr(
                    "repository_id",
                    artifact_registry_repository_id_from_local(&repository_id_full_local),
                ),
                attr("format", Expression::String("DOCKER".to_string())),
                attr(
                    "description",
                    expr::template(format!(
                        "Deployment {} container artifact registry",
                        registry.id
                    )),
                ),
                attr("labels", labels(ctx, "artifact-registry")),
            ],
        ));

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
            let id_template = service_account_id_template(&format!("{label}-{suffix}"));
            fragment.resource_blocks.push(resource_block(
                "google_service_account",
                &sa_label,
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr("account_id", id_template),
                    attr(
                        "display_name",
                        expr::template(format!(
                            "Deployment {} artifact registry {} identity",
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

        emit_management_repository_bindings(ctx, &mut fragment, label)?;
        emit_service_account_repository_bindings(ctx, &mut fragment, label)?;

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

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let pull = format!("{label}_pull");
        let push = format!("{label}_push");
        Ok(Some(expr::object([
            ("service", Expression::String("gar".to_string())),
            (
                "repositoryName",
                expr::traversal(["google_artifact_registry_repository", label, "id"]),
            ),
            (
                "pullServiceAccountEmail",
                expr::traversal(["google_service_account", &pull, "email"]),
            ),
            (
                "pushServiceAccountEmail",
                expr::traversal(["google_service_account", &push, "email"]),
            ),
        ])))
    }
}

fn emit_management_repository_bindings(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    registry_label: &str,
) -> Result<()> {
    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };
    let refs = management_permission_refs(ctx);
    if refs.is_empty() {
        return Ok(());
    }

    let member = service_account_member_for_label(management_label);
    let context = permission_context(management_label, ctx.stack.id()).with_resource_name(format!(
        "${{google_artifact_registry_repository.{registry_label}.name}}"
    ));
    let generator = GcpRuntimePermissionsGenerator::new();

    for permission_set_ref in refs {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if !permission_set.id.starts_with("artifact-registry/") {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Resource, &context)
            .map_err(|err| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "failed to generate GCP artifact-registry IAM grant plan for '{}': {}",
                        permission_set.id, err
                    ),
                })
            })?;
        let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
        let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

        for (idx, binding) in bindings.into_iter().enumerate() {
            let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
            let role = role_expression_for_binding(&binding.role, &custom_roles)?;

            match binding.target {
                GcpBindingTargetScope::Project => {}
                GcpBindingTargetScope::CurrentResource => {
                    let mut body = vec![
                        attr("project", expr::raw("var.gcp_project")),
                        attr(
                            "location",
                            expr::traversal([
                                "google_artifact_registry_repository",
                                registry_label,
                                "location",
                            ]),
                        ),
                        attr(
                            "repository",
                            expr::traversal([
                                "google_artifact_registry_repository",
                                registry_label,
                                "name",
                            ]),
                        ),
                        attr("role", role),
                        attr("member", member.clone()),
                    ];
                    if let Some(condition) = binding.condition {
                        body.push(nested(crate::block::block(
                            "condition",
                            [
                                attr("title", Expression::String(condition.title)),
                                attr("description", Expression::String(condition.description)),
                                attr("expression", expr::template(condition.expression)),
                            ],
                        )));
                    }
                    fragment.resource_blocks.push(resource_block(
                        "google_artifact_registry_repository_iam_member",
                        &format!("{role_label}_{registry_label}_management_repository_{idx}"),
                        body,
                    ));
                }
            }
        }
    }

    Ok(())
}

fn emit_service_account_repository_bindings(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    registry_label: &str,
) -> Result<()> {
    let service_accounts: Vec<(&str, &ServiceAccount)> = ctx
        .stack
        .resources()
        .filter_map(|(resource_id, entry)| {
            let service_account = entry.config.downcast_ref::<ServiceAccount>()?;
            let label = ctx.name_for(resource_id)?;
            Some((label, service_account))
        })
        .collect();

    for (service_account_label, service_account) in service_accounts {
        for permission_set in &service_account.stack_permission_sets {
            if !permission_set.id.starts_with("artifact-registry/") {
                continue;
            }
            emit_repository_bindings_for_member(
                fragment,
                registry_label,
                service_account_label,
                ctx.stack.id(),
                "service_account",
                permission_set,
            )?;
        }
    }

    Ok(())
}

fn emit_repository_bindings_for_member(
    fragment: &mut TfFragment,
    registry_label: &str,
    member_label: &str,
    stack_name: &str,
    binding_owner: &str,
    permission_set: &PermissionSet,
) -> Result<()> {
    if permission_set.platforms.gcp.is_none() {
        return Ok(());
    }

    let member = service_account_member_for_label(member_label);
    let context = permission_context(member_label, stack_name).with_resource_name(format!(
        "${{google_artifact_registry_repository.{registry_label}.name}}"
    ));
    let generator = GcpRuntimePermissionsGenerator::new();
    let grant_plan = generator
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .map_err(|err| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "failed to generate GCP artifact-registry IAM grant plan for '{}': {}",
                    permission_set.id, err
                ),
            })
        })?;
    let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

    for (idx, binding) in bindings.into_iter().enumerate() {
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        let role = role_expression_for_binding(&binding.role, &custom_roles)?;
        let mut body = vec![
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "location",
                expr::traversal([
                    "google_artifact_registry_repository",
                    registry_label,
                    "location",
                ]),
            ),
            attr(
                "repository",
                expr::traversal([
                    "google_artifact_registry_repository",
                    registry_label,
                    "name",
                ]),
            ),
            attr("role", role),
            attr("member", member.clone()),
        ];
        if let Some(condition) = binding.condition {
            body.push(nested(crate::block::block(
                "condition",
                [
                    attr("title", Expression::String(condition.title)),
                    attr("description", Expression::String(condition.description)),
                    attr("expression", expr::template(condition.expression)),
                ],
            )));
        }
        fragment.resource_blocks.push(resource_block(
            "google_artifact_registry_repository_iam_member",
            &format!(
                "{role_label}_{registry_label}_{binding_owner}_{member_label}_repository_{idx}"
            ),
            body,
        ));
    }

    Ok(())
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

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };

    let mut refs = Vec::new();
    refs.extend(global_permission_refs(profile));
    refs.extend(resource_permission_refs(profile, ctx.resource_id));
    refs
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    profile
        .0
        .get(resource_id)
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
