//! Pod and NetworkPolicy specs for Kubernetes sandbox sessions.
//!
//! Kept separate from the controller because this is where the isolation guarantees live, and
//! a manifest is checkable without a cluster. Every rule below traces to something measured on
//! GKE rather than to a reading of the docs.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Capabilities, Container, EnvVar, Pod, PodSecurityContext, PodSpec, ResourceRequirements,
    SecurityContext,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use alien_core::{Sandbox, SandboxCode};

/// Label carrying the sandbox a pod belongs to; the enumeration scope for reaping.
pub const LABEL_SANDBOX: &str = "alien.dev/sandbox";
/// Label carrying the session id within that sandbox.
pub const LABEL_SESSION: &str = "alien.dev/sandbox-session";

/// The uid sandboxed workloads run as.
const SANDBOX_UID: i64 = 65534;

/// Names the pod backing one session.
pub fn pod_name(sandbox: &str, session_id: &str) -> String {
    format!("alien-sbx-{sandbox}-{session_id}")
}

/// Builds the pod for one sandbox session.
///
/// `runtime_class` is required rather than optional: a plain pod shares the node kernel with
/// everything else on it, and this resource exists to run hostile code.
pub fn sandbox_pod(
    sandbox: &Sandbox,
    session_id: &str,
    namespace: &str,
    runtime_class: &str,
    node_selector: Option<BTreeMap<String, String>>,
    capability_public_key: Option<&str>,
) -> Pod {
    let image = match &sandbox.code {
        SandboxCode::Image { image } => image.clone(),
        // Unreachable through any supported path: `Sandbox::validate_for_platform` refuses
        // Source on every platform, because no backend builds a sandbox image. Left as an empty
        // string rather than a panic — the API server rejects a pod with no image at create,
        // which fails an operator loudly without taking it down.
        SandboxCode::Source { .. } => String::new(),
    };

    let labels = BTreeMap::from([
        (LABEL_SANDBOX.to_string(), sandbox.id.clone()),
        (LABEL_SESSION.to_string(), session_id.to_string()),
    ]);

    let declared = sandbox.resolved_limits();
    let limits = BTreeMap::from([
        ("cpu".to_string(), Quantity(declared.cpu.clone())),
        ("memory".to_string(), Quantity(declared.memory.clone())),
        (
            "ephemeral-storage".to_string(),
            Quantity(declared.disk.clone()),
        ),
    ]);

    Pod {
        metadata: ObjectMeta {
            name: Some(pod_name(&sandbox.id, session_id)),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            runtime_class_name: Some(runtime_class.to_string()),
            node_selector,
            // No identity by default. A mounted token is a credential the untrusted code can
            // read, and it is the workload's, not the sandbox's.
            automount_service_account_token: Some(false),
            // A sandbox that exits stays exited; restarting hostile code hands it another go.
            restart_policy: Some("Never".to_string()),
            // The kubelet kills the pod at the deadline, so the ceiling holds even if whatever
            // created the session never comes back to terminate it.
            active_deadline_seconds: sandbox.session.max_lifetime_seconds.map(i64::from),
            enable_service_links: Some(false),
            security_context: Some(PodSecurityContext {
                run_as_non_root: Some(true),
                run_as_user: Some(SANDBOX_UID),
                run_as_group: Some(SANDBOX_UID),
                fs_group: Some(SANDBOX_UID),
                ..Default::default()
            }),
            containers: vec![Container {
                name: "sandbox".to_string(),
                image: Some(image),
                env: capability_public_key.map(capability_environment),
                security_context: Some(SecurityContext {
                    allow_privilege_escalation: Some(false),
                    privileged: Some(false),
                    read_only_root_filesystem: Some(true),
                    run_as_non_root: Some(true),
                    run_as_user: Some(SANDBOX_UID),
                    capabilities: Some(Capabilities {
                        drop: Some(vec!["ALL".to_string()]),
                        add: None,
                    }),
                    ..Default::default()
                }),
                resources: Some(ResourceRequirements {
                    limits: Some(limits),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds an unclaimed pool pod: the same hardened spec, labelled idle and owned by no session.
///
/// The spec must be identical to a session pod's, or the pool would hand out something with
/// different isolation from what a directly-created session gets.
///
/// Named by the apiserver through `generateName` rather than by us. A claimed pod keeps the name
/// it was created with, so any scheme derived from the pool's current depth reissues a name that
/// is still taken as soon as pods are in use — the pool then fails to refill exactly when it is
/// under load, which is the case it exists for.
pub fn idle_pool_pod(
    sandbox: &Sandbox,
    namespace: &str,
    runtime_class: &str,
    node_selector: Option<BTreeMap<String, String>>,
    capability_public_key: Option<&str>,
) -> Pod {
    let mut pod = sandbox_pod(
        sandbox,
        "pool",
        namespace,
        runtime_class,
        node_selector,
        capability_public_key,
    );
    pod.metadata.name = None;
    pod.metadata.generate_name = Some(format!("{}-", pod_name(&sandbox.id, "pool")));
    pod.metadata.labels = Some(crate::sandbox::idle_pod_labels(&sandbox.id));
    pod
}

/// Environment the agent needs to authorize callers by capability.
///
/// Kubernetes cannot authorize by transport the way AWS does: a pod IP is reachable by anything
/// on the cluster network, so the agent has to check a token rather than trust the connection.
/// Only the **public** key goes here, because a pod's environment is readable by the untrusted
/// code inside it.
pub fn capability_environment(public_key_base64: &str) -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: "ALIEN_SANDBOX_AUTHORIZATION".to_string(),
            value: Some("capability".to_string()),
            value_from: None,
        },
        EnvVar {
            name: "ALIEN_SANDBOX_PUBLIC_KEY".to_string(),
            value: Some(public_key_base64.to_string()),
            value_from: None,
        },
    ]
}

/// Builds the egress NetworkPolicy for a sandbox.
///
/// Under `deny` there are no egress rules at all, which denies everything. Under `allow` the
#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{SandboxEgress, SandboxLimits, SandboxSessionPolicy};

    fn sandbox(egress: SandboxEgress) -> Sandbox {
        Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: Some(3600),
                idle_suspend_seconds: None,
            })
            .build()
    }

    #[test]
    fn a_pod_always_carries_a_sandboxed_runtime_class() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let spec = pod.spec.expect("a spec");

        assert_eq!(spec.runtime_class_name.as_deref(), Some("gvisor"));
    }

    /// A mounted token is the workload's credential, readable by the untrusted code beside it.
    #[test]
    fn a_pod_mounts_no_service_account_token() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let spec = pod.spec.expect("a spec");

        assert_eq!(spec.automount_service_account_token, Some(false));
        assert_eq!(spec.enable_service_links, Some(false));
    }

    #[test]
    fn a_pod_is_unprivileged_with_a_read_only_root() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let container = pod.spec.expect("a spec").containers.remove(0);
        let security = container.security_context.expect("a security context");

        assert_eq!(security.run_as_non_root, Some(true));
        assert_eq!(security.run_as_user, Some(SANDBOX_UID));
        assert_eq!(security.allow_privilege_escalation, Some(false));
        assert_eq!(security.privileged, Some(false));
        assert_eq!(security.read_only_root_filesystem, Some(true));
        assert_eq!(
            security.capabilities.expect("capabilities").drop,
            Some(vec!["ALL".to_string()])
        );
    }

    #[test]
    fn a_pod_never_restarts() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        assert_eq!(
            pod.spec.expect("a spec").restart_policy.as_deref(),
            Some("Never"),
            "restarting hostile code hands it another attempt"
        );
    }

    /// The kubelet is what makes a session deadline a ceiling rather than a hope: it kills the
    /// pod whether or not whatever created the session ever comes back to terminate it. This is
    /// the only backend with the primitive, which is why the field is refused elsewhere.
    /// A claimed pod keeps its name, so any name derived from the pool's current depth is
    /// reissued while the old one is still in use — the pool then fails to refill exactly when
    /// it is under load, which is the case the pool exists for. `generateName` makes the
    /// collision impossible rather than unlikely.
    #[test]
    fn a_pool_pod_is_named_by_the_apiserver_so_two_can_never_collide() {
        let pod = idle_pool_pod(&sandbox(SandboxEgress::Deny), "sbx", "gvisor", None, None);

        assert_eq!(pod.metadata.name, None, "a fixed name is what collides");
        assert_eq!(
            pod.metadata.generate_name.as_deref(),
            Some("alien-sbx-agent-pool-"),
            "the prefix still says which sandbox the pod belongs to"
        );
    }

    #[test]
    fn a_pod_carries_the_declared_session_deadline() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        assert_eq!(
            pod.spec.expect("a spec").active_deadline_seconds,
            Some(3600)
        );
    }

    /// A sandbox that declares no deadline must not acquire one by default — an unexpected kill
    /// mid-session is worse than an unbounded one the caller chose.
    #[test]
    fn a_pod_without_a_declared_deadline_carries_none() {
        let mut config = sandbox(SandboxEgress::Deny);
        config.session.max_lifetime_seconds = None;
        let pod = sandbox_pod(&config, "s1", "sbx", "gvisor", None, None);
        assert_eq!(pod.spec.expect("a spec").active_deadline_seconds, None);
    }

    #[test]
    fn a_pod_carries_the_declared_ceilings() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let container = pod.spec.expect("a spec").containers.remove(0);
        let limits = container
            .resources
            .expect("resources")
            .limits
            .expect("limits");

        assert_eq!(limits.get("cpu"), Some(&Quantity("1".to_string())));
        assert_eq!(limits.get("memory"), Some(&Quantity("2Gi".to_string())));
        assert_eq!(
            limits.get("ephemeral-storage"),
            Some(&Quantity("20Gi".to_string()))
        );
    }

    /// A pooled pod is handed to a session that did not create it, so anything weaker here
    /// would silently downgrade isolation for every warm start — the common path.
    #[test]
    fn a_pooled_pod_has_identical_isolation_to_a_session_pod() {
        let sandbox = sandbox(SandboxEgress::Deny);
        let session = sandbox_pod(&sandbox, "s1", "sbx", "gvisor", None, None);
        let pooled = idle_pool_pod(&sandbox, "sbx", "gvisor", None, None);

        assert_eq!(
            session.spec, pooled.spec,
            "a pooled pod must be spec-identical to one created for a session"
        );
    }

    /// An idle pod carries no session label. If it did, reaping a sandbox's sessions would
    /// destroy the pool, and enumeration could not tell a spare from a live session.
    #[test]
    fn a_pooled_pod_carries_no_session_label_until_claimed() {
        let pooled = idle_pool_pod(&sandbox(SandboxEgress::Deny), "sbx", "gvisor", None, None);
        let labels = pooled.metadata.labels.expect("labels");

        assert_eq!(labels.get(LABEL_SANDBOX), Some(&"agent".to_string()));
        assert!(!labels.contains_key(LABEL_SESSION));
    }

    /// Only the public half reaches a pod. Its environment is readable by the untrusted code
    /// inside it, so a signing key there would let the sandbox mint its own capabilities.
    #[test]
    fn a_pod_carries_the_public_key_and_the_capability_mode() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            Some("cHVibGlj"),
        );

        let environment = pod.spec.expect("a spec").containers[0]
            .env
            .clone()
            .expect("capability mode needs an environment");

        let value = |name: &str| {
            environment
                .iter()
                .find(|variable| variable.name == name)
                .and_then(|variable| variable.value.clone())
        };

        assert_eq!(
            value("ALIEN_SANDBOX_AUTHORIZATION").as_deref(),
            Some("capability")
        );
        assert_eq!(
            value("ALIEN_SANDBOX_PUBLIC_KEY").as_deref(),
            Some("cHVibGlj")
        );
        assert!(
            !environment
                .iter()
                .any(|variable| variable.name.contains("PRIVATE")
                    || variable.name.contains("SIGNING")),
            "no signing material may reach a sandbox pod"
        );
    }

    #[test]
    fn pods_are_enumerable_by_sandbox_for_reaping() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::Deny),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let labels = pod.metadata.labels.expect("labels");

        assert_eq!(labels.get(LABEL_SANDBOX), Some(&"agent".to_string()));
        assert_eq!(labels.get(LABEL_SESSION), Some(&"s1".to_string()));
        assert_eq!(pod_name("agent", "s1"), "alien-sbx-agent-s1");
    }
}
