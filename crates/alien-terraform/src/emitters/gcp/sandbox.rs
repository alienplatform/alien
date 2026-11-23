//! GCP Agent Platform sandbox emitter.
//!
//! The template is release-owned, so setup emits only engine-scoped grants: the remote grant and,
//! on a Frozen sandbox, the manager's template verbs. Domain-scoped egress is refused here, since
//! the single internet-access switch cannot express a hostname list.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::{
        agent_platform_engine::ENGINE_RESOURCE,
        helpers::{
            binding_label_for_role, downcast, emit_custom_roles_for_bindings, labels,
            permission_context, required_label, role_expression_for_binding,
            service_account_id_template, service_account_member_for_label,
        },
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, GcpAgentPlatformEngine, RemoteBindings, RemoteStackManagement,
    ResourceLifecycle, Result, Sandbox, SandboxEgress, SigningVerifier,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    generators::{GcpBindingResourceKind, GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

/// Serde `service` tag of the `GcpAgentPlatformSandboxBinding`, and the resource-name shapes
/// the engine and template are addressed by. Kept together so the binding this emits is the one
/// the provider deserializes.
const AGENT_PLATFORM_SERVICE: &str = "sandbox-gcp-agent-platform";

/// Refuses domain-scoped egress, which Agent Platform's single internet-access switch cannot carry.
///
/// The switch semantics live in `SandboxEgress::internet_access_switch`, so this and the provider's
/// template mapping cannot drift on which modes are expressible. Names the sandbox and both
/// accepted modes.
fn refuse_domain_egress(sandbox: &Sandbox) -> Result<()> {
    if sandbox.egress.internet_access_switch().is_some() {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::OperationNotSupported {
        operation: format!("terraform emit sandbox '{}'", sandbox.id()),
        reason: "Agent Platform egress is a single internet-access switch, so a hostname list has \
                 nothing to render into. Declare egress: deny or egress: allow"
            .to_string(),
    }))
}

/// Terraform label of the engine this sandbox's sessions hang under.
fn engine_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    let engine_id = GcpAgentPlatformEngine::id_for_sandbox(ctx.resource_id);
    ctx.name_for(&engine_id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("sandbox '{}' has no engine '{engine_id}'", ctx.resource_id),
        })
    })
}

/// The engine name a session is created under.
///
/// Vertex assigns the id, so for a Frozen engine this reads the server-assigned name off the block
/// that created it. A Live engine is made by its controller after apply, where setup has no address
/// to reference and the controller republishes the binding with the name it was given.
fn engine_name(ctx: &EmitContext<'_>, label: &str) -> Result<String> {
    if ctx.resource.lifecycle != ResourceLifecycle::Frozen {
        return Ok(format!(
            "projects/${{var.gcp_project}}/locations/${{var.gcp_region}}/reasoningEngines/{label}"
        ));
    }
    Ok(format!(
        "${{{ENGINE_RESOURCE}.{}.name}}",
        engine_label(ctx)?
    ))
}

/// The engine, template, region and ttl fields a linked worker's environment reads.
///
/// The template id is assigned when the controller creates it, after apply, so the path below is
/// what setup can name and the controller replaces it in the binding it publishes.
/// `maxLifetimeSeconds` is present only when the declaration set a lifetime, matching the binding's
/// `skip_serializing_if`.
fn agent_platform_fields(
    ctx: &EmitContext<'_>,
    sandbox: &Sandbox,
    label: &str,
) -> Result<Vec<(&'static str, Expression)>> {
    let engine = engine_name(ctx, label)?;
    let mut fields = vec![
        (
            "egress",
            hcl::to_expression(&sandbox.egress)
                .into_alien_error()
                .context(ErrorData::GenericError {
                    message: "serializing the sandbox egress bound".to_string(),
                })?,
        ),
        ("engine", expr::template(engine.clone())),
        (
            "template",
            expr::template(format!("{engine}/sandboxEnvironmentTemplates/{label}")),
        ),
        ("region", expr::raw("var.gcp_region")),
        (
            "allowEgress",
            // The switch the serving template carries. `internet` goes through the gateway path and
            // keeps the switch closed; the controller reports the same.
            Expression::Bool(matches!(sandbox.egress, SandboxEgress::Allow)),
        ),
    ];
    if let Some(seconds) = sandbox.lifecycle.max_lifetime_seconds {
        fields.push((
            "maxLifetimeSeconds",
            Expression::Number(hcl::Number::from(seconds as i64)),
        ));
    }
    if !sandbox.preview_ports.is_empty() {
        fields.push((
            "previewPorts",
            Expression::Array(
                sandbox
                    .preview_ports
                    .iter()
                    .map(|port| Expression::Number(hcl::Number::from(i64::from(*port))))
                    .collect(),
            ),
        ));
    }
    Ok(fields)
}

/// Emits the GCP Agent Platform sandbox: nothing of its own, plus the region its registration
/// records and the binding a linked worker reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAgentPlatformSandboxEmitter;

impl TfEmitter for GcpAgentPlatformSandboxEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // The environment template is release-owned: a new image replaces it, so it belongs to the
        // controller under either lifecycle. Only its engine is ever a setup resource.
        let mut fragment = TfFragment::default();
        emit_remote_access(ctx, &mut fragment)?;
        emit_template_management(ctx, &mut fragment)?;
        emit_preview_signer(ctx, &mut fragment)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        // The engine id is not here: it is server-assigned, and the engine registers its own.
        let mut fields = vec![("region", expr::raw("var.gcp_region"))];
        // The template controller builds the binding a workload reads, so the signing identity
        // setup created reaches it through the import data.
        if has_preview_signer(ctx, sandbox) {
            let label = required_label(ctx)?;
            fields.push((
                "previewServiceAccount",
                expr::template(format!(
                    "${{google_service_account.{}.email}}",
                    preview_label(label)
                )),
            ));
        }
        Ok(expr::object(fields))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        let mut fields = agent_platform_fields(ctx, sandbox, label)?;
        if has_preview_signer(ctx, sandbox) {
            fields.push((
                "previewServiceAccount",
                expr::template(format!(
                    "${{google_service_account.{}.email}}",
                    preview_label(label)
                )),
            ));
        }
        fields.push((
            "service",
            Expression::String(AGENT_PLATFORM_SERVICE.to_string()),
        ));
        if let Some(policy) = &sandbox.default_policy {
            fields.push((
                "defaultPolicy",
                hcl::to_expression(policy)
                    .into_alien_error()
                    .context(ErrorData::GenericError {
                        message: "serializing sandbox default policy".into(),
                    })?,
            ));
        }

        Ok(Some(expr::object(fields)))
    }
}

