//! Whether a cluster can run sandboxes at all.
//!
//! This check exists because of how GKE Autopilot behaves: an unschedulable sandbox
//! pod does **not** get rejected. Node auto-provisioning picks it up instead —
//! `TriggeredScaleUp ... 0->1 (max: 1000)` — so the pod sits in `Pending` while nodes are
//! created and billed.
//!
//! Two consequences, and they pull in opposite directions:
//!
//! - A controller that fail-fasts on `Pending` is wrong; that is a cluster working as designed.
//! - A controller that waits silently spends the customer's money on a cluster that may have no
//!   sandboxed runtime at all.
//!
//! So eligibility is decided **before** a pod is created, against the RuntimeClass list, which
//! is cluster-scoped and therefore independent of whether any node exists yet.

use k8s_openapi::api::node::v1::RuntimeClass;

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Runtime handlers that provide a sandboxed kernel boundary.
///
/// A plain pod shares the node kernel with everything on it, so an unrecognised handler is
/// refused rather than accepted with a warning.
const SANDBOXED_HANDLERS: &[&str] = &["gvisor", "runsc", "kata", "kata-containers", "kata-qemu"];

/// Confirms the cluster declares the requested RuntimeClass and that it is a sandboxed one.
pub fn require_sandboxed_runtime_class(
    sandbox_id: &str,
    requested: &str,
    available: &[RuntimeClass],
) -> Result<()> {
    let Some(runtime_class) = available
        .iter()
        .find(|candidate| candidate.metadata.name.as_deref() == Some(requested))
    else {
        let names: Vec<&str> = available
            .iter()
            .filter_map(|candidate| candidate.metadata.name.as_deref())
            .collect();

        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "the cluster declares no RuntimeClass '{requested}'; it has {names:?}. A sandbox \
                 needs a sandboxed runtime, and a pod would sit in Pending rather than fail."
            ),
            resource_id: Some(sandbox_id.to_string()),
        }));
    };

    if !is_sandboxed_handler(&runtime_class.handler) {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "RuntimeClass '{requested}' uses handler '{}', which is not a sandboxed runtime. \
                 Untrusted code would share the node kernel.",
                runtime_class.handler
            ),
            resource_id: Some(sandbox_id.to_string()),
        }));
    }

    Ok(())
}

fn is_sandboxed_handler(handler: &str) -> bool {
    let handler = handler.to_ascii_lowercase();
    SANDBOXED_HANDLERS
        .iter()
        .any(|known| handler == *known || handler.starts_with(&format!("{known}-")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn runtime_class(name: &str, handler: &str) -> RuntimeClass {
        RuntimeClass {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            handler: handler.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_sandboxed_runtime_class_is_accepted() {
        for (name, handler) in [
            ("gvisor", "runsc"),
            ("kata", "kata-containers"),
            ("kata-qemu", "kata-qemu"),
        ] {
            require_sandboxed_runtime_class("agent", name, &[runtime_class(name, handler)])
                .unwrap_or_else(|error| panic!("{name}/{handler} should be accepted: {error}"));
        }
    }

    /// The whole point of the resource. A cluster without a sandboxed runtime must be refused at
    /// provision, not discovered when a pod never schedules.
    #[test]
    fn a_cluster_without_the_runtime_class_is_refused_before_any_pod_is_created() {
        let error = require_sandboxed_runtime_class("agent", "gvisor", &[])
            .expect_err("an empty cluster must be refused");

        let rendered = error.to_string();
        assert!(rendered.contains("gvisor"), "names what is missing: {rendered}");
        assert!(
            rendered.contains("Pending"),
            "explains why waiting is not the answer: {rendered}"
        );
    }

    /// A RuntimeClass exists but points at the ordinary container runtime. Accepting it would
    /// run untrusted code on the shared node kernel while reporting success.
    #[test]
    fn an_unsandboxed_handler_is_refused_even_when_the_name_matches() {
        let error = require_sandboxed_runtime_class(
            "agent",
            "gvisor",
            &[runtime_class("gvisor", "runc")],
        )
        .expect_err("runc is not a sandbox");

        assert!(error.to_string().contains("runc"), "names the handler");
    }

    #[test]
    fn the_available_classes_are_listed_so_the_operator_can_act() {
        let error = require_sandboxed_runtime_class(
            "agent",
            "gvisor",
            &[runtime_class("kata", "kata-containers")],
        )
        .expect_err("gvisor is absent");

        assert!(
            error.to_string().contains("kata"),
            "an error that does not say what IS available makes the operator go looking"
        );
    }
}

#[cfg(test)]
mod live_cluster_shape {
    use super::*;

    /// The RuntimeClasses a GKE Autopilot cluster declares, as a cluster returns them rather
    /// than as an author imagined them. Only `gvisor` is a sandbox runtime; the others are here
    /// because eligibility has to pick it out of a list that contains ordinary ones.
    fn autopilot_runtime_classes() -> Vec<RuntimeClass> {
        [
            ("confidential-linked-runner", "confidential-linked-runner"),
            ("gvisor", "gvisor"),
            ("linked-runner", "linked-runner"),
        ]
        .into_iter()
        .map(|(name, handler)| RuntimeClass {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            handler: handler.to_string(),
            ..Default::default()
        })
        .collect()
    }

    #[test]
    fn gvisor_on_a_real_cluster_is_accepted() {
        require_sandboxed_runtime_class("sbx", "gvisor", &autopilot_runtime_classes())
            .expect("gvisor is a sandboxed runtime and the cluster declares it");
    }

    /// The other two classes that cluster declares are *not* sandboxed runtimes. Naming one of
    /// them must fail, or a sandbox would run on a shared kernel while looking configured.
    #[test]
    fn a_non_sandboxed_class_on_the_same_cluster_is_refused() {
        for name in ["linked-runner", "confidential-linked-runner"] {
            let error = require_sandboxed_runtime_class("sbx", name, &autopilot_runtime_classes())
                .expect_err("only a sandboxed handler may run untrusted code");
            assert!(
                error.to_string().contains(name),
                "the refusal must name the class asked for: {error}"
            );
        }
    }

    /// The message has to carry what the cluster *does* have — an operator reading
    /// "no RuntimeClass 'gvisor'" needs to know whether to install gVisor or fix a typo.
    #[test]
    fn a_missing_class_reports_what_the_cluster_offers() {
        let error = require_sandboxed_runtime_class("sbx", "kata-containers", &autopilot_runtime_classes())
            .expect_err("the cluster has no kata-containers");
        let message = error.to_string();
        assert!(message.contains("gvisor"), "must list what is available: {message}");
    }
}
