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
    /// `--allow-egress` is a switch: rendering the list as `true` opens every address it was
    /// written to exclude, and rendering it as `false` denies every one it was written to permit.
    /// Neither is the declaration, so neither is emitted.
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
}