/// Attaches this sandbox's remote grant to the stack's shared Remote Bindings identity, scoped to
/// the engine and nothing wider: engine IAM covers only the sessions under it, while GCP's only
/// wider scope is the whole project — every sibling sandbox in the deployment.
fn emit_remote_access(ctx: &EmitContext<'_>, fragment: &mut TfFragment) -> Result<()> {
    let (Some(definition), Some(access_label)) = (
        alien_core::remote_bindings::remote_binding_is_deliverable(ctx.resource)
            .then(|| alien_core::remote_bindings::remote_binding_for_entry(ctx.resource))
            .flatten(),
        remote_bindings_label(ctx),
    ) else {
        return Ok(());
    };
    if ctx.resource.lifecycle != ResourceLifecycle::Frozen {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform publish sandbox '{}' remotely", ctx.resource_id),
            reason: "GCP refuses an IAM binding on a reasoning engine that does not exist yet, \
                     and a Live engine is created by its controller after apply. Declare the \
                     sandbox as frozen"
                .to_string(),
        }));
    }
    emit_engine_grant(
        ctx,
        fragment,
        definition.permission_set,
        access_label,
        "access",
    )
}

/// Whether the stack grants its management identity `sandbox/templates` on this Frozen sandbox,
/// so it creates and replaces the sandbox's templates. Frozen only: a Live engine's manager
/// already holds `sandbox/provision`.
fn grants_template_management(ctx: &EmitContext<'_>) -> bool {
    const TEMPLATES: &str = "sandbox/templates";
    ctx.resource.lifecycle == ResourceLifecycle::Frozen
        && ctx
            .stack
            .management()
            .profile()
            .and_then(|profile| profile.0.get(ctx.resource_id))
            .is_some_and(|refs| refs.iter().any(|reference| reference.id() == TEMPLATES))
}

/// Label of the stack's management identity, which copies the sandbox image into the repository.
/// `None` when there is no remote manager identity: the deploying credentials run the template
/// controller, so there is no member to bind.
fn management_label<'a>(ctx: &'a EmitContext<'_>) -> Result<Option<&'a str>> {
    let Some(management_id) = ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE).then_some(id)
    }) else {
        return Ok(None);
    };
    ctx.name_for(management_id).map(Some).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("management resource '{management_id}' has no Terraform name"),
        })
    })
}

/// Lets the stack's management identity create and replace this sandbox's templates, on its
/// engine only, and makes the sandbox's image repository. The repository is made whether or not
/// there is a manager identity, as direct setup does: a setup that finds it missing asks for a
/// rerun.
fn emit_template_management(ctx: &EmitContext<'_>, fragment: &mut TfFragment) -> Result<()> {
    let management_label = management_label(ctx)?;
    if let (true, Some(management_label)) = (grants_template_management(ctx), management_label) {
        emit_engine_grant(
            ctx,
            fragment,
            "sandbox/templates",
            management_label,
            "templates",
        )?;
    }
    emit_image_repository(ctx, fragment, management_label)
}

// Permissions of the custom role that lets the manager copy the sandbox image in; shared with
// direct setup, so the two grant the same thing.
use alien_core::gar_repository::SANDBOX_IMAGE_WRITE_PERMISSIONS as IMAGE_WRITE_PERMISSIONS;

/// Terraform label of the stack-wide custom role carrying [`IMAGE_WRITE_PERMISSIONS`].
const IMAGE_WRITE_ROLE_LABEL: &str = "gcp_role_sandbox_image_write";

/// Creates this sandbox's dedicated image repository and lets the manager write to it, and to
/// nothing else.
///
/// The manager's registry may be in another project or cloud, and Agent Platform refuses an image
/// the creating identity cannot read, so the controller copies the image here by digest before it
/// creates the template. The repository is never the stack's registry: that one has a push identity
/// and workload bindings. Name, format and cleanup policy match what the controller would create,
/// so a repository made here is the one it finds.
fn emit_image_repository(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    management_label: Option<&str>,
) -> Result<()> {
    let label = format!("{}_image", required_label(ctx)?);
    let repository_name = format!(
        "replace(lower(format(\"%s-{}-img\", local.resource_prefix)), \"_\", \"-\")",
        ctx.resource_id
    );
    // The repository is refused until the Artifact Registry API is on, so it waits for the
    // stack's activation of it.
    let registry_api = ctx.stack.resources().find_map(|(id, entry)| {
        entry
            .config
            .downcast_ref::<alien_core::ServiceActivation>()
            .filter(|activation| activation.service_name == "artifactregistry.googleapis.com")
            .and_then(|_| ctx.name_for(id))
    });
    let depends_on = registry_api.map(|name| {
        attr(
            "depends_on",
            expr::raw(format!("[google_project_service.{name}]")),
        )
    });
    fragment.resource_blocks.push(resource_block(
        "google_artifact_registry_repository",
        &label,
        [
            attr("project", expr::raw("var.gcp_project")),
            attr("location", expr::raw("var.gcp_region")),
            attr("repository_id", expr::raw(repository_name.clone())),
            attr("format", Expression::String("DOCKER".to_string())),
            attr(
                "description",
                expr::template(format!("Image of sandbox '{}'", ctx.resource_id)),
            ),
            attr("labels", labels(ctx, "sandbox")),
        ]
        .into_iter()
        .chain(depends_on)
        .chain([
            // Tags are made from the digest they name and never move.
            nested(block(
                "docker_config",
                [attr("immutable_tags", Expression::Bool(true))],
            )),
            nested(block(
                "cleanup_policies",
                [
                    attr("id", Expression::String("delete-untagged".to_string())),
                    attr("action", Expression::String("DELETE".to_string())),
                    nested(block(
                        "condition",
                        [
                            attr("tag_state", Expression::String("UNTAGGED".to_string())),
                            attr("older_than", Expression::String("604800s".to_string())),
                        ],
                    )),
                ],
            )),
            // A repository id is at most 63 characters; the same limit direct setup applies.
            nested(block(
                "lifecycle",
                [nested(block(
                    "precondition",
                    [
                        attr(
                            "condition",
                            expr::raw(format!("length({repository_name}) <= 63")),
                        ),
                        attr(
                            "error_message",
                            Expression::String(format!(
                                "The image repository name of sandbox '{}' (resource_prefix, the sandbox id and '-img') is longer than the 63 characters Artifact Registry allows. Use a shorter resource_prefix.",
                                ctx.resource_id
                            )),
                        ),
                    ],
                ))],
            )),
        ]),
    ));
    emit_verifier_read(ctx, fragment, &label);
    let Some(management_label) = management_label else {
        // Nobody to grant: the repository is all there is, as in direct setup.
        return Ok(());
    };
    fragment.push_shared_resource(resource_block(
        "google_project_iam_custom_role",
        IMAGE_WRITE_ROLE_LABEL,
        [
            attr("count", expr::raw("var.gcp_manage_custom_roles ? 1 : 0")),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "role_id",
                expr::raw("format(\"role_%s_sandbox_image_write\", local.gcp_custom_role_prefix)"),
            ),
            attr(
                "title",
                expr::template("Copy a sandbox image into its repository".to_string()),
            ),
            attr(
                "description",
                expr::template(
                    "Read, upload and tag in a sandbox image repository. Cannot delete."
                        .to_string(),
                ),
            ),
            attr("stage", Expression::String("GA".to_string())),
            attr(
                "permissions",
                Expression::Array(
                    IMAGE_WRITE_PERMISSIONS
                        .iter()
                        .map(|permission| Expression::String((*permission).to_string()))
                        .collect(),
                ),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_artifact_registry_repository_iam_member",
        &format!("{label}_write"),
        [
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "location",
                expr::traversal(["google_artifact_registry_repository", label.as_str(), "location"]),
            ),
            attr(
                "repository",
                expr::traversal(["google_artifact_registry_repository", label.as_str(), "name"]),
            ),
            attr(
                "role",
                expr::raw(format!(
                    "var.gcp_manage_custom_roles ? google_project_iam_custom_role.{IMAGE_WRITE_ROLE_LABEL}[0].name : format(\"projects/%s/roles/role_%s_sandbox_image_write\", var.gcp_project, local.gcp_custom_role_prefix)"
                )),
            ),
            attr("member", service_account_member_for_label(management_label)),
        ],
    ));
    Ok(())
}

