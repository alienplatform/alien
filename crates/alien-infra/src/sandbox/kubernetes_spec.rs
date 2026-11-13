//! Pod specs for Kubernetes sandbox sessions.
//!
//! Kept separate from the controller because this is where the isolation guarantees live, and
//! a manifest is checkable without a cluster. Every rule below traces to something measured on
//! GKE rather than to a reading of the docs.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Capabilities, Container, EmptyDirVolumeSource, EnvVar, EnvVarSource, ObjectFieldSelector, Pod,
    PodSecurityContext, PodSpec, ResourceRequirements, SeccompProfile, SecurityContext, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use k8s_openapi::api::core::v1::Toleration;

use alien_core::sandbox_image::Authorization;
use alien_core::{Sandbox, SandboxCode};

/// Label carrying the sandbox a pod belongs to; the enumeration scope for reaping.
pub const LABEL_SANDBOX: &str = "alien.dev/sandbox";
/// Label carrying the session id within that sandbox.
pub const LABEL_SESSION: &str = "alien.dev/sandbox-session";

/// The session root of the sandbox image contract (`$ALIEN_SANDBOX_ROOT`): where a session
/// writes its work. Mounted over the image's read-only root as pod-local storage.
const SESSION_ROOT_VOLUME: &str = "session-root";
const SESSION_ROOT_PATH: &str = "/sandbox";

/// The one writable directory in a session pod, whose root filesystem is read-only. An
/// unprivileged agent writes the egress gateway's trust bundle here, and commands use it as
/// scratch space. It is a pod-local `emptyDir`, counted against the pod's ephemeral storage.
const SCRATCH_VOLUME: &str = "tmp";
const SCRATCH_PATH: &str = "/tmp";

/// Where sandbox pods may run, as the chart configured it (`sandbox.nodeSelector` and
/// `sandbox.tolerations`). Applied on top of whatever the RuntimeClass's own `scheduling` sets;
/// the cluster merges the two and refuses a pod that conflicts with the class.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SandboxPlacement {
    pub node_selector: Option<BTreeMap<String, String>>,
    pub tolerations: Option<Vec<Toleration>>,
}

