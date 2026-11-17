//! The cluster-side limit on the Operator's `patch` of workloads.
//!
//! Kubernetes RBAC cannot limit a `patch` to one field, so an operation that
//! restarts a Deployment, StatefulSet or DaemonSet (`kubectl rollout restart`
//! sets the `kubectl.kubernetes.io/restartedAt` annotation on the pod template)
//! would otherwise let the Operator change the image, environment, service
//! account, volumes or security settings of any workload it can patch. The
//! chart therefore ships a ValidatingAdmissionPolicy and binding together with
//! the `patch` rule: for requests from the Operator's ServiceAccount, an UPDATE
//! is admitted only when the object equals the stored one except for that
//! annotation. The rule and the policy come from one decision in the
//! generator and are rendered together or not at all.
//!
//! Scope of the policy:
//! - Only the main resource is matched. `deployments/status` is never granted,
//!   and `deployments/scale` (an existing, separate grant) only changes the
//!   replica count.
//! - Other users are unaffected: a match condition selects the Operator's
//!   ServiceAccount.
//! - The API needs Kubernetes 1.30 or later. Charts render the rule and the
//!   policy only when the cluster advertises it.
//!
//! CEL cannot remove a key from a map or list the fields of an object, so
//! "equal except one annotation" is written as explicit comparisons: the
//! annotation map key by key, and every other field of the spec and of the pod
//! template metadata by name. `object.spec.?f == oldObject.spec.?f` is true
//! when the field is absent on both sides. The field lists are checked
//! against the live API schema by `tests/workload_restart_kind.rs`, so a field
//! added by a newer Kubernetes fails that test instead of silently going
//! unchecked.

/// Annotation `kubectl rollout restart` sets on the pod template.
pub(crate) const RESTART_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

/// Template condition: the cluster serves the admission policy API.
pub(crate) const POLICY_API_CONDITION: &str =
    r#".Capabilities.APIVersions.Has "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy""#;

/// One workload kind the `patch` grant can cover.
pub(crate) struct RestartKind {
    /// RBAC resource, also the resource the policy matches.
    pub resource: &'static str,
    /// Every field of `spec` except `template`.
    spec_fields: &'static [&'static str],
    /// Annotation of the workload itself that the API server rewrites when the pod template
    /// changes, so a restart changes it too. DaemonSets keep a counter of template changes.
    server_annotation: Option<&'static str>,
}

pub(crate) const RESTART_KINDS: &[RestartKind] = &[
    RestartKind {
        resource: "deployments",
        spec_fields: &[
            "minReadySeconds",
            "paused",
            "progressDeadlineSeconds",
            "replicas",
            "revisionHistoryLimit",
            "selector",
            "strategy",
        ],
        server_annotation: None,
    },
    RestartKind {
        resource: "statefulsets",
        spec_fields: &[
            "minReadySeconds",
            "ordinals",
            "persistentVolumeClaimRetentionPolicy",
            "podManagementPolicy",
            "replicas",
            "revisionHistoryLimit",
            "selector",
            "serviceName",
            "updateStrategy",
            "volumeClaimTemplates",
        ],
        server_annotation: None,
    },
    RestartKind {
        resource: "daemonsets",
        spec_fields: &["minReadySeconds", "revisionHistoryLimit", "selector", "updateStrategy"],
        server_annotation: Some("deprecated.daemonset.template.generation"),
    },
];

/// Every field of the pod template's metadata except `annotations`.
pub(crate) const TEMPLATE_METADATA_FIELDS: &[&str] = &[
    "creationTimestamp",
    "deletionGracePeriodSeconds",
    "deletionTimestamp",
    "finalizers",
    "generateName",
    "generation",
    "labels",
    "managedFields",
    "name",
    "namespace",
    "ownerReferences",
    "resourceVersion",
    "selfLink",
    "uid",
];