/// On a protected install the signing verifier creates the template, and Agent Platform checks
/// that the creating identity can read the image. It reads this one repository and nothing else.
/// Granted whenever a verifier exists, with or without a management identity.
fn emit_verifier_read(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str) {
    if ctx
        .stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type() == SigningVerifier::RESOURCE_TYPE)
    {
        fragment.resource_blocks.push(resource_block(
            "google_artifact_registry_repository_iam_member",
            &format!("{label}_verifier_read"),
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
                attr(
                    "role",
                    Expression::String("roles/artifactregistry.reader".to_string()),
                ),
                attr(
                    "member",
                    expr::template(
                        "serviceAccount:${google_service_account.signing_verifier.email}",
                    ),
                ),
            ],
        ));
    }
}

/// Binds `permission_set_id` to `member_label`'s service account on this sandbox's engine.
fn emit_engine_grant(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    permission_set_id: &str,
    member_label: &str,
    purpose: &str,
) -> Result<()> {
    let engine_label = engine_label(ctx)?;
    let permission_set =
        alien_permissions::get_permission_set(permission_set_id).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!("{permission_set_id} permission set is not registered"),
            })
        })?;

    let context = permission_context(member_label, ctx.stack.id())
        .with_resource_name(format!("${{{ENGINE_RESOURCE}.{engine_label}.name}}"));
    let plan = GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: format!("failed to generate GCP {permission_set_id} engine permissions"),
        })?;

    let bindings = plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &plan, &bindings)?;
    let member = service_account_member_for_label(member_label);
    for (index, binding) in bindings.iter().enumerate() {
        if binding.resource_kind != Some(GcpBindingResourceKind::VertexAiReasoningEngine) {
            continue;
        }
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        fragment.resource_blocks.push(resource_block(
            "google_vertex_ai_reasoning_engine_iam_member",
            &format!("{role_label}_{engine_label}_{purpose}_{index}"),
            [
                // The engine type lives only in google-beta, and so does its IAM member.
                attr("provider", expr::raw("google-beta")),
                // The created engine rather than a name derived from the label: Vertex assigns the
                // id, and referencing the block is also what orders the grant after it.
                attr(
                    "reasoning_engine",
                    expr::traversal([ENGINE_RESOURCE, engine_label, "name"]),
                ),
                attr(
                    "role",
                    role_expression_for_binding(&binding.role, &custom_roles)?,
                ),
                attr("member", member.clone()),
            ],
        ));
    }

    Ok(())
}

/// Terraform label of the identity that signs this sandbox's preview edge credential.
fn preview_label(label: &str) -> String {
    format!("{label}_preview")
}

/// The one permission the workload needs on the signing identity. `signJwt` and nothing else: the
/// sibling verbs (`getAccessToken`, `signBlob`, `implicitDelegation`) would let the workload mint
/// a token for the identity or sign arbitrary blobs as it. Not a registry permission set: it
/// binds a service account, which the generator can only express for the executing identity.
const SIGN_JWT_PERMISSION: &str = "iam.serviceAccounts.signJwt";

/// Terraform label of the custom role that carries [`SIGN_JWT_PERMISSION`]. Not derived from the
/// sandbox: the role is project-wide and its id is fixed.
const PREVIEW_SIGN_ROLE_LABEL: &str = "gcp_role_sandbox_preview_sign";

/// Whether setup creates the edge-signing identity: only where the sandbox needs one (preview
/// ports, or an `internet` sandbox's private ingress) and the deployment owns the engine, so a
/// sandbox with neither adds nothing to an existing plan.
fn has_preview_signer(ctx: &EmitContext<'_>, sandbox: &Sandbox) -> bool {
    sandbox.needs_gcp_edge_signer() && ctx.resource.lifecycle == ResourceLifecycle::Frozen
}

