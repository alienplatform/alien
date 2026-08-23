//! GCP Sandbox — nothing built, because Cloud Run already ships the launcher.
//!
//! A Cloud Run sandbox is a nested gVisor sandbox started by a binary Cloud Run injects into the
//! container when it carries `sandboxLauncher`, which the `gcp_sandbox_launcher` preflight sets on
//! the worker hosting the sandbox. There is no control plane to provision, no group to name and no
//! endpoint to hand over: setup's whole contribution is telling the runtime where the binary is
//! and whether sandboxes may reach the network.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxEgress};
use alien_error::AlienError;
use hcl::expr::Expression;

/// Refuses an egress mode the launcher cannot deliver.
///
/// `--allow-egress` is a switch, so a hostname list has nowhere to go and would otherwise be
/// carried as its nearest boolean — denying everything the declaration asked to permit, with
/// nothing anywhere saying so.
fn refuse_unsupported_egress(sandbox: &Sandbox) -> Result<()> {
    match &sandbox.egress {
        SandboxEgress::Deny | SandboxEgress::Allow => Ok(()),
        SandboxEgress::AllowDomains { .. } => Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform emit sandbox '{}'", sandbox.id()),
            reason: "the Cloud Run sandbox launcher takes a single egress switch, so a hostname \
                     list has nothing to render into. Declare egress: deny or egress: allow"
                .to_string(),
        })),
    }
}

/// Where Cloud Run mounts the sandbox CLI inside a launcher-enabled container.
const LAUNCHER_PATH: &str = "/usr/local/gcp/bin/sandbox";

/// Emits the launcher's location; Cloud Run provides everything else.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcpSandboxEmitter;

impl TfEmitter for GcpSandboxEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // Deliberately empty: see the module note. The launcher arrives with the container.
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = required_label(ctx)?;
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        refuse_unsupported_egress(sandbox)?;
        Ok(expr::object([
            (
                "launcherPath",
                Expression::String(LAUNCHER_PATH.to_string()),
            ),
            (
                "allowEgress",
                Expression::Bool(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        refuse_unsupported_egress(sandbox)?;
        Ok(Some(expr::object([
            ("service", Expression::String("sandbox-gcp".to_string())),
            (
                "launcherPath",
                Expression::String(LAUNCHER_PATH.to_string()),
            ),
            // Carried in the binding rather than passed per create: the launcher takes
            // `--allow-egress` per sandbox, and a limit the application supplies is one it can
            // decline to supply.
            (
                "allowEgress",
                Expression::Bool(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
        ])))
    }
}

/// Serde `service` tag of the T07 `GcpAgentPlatformSandboxBinding`, and the resource-name shapes
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
/// `engine` and `template` carry runtime-assigned ids, so at emit time they are addressed by a
/// resource-name convention over the setup label rather than a Terraform resource attribute — this
/// emitter is unregistered and the Live path takes the real names from the controller's binding
/// params. `sessionTtlSeconds` is present only when the declaration set a lifetime, matching the
/// binding's `skip_serializing_if`.
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
/// template, the region and the session ttl (T07).
///
/// Unregistered on purpose, like the provider it feeds (T05): `built_ins` keeps Cloud Run as the
/// GCP sandbox backend, so the generator never dispatches here and this is exercised by direct
/// unit test until the cutover moves the registration. The engine is a Frozen setup resource with
/// no Terraform analogue — Vertex exposes no `google_…reasoning_engine` — so `emit` is empty as in
/// `gcp/ai.rs` and identity travels in the binding, not a resource block.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAgentPlatformSandboxEmitter;

impl TfEmitter for GcpAgentPlatformSandboxEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        // The engine is setup-created and monitored only; the template is reconciled by the Live
        // controller after apply. Both carry runtime-assigned names, so neither is a resource block.
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
    use super::*;
    use alien_core::{SandboxCode, SandboxSessionPolicy};

    fn sandbox_with(egress: SandboxEgress) -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build()
    }

    /// A hostname list is refused rather than carried as its nearest boolean.
    ///
    /// `--allow-egress` is a switch: rendering the list as `true` or `false` opens or denies
    /// addresses the declaration did not say to. Neither is the declaration, so neither is emitted.
    ///
    /// The second gate, not the first — a customer meets `domainEgressRules` at plan time. This
    /// one covers the paths that render without planning.
    #[test]
    fn a_hostname_allowlist_is_refused_rather_than_approximated() {
        let error = refuse_unsupported_egress(&sandbox_with(SandboxEgress::AllowDomains {
            domains: vec!["api.example.com".to_string()],
        }))
        .expect_err("a hostname list has nothing to render into on Cloud Run");

        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
        assert!(
            error.to_string().contains("agents"),
            "the refusal has to name the sandbox: {error}"
        );

        for accepted in [SandboxEgress::Deny, SandboxEgress::Allow] {
            refuse_unsupported_egress(&sandbox_with(accepted.clone()))
                .unwrap_or_else(|error| panic!("{accepted:?} is a switch position: {error}"));
        }
    }

    // ---- Agent Platform emitter: unregistered, so exercised by direct invocation. -------------

    mod agent_platform {
        use super::super::*;
        use alien_core::bindings::SandboxBinding;
        use alien_core::{ResourceLifecycle, SandboxCode, SandboxSessionPolicy, Stack, StackSettings};
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
            let resource = stack.resources.get("agents").expect("the sandbox is in the stack");
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

        /// The emitted keys are read against the T07 binding type, not a second hand-typed list, so
        /// a rename on either side fails here rather than reaching a customer's cluster. The ttl is
        /// set on both sides so the key sets are comparable whole.
        #[test]
        fn emitted_binding_keys_match_the_t07_binding_type() {
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
                "emitted keys must track the T07 binding type"
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