/// Metadata of the workload itself that the request could otherwise set.
const OBJECT_METADATA_FIELDS: &[&str] = &["finalizers", "labels", "ownerReferences"];

/// The restart kind for an RBAC resource, if it is one.
pub(crate) fn restart_kind(resource: &str) -> Option<&'static RestartKind> {
    RESTART_KINDS.iter().find(|kind| kind.resource == resource)
}

/// Whether this rule verb is the `patch` the policy exists for. The grant is
/// never rendered without it.
pub(crate) fn is_restart_patch(api_group: &str, resource: &str, verb: &str) -> bool {
    api_group == "apps" && verb == "patch" && restart_kind(resource).is_some()
}

fn same(path: &str, fields: &[&str]) -> String {
    fields
        .iter()
        .map(|field| format!("object.{path}.?{field} == oldObject.{path}.?{field}"))
        .collect::<Vec<_>>()
        .join(" && ")
}

/// Named CEL variables: the annotation maps (empty when absent) of the pod template and of the
/// workload itself, before and after.
pub(crate) const VARIABLES: &[(&str, &str)] = &[
    ("newTemplateAnnotations", "object.spec.template.metadata.?annotations.orValue({})"),
    ("oldTemplateAnnotations", "oldObject.spec.template.metadata.?annotations.orValue({})"),
    ("newAnnotations", "object.metadata.?annotations.orValue({})"),
    ("oldAnnotations", "oldObject.metadata.?annotations.orValue({})"),
];

/// The annotation maps `new` and `old` hold the same keys and values, except for the `ignored`
/// keys, which may be added, changed or removed.
fn same_annotations_except(new: &str, old: &str, ignored: &[&str]) -> String {
    let is_ignored = if ignored.is_empty() {
        "false".to_owned()
    } else {
        ignored
            .iter()
            .map(|key| format!("k == '{key}'"))
            .collect::<Vec<_>>()
            .join(" || ")
    };
    format!(
        "variables.{new}.all(k, {is_ignored} || (k in variables.{old} && variables.{old}[k] == variables.{new}[k])) && variables.{old}.all(k, {is_ignored} || k in variables.{new})"
    )
}

/// The validations (expression, message) of a kind's policy.
pub(crate) fn validations(kind: &RestartKind) -> Vec<(String, &'static str)> {
    let server_annotations: Vec<&str> = kind.server_annotation.into_iter().collect();
    vec![
        (
            same_annotations_except(
                "newTemplateAnnotations",
                "oldTemplateAnnotations",
                &[RESTART_ANNOTATION],
            ),
            "a restart may change only the restartedAt annotation of the pod template",
        ),
        (
            "object.spec.template.spec == oldObject.spec.template.spec".to_owned(),
            "the pod spec (image, environment, service account, volumes, security settings) cannot be changed by a restart",
        ),
        (
            same("spec.template.metadata", TEMPLATE_METADATA_FIELDS),
            "the pod template's labels and other metadata cannot be changed by a restart",
        ),
        (
            same("spec", kind.spec_fields),
            "the workload spec (replicas, selector, strategy, ...) cannot be changed by a restart",
        ),
        (
            format!(
                "{} && {}",
                same("metadata", OBJECT_METADATA_FIELDS),
                same_annotations_except("newAnnotations", "oldAnnotations", &server_annotations)
            ),
            "the workload's labels, annotations, owners and finalizers cannot be changed by a restart",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_compares_selector_replicas_and_update_strategy_fields() {
        for kind in RESTART_KINDS {
            assert!(kind.spec_fields.contains(&"selector"), "{}", kind.resource);
        }
        assert!(restart_kind("deployments").is_some());
        assert!(restart_kind("deployments/scale").is_none());
        assert!(restart_kind("deployments/status").is_none());
        assert!(!is_restart_patch("apps", "deployments", "update"));
        assert!(is_restart_patch("apps", "daemonsets", "patch"));
        assert!(!is_restart_patch("", "deployments", "patch"));
    }
}