/// Creates the per-deployment identity the GCP edge accepts a signed JWT from, and lets the
/// workload identities that reach this sandbox sign as it.
///
/// The edge refuses a JWT whose principal lacks the execute verb on the session. Giving that verb
/// to a dedicated identity, on the engine only, keeps it away from the workload: the workload asks
/// IAM Credentials for a short-lived JWT as this one identity, and can sign as no other.
fn emit_preview_signer(ctx: &EmitContext<'_>, fragment: &mut TfFragment) -> Result<()> {
    let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
    if !has_preview_signer(ctx, sandbox) {
        return Ok(());
    }
    let label = required_label(ctx)?;
    let signer_label = preview_label(label);
    fragment.resource_blocks.push(resource_block(
        "google_service_account",
        &signer_label,
        [
            attr("project", expr::raw("var.gcp_project")),
            attr("account_id", service_account_id_template(&signer_label)),
            attr(
                "display_name",
                expr::template(format!(
                    "${{local.deployment_name}}: Preview edge signer ({})",
                    ctx.resource_id
                )),
            ),
            attr(
                "description",
                expr::template(
                    "Signs the short-lived credential the Agent Platform edge checks for preview. \
                     Holds the execute verb on this deployment's engine and nothing else."
                        .to_string(),
                ),
            ),
        ],
    ));
    emit_engine_grant(
        ctx,
        fragment,
        "sandbox/preview-edge",
        &signer_label,
        "preview",
    )?;

    let owners = execute_owner_labels(ctx);
    if owners.is_empty() {
        return Ok(());
    }
    // One role for the whole stack: its id is fixed, so a second sandbox emitting its own address
    // would create the same role twice and fail at apply. Every sandbox emits this identical
    // block, which the generator keeps once, and every signer binding points to it.
    let role_label = PREVIEW_SIGN_ROLE_LABEL;
    fragment.push_shared_resource(resource_block(
        "google_project_iam_custom_role",
        &role_label,
        [
            attr("count", expr::raw("var.gcp_manage_custom_roles ? 1 : 0")),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "role_id",
                expr::raw("format(\"role_%s_sandbox_preview_sign\", local.gcp_custom_role_prefix)"),
            ),
            attr(
                "title",
                expr::template("Sign the preview edge credential".to_string()),
            ),
            attr(
                "description",
                expr::template(
                    "Sign a JWT as the preview edge signer. Granted on that one service account."
                        .to_string(),
                ),
            ),
            attr("stage", Expression::String("GA".to_string())),
            attr(
                "permissions",
                Expression::Array(vec![Expression::String(SIGN_JWT_PERMISSION.to_string())]),
            ),
        ],
    ));
    for owner in owners {
        fragment.resource_blocks.push(resource_block(
            "google_service_account_iam_member",
            &format!("{signer_label}_sign_{owner}"),
            [
                attr(
                    "service_account_id",
                    expr::traversal(["google_service_account", signer_label.as_str(), "name"]),
                ),
                attr(
                    "role",
                    expr::raw(format!(
                        "var.gcp_manage_custom_roles ? google_project_iam_custom_role.{role_label}[0].name : format(\"projects/%s/roles/role_%s_sandbox_preview_sign\", var.gcp_project, local.gcp_custom_role_prefix)"
                    )),
                ),
                attr("member", service_account_member_for_label(&owner)),
            ],
        ));
    }
    Ok(())
}

