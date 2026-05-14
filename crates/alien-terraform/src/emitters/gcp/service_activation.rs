//! GCP ServiceActivation — `google_project_service`.
//!
//! `ServiceActivation` resources are added to the stack by the
//! `GcpServiceActivationMutation` preflight when the stack contains
//! resources that need a GCP API enabled (Cloud Run, Pub/Sub, etc.).
//! The emitter turns each `ServiceActivation` into a
//! `google_project_service` so the API is enabled before any
//! dependent resource attempts to use it.
//!
//! `disable_on_destroy = false` is the right default: customers may be
//! using these APIs outside the stack, and `terraform destroy` should
//! not break unrelated workloads. `disable_dependent_services = false`
//! likewise — explicit destruction of dependent services is the
//! customer's call.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, Result, ServiceActivation};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpServiceActivationEmitter;

impl TfEmitter for GcpServiceActivationEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let activation = downcast::<ServiceActivation>(ctx, ServiceActivation::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        Ok(TfFragment::default().with_resource(resource_block(
            "google_project_service",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "service",
                    Expression::String(activation.service_name.clone()),
                ),
                attr("disable_on_destroy", Expression::Bool(false)),
                attr("disable_dependent_services", Expression::Bool(false)),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<ServiceActivation>(ctx, ServiceActivation::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "serviceName",
                expr::traversal(["google_project_service", label, "service"]),
            ),
            ("activated", Expression::Bool(true)),
        ]))
    }
}