impl SandboxPlacement {
    /// Reads the placement the chart passed to the Operator. An unset or empty variable means no
    /// constraint; one that does not parse is an error rather than a silently unconstrained pod.
    pub fn from_values(
        node_selector: Option<&str>,
        tolerations: Option<&str>,
    ) -> Result<Self, String> {
        fn present(value: Option<&str>) -> Option<&str> {
            value.map(str::trim).filter(|value| !value.is_empty())
        }
        let node_selector = match present(node_selector) {
            None => None,
            Some(json) => Some(
                serde_json::from_str::<BTreeMap<String, String>>(json).map_err(|error| {
                    format!("sandbox node selector is not a JSON object of strings: {error}")
                })?,
            )
            .filter(|map| !map.is_empty()),
        };
        if let Some(arch) = node_selector.as_ref().and_then(|selector| {
            ["kubernetes.io/arch", "beta.kubernetes.io/arch"]
                .into_iter()
                .filter_map(|key| selector.get(key))
                .find(|value| value.as_str() != "amd64")
        }) {
            return Err(format!(
                "sandbox node selector asks for {arch} nodes, but the sandbox agent is built for \
                 amd64 only; remove the architecture from sandbox.nodeSelector or select amd64 nodes"
            ));
        }
        let tolerations = match present(tolerations) {
            None => None,
            Some(json) => Some(
                serde_json::from_str::<Vec<Toleration>>(json).map_err(|error| {
                    format!("sandbox tolerations are not a JSON array of tolerations: {error}")
                })?,
            )
            .filter(|list| !list.is_empty()),
        };
        Ok(Self {
            node_selector,
            tolerations,
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::from_values(
            std::env::var(alien_core::ENV_ALIEN_SANDBOX_NODE_SELECTOR)
                .ok()
                .as_deref(),
            std::env::var(alien_core::ENV_ALIEN_SANDBOX_TOLERATIONS)
                .ok()
                .as_deref(),
        )
    }

    /// Sets the placement on a pod built by this module.
    pub fn apply(&self, pod: &mut Pod) {
        let spec = pod.spec.get_or_insert_default();
        spec.node_selector = self.node_selector.clone();
        spec.tolerations = self.tolerations.clone();
    }
}

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
    sandbox_pod_with_labels(
        sandbox,
        session_id,
        namespace,
        runtime_class,
        node_selector,
        capability_public_key,
        BTreeMap::new(),
    )
}

/// Builds a sandbox pod and stamps deployment-scoped cleanup ownership.
pub(crate) fn sandbox_pod_with_labels(
    sandbox: &Sandbox,
    session_id: &str,
    namespace: &str,
    runtime_class: &str,
    node_selector: Option<BTreeMap<String, String>>,
    capability_public_key: Option<&str>,
    deployment_labels: BTreeMap<String, String>,
) -> Pod {
    let image = match &sandbox.code {
        SandboxCode::Image { image, .. } => image.clone(),
        // `validate_for_platform` refuses Source off AWS and Kubernetes preflights against this
        // platform, so this is unreachable. Empty rather than a panic: the API server rejects a
        // pod with no image at create, failing an operator loudly without taking it down.
        SandboxCode::Source { .. } => String::new(),
    };

    let mut labels = BTreeMap::from([
        (LABEL_SANDBOX.to_string(), sandbox.id.clone()),
        (LABEL_SESSION.to_string(), session_id.to_string()),
    ]);
    labels.extend(deployment_labels);

    let declared = sandbox.resolved_limits();
    let root_boot = sandbox.image_user == Some(0);
    let image_uid = sandbox.image_user.filter(|uid| *uid != 0).map(i64::from);
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
            annotations: sandbox.lifecycle.max_lifetime_seconds.map(|seconds| {
                BTreeMap::from([(
                    crate::sandbox::ANNOTATION_MAX_LIFETIME.to_string(),
                    seconds.to_string(),
                )])
            }),
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
            // No `activeDeadlineSeconds`: the kubelet counts it from the pod's start, and an idle
            // pool pod can wait for hours, so a session would inherit whatever time the pod had
            // already spent waiting. The ceiling is recorded on the pod instead and the deadline
            // starts at the claim (see `annotate_claim`); the Operator deletes a session at it, and
            // the claim also sets the kubelet's `activeDeadlineSeconds` as a backstop.
            enable_service_links: Some(false),
            // The image's own USER runs the sandbox. `runAsNonRoot` makes the kubelet refuse an
            // image that would start as root, so root stays refused without choosing a uid for
            // the image. A uid is pinned only when the build recorded one.
            //
            // A declaration whose image USER is root is the one exception: the agent starts as
            // root so it can run the image's own startup command with a bounded capability set and
            // drop every session command to a fixed non-root uid (see `container_security_context`).
            security_context: Some(PodSecurityContext {
                run_as_non_root: Some(!root_boot),
                run_as_user: if root_boot { Some(0) } else { image_uid },
                seccomp_profile: Some(SeccompProfile {
                    type_: "RuntimeDefault".to_string(),
                    localhost_profile: None,
                }),
                ..Default::default()
            }),
            // Both writable places are sized from `limits.disk`: a session cannot fill the
            // node's disk through them, and the kubelet evicts the pod when it does.
            volumes: Some(
                [SCRATCH_VOLUME, SESSION_ROOT_VOLUME]
                    .into_iter()
                    .map(|name| Volume {
                        name: name.to_string(),
                        empty_dir: Some(EmptyDirVolumeSource {
                            size_limit: Some(Quantity(declared.disk.clone())),
                            ..Default::default()
                        }),
                        ..Default::default()
                    })
                    .chain(root_boot.then(|| Volume {
                        name: PROTECTED_VOLUME.to_string(),
                        empty_dir: Some(EmptyDirVolumeSource {
                            size_limit: Some(Quantity(PROTECTED_VOLUME_LIMIT.to_string())),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }))
                    .collect(),
            ),
            init_containers: root_boot.then(|| vec![stage_protected_files(&image)]),
            containers: vec![Container {
                name: "sandbox".to_string(),
                image: Some(image.clone()),
                env: container_env(sandbox, capability_public_key, root_boot),
                volume_mounts: Some(
                    [
                        VolumeMount {
                            name: SCRATCH_VOLUME.to_string(),
                            mount_path: SCRATCH_PATH.to_string(),
                            ..Default::default()
                        },
                        VolumeMount {
                            name: SESSION_ROOT_VOLUME.to_string(),
                            mount_path: SESSION_ROOT_PATH.to_string(),
                            ..Default::default()
                        },
                    ]
                    .into_iter()
                    .chain(root_boot.then(protected_mounts).into_iter().flatten())
                    .collect(),
                ),
                security_context: Some(container_security_context(root_boot)),
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

/// The container's environment: capability authorization, the declared preview ports and, for a
/// root image, the two settings that switch the agent to bounded root boot.
fn container_env(
    sandbox: &Sandbox,
    capability_public_key: Option<&str>,
    root_boot: bool,
) -> Option<Vec<EnvVar>> {
    let mut env = capability_public_key.map(|key| {
        let mut env = capability_environment(key);
        // The ports the agent may forward to, from the declaration. The agent checks
        // a request's port against this, so what a caller names is never the source.
        if !sandbox.preview_ports.is_empty() {
            env.push(plain_env(
                "ALIEN_SANDBOX_PREVIEW_PORTS",
                sandbox
                    .preview_ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        env
    });
    if root_boot {
        // Set here rather than in the image: the layered image serves both modes, and a root-only
        // setting baked into it would stop a non-root pod from booting. The pod env wins over the
        // image's own `ALIEN_SANDBOX_ISOLATION=platform`.
        let env = env.get_or_insert_default();
        env.push(plain_env("ALIEN_SANDBOX_IMAGE_COMMAND", "true".to_string()));
        env.push(plain_env(
            alien_core::sandbox_image::ENV_PROTECTED_MOUNTS,
            "true".to_string(),
        ));
        env.push(plain_env(
            "ALIEN_SANDBOX_ISOLATION",
            alien_core::sandbox_image::Isolation::UidSplit
                .env_value()
                .to_string(),
        ));
    }
    env
}

/// The volume the init container fills with the agent binary and `/opt/alien`.
const PROTECTED_VOLUME: &str = "alien-protected";
/// Room for the agent binary and the image's `/opt/alien` metadata.
const PROTECTED_VOLUME_LIMIT: &str = "256Mi";
const PROTECTED_STAGING_PATH: &str = "/stage";
const PROTECTED_METADATA_PATH: &str = "/opt/alien";

/// A pod's default seccomp profile blocks `unshare`, so the agent cannot protect its own files
/// with a private mount namespace here. The pod does it: this init container copies the agent
/// (from the sandbox image itself, so it is the pinned build the container runs) and `/opt/alien`
/// into a volume, and the sandbox container mounts that volume read-only over both paths.
///
/// It runs as uid 0 only because the image's `/opt/alien` files are root-owned and mode 0600; it
/// holds no capability, cannot gain one, and has a read-only root filesystem.
fn stage_protected_files(image: &str) -> Container {
    Container {
        name: "stage-protected-files".to_string(),
        image: Some(image.to_string()),
        command: Some(vec![
            alien_core::sandbox_image::AGENT_PATH.to_string(),
            alien_core::sandbox_image::STAGE_PROTECTED_FILES_ARGUMENT.to_string(),
            PROTECTED_STAGING_PATH.to_string(),
        ]),
        volume_mounts: Some(vec![VolumeMount {
            name: PROTECTED_VOLUME.to_string(),
            mount_path: PROTECTED_STAGING_PATH.to_string(),
            ..Default::default()
        }]),
        security_context: Some(SecurityContext {
            allow_privilege_escalation: Some(false),
            privileged: Some(false),
            read_only_root_filesystem: Some(true),
            run_as_non_root: Some(false),
            run_as_user: Some(0),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                add: None,
            }),
            ..Default::default()
        }),
        resources: Some(ResourceRequirements {
            limits: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity("250m".to_string())),
                ("memory".to_string(), Quantity("128Mi".to_string())),
            ])),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn protected_mounts() -> Vec<VolumeMount> {
    vec![
        VolumeMount {
            name: PROTECTED_VOLUME.to_string(),
            mount_path: alien_core::sandbox_image::AGENT_PATH.to_string(),
            sub_path: Some(alien_core::sandbox_image::STAGED_AGENT_NAME.to_string()),
            read_only: Some(true),
            ..Default::default()
        },
        VolumeMount {
            name: PROTECTED_VOLUME.to_string(),
            mount_path: PROTECTED_METADATA_PATH.to_string(),
            sub_path: Some(alien_core::sandbox_image::STAGED_METADATA_NAME.to_string()),
            read_only: Some(true),
            ..Default::default()
        },
    ]
}

fn plain_env(name: &str, value: String) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value),
        value_from: None,
    }
}

/// The container's security context.
///
/// A non-root image drops every capability and keeps a read-only root. A root image's agent
/// needs exactly [`alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_SET`], which it passes to the
/// image's startup command and withdraws from every session command. Never `NET_ADMIN`,
/// `SYS_ADMIN` or `NET_RAW`. Privilege escalation stays off: the agent changes uid with
/// `setuid`, which `no_new_privs` does not restrict. The root filesystem is writable because the
/// agent records the command identity in `/etc` and the image's own startup writes to it; the
/// writable layer is bounded by `limits.disk` through the ephemeral-storage limit.
fn container_security_context(root_boot: bool) -> SecurityContext {
    let add = root_boot.then(|| {
        alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_SET
            .iter()
            .map(|(name, _)| name.to_string())
            .collect()
    });
    SecurityContext {
        allow_privilege_escalation: Some(false),
        privileged: Some(false),
        read_only_root_filesystem: Some(!root_boot),
        run_as_non_root: Some(!root_boot),
        capabilities: Some(Capabilities {
            drop: Some(vec!["ALL".to_string()]),
            add,
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
    idle_pool_pod_with_labels(
        sandbox,
        namespace,
        runtime_class,
        node_selector,
        capability_public_key,
        BTreeMap::new(),
    )
}

/// Name of the agent's readiness path: 200 once the pod's NetworkPolicy is proven in force.
const NETWORK_READY_PATH: &str = "/v1/network-ready";

/// Makes the pod Ready only after its NetworkPolicy is proven in force.
///
/// On a fresh node the policy can load after the pod starts, and until it does the pod's egress
/// is unrestricted. The agent tries to connect to `operator_address` (a pod address every sandbox
/// policy refuses) and answers the readiness probe only once that connection is dropped. The
/// broker claims only Ready pods. `None` (a chart that predates the address) leaves the pod
/// as it was: ready when its container runs.
pub(crate) fn require_enforced_policy(pod: &mut Pod, operator_address: Option<&str>) {
    let Some(address) = operator_address.filter(|address| !address.is_empty()) else {
        return;
    };
    let Some(spec) = pod.spec.as_mut() else {
        return;
    };
    let container = &mut spec.containers[0];
    container.env.get_or_insert_default().push(EnvVar {
        name: "ALIEN_SANDBOX_POLICY_CANARY".to_string(),
        value: Some(address.to_string()),
        value_from: None,
    });
    container.readiness_probe = Some(k8s_openapi::api::core::v1::Probe {
        http_get: Some(k8s_openapi::api::core::v1::HTTPGetAction {
            path: Some(NETWORK_READY_PATH.to_string()),
            port: k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                alien_core::sandbox_image::AGENT_PORT as i32,
            ),
            ..Default::default()
        }),
        period_seconds: Some(2),
        failure_threshold: Some(1),
        ..Default::default()
    });
}

pub(crate) fn idle_pool_pod_with_labels(
    sandbox: &Sandbox,
    namespace: &str,
    runtime_class: &str,
    node_selector: Option<BTreeMap<String, String>>,
    capability_public_key: Option<&str>,
    deployment_labels: BTreeMap<String, String>,
) -> Pod {
    let mut pod = sandbox_pod_with_labels(
        sandbox,
        "pool",
        namespace,
        runtime_class,
        node_selector,
        capability_public_key,
        deployment_labels.clone(),
    );
    pod.metadata.name = None;
    pod.metadata.generate_name = Some(format!("{}-", pod_name(&sandbox.id, "pool")));
    let mut labels = crate::sandbox::idle_pod_labels(&sandbox.id);
    labels.extend(deployment_labels);
    pod.metadata.labels = Some(labels);
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
            value: Some(Authorization::Capability.env_value().to_string()),
            value_from: None,
        },
        EnvVar {
            name: "ALIEN_SANDBOX_PUBLIC_KEY".to_string(),
            value: Some(public_key_base64.to_string()),
            value_from: None,
        },
        // A warm pod starts before the broker assigns its logical session label. Its immutable
        // UID is available at startup and cannot be reused by a replacement at the same IP.
        EnvVar {
            name: "ALIEN_SANDBOX_SESSION_ID".to_string(),
            value: None,
            value_from: Some(EnvVarSource {
                field_ref: Some(ObjectFieldSelector {
                    api_version: Some("v1".to_string()),
                    field_path: "metadata.uid".to_string(),
                }),
                ..Default::default()
            }),
        },
        EnvVar {
            name: "ALIEN_SANDBOX_GENERATION".to_string(),
            value: Some("1".to_string()),
            value_from: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{SandboxEgress, SandboxLifecyclePolicy, SandboxLimits};

    fn sandbox(egress: SandboxEgress) -> Sandbox {
        Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
                bundled_from: None,
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(3600),
                idle_pause_seconds: None,
            })
            .build()
    }

    /// With the Operator's address the pod is Ready only once the agent proves its policy is in
    /// force; without it (an older chart) the pod is unchanged.
    #[test]
    fn the_pod_is_ready_only_when_its_policy_is_proven_in_force() {
        let build = || {
            sandbox_pod(
                &sandbox(SandboxEgress::Internet),
                "s1",
                "sbx",
                "gvisor",
                None,
                None,
            )
        };
        let mut gated = build();
        require_enforced_policy(&mut gated, Some("10.1.2.3:8080"));
        let container = &gated.spec.as_ref().unwrap().containers[0];
        let probe = container
            .readiness_probe
            .as_ref()
            .expect("a readiness probe");
        assert_eq!(
            probe.http_get.as_ref().unwrap().path.as_deref(),
            Some("/v1/network-ready")
        );
        assert!(container.env.as_ref().unwrap().iter().any(|variable| {
            variable.name == "ALIEN_SANDBOX_POLICY_CANARY"
                && variable.value.as_deref() == Some("10.1.2.3:8080")
        }));

        for absent in [None, Some("")] {
            let mut pod = build();
            require_enforced_policy(&mut pod, absent);
            assert_eq!(pod, build());
        }
    }

    #[test]
    fn chart_placement_reaches_the_pod() {
        let placement = SandboxPlacement::from_values(
            Some(r#"{"pool":"sandboxes"}"#),
            Some(r#"[{"key":"sandbox","operator":"Exists","effect":"NoSchedule"}]"#),
        )
        .expect("valid placement");
        let mut pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        placement.apply(&mut pod);
        let spec = pod.spec.unwrap();
        assert_eq!(spec.node_selector.unwrap()["pool"], "sandboxes");
        assert_eq!(spec.tolerations.unwrap()[0].key.as_deref(), Some("sandbox"));
    }

    #[test]
    fn empty_or_unset_placement_adds_no_constraint() {
        for (selector, tolerations) in [
            (None, None),
            (Some("{}"), Some("[]")),
            (Some(" "), Some("")),
        ] {
            let placement = SandboxPlacement::from_values(selector, tolerations).unwrap();
            assert_eq!(placement, SandboxPlacement::default());
        }
    }

    /// The layered agent is amd64 only, so a selector for any other architecture could only end
    /// in a pod that cannot start; it is refused with the reason instead.
    #[test]
    fn a_non_amd64_node_selector_is_refused_with_the_reason() {
        for key in ["kubernetes.io/arch", "beta.kubernetes.io/arch"] {
            for arch in ["arm64", "arm"] {
                let error =
                    SandboxPlacement::from_values(Some(&format!(r#"{{"{key}":"{arch}"}}"#)), None)
                        .unwrap_err();
                assert!(error.contains("amd64 only"), "{error}");
            }
            assert!(
                SandboxPlacement::from_values(Some(&format!(r#"{{"{key}":"amd64"}}"#)), None)
                    .is_ok()
            );
        }
    }

    #[test]
    fn unparseable_placement_is_an_error_not_an_unconstrained_pod() {
        assert!(SandboxPlacement::from_values(Some("not json"), None).is_err());
        assert!(SandboxPlacement::from_values(None, Some("{}")).is_err());
    }

    #[test]
    fn a_pod_always_carries_a_sandboxed_runtime_class() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let spec = pod.spec.expect("a spec");

        assert_eq!(spec.runtime_class_name.as_deref(), Some("gvisor"));
    }

    #[test]
    fn operator_pool_pod_carries_deployment_cleanup_ownership() {
        let pod = idle_pool_pod_with_labels(
            &sandbox(SandboxEgress::None),
            "sbx",
            "gvisor",
            None,
            None,
            BTreeMap::from([
                ("managed-by".to_string(), "runtime".to_string()),
                (
                    "alien.dev/deployment".to_string(),
                    "test-release".to_string(),
                ),
            ]),
        );
        let labels = pod.metadata.labels.expect("labels");
        assert_eq!(
            labels.get("managed-by").map(String::as_str),
            Some("runtime")
        );
        assert_eq!(
            labels.get("alien.dev/deployment").map(String::as_str),
            Some("test-release")
        );
        assert_eq!(labels.get(LABEL_SANDBOX).map(String::as_str), Some("agent"));
    }

    /// A mounted token is the workload's credential, readable by the untrusted code beside it.
    #[test]
    fn a_pod_mounts_no_service_account_token() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
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
            &sandbox(SandboxEgress::None),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        let container = pod.spec.expect("a spec").containers.remove(0);
        let security = container.security_context.expect("a security context");

        assert_eq!(security.run_as_non_root, Some(true));
        assert_eq!(
            security.run_as_user, None,
            "the image's own USER runs the sandbox; no uid is forced on it"
        );
        assert_eq!(security.allow_privilege_escalation, Some(false));
        assert_eq!(security.privileged, Some(false));
        assert_eq!(security.read_only_root_filesystem, Some(true));
        assert_eq!(
            security.capabilities.expect("capabilities").drop,
            Some(vec!["ALL".to_string()])
        );
    }

    /// With a read-only root, `/tmp` is the only place an unprivileged agent can write the
    /// gateway trust bundle it is handed with every policy delivery. Without a writable `/tmp`
    /// that delivery fails, and with it every gateway-backed session create. It must be a
    /// pod-local volume, never a host path or anything shared with another pod.
    #[test]
    fn a_pod_has_a_pod_local_writable_tmp() {
        let mut spec = sandbox_pod(
            &sandbox(SandboxEgress::Internet),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        )
        .spec
        .expect("a spec");
        let mount = spec.containers.remove(0).volume_mounts.expect("mounts");
        let volumes = spec.volumes.expect("volumes");
        assert_eq!(mount.len(), 2);
        assert_eq!(volumes.len(), 2);
        for (path, mount) in ["/tmp", "/sandbox"].iter().zip(mount.iter()) {
            assert_eq!(&mount.mount_path, path);
            assert_ne!(mount.read_only, Some(true));
            let volume = volumes
                .iter()
                .find(|volume| volume.name == mount.name)
                .expect("a volume for the mount");
            let empty_dir = volume.empty_dir.as_ref().expect("pod-local storage only");
            assert_eq!(
                empty_dir.size_limit.as_ref().map(|limit| limit.0.as_str()),
                Some(
                    sandbox(SandboxEgress::Internet)
                        .resolved_limits()
                        .disk
                        .as_str()
                ),
                "sized from limits.disk"
            );
            assert!(volume.host_path.is_none());
        }
    }

    /// The image's non-root USER is honoured rather than replaced by a fixed uid, and the pod
    /// still refuses root and carries the runtime's default seccomp profile.
    #[test]
    fn a_pod_runs_as_the_images_user_and_refuses_root() {
        let mut declared = sandbox(SandboxEgress::None);
        declared.image_user = Some(1001);
        let pod_spec = sandbox_pod(&declared, "s1", "sbx", "gvisor", None, None)
            .spec
            .expect("a spec");
        let security = pod_spec.security_context.expect("pod security context");
        assert_eq!(security.run_as_user, Some(1001));
        assert_eq!(security.run_as_non_root, Some(true));
        assert_eq!(
            security.seccomp_profile.expect("seccomp").type_,
            "RuntimeDefault"
        );

        let unrecorded = sandbox_pod(
            &sandbox(SandboxEgress::None),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        )
        .spec
        .expect("a spec")
        .security_context
        .expect("pod security context");
        assert_eq!(unrecorded.run_as_user, None);
        assert_eq!(
            unrecorded.run_as_non_root,
            Some(true),
            "an image that would start as root is refused by the kubelet"
        );
    }

    /// A root image boots the agent as root with exactly the boot capabilities and a writable
    /// root (the agent writes the command identity and protects its own files), while the pod
    /// stays unprivileged, seccomp-confined and denied every other capability.
    #[test]
    fn a_root_image_pod_boots_with_exactly_the_boot_capabilities() {
        let mut declared = sandbox(SandboxEgress::Internet);
        declared.image_user = Some(0);
        let pod = sandbox_pod(&declared, "s1", "sbx", "gvisor", None, Some("a2V5"));
        let spec = pod.spec.expect("a spec");
        let pod_security = spec.security_context.expect("pod security context");
        assert_eq!(pod_security.run_as_user, Some(0));
        assert_ne!(pod_security.run_as_non_root, Some(true));
        assert_eq!(
            pod_security.seccomp_profile.expect("seccomp").type_,
            "RuntimeDefault"
        );
        let container = &spec.containers[0];
        let security = container.security_context.as_ref().expect("security");
        assert_eq!(security.privileged, Some(false));
        assert_eq!(security.allow_privilege_escalation, Some(false));
        assert_eq!(security.read_only_root_filesystem, Some(false));
        let capabilities = security.capabilities.as_ref().expect("capabilities");
        assert_eq!(capabilities.drop, Some(vec!["ALL".to_string()]));
        let mut added = capabilities.add.clone().expect("the boot capabilities");
        added.sort();
        let mut expected: Vec<String> = alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_SET
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        expected.sort();
        assert_eq!(added, expected);
        for forbidden in ["NET_ADMIN", "SYS_ADMIN", "NET_RAW", "ALL"] {
            assert!(!added.iter().any(|name| name == forbidden));
        }
        let env = container.env.as_ref().expect("env");
        let value = |name: &str| {
            env.iter()
                .find(|variable| variable.name == name)
                .and_then(|variable| variable.value.clone())
        };
        assert_eq!(
            value("ALIEN_SANDBOX_IMAGE_COMMAND").as_deref(),
            Some("true")
        );
        assert_eq!(
            value("ALIEN_SANDBOX_ISOLATION").as_deref(),
            Some("uid-split")
        );
        assert_eq!(
            value("ALIEN_SANDBOX_PROTECTED_MOUNTS").as_deref(),
            Some("true")
        );
        // The agent binary and /opt/alien come from a volume mounted read-only, filled by an
        // init container that holds no capability.
        let mounts = container.volume_mounts.as_ref().expect("mounts");
        for path in ["/usr/local/bin/alien-sandbox-agent", "/opt/alien"] {
            let mount = mounts
                .iter()
                .find(|mount| mount.mount_path == path)
                .unwrap_or_else(|| panic!("{path} is mounted"));
            assert_eq!(mount.read_only, Some(true), "{path}");
            assert!(mount.sub_path.is_some(), "{path}");
        }
        let init = &spec.init_containers.expect("the staging init container")[0];
        let init_security = init.security_context.as_ref().expect("init security");
        assert_eq!(init_security.read_only_root_filesystem, Some(true));
        assert_eq!(init_security.allow_privilege_escalation, Some(false));
        let init_caps = init_security.capabilities.as_ref().expect("init caps");
        assert_eq!(init_caps.drop, Some(vec!["ALL".to_string()]));
        assert_eq!(init_caps.add, None);
        assert_eq!(init.image, container.image);
    }

    /// Root mode is chosen by the declaration, never by the image: a non-root declaration's pod
    /// carries none of it, so an image that works today boots the way it always did.
    #[test]
    fn a_non_root_pod_carries_no_root_boot_settings() {
        for user in [None, Some(1001)] {
            let mut declared = sandbox(SandboxEgress::Internet);
            declared.image_user = user;
            let pod = sandbox_pod(&declared, "s1", "sbx", "gvisor", None, Some("a2V5"));
            let spec = pod.spec.expect("a spec");
            let container = &spec.containers[0];
            let security = container.security_context.as_ref().expect("security");
            assert_eq!(security.read_only_root_filesystem, Some(true));
            assert_eq!(security.run_as_non_root, Some(true));
            assert_eq!(security.capabilities.as_ref().unwrap().add, None);
            for variable in container.env.as_ref().expect("env") {
                assert_ne!(variable.name, "ALIEN_SANDBOX_IMAGE_COMMAND");
                assert_ne!(variable.name, "ALIEN_SANDBOX_ISOLATION");
                assert_ne!(variable.name, "ALIEN_SANDBOX_PROTECTED_MOUNTS");
            }
            assert!(spec.init_containers.is_none());
            assert_eq!(container.volume_mounts.as_ref().unwrap().len(), 2);
        }
    }

    #[test]
    fn a_pod_never_restarts() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
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

    /// A claimed pod keeps its name, so any name derived from the pool's current depth is
    /// reissued while the old one is still in use — the pool then fails to refill exactly when
    /// it is under load, which is the case the pool exists for. `generateName` makes the
    /// collision impossible rather than unlikely.
    #[test]
    fn a_pool_pod_is_named_by_the_apiserver_so_two_can_never_collide() {
        let pod = idle_pool_pod(&sandbox(SandboxEgress::None), "sbx", "gvisor", None, None);

        assert_eq!(pod.metadata.name, None, "a fixed name is what collides");
        assert_eq!(
            pod.metadata.generate_name.as_deref(),
            Some("alien-sbx-agent-pool-"),
            "the prefix still says which sandbox the pod belongs to"
        );
    }

    /// The ceiling is recorded on the pod rather than given to the kubelet as
    /// `activeDeadlineSeconds`, which counts from the pod's start: an idle pool pod can wait for
    /// hours, and the session that claims it would start with that time already spent.
    #[test]
    fn a_pod_records_the_declared_lifetime_and_leaves_the_kubelet_deadline_unset() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
            "s1",
            "sbx",
            "gvisor",
            None,
            None,
        );
        assert_eq!(
            pod.metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(crate::sandbox::ANNOTATION_MAX_LIFETIME))
                .map(String::as_str),
            Some("3600")
        );
        assert_eq!(pod.spec.expect("a spec").active_deadline_seconds, None);
    }

    /// A sandbox that declares no deadline must not acquire one by default — an unexpected kill
    /// mid-session is worse than an unbounded one the caller chose.
    #[test]
    fn a_pod_without_a_declared_deadline_records_none() {
        let mut config = sandbox(SandboxEgress::None);
        config.lifecycle.max_lifetime_seconds = None;
        let pod = sandbox_pod(&config, "s1", "sbx", "gvisor", None, None);
        assert!(pod.metadata.annotations.is_none());
    }

    #[test]
    fn a_pod_carries_the_declared_ceilings() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
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
        let sandbox = sandbox(SandboxEgress::None);
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
        let pooled = idle_pool_pod(&sandbox(SandboxEgress::None), "sbx", "gvisor", None, None);
        let labels = pooled.metadata.labels.expect("labels");

        assert_eq!(labels.get(LABEL_SANDBOX), Some(&"agent".to_string()));
        assert!(!labels.contains_key(LABEL_SESSION));
    }

    /// Only the public half reaches a pod. Its environment is readable by the untrusted code
    /// inside it, so a signing key there would let the sandbox mint its own capabilities.
    #[test]
    fn a_pod_carries_the_public_key_and_the_capability_mode() {
        let pod = sandbox_pod(
            &sandbox(SandboxEgress::None),
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
            &sandbox(SandboxEgress::None),
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
