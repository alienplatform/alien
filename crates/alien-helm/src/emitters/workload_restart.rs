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
//! - Only UPDATE is matched (a patch, server-side apply included, is admitted
//!   as an UPDATE of an existing object); CREATE is not granted.
//! - Other users are unaffected: a match condition selects the Operator's
//!   ServiceAccount.
//! - The API needs Kubernetes 1.30 or later. Charts render the rule and the
//!   policy only when the cluster advertises it.
//!
//! CEL cannot remove a key from a map, so "equal except one annotation" is
//! written as comparisons that leave the annotation out:
//! - the whole pod spec (`template.spec`) is compared as one value;
//! - every other key of `spec` and of the pod template's metadata is compared
//!   by iterating the keys that are present on either side (`dyn(...)` reads an
//!   object as a map), so a field a future Kubernetes adds is compared too, not
//!   silently skipped;
//! - the pod template's annotations are compared key by key, leaving out the
//!   restart annotation (an absent map counts as empty);
//! - of the workload's own metadata, the fields a request can set (labels,
//!   annotations, owner references, finalizers) are compared. The server
//!   changes the rest (generation, resource version, managed fields) itself.

/// Annotation `kubectl rollout restart` sets on the pod template.
pub(crate) const RESTART_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

/// Annotation of the workload itself that the API server rewrites when a
/// DaemonSet's pod template changes (a counter of template changes), so a
/// restart changes it too. It carries no privilege.
const SERVER_ANNOTATION: &str = "deprecated.daemonset.template.generation";

/// Template condition: the cluster serves the admission policy API.
pub(crate) const POLICY_API_CONDITION: &str =
    r#".Capabilities.APIVersions.Has "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy""#;

/// RBAC resources of the workload kinds a restart can patch.
pub(crate) const RESTART_RESOURCES: [&str; 3] = ["deployments", "statefulsets", "daemonsets"];

/// Metadata of the workload itself that a request could otherwise set.
const OBJECT_METADATA_FIELDS: [&str; 3] = ["finalizers", "labels", "ownerReferences"];

/// Whether this rule verb is the `patch` the policy exists for. The grant is
/// never rendered without it.
pub(crate) fn is_restart_patch(api_group: &str, resource: &str, verb: &str) -> bool {
    api_group == "apps" && verb == "patch" && RESTART_RESOURCES.contains(&resource)
}

/// Named CEL variables: the spec and the pod template's metadata read as maps,
/// and the annotation maps (empty when absent), before and after.
pub(crate) const VARIABLES: [(&str, &str); 8] = [
    ("newSpec", "dyn(object.spec)"),
    ("oldSpec", "dyn(oldObject.spec)"),
    ("newTemplateMetadata", "dyn(object.spec.template.metadata)"),
    ("oldTemplateMetadata", "dyn(oldObject.spec.template.metadata)"),
    ("newTemplateAnnotations", "object.spec.template.metadata.?annotations.orValue({})"),
    ("oldTemplateAnnotations", "oldObject.spec.template.metadata.?annotations.orValue({})"),
    ("newAnnotations", "object.metadata.?annotations.orValue({})"),
    ("oldAnnotations", "oldObject.metadata.?annotations.orValue({})"),
];

/// `new` and `old` (CEL variables holding maps) have the same keys and the
/// same value for each key, leaving out the `ignored` keys.
fn same_except(new: &str, old: &str, ignored: &[&str]) -> String {
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

/// The validations (expression, message) of the policy.
pub(crate) fn validations() -> Vec<(String, &'static str)> {
    let metadata = OBJECT_METADATA_FIELDS
        .iter()
        .map(|field| format!("object.metadata.?{field} == oldObject.metadata.?{field}"))
        .collect::<Vec<_>>()
        .join(" && ");
    vec![
        (
            "oldObject != null".to_owned(),
            "only an update of an existing workload is admitted",
        ),
        (
            "object.spec.template.spec == oldObject.spec.template.spec".to_owned(),
            "the pod spec (image, environment, service account, volumes, security settings) cannot be changed by a restart",
        ),
        (
            same_except("newSpec", "oldSpec", &["template"]),
            "the workload spec (replicas, selector, strategy, ...) cannot be changed by a restart",
        ),
        (
            same_except("newTemplateMetadata", "oldTemplateMetadata", &["annotations"]),
            "the pod template's labels and other metadata cannot be changed by a restart",
        ),
        (
            same_except(
                "newTemplateAnnotations",
                "oldTemplateAnnotations",
                &[RESTART_ANNOTATION],
            ),
            "a restart may change only the restartedAt annotation of the pod template",
        ),
        (
            format!(
                "{metadata} && {}",
                same_except("newAnnotations", "oldAnnotations", &[SERVER_ANNOTATION])
            ),
            "the workload's labels, annotations, owners and finalizers cannot be changed by a restart",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_main_resource_of_three_workload_kinds_is_a_restart_patch() {
        assert!(is_restart_patch("apps", "deployments", "patch"));
        assert!(is_restart_patch("apps", "daemonsets", "patch"));
        assert!(!is_restart_patch("apps", "deployments", "update"));
        assert!(!is_restart_patch("apps", "deployments/scale", "patch"));
        assert!(!is_restart_patch("apps", "deployments/status", "patch"));
        assert!(!is_restart_patch("apps", "replicasets", "patch"));
        assert!(!is_restart_patch("", "deployments", "patch"));
    }
}
