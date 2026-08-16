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
use alien_core::{import::EmitContext, Result, Sandbox, SandboxEgress};
use hcl::expr::Expression;

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
        let _ = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([(
            "launcherPath",
            Expression::String(LAUNCHER_PATH.to_string()),
        )]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
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
