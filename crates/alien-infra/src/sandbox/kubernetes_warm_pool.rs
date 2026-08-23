//! Warm pool for Kubernetes sandbox sessions.
//!
//! Exists because of a measurement, not a preference: **2.7s warm against 79s cold** on GKE
//! Autopilot. 79s is per agent turn, and the cold path fires whenever the gVisor node pool has
//! scaled to zero — which for a per-turn workload is often. A pool of pre-created idle pods is
//! the difference between usable and not.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Pod;

use crate::sandbox::kubernetes_spec::{LABEL_SANDBOX, LABEL_SESSION};

/// Marks a pod as pooled and not yet claimed.
pub const LABEL_POOL_STATE: &str = "alien.dev/sandbox-pool";
/// Value of [`LABEL_POOL_STATE`] while a pod is available.
pub const POOL_STATE_IDLE: &str = "idle";
/// Value of [`LABEL_POOL_STATE`] once a session owns the pod.
pub const POOL_STATE_CLAIMED: &str = "claimed";

/// Selector matching pods this sandbox can still hand out.
pub fn idle_selector(sandbox_id: &str) -> String {
    format!("{LABEL_SANDBOX}={sandbox_id},{LABEL_POOL_STATE}={POOL_STATE_IDLE}")
}

/// Selector matching every pod belonging to a sandbox, claimed or not.
pub fn all_pods_selector(sandbox_id: &str) -> String {
    format!("{LABEL_SANDBOX}={sandbox_id}")
}

/// How many idle pods to create to reach the target.
///
/// Saturating, because a pool that shrank below target after a burst must not ask for a
/// negative number of pods.
pub fn pool_deficit(target: usize, idle_now: usize) -> usize {
    target.saturating_sub(idle_now)
}

/// Claims an idle pod for a session by rewriting its labels.
///
/// **The claim is won by Kubernetes, not by this function.** The caller writes the mutated pod
/// back with the `resourceVersion` it read, and the API server rejects a stale one with 409
/// Conflict — so of two callers racing for the same pod, exactly one update lands and the loser
/// retries against a different pod. Any scheme that checked "is it idle?" and then wrote would
/// hand the same pod to both.
pub fn claim_idle_pod(pod: &mut Pod, session_id: &str) -> bool {
    let Some(labels) = pod.metadata.labels.as_mut() else {
        return false;
    };

    if labels.get(LABEL_POOL_STATE).map(String::as_str) != Some(POOL_STATE_IDLE) {
        return false;
    }

    labels.insert(LABEL_POOL_STATE.to_string(), POOL_STATE_CLAIMED.to_string());
    labels.insert(LABEL_SESSION.to_string(), session_id.to_string());
    true
}

/// Labels an idle pod: it belongs to the sandbox but to no session yet.
pub fn idle_pod_labels(sandbox_id: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (LABEL_SANDBOX.to_string(), sandbox_id.to_string()),
        (LABEL_POOL_STATE.to_string(), POOL_STATE_IDLE.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn pod_with(labels: BTreeMap<String, String>) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some("alien-sbx-agent-pooled".to_string()),
                labels: Some(labels),
                resource_version: Some("42".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn an_idle_pod_belongs_to_the_sandbox_but_to_no_session() {
        let labels = idle_pod_labels("agent");

        assert_eq!(labels.get(LABEL_SANDBOX), Some(&"agent".to_string()));
        assert_eq!(
            labels.get(LABEL_POOL_STATE),
            Some(&POOL_STATE_IDLE.to_string())
        );
        assert!(
            !labels.contains_key(LABEL_SESSION),
            "an unclaimed pod must not carry a session label, or reaping would target it"
        );
    }

    #[test]
    fn claiming_marks_the_pod_and_names_its_session() {
        let mut pod = pod_with(idle_pod_labels("agent"));

        assert!(claim_idle_pod(&mut pod, "s1"));

        let labels = pod.metadata.labels.expect("labels");
        assert_eq!(
            labels.get(LABEL_POOL_STATE),
            Some(&POOL_STATE_CLAIMED.to_string())
        );
        assert_eq!(labels.get(LABEL_SESSION), Some(&"s1".to_string()));
    }

    /// Second-line defence only. The real guarantee is the API server rejecting a stale
    /// resourceVersion, but a claimed pod must not be re-claimable even in memory.
    #[test]
    fn an_already_claimed_pod_cannot_be_claimed_again() {
        let mut pod = pod_with(idle_pod_labels("agent"));
        assert!(claim_idle_pod(&mut pod, "s1"));

        assert!(
            !claim_idle_pod(&mut pod, "s2"),
            "a claimed pod must not be handed to a second session"
        );
        assert_eq!(
            pod.metadata.labels.expect("labels").get(LABEL_SESSION),
            Some(&"s1".to_string()),
            "a refused claim must not overwrite the owner"
        );
    }

    #[test]
    fn a_pod_with_no_labels_is_not_claimable() {
        let mut pod = Pod::default();
        assert!(!claim_idle_pod(&mut pod, "s1"));
    }

    /// The write-back carries this, and a stale one is how the API server settles a race.
    #[test]
    fn claiming_preserves_the_resource_version_the_caller_read() {
        let mut pod = pod_with(idle_pod_labels("agent"));
        claim_idle_pod(&mut pod, "s1");

        assert_eq!(
            pod.metadata.resource_version.as_deref(),
            Some("42"),
            "dropping it would turn a conflicting update into a blind overwrite"
        );
    }

    #[test]
    fn the_idle_selector_excludes_claimed_pods() {
        let selector = idle_selector("agent");

        assert!(selector.contains(&format!("{LABEL_POOL_STATE}={POOL_STATE_IDLE}")));
        assert!(
            !all_pods_selector("agent").contains(LABEL_POOL_STATE),
            "reaping must match claimed and idle pods alike"
        );
    }

    #[test]
    fn the_deficit_never_goes_negative() {
        assert_eq!(pool_deficit(3, 0), 3);
        assert_eq!(pool_deficit(3, 2), 1);
        assert_eq!(pool_deficit(3, 3), 0);
        assert_eq!(
            pool_deficit(3, 5),
            0,
            "a pool above target asks for nothing, not a negative count"
        );
    }
}
