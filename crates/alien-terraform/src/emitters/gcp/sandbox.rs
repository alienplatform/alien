//! GCP Agent Platform sandbox emitter.
//!
//! The sandbox is the release-owned environment template, which the runtime controller creates
//! under its engine, so setup emits no resource for it — only the remote grant, which is scoped to
//! the engine its own emitter creates. It refuses domain-scoped egress here rather than at apply,
//! because the single internet-access switch cannot express a hostname list.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::{
        agent_platform_engine::ENGINE_RESOURCE,
        helpers::{
            binding_label_for_role, downcast, emit_custom_roles_for_bindings, permission_context,
            required_label, role_expression_for_binding, service_account_member_for_label,
        },
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, GcpAgentPlatformEngine, RemoteBindings, ResourceLifecycle,
    Result, Sandbox,
};
use alien_error::{AlienError, Context};
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
/// `sessionTtlSeconds` is present only when the declaration set a lifetime, matching the binding's
/// `skip_serializing_if`.
fn agent_platform_fields(
    ctx: &EmitContext<'_>,
    sandbox: &Sandbox,
    label: &str,
) -> Result<Vec<(&'static str, Expression)>> {
    let engine = engine_name(ctx, label)?;
    let mut fields = vec![
        ("engine", expr::template(engine.clone())),
        (
            "template",
            expr::template(format!("{engine}/sandboxEnvironmentTemplates/{label}")),
        ),
        ("region", expr::raw("var.gcp_region")),
    ];
    if let Some(seconds) = sandbox.session.max_lifetime_seconds {
        fields.push((
            "sessionTtlSeconds",
            Expression::Number(hcl::Number::from(seconds as i64)),
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
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        // The engine id is not here: it is server-assigned, and the engine registers its own.
        Ok(expr::object([("region", expr::raw("var.gcp_region"))]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        let mut fields = agent_platform_fields(ctx, sandbox, label)?;
        fields.push((
            "service",
            Expression::String(AGENT_PLATFORM_SERVICE.to_string()),
        ));
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
    let engine_label = engine_label(ctx)?;
    let permission_set = alien_permissions::get_permission_set(definition.permission_set)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "{} permission set is not registered",
                    definition.permission_set
                ),
            })
        })?;

    let context = permission_context(access_label, ctx.stack.id())
        .with_resource_name(format!("${{{ENGINE_RESOURCE}.{engine_label}.name}}"));
    let plan = GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "failed to generate GCP remote Sandbox permissions".to_string(),
        })?;

    let bindings = plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &plan, &bindings)?;
    let member = service_account_member_for_label(access_label);
    for (index, binding) in bindings.iter().enumerate() {
        if binding.resource_kind != Some(GcpBindingResourceKind::VertexAiReasoningEngine) {
            continue;
        }
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        fragment.resource_blocks.push(resource_block(
            "google_vertex_ai_reasoning_engine_iam_member",
            &format!("{role_label}_{engine_label}_access_{index}"),
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
            ResourceLifecycle, SandboxCode, SandboxEgress, SandboxSessionPolicy, Stack,
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
                })
                .egress(egress)
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: ttl,
                    idle_suspend_seconds: None,
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

        fn attribute(block: &hcl::structure::Block, key: &str) -> String {
            block
                .body
                .iter()
                .find_map(|structure| match structure {
                    Structure::Attribute(attribute) if attribute.key.as_str() == key => {
                        Some(attribute.expr.to_string())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the block carries no '{key}': {block:?}"))
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

            // A sandbox nobody published gets no grant, whatever else the stack declares.
            let unpublished =
                stack_with(SandboxEgress::Allow, None, ResourceLifecycle::Frozen, false);
            let fragment = with_context(&unpublished, |ctx| {
                GcpAgentPlatformSandboxEmitter.emit(ctx).expect("renders")
            });
            assert!(fragment.resource_blocks.is_empty(), "{fragment:?}");
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

        /// The emitted keys are read against the binding type, not a second hand-typed list, so
        /// a rename on either side fails here rather than reaching a customer's cluster. The ttl is
        /// set on both sides so the key sets are comparable whole.
        #[test]
        fn emitted_binding_keys_match_the_binding_type() {
            let emitted = emit_binding(SandboxEgress::Allow, Some(3600))
                .expect("the binding renders")
                .expect("an Agent Platform sandbox has a binding");

            let type_json = serde_json::to_value(SandboxBinding::gcp_agent_platform(
                "e",
                "t",
                "us-central1",
                Some(3600),
            ))
            .expect("the binding type serializes");
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

        /// A hostname list has no representation in the single internet-access switch, so it is
        /// refused naming the sandbox and both accepted modes — not approximated to a boolean.
        #[test]
        fn domain_egress_is_refused_naming_the_sandbox_and_modes() {
            let error = emit_binding(
                SandboxEgress::AllowDomains {
                    domains: vec!["api.example.com".to_string()],
                },
                None,
            )
            .expect_err("a hostname list has nothing to render into on Agent Platform");

            assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
            let rendered = error.to_string();
            assert!(rendered.contains("agents"), "names the sandbox: {rendered}");
            assert!(
                rendered.contains("allow") && rendered.contains("deny"),
                "names both accepted modes: {rendered}"
            );

            for accepted in [SandboxEgress::Deny, SandboxEgress::Allow] {
                emit_binding(accepted.clone(), None)
                    .unwrap_or_else(|error| panic!("{accepted:?} is a switch position: {error}"));
            }
        }

        /// A declared lifetime reaches the binding; an absent one is omitted, matching the binding's
        /// `skip_serializing_if` so the two never disagree on whether the key is present.
        #[test]
        fn session_ttl_is_present_only_when_declared() {
            let with_ttl = emit_binding(SandboxEgress::Deny, Some(1800))
                .expect("renders")
                .expect("binding");
            assert!(
                object_keys(&with_ttl).contains("sessionTtlSeconds"),
                "a declared lifetime reaches the binding"
            );

            let without = emit_binding(SandboxEgress::Deny, None)
                .expect("renders")
                .expect("binding");
            assert!(
                !object_keys(&without).contains("sessionTtlSeconds"),
                "an undeclared lifetime is absent from the binding"
            );
        }
    }
}
