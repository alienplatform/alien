//! GCP Agent Platform sandbox emitter.
//!
//! Emits the Agent Platform sandbox binding — the engine and template resource-name shapes the runtime provider reads
//! — and refuses domain-scoped egress, which the single internet-access switch cannot express.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Result, Sandbox};
use alien_error::AlienError;
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

/// The engine, template, region and ttl fields shared by the import ref and the binding ref.
///
/// `engine` and `template` carry runtime-assigned ids addressed by a resource-name convention over
/// the setup label rather than a Terraform resource attribute; the Live path takes the real names
/// from the controller's binding params. `sessionTtlSeconds` is present only when the declaration
/// set a lifetime, matching the binding's `skip_serializing_if`.
fn agent_platform_fields(sandbox: &Sandbox, label: &str) -> Vec<(&'static str, Expression)> {
    let mut fields = vec![
        (
            "engine",
            expr::template(format!(
                "projects/${{var.gcp_project}}/locations/${{var.gcp_region}}/reasoningEngines/{label}"
            )),
        ),
        (
            "template",
            expr::template(format!(
                "projects/${{var.gcp_project}}/locations/${{var.gcp_region}}/reasoningEngines/{label}/sandboxEnvironmentTemplates/{label}"
            )),
        ),
        ("region", expr::raw("var.gcp_region")),
    ];
    if let Some(seconds) = sandbox.session.max_lifetime_seconds {
        fields.push((
            "sessionTtlSeconds",
            Expression::Number(hcl::Number::from(seconds as i64)),
        ));
    }
    fields
}

/// Emits the GCP Agent Platform sandbox binding: the durable Agent Engine, the release-owned
/// template, the region and the session ttl.
///
/// The engine is a Live resource with its own controller and no Terraform analogue — Vertex
/// exposes no `google_…reasoning_engine` — so `emit` is empty as in `gcp/ai.rs` and identity
/// travels in the binding, not a resource block.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAgentPlatformSandboxEmitter;

impl TfEmitter for GcpAgentPlatformSandboxEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // The engine and template are created by Live controllers after apply and carry
        // runtime-assigned names, so neither is a Terraform resource block.
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        Ok(expr::object(agent_platform_fields(sandbox, label)))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_domain_egress(sandbox)?;
        let mut fields = agent_platform_fields(sandbox, label);
        fields.push((
            "service",
            Expression::String(AGENT_PLATFORM_SERVICE.to_string()),
        ));
        Ok(Some(expr::object(fields)))
    }
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
        use indexmap::IndexMap;
        use std::collections::BTreeSet;

        fn emit_binding(egress: SandboxEgress, ttl: Option<u32>) -> Result<Option<Expression>> {
            let stack = Stack::new("acme".to_string())
                .add(
                    Sandbox::new("agents".to_string())
                        .code(SandboxCode::Image {
                            image: "ubuntu".to_string(),
                        })
                        .egress(egress)
                        .session(SandboxSessionPolicy {
                            max_lifetime_seconds: ttl,
                            idle_suspend_seconds: None,
                        })
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            let resource = stack
                .resources
                .get("agents")
                .expect("the sandbox is in the stack");
            let names = IndexMap::from([("agents".to_string(), "agents".to_string())]);
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack: &stack,
                resource,
                resource_id: "agents",
                platform: alien_core::Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
            };
            GcpAgentPlatformSandboxEmitter.emit_binding_ref(&ctx)
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
