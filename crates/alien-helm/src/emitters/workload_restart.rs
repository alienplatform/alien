//! The cluster-side limit on the Operator's `patch` of workloads.
//!
//! Kubernetes RBAC cannot limit a `patch` to one field, so an operation that
//! restarts a Deployment, StatefulSet or DaemonSet (`kubectl rollout restart`
//! sets the `kubectl.kubernetes.io/restartedAt` annotation on the pod template)
//! would otherwise let the Operator change the image, environment, service
//! account, volumes or privilege settings of any workload it can patch. The
//! chart therefore ships a ValidatingAdmissionPolicy and binding together with
//! the `patch` rule: for requests from the Operator's ServiceAccount, an UPDATE
//! is admitted only when the object equals the stored one except for that
//! annotation. The rule and the policy come from one decision in the
//! generator and are rendered together or not at all.
//!
//! The policy, its binding and the Role or ClusterRole carrying the `patch`
//! rule are ordinary release objects. The Role grants nothing alone; the
//! binding that gives it to the Operator is a Helm hook run after the release
//! objects are applied (`post-install`, `post-upgrade`, `post-rollback`), and
//! removed by a `pre-delete` hook before the policy is deleted. A failed
//! release therefore creates no binding newer than the policy, and a rollback
//! to a chart from before this feature removes the Role (the binding left
//! behind grants nothing).
//!
//! A new policy is not enforced by the API server for a moment (measured on
//! Kubernetes 1.31: a quarter to half a second), and a rule bound in that
//! moment would admit any patch. Before the binding hook, therefore, hook
//! objects run `alien-operator verify-workload-patch-policy` as a throwaway
//! ServiceAccount that the policy treats like the Operator's and that may patch
//! one named Deployment with zero replicas. The command patches a field a
//! restart never changes until the policy refuses it, then checks that a
//! restart is admitted. Helm waits for that Job, so a cluster that does not
//! enforce the policy fails the release before the Operator can patch anything.
//! A cluster can have several API servers behind one address, and one that
//! already refuses says nothing about another that has not loaded the policy.
//! The command counts the serving API servers from the addresses of the
//! `kubernetes` Service's EndpointSlice in `default` (the probe account may get
//! that one object and nothing else), sends every request on a new connection,
//! and needs three times that many refusals in a row over at least 5 seconds,
//! then admits restarts the same number of times. When the count cannot be read
//! (forbidden, or not there) it does not assume one server: it needs at least 10
//! refusals over at least 10 seconds, and logs which path it used.
//! The probe objects are removed when the release succeeds; after a failed
//! release the Job (and its log) and the probe's own objects stay until the next
//! successful release, and none of them can patch anything but the probe
//! Deployment.
//!
//! Limit: new connections reach an API server by chance (the cluster's service
//! address picks one), so the repeated refusals make it very likely, not
//! certain, that every API server enforces the policy.
//!
//! Limits: a cluster that serves the policy API with the admission plugin
//! disabled passes no probe (the release fails); a policy deleted by hand
//! leaves the rule unconfined.
//!
//! A product chart's Operator also manages dynamic workloads in the release
//! namespace (its Role already allows create/update/delete of Deployments
//! there). Updates of workloads that already carry the Operator's dynamic
//! workload label are exempt from the policy in that namespace only.
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

/// Label the Operator puts on the workloads it creates for dynamic containers: a workload that
/// carries it is one the Operator creates and updates itself (see
/// [`dynamic_workload_exemption`]).
pub(crate) const DYNAMIC_WORKLOAD_LABEL: &str = "alien.dev/dynamic-deployment";

/// Template condition: the cluster serves the admission policy API.
pub(crate) const POLICY_API_CONDITION: &str =
    r#".Capabilities.APIVersions.Has "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy""#;

/// RBAC resources of the workload kinds a restart can patch.
pub(crate) const RESTART_RESOURCES: [&str; 3] = ["deployments", "statefulsets", "daemonsets"];

/// Metadata of the workload itself that a request could otherwise set.
const OBJECT_METADATA_FIELDS: [&str; 4] =
    ["finalizers", "generateName", "labels", "ownerReferences"];

/// Match condition of a product chart whose Operator also manages dynamic
/// workloads: in the release namespace, an update of a workload that already
/// carries the dynamic-workload label is not the policy's business. In that
/// namespace the Operator's dynamic-container Role already lets it create,
/// update and delete Deployments, so this takes nothing from the policy. The
/// label is read from the STORED object (`oldObject`): the Operator cannot
/// exempt an existing workload by labelling it, because that update is itself
/// subject to the policy, which freezes labels.
pub(crate) fn dynamic_workload_exemption(namespace_expression: &str) -> String {
    format!(
        "!(request.namespace == {namespace_expression} && oldObject.metadata.?labels.orValue({{}}).exists(k, k == '{DYNAMIC_WORKLOAD_LABEL}'))"
    )
}

/// Whether this rule verb is the `patch` the policy exists for. The grant is
/// never rendered without it.
pub(crate) fn is_restart_patch(api_group: &str, resource: &str, verb: &str) -> bool {
    api_group == "apps" && verb == "patch" && RESTART_RESOURCES.contains(&resource)
}

/// Named CEL variables: the spec and the pod template's metadata read as maps,
/// and the annotation maps (empty when absent), before and after.
pub(crate) const VARIABLES: [(&str, &str); 11] = [
    // Only a DaemonSet's own metadata carries the server's template counter.
    (
        "serverAnnotation",
        "object.kind == 'DaemonSet' ? 'deprecated.daemonset.template.generation' : ''",
    ),
    ("newTemplate", "dyn(object.spec.template)"),
    ("oldTemplate", "dyn(oldObject.spec.template)"),
    ("newSpec", "dyn(object.spec)"),
    ("oldSpec", "dyn(oldObject.spec)"),
    ("newTemplateMetadata", "dyn(object.spec.template.metadata)"),
    (
        "oldTemplateMetadata",
        "dyn(oldObject.spec.template.metadata)",
    ),
    (
        "newTemplateAnnotations",
        "object.spec.template.metadata.?annotations.orValue({})",
    ),
    (
        "oldTemplateAnnotations",
        "oldObject.spec.template.metadata.?annotations.orValue({})",
    ),
    ("newAnnotations", "object.metadata.?annotations.orValue({})"),
    (
        "oldAnnotations",
        "oldObject.metadata.?annotations.orValue({})",
    ),
];

/// `new` and `old` (CEL variables holding maps) have the same keys and the
/// same value for each key, leaving out the `ignored` keys. An ignored entry
/// starting with `variables.` is a CEL expression for the key, otherwise a
/// literal key.
fn same_except(new: &str, old: &str, ignored: &[&str]) -> String {
    let is_ignored = if ignored.is_empty() {
        "false".to_owned()
    } else {
        ignored
            .iter()
            .map(|key| {
                if key.starts_with("variables.") {
                    format!("k == {key}")
                } else {
                    format!("k == '{key}'")
                }
            })
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
            "the pod spec (image, environment, service account, volumes, privilege settings) cannot be changed by a restart",
        ),
        (
            same_except("newSpec", "oldSpec", &["template"]),
            "the workload spec (replicas, selector, strategy, ...) cannot be changed by a restart",
        ),
        (
            same_except("newTemplate", "oldTemplate", &["metadata", "spec"]),
            "the pod template cannot be changed by a restart",
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
                same_except("newAnnotations", "oldAnnotations", &["variables.serverAnnotation"])
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