/// Terraform labels of the workload identities whose profile grants `sandbox/execute` on this
/// sandbox, by name or through the wildcard.
fn execute_owner_labels(ctx: &EmitContext<'_>) -> Vec<String> {
    const EXECUTE: &str = "sandbox/execute";
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let granted = [ctx.resource_id, "*"].iter().any(|key| {
            profile
                .0
                .get(*key)
                .is_some_and(|refs| refs.iter().any(|reference| reference.id() == EXECUTE))
        });
        if !granted {
            continue;
        }
        if let Some(label) = ctx.name_for(&format!("{profile_name}-sa")) {
            owners.push(label.to_string());
        }
    }
    owners
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
    mod agent_platform {
        use super::super::*;
        use alien_core::bindings::SandboxBinding;
        use alien_core::{
            ResourceLifecycle, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
            StackSettings,
        };
        use hcl::structure::Structure;
        use indexmap::IndexMap;
        use std::collections::BTreeSet;

        /// The stack `GcpAgentPlatformEngineMutation` produces: the sandbox and the engine it
        /// hangs under, sharing a lifecycle.
        fn stack_with(
            egress: SandboxEgress,
            ttl: Option<u32>,
            lifecycle: ResourceLifecycle,
            remote_access: bool,
        ) -> Stack {
            let sandbox = Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(egress)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: ttl,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .build();
            let builder = Stack::new("acme".to_string()).add(
                GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                lifecycle,
            );
            if remote_access {
                builder
                    .add(RemoteBindings::new("access".to_string()).build(), lifecycle)
                    .add_with_remote_access(sandbox, lifecycle)
                    .build()
            } else {
                builder.add(sandbox, lifecycle).build()
            }
        }

        fn names() -> IndexMap<String, String> {
            IndexMap::from([
                ("agents".to_string(), "agents".to_string()),
                ("agents-engine".to_string(), "agents_engine".to_string()),
                ("access".to_string(), "access".to_string()),
            ])
        }

        fn with_context<T>(stack: &Stack, run: impl FnOnce(&EmitContext<'_>) -> T) -> T {
            let resource = stack
                .resources
                .get("agents")
                .expect("the sandbox is in the stack");
            let names = names();
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            run(&ctx)
        }

        fn emit_binding(egress: SandboxEgress, ttl: Option<u32>) -> Result<Option<Expression>> {
            let stack = stack_with(egress, ttl, ResourceLifecycle::Frozen, false);
            with_context(&stack, |ctx| {
                GcpAgentPlatformSandboxEmitter.emit_binding_ref(ctx)
            })
        }

        fn object_keys(expr: &Expression) -> BTreeSet<String> {
            match expr {
                Expression::Object(map) => map
                    .keys()
                    .map(|key| match key {
                        hcl::expr::ObjectKey::Identifier(id) => id.as_str().to_string(),
                        hcl::expr::ObjectKey::Expression(Expression::String(s)) => s.clone(),
                        other => panic!("unexpected object key: {other:?}"),
                    })
                    .collect(),
                other => panic!("expected an object, got {other:?}"),
            }
        }

        fn attribute_opt(block: &hcl::structure::Block, key: &str) -> Option<String> {
            block.body.iter().find_map(|structure| match structure {
                Structure::Attribute(attribute) if attribute.key.as_str() == key => {
                    Some(attribute.expr.to_string())
                }
                _ => None,
            })
        }

        fn attribute(block: &hcl::structure::Block, key: &str) -> String {
            attribute_opt(block, key)
                .unwrap_or_else(|| panic!("the block carries no '{key}': {block:?}"))
        }

        /// Engine grants that are not the edge signer's. Every sandbox now has a signer, which
        /// holds the execute verb on the engine, so "no engine grant" means no other member.
        fn engine_grants_besides_the_signer(fragment: &TfFragment) -> Vec<&hcl::structure::Block> {
            fragment
                .resource_blocks
                .iter()
                .filter(|block| {
                    block.labels.first().map(|l| l.as_str())
                        == Some("google_vertex_ai_reasoning_engine_iam_member")
                })
                .filter(|block| !attribute(block, "member").contains("_preview"))
                .collect()
        }

        /// The grant names the engine by reading the created block's server-assigned `name` — a
        /// name derived from the setup label would be a scope no engine answers to, so the caller
        /// would hold no session access at all while `PERMISSIONS.md` still advertised the grant.
        #[test]
        fn the_engine_grant_references_the_created_engine() {
            let stack = stack_with(SandboxEgress::Allow, None, ResourceLifecycle::Frozen, true);
            let fragment = with_context(&stack, |ctx| {
                GcpAgentPlatformSandboxEmitter
                    .emit(ctx)
                    .expect("a frozen remote sandbox renders its grant")
            });

            let member = fragment
                .resource_blocks
                .iter()
                .find(|block| {
                    block.labels.first().map(|label| label.as_str())
                        == Some("google_vertex_ai_reasoning_engine_iam_member")
                })
                .expect("the remote grant renders an engine-scoped IAM member");
            assert_eq!(
                attribute(member, "reasoning_engine"),
                format!("{ENGINE_RESOURCE}.agents_engine.name")
            );
            assert_eq!(attribute(member, "provider"), "google-beta");
            assert_eq!(
                attribute(member, "member"),
                "\"serviceAccount:${google_service_account.access.email}\""
            );
            // The role is the generated custom one, not a predefined role: every predefined role
            // carrying the execute verb carries the rest of Vertex AI with it.
            assert!(
                attribute(member, "role").contains("google_project_iam_custom_role"),
                "{member:?}"
            );

            // A sandbox nobody published gets no engine grant, whatever else the stack declares.
            let unpublished =
                stack_with(SandboxEgress::Allow, None, ResourceLifecycle::Frozen, false);
            let fragment = with_context(&unpublished, |ctx| {
                GcpAgentPlatformSandboxEmitter.emit(ctx).expect("renders")
            });
            assert!(
                engine_grants_besides_the_signer(&fragment).is_empty(),
                "{fragment:?}"
            );
        }

        /// The engine grant follows the profile: one member when `sandbox/templates` is granted on
        /// this sandbox, none without it. Where the member lands is pinned by the generator test
        /// `a_frozen_gcp_sandbox_manager_gets_template_verbs_on_its_engine_only`.
        #[test]
        fn a_live_sandbox_gets_its_image_repository_but_no_engine_grant() {
            let sandbox = Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .build();
            let stack = Stack::new("acme".to_string())
                .add(
                    GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .add(
                    RemoteStackManagement::new("manager".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(sandbox, ResourceLifecycle::Live)
                .build();
            let mut names = names();
            names.insert("manager".to_string(), "manager".to_string());
            let resource = stack.resources.get("agents").expect("sandbox");
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack: &stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
            let kinds: Vec<_> = fragment
                .resource_blocks
                .iter()
                .filter_map(|block| block.labels.first().map(|l| l.as_str().to_string()))
                .collect();
            assert!(
                kinds.contains(&"google_artifact_registry_repository".to_string()),
                "{kinds:?}"
            );
            assert!(
                !kinds.contains(&"google_vertex_ai_reasoning_engine_iam_member".to_string()),
                "{kinds:?}"
            );
        }

        /// Direct setup always makes the repository, so a template install does too: with no
        /// management identity it is the only thing emitted (no role, no grant), and it refuses a
        /// name longer than Artifact Registry allows.
        #[test]
        fn the_image_repository_is_made_without_a_management_identity() {
            let sandbox = Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .build();
            let stack = Stack::new("acme".to_string())
                .add(
                    GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .add(sandbox, ResourceLifecycle::Live)
                .build();
            let resource = stack.resources.get("agents").expect("sandbox");
            let settings = StackSettings::default();
            let names = names();
            let ctx = EmitContext {
                stack: &stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
            let kinds: Vec<_> = fragment
                .resource_blocks
                .iter()
                .filter_map(|block| block.labels.first().map(|l| l.as_str().to_string()))
                .collect();
            assert_eq!(
                kinds,
                vec!["google_artifact_registry_repository".to_string()],
                "{fragment:?}"
            );
            let rendered = format!("{:?}", fragment.resource_blocks[0]);
            assert!(
                rendered.contains("precondition") && rendered.contains("63"),
                "{rendered}"
            );
        }

        #[test]
        fn the_image_repository_waits_for_the_registry_api_activation() {
            let sandbox = Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .build();
            let stack = Stack::new("acme".to_string())
                .add(
                    GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .add(
                    RemoteStackManagement::new("manager".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    alien_core::ServiceActivation::new("enable-artifact-registry".to_string())
                        .service_name("artifactregistry.googleapis.com".to_string())
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .add(sandbox, ResourceLifecycle::Live)
                .build();
            let mut names = names();
            names.insert("manager".to_string(), "manager".to_string());
            names.insert(
                "enable-artifact-registry".to_string(),
                "enable_artifact_registry".to_string(),
            );
            let resource = stack.resources.get("agents").expect("sandbox");
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack: &stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
            let kinds: Vec<_> = fragment
                .resource_blocks
                .iter()
                .filter_map(|block| block.labels.first().map(|l| l.as_str().to_string()))
                .collect();
            let repository = fragment
                .resource_blocks
                .iter()
                .find(|block| {
                    block
                        .labels
                        .first()
                        .is_some_and(|l| l.as_str() == "google_artifact_registry_repository")
                })
                .expect("repository");
            assert!(attribute(repository, "depends_on")
                .contains("google_project_service.enable_artifact_registry"));
        }

        #[test]
        fn template_management_is_bound_only_when_granted() {
            let render = |with_profile: bool, with_verifier: bool, with_manager: bool| {
                let sandbox = Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                        bundled_from: None,
                    })
                    .egress(SandboxEgress::Allow)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                        ..Default::default()
                    })
                    .build();
                let mut builder = Stack::new("acme".to_string())
                    .add(
                        GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    )
                    .add(sandbox, ResourceLifecycle::Frozen);
                if with_manager {
                    builder = builder.add(
                        RemoteStackManagement::new("manager".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    );
                }
                if with_verifier {
                    builder = builder.add(
                        alien_core::SigningVerifier::new("verifier".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    );
                }
                if with_profile {
                    builder =
                        builder.management(alien_core::permissions::ManagementPermissions::Extend(
                            alien_core::permissions::PermissionProfile::new()
                                .resource("agents", ["sandbox/templates"]),
                        ));
                }
                let stack = builder.build();
                let mut names = names();
                names.insert("manager".to_string(), "manager".to_string());
                let resource = stack.resources.get("agents").expect("sandbox");
                let settings = StackSettings::default();
                let ctx = EmitContext {
                    stack: &stack,
                    resource,
                    resource_id: "agents",
                    platform: alien_core::Platform::Gcp,
                    targets_kubernetes: false,
                    stack_settings: &settings,
                    names: &names,
                    pins: &alien_core::SetupPins::default(),
                };
                GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders")
            };
            let granted =
                |with_profile: bool, with_verifier: bool| render(with_profile, with_verifier, true);

            // A protected install with no management identity still lets the verifier read the
            // image repository, and gets no writer grant.
            let unmanaged = render(false, true, false);
            assert!(
                unmanaged.resource_blocks.iter().any(|block| {
                    block.labels.get(1).map(|l| l.as_str()) == Some("agents_image_verifier_read")
                }),
                "{unmanaged:?}"
            );
            assert!(!unmanaged.resource_blocks.iter().any(|block| {
                block.labels.get(1).map(|l| l.as_str()) == Some("agents_image_write")
            }));
            assert!(!render(false, false, false)
                .resource_blocks
                .iter()
                .any(|block| {
                    block.labels.get(1).map(|l| l.as_str()) == Some("agents_image_verifier_read")
                }));

            let fragment = granted(true, false);
            let members = engine_grants_besides_the_signer(&fragment);
            assert_eq!(members.len(), 1, "{fragment:?}");
            assert!(engine_grants_besides_the_signer(&granted(false, false)).is_empty());

            // On a protected install the verifier creates the template, so it reads the image
            // repository too: one reader member on that repository, none without a verifier.
            let protected = granted(true, true);
            let verifier_reads: Vec<_> = protected
                .resource_blocks
                .iter()
                .filter(|block| {
                    block.labels.get(1).map(|l| l.as_str()) == Some("agents_image_verifier_read")
                })
                .collect();
            assert_eq!(verifier_reads.len(), 1, "{protected:?}");
            assert_eq!(
                attribute(verifier_reads[0], "role"),
                "\"roles/artifactregistry.reader\""
            );
            assert!(!fragment.resource_blocks.iter().any(|block| {
                block.labels.get(1).map(|l| l.as_str()) == Some("agents_image_verifier_read")
            }));

            // The same grant gives the manager a dedicated image repository to copy into, with a
            // writer role that can upload and tag but never delete.
            let kinds = |kind: &str| {
                fragment
                    .resource_blocks
                    .iter()
                    .filter(|block| block.labels.first().map(|l| l.as_str()) == Some(kind))
                    .count()
            };
            assert_eq!(
                kinds("google_artifact_registry_repository"),
                1,
                "{fragment:?}"
            );
            assert_eq!(
                kinds("google_artifact_registry_repository_iam_member"),
                1,
                "{fragment:?}"
            );
            assert!(fragment.resource_blocks.iter().any(|block| {
                block.labels.get(1).map(|l| l.as_str()) == Some(IMAGE_WRITE_ROLE_LABEL)
            }));
            assert!(IMAGE_WRITE_PERMISSIONS
                .iter()
                .all(|permission| !permission.contains("delete")));
        }

        /// A sandbox that declares preview ports gets its own edge-signing identity: it holds the
        /// execute verb on the engine and nothing else, and the workload identity may sign as it
        /// (and as nothing else) through a role carrying only `signJwt`. A sandbox without
        /// preview ports renders none of it, which is what keeps an existing stack's plan empty.
        #[test]
        fn preview_ports_render_a_dedicated_signing_identity() {
            let render = |ports: Vec<u16>, granted: bool| {
                let sandbox = Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                        bundled_from: None,
                    })
                    .egress(SandboxEgress::Allow)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                        ..Default::default()
                    })
                    .preview_ports(ports)
                    .build();
                let mut builder = Stack::new("acme".to_string())
                    .add(
                        GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    )
                    .add(sandbox, ResourceLifecycle::Frozen);
                if granted {
                    builder = builder.permission(
                        "worker",
                        alien_core::permissions::PermissionProfile::new()
                            .resource("agents", ["sandbox/execute"]),
                    );
                }
                let stack = builder.build();
                let mut names = names();
                names.insert("worker-sa".to_string(), "worker_sa".to_string());
                let resource = stack.resources.get("agents").expect("sandbox");
                let settings = StackSettings::default();
                let ctx = EmitContext {
                    stack: &stack,
                    resource,
                    resource_id: "agents",
                    platform: alien_core::Platform::Gcp,
                    targets_kubernetes: false,
                    stack_settings: &settings,
                    names: &names,
                    pins: &alien_core::SetupPins::default(),
                };
                let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
                let binding = GcpAgentPlatformSandboxEmitter
                    .emit_binding_ref(&ctx)
                    .expect("renders")
                    .expect("binding");
                let import_ref = GcpAgentPlatformSandboxEmitter
                    .emit_import_ref(&ctx)
                    .expect("renders");
                (fragment, binding, import_ref)
            };
            let kinds = |fragment: &TfFragment| -> Vec<String> {
                fragment
                    .resource_blocks
                    .iter()
                    .filter_map(|block| block.labels.first().map(|l| l.as_str().to_string()))
                    .collect()
            };

            // No preview ports still gets the identity: interactive commands reach the agent
            // through the edge on every GCP sandbox.
            let (none, binding, none_import) = render(vec![], true);
            assert_eq!(
                kinds(&none)
                    .iter()
                    .filter(|k| k.as_str() == "google_service_account")
                    .count(),
                1,
                "{none:?}"
            );
            assert!(object_keys(&binding).contains("previewServiceAccount"));
            assert!(object_keys(&none_import).contains("previewServiceAccount"));

            let (fragment, binding, import_ref) = render(vec![8080, 8081], true);
            let all = kinds(&fragment);
            let count = |kind: &str| all.iter().filter(|k| k.as_str() == kind).count();
            assert_eq!(count("google_service_account"), 1, "{all:?}");
            // Execute on the engine for the signing identity, and only for it.
            let engine_members: Vec<_> = fragment
                .resource_blocks
                .iter()
                .filter(|b| {
                    b.labels.first().map(|l| l.as_str())
                        == Some("google_vertex_ai_reasoning_engine_iam_member")
                })
                .collect();
            assert_eq!(engine_members.len(), 1, "{fragment:?}");
            assert_eq!(
                attribute(engine_members[0], "member"),
                "\"serviceAccount:${google_service_account.agents_preview.email}\""
            );
            // The workload may sign as the signing identity: a binding on that service account.
            let signers: Vec<_> = fragment
                .resource_blocks
                .iter()
                .filter(|b| {
                    b.labels.first().map(|l| l.as_str())
                        == Some("google_service_account_iam_member")
                })
                .collect();
            assert_eq!(signers.len(), 1, "{fragment:?}");
            assert_eq!(
                attribute(signers[0], "service_account_id"),
                "google_service_account.agents_preview.name"
            );
            assert_eq!(
                attribute(signers[0], "member"),
                "\"serviceAccount:${google_service_account.worker_sa.email}\""
            );
            let role = fragment
                .resource_blocks
                .iter()
                .find(|b| {
                    attribute_opt(b, "permissions")
                        .is_some_and(|p| p.contains("iam.serviceAccounts.signJwt"))
                })
                .expect("a custom role carries signJwt");
            let permissions = attribute(role, "permissions");
            assert!(
                !permissions.contains("getAccessToken") && !permissions.contains("signBlob"),
                "only signJwt: {permissions}"
            );
            assert!(object_keys(&binding).contains("previewServiceAccount"));
            // The deployment's controller builds the binding a workload reads, from the import
            // data: the identity must travel there too, or a deployed sandbox cannot sign.
            assert!(
                object_keys(&import_ref).contains("previewServiceAccount"),
                "import data names the signer: {import_ref:?}"
            );

            // Nothing holds the signing verb unless a profile gives the workload this sandbox.
            let (ungranted, _, _) = render(vec![8080], false);
            assert!(
                !kinds(&ungranted).contains(&"google_service_account_iam_member".to_string()),
                "{ungranted:?}"
            );
        }

        /// Preview on GCP is command access, so the right to sign as the edge signer goes to the
        /// profiles that hold `sandbox/execute` on the sandbox, by name or wildcard, and to no
        /// other profile in the stack, whatever else it holds.
        #[test]
        fn the_edge_signer_is_grantable_only_to_the_execute_owners() {
            use alien_core::permissions::PermissionProfile;
            let sandbox = Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .preview_ports(vec![8080])
                .build();
            let stack = Stack::new("acme".to_string())
                .add(
                    GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(sandbox, ResourceLifecycle::Frozen)
                .permission(
                    "named",
                    PermissionProfile::new().resource("agents", ["sandbox/execute"]),
                )
                .permission(
                    "wildcard",
                    PermissionProfile::new().resource("*", ["sandbox/execute"]),
                )
                .permission(
                    "other-sandbox",
                    PermissionProfile::new().resource("elsewhere", ["sandbox/execute"]),
                )
                .permission(
                    "heartbeat-only",
                    PermissionProfile::new().resource("agents", ["sandbox/heartbeat"]),
                )
                .permission("empty", PermissionProfile::new())
                .build();
            let mut names = names();
            for profile in [
                "named",
                "wildcard",
                "other-sandbox",
                "heartbeat-only",
                "empty",
            ] {
                names.insert(
                    format!("{profile}-sa"),
                    format!("{}_sa", profile.replace('-', "_")),
                );
            }
            let resource = stack.resources.get("agents").expect("sandbox");
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack: &stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
            let mut members: Vec<String> = fragment
                .resource_blocks
                .iter()
                .filter(|b| {
                    b.labels.first().map(|l| l.as_str())
                        == Some("google_service_account_iam_member")
                })
                .map(|b| attribute(b, "member"))
                .collect();
            members.sort();
            assert_eq!(
                members,
                vec![
                    "\"serviceAccount:${google_service_account.named_sa.email}\"".to_string(),
                    "\"serviceAccount:${google_service_account.wildcard_sa.email}\"".to_string(),
                ],
                "only the two profiles that hold execute on this sandbox may sign as the edge signer"
            );
        }

        /// Every item the package creates for a sandbox is listed in
        /// `alien_core::sandbox_setup_parity`, with whether direct setup creates it. An item the
        /// emitter produces that the table does not name is a piece direct setup would miss
        /// without anyone noticing; a listed item the emitter no longer produces is a stale row.
        #[test]
        fn every_item_the_package_creates_is_accounted_for_in_direct_setup() {
            use alien_core::permissions::{ManagementPermissions, PermissionProfile};
            use alien_core::sandbox_setup_parity::{DirectSetup, GCP_SANDBOX_ITEMS};

            // Remote access excludes preview ports and internet egress, so two stacks between
            // them reach every item: one published for a remote caller, one with preview.
            let build = |remote_access: bool| {
                let base = Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                        bundled_from: None,
                    })
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                        ..Default::default()
                    });
                let builder = Stack::new("acme".to_string())
                    .add(
                        GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    )
                    .add(
                        RemoteStackManagement::new("manager".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    )
                    .add(
                        RemoteBindings::new("access".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    );
                let builder = if remote_access {
                    builder.add_with_remote_access(
                        base.egress(SandboxEgress::Allow).build(),
                        ResourceLifecycle::Frozen,
                    )
                } else {
                    builder.add(
                        base.egress(SandboxEgress::Internet)
                            .preview_ports(vec![8080])
                            .build(),
                        ResourceLifecycle::Frozen,
                    )
                };
                builder
                    .management(ManagementPermissions::Extend(
                        PermissionProfile::new().resource("agents", ["sandbox/templates"]),
                    ))
                    .permission(
                        "runner",
                        PermissionProfile::new().resource("agents", ["sandbox/execute"]),
                    )
                    .build()
            };

            let mut emitted = BTreeSet::new();
            for remote_access in [true, false] {
                let stack = build(remote_access);
                let mut names = names();
                names.insert("manager".to_string(), "manager".to_string());
                names.insert("runner-sa".to_string(), "runner_sa".to_string());
                let resource = stack.resources.get("agents").expect("sandbox");
                let settings = StackSettings::default();
                let ctx = EmitContext {
                    stack: &stack,
                    resource,
                    resource_id: "agents",
                    platform: alien_core::Platform::Gcp,
                    targets_kubernetes: false,
                    stack_settings: &settings,
                    names: &names,
                    pins: &alien_core::SetupPins::default(),
                };
                let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");
                for block in fragment.resource_blocks.iter() {
                    let kind = block.labels.first().map(|l| l.as_str()).unwrap_or_default();
                    let label = block.labels.get(1).map(|l| l.as_str()).unwrap_or_default();
                    let item = match kind {
                        "google_vertex_ai_reasoning_engine_iam_member" => {
                            let purpose = ["_access_", "_templates_", "_preview_"]
                                .iter()
                                .find(|purpose| label.contains(*purpose))
                                .unwrap_or_else(|| {
                                    panic!("an engine grant with no purpose: {label}")
                                });
                            format!("engine-grant:{}", purpose.trim_matches('_'))
                        }
                        "google_service_account" => "preview-signer-account".to_string(),
                        "google_project_iam_custom_role" => "preview-sign-role".to_string(),
                        "google_service_account_iam_member" => "preview-sign-binding".to_string(),
                        "google_artifact_registry_repository" => "image-repository".to_string(),
                        "google_artifact_registry_repository_iam_member"
                            if label.ends_with("_write") =>
                        {
                            "image-repository-write".to_string()
                        }
                        other => panic!(
                            "the Google Cloud sandbox emitter produced `{other}` ({label}), which \
                             alien_core::sandbox_setup_parity does not account for. Add it there \
                             as `Creates` once direct setup creates it with the same grant, \
                             `Pending`, or `PackageOnly` with the build refusal that covers it"
                        ),
                    };
                    emitted.insert(item);
                }
            }

            let listed: BTreeSet<String> = GCP_SANDBOX_ITEMS
                .iter()
                .map(|(item, _)| item.to_string())
                .collect();
            assert_eq!(
                emitted, listed,
                "the emitter's items and alien_core::sandbox_setup_parity::GCP_SANDBOX_ITEMS differ"
            );

            // Every item is created by direct setup: the controller tests in alien-infra
            // (`gcp_engine_setup_tests`) prove the same role, scope and member for each. A new
            // Pending item has to be listed here on purpose.
            let pending: BTreeSet<&str> = GCP_SANDBOX_ITEMS
                .iter()
                .filter(|(_, status)| matches!(status, DirectSetup::Pending(_)))
                .map(|(item, _)| *item)
                .collect();
            assert!(
                pending.is_empty(),
                "a Pending item has no controller test yet: {pending:?}"
            );
        }

        /// GCP refuses an IAM binding whose engine does not exist yet, and a Live engine is created
        /// by its controller after apply. Emitting the member anyway would reference a block this
        /// template never renders.
        #[test]
        fn a_live_remote_sandbox_is_refused_naming_the_lifecycle() {
            let stack = stack_with(SandboxEgress::Allow, None, ResourceLifecycle::Live, true);
            let error = with_context(&stack, |ctx| {
                GcpAgentPlatformSandboxEmitter
                    .emit(ctx)
                    .expect_err("a Live engine has no scope a grant can name")
            });

            assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
            let rendered = error.to_string();
            assert!(rendered.contains("agents"), "names the sandbox: {rendered}");
            assert!(rendered.contains("frozen"), "names the fix: {rendered}");
        }

        /// An `internet` sandbox answers only through its private endpoint, to the edge credential
        /// the signing identity signs, so its binding names that identity without preview ports.
        #[test]
        fn an_internet_sandbox_gets_the_signing_identity_without_preview_ports() {
            let emitted = emit_binding(SandboxEgress::Internet, Some(3600))
                .expect("the binding renders")
                .expect("an Agent Platform sandbox has a binding");
            assert!(object_keys(&emitted).contains("previewServiceAccount"));
            // Every other sandbox has it too: interactive commands go through the same edge.
            let open = emit_binding(SandboxEgress::Allow, Some(3600))
                .expect("the binding renders")
                .expect("an Agent Platform sandbox has a binding");
            assert!(object_keys(&open).contains("previewServiceAccount"));
        }

        /// The emitted keys are read against the binding type, not a second hand-typed list, so
        /// a rename on either side fails here rather than reaching a customer's cluster. The ttl is
        /// set on both sides so the key sets are comparable whole.
        #[test]
        fn emitted_binding_keys_match_the_binding_type() {
            let emitted = emit_binding(SandboxEgress::Allow, Some(3600))
                .expect("the binding renders")
                .expect("an Agent Platform sandbox has a binding");

            // Every sandbox's setup creates the edge signer, so the binding names it.
            let mut binding =
                SandboxBinding::gcp_agent_platform("e", "t", "us-central1", Some(3600), true);
            if let SandboxBinding::GcpAgentPlatform(inner) = &mut binding {
                inner.preview_service_account =
                    Some("signer@p.iam.gserviceaccount.com".to_string().into());
            }
            let type_json = serde_json::to_value(binding).expect("the binding type serializes");
            let type_keys: BTreeSet<String> = type_json
                .as_object()
                .expect("the binding serializes as an object")
                .keys()
                .cloned()
                .collect();

            assert_eq!(
                object_keys(&emitted),
                type_keys,
                "emitted keys must track the binding type"
            );
        }

        /// The binding is where a workload reads whether the sandbox is isolated, so each mode must
        /// render its own value, not only a key of the right name.
        #[test]
        fn allow_egress_renders_the_declared_mode() {
            for (egress, expected) in [
                (SandboxEgress::None, false),
                // `internet` goes through the gateway: the template's own switch stays closed.
                (SandboxEgress::Internet, false),
                (SandboxEgress::Allow, true),
            ] {
                let emitted = emit_binding(egress.clone(), None)
                    .expect("the binding renders")
                    .expect("an Agent Platform sandbox has a binding");
                let Expression::Object(map) = &emitted else {
                    panic!("expected an object, got {emitted:?}");
                };
                let value = map
                    .iter()
                    .find_map(|(key, value)| {
                        let name = match key {
                            hcl::expr::ObjectKey::Identifier(id) => id.as_str(),
                            hcl::expr::ObjectKey::Expression(Expression::String(s)) => s.as_str(),
                            _ => return None,
                        };
                        (name == "allowEgress").then_some(value)
                    })
                    .expect("allowEgress is emitted");
                assert_eq!(value, &Expression::Bool(expected), "{egress:?}");
            }
        }

        /// A declared lifetime reaches the binding under the name the type reads, and an absent
        /// one is omitted to match the binding's `skip_serializing_if`. Both sides are asserted:
        /// a key spelled differently on one of them deserializes as absent in the workload, so a
        /// declared ceiling would be dropped rather than refused.
        #[test]
        fn a_lifetime_ceiling_is_present_only_when_declared() {
            let with_ttl = emit_binding(SandboxEgress::None, Some(1800))
                .expect("renders")
                .expect("binding");
            assert!(
                object_keys(&with_ttl).contains("maxLifetimeSeconds"),
                "a declared lifetime reaches the binding"
            );

            let typed = serde_json::to_value(alien_core::GcpAgentPlatformSandboxBinding {
                default_policy: None,
                egress: None,
                engine: alien_core::BindingValue::Value("engine".to_string()),
                template: alien_core::BindingValue::Value("template".to_string()),
                region: alien_core::BindingValue::Value("us-central1".to_string()),
                max_lifetime_seconds: Some(1800),
                allow_egress: false,
                preview_service_account: None,
                preview_ports: Vec::new(),
                private_endpoint: None,
            })
            .expect("the binding type serializes");
            assert_eq!(
                typed
                    .get("maxLifetimeSeconds")
                    .and_then(serde_json::Value::as_i64),
                Some(1800),
                "the binding type must read the name the module emits"
            );

            let without = emit_binding(SandboxEgress::None, None)
                .expect("renders")
                .expect("binding");
            assert!(
                !object_keys(&without).contains("maxLifetimeSeconds"),
                "an undeclared lifetime is absent from the binding"
            );
        }
    }
}
