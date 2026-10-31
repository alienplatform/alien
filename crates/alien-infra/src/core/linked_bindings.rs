//! Linked binding drift.
//!
//! A linked resource can change what its binding publishes without the workloads that link it
//! being planned for update: a sandbox promoting a rebuilt image, a re-registration, an address
//! read again on a Ready tick. The executor re-renders a workload only when a dependency is
//! planned for create or update, so a workload compares the binding variables it would render now
//! with the configuration its platform actually runs, and refreshes only when one differs.
//!
//! Nothing is stored: the comparison reads the live configuration each time, so it also finds a
//! workload that went stale before this check existed.

use std::collections::HashMap;

use alien_core::{ResourceRef, ResourceStatus};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::Result;

/// The linked bindings of a workload as it would render them now.
pub struct RenderedLinkedBindings {
    /// The binding variables (`ALIEN_<LINK>_BINDING`).
    pub variables: HashMap<String, String>,
    /// The raw binding values, which a platform may project further (Kubernetes turns secret
    /// references into `secretKeyRef`s).
    pub bindings: Vec<(String, serde_json::Value)>,
}

/// Renders the bindings of `links`, or `None` when there is nothing to compare: no links, or a
/// linked resource that is not Running. A dependency in the middle of its own flow has not
/// reached the value its dependents should see.
pub async fn render_linked_bindings(
    ctx: &ResourceControllerContext<'_>,
    links: &[ResourceRef],
    resource_id: &str,
) -> Result<Option<RenderedLinkedBindings>> {
    if links.is_empty() || !links_settled(ctx, links) {
        return Ok(None);
    }
    let (variables, bindings) = EnvironmentVariableBuilder::try_new(&HashMap::new())?
        .add_linked_resources(links, ctx, resource_id)
        .await?
        .build_with_bindings();
    Ok(Some(RenderedLinkedBindings {
        variables,
        bindings,
    }))
}

/// Whether every linked resource is Running, so its binding is the one dependents should see.
pub fn links_settled(ctx: &ResourceControllerContext<'_>, links: &[ResourceRef]) -> bool {
    links.iter().all(|link| {
        ctx.state
            .resources
            .get(link.id())
            .is_some_and(|state| state.status == ResourceStatus::Running)
    })
}

/// The first linked resource that is not Running, for a `DependencyNotReady` error.
pub fn unsettled_link<'a>(
    ctx: &ResourceControllerContext<'_>,
    links: &'a [ResourceRef],
) -> Option<&'a ResourceRef> {
    links.iter().find(|link| {
        ctx.state
            .resources
            .get(link.id())
            .is_none_or(|state| state.status != ResourceStatus::Running)
    })
}

/// Whether two binding variable values are the same. Binding variables are JSON, compared as
/// values: key order and formatting are not a difference. A value that is not JSON is compared as
/// a string.
pub fn same_binding_value(expected: &str, live: &str) -> bool {
    expected == live
        || matches!(
            (
                serde_json::from_str::<serde_json::Value>(expected),
                serde_json::from_str::<serde_json::Value>(live),
            ),
            (Ok(expected), Ok(live)) if expected == live
        )
}

/// Names of the variables in `expected` that `live` lacks or holds with another value, sorted.
/// Only `expected`'s names are compared: every other live variable (the user's environment, the
/// workload's own binding, platform variables) is not.
pub fn drifted_bindings(
    expected: &HashMap<String, String>,
    live: &HashMap<String, String>,
) -> Vec<String> {
    let mut drifted: Vec<String> = expected
        .iter()
        .filter(|(name, value)| {
            live.get(*name)
                .is_none_or(|live| !same_binding_value(value, live))
        })
        .map(|(name, _)| name.clone())
        .collect();
    drifted.sort();
    drifted
}

/// The linked binding variables that differ between what the workload would render now and
/// `live`. Empty when nothing moved, or when a link is not settled.
pub async fn linked_binding_drift(
    ctx: &ResourceControllerContext<'_>,
    links: &[ResourceRef],
    resource_id: &str,
    live: &HashMap<String, String>,
) -> Result<Vec<String>> {
    Ok(
        match render_linked_bindings(ctx, links, resource_id).await? {
            Some(rendered) => drifted_bindings(&rendered.variables, live),
            None => Vec::new(),
        },
    )
}

#[cfg(feature = "kubernetes")]
mod kubernetes {
    use std::collections::HashMap;

    use alien_core::ResourceRef;
    use k8s_openapi::api::core::v1::EnvVar;

    use super::{render_linked_bindings, same_binding_value};
    use crate::core::{projected_env_vars, ResourceControllerContext};
    use crate::error::Result;

    /// The linked binding entries of a pod's environment that differ from what the workload
    /// would render now, by name. The rendered entries are projected exactly as the workload's
    /// manifest projects them: the binding variable carries the binding with its secret
    /// references resolved, beside a `secretKeyRef` per secret. A binding variable is compared
    /// as JSON; a `secretKeyRef` by its reference. Empty when nothing moved, or when a link is
    /// not settled.
    pub async fn kubernetes_linked_binding_drift(
        ctx: &ResourceControllerContext<'_>,
        links: &[ResourceRef],
        resource_id: &str,
        live: &[EnvVar],
    ) -> Result<Vec<String>> {
        let Some(rendered) = render_linked_bindings(ctx, links, resource_id).await? else {
            return Ok(Vec::new());
        };
        let expected = projected_env_vars(None, rendered.bindings, HashMap::new())?;
        let live: HashMap<&str, &EnvVar> =
            live.iter().map(|var| (var.name.as_str(), var)).collect();
        let mut drifted: Vec<String> = expected
            .iter()
            .filter(|expected| {
                live.get(expected.name.as_str()).is_none_or(|live| {
                    let same_value = match (expected.value.as_deref(), live.value.as_deref()) {
                        (Some(expected), Some(live)) => same_binding_value(expected, live),
                        (expected, live) => expected == live,
                    };
                    !same_value || expected.value_from != live.value_from
                })
            })
            .map(|expected| expected.name.clone())
            .collect();
        drifted.sort();
        drifted.dedup();
        Ok(drifted)
    }

    /// The environment of the container named `container` in a live pod template, or `None` when
    /// the read carries no such container to compare against.
    pub fn pod_template_env<'a>(
        pod_spec: Option<&'a k8s_openapi::api::core::v1::PodSpec>,
        container: &str,
    ) -> Option<&'a [EnvVar]> {
        pod_spec?
            .containers
            .iter()
            .find(|candidate| candidate.name == container)
            .map(|found| found.env.as_deref().unwrap_or_default())
    }
}

#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn reformatted_json_is_not_a_difference() {
        let expected = map(&[("ALIEN_SB_BINDING", r#"{"a":1,"b":{"c":"x"}}"#)]);
        let live = map(&[
            (
                "ALIEN_SB_BINDING",
                "{\n  \"b\": {\"c\": \"x\"},\n  \"a\": 1\n}",
            ),
            ("UNRELATED", "anything"),
        ]);
        assert!(drifted_bindings(&expected, &live).is_empty());
    }

    #[test]
    fn a_moved_or_missing_binding_is_named() {
        let expected = map(&[
            ("ALIEN_A_BINDING", r#"{"v":"2.0"}"#),
            ("ALIEN_B_BINDING", r#"{"v":"1"}"#),
            ("ALIEN_C_BINDING", "plain"),
        ]);
        let live = map(&[
            ("ALIEN_A_BINDING", r#"{"v":"1.0"}"#),
            ("ALIEN_C_BINDING", "plain"),
        ]);
        assert_eq!(
            drifted_bindings(&expected, &live),
            vec!["ALIEN_A_BINDING".to_string(), "ALIEN_B_BINDING".to_string()]
        );
    }
}

/// The Kubernetes workloads' Ready comparison against a real apiserver, which is what can say
/// whether a pod template read back from the cluster still matches what the controller renders.
///
/// `#[ignore]` because it needs a cluster whose nodes hold `registry.k8s.io/pause:3.10` (every Kind
/// node does). Run with:
///
/// ```text
/// ALIEN_LINKED_BINDING_KUBECONFIG=/path/to/kubeconfig \
///   cargo test -p alien-infra --features all-platforms --lib \
///   core::linked_bindings::cluster -- --ignored --nocapture --test-threads=1
/// ```
///
/// Each test creates its namespace and deletes it at the end.
#[cfg(all(test, feature = "kubernetes"))]
mod cluster {
    use alien_core::{
        ClientConfig, Container, ContainerCode, Daemon, DaemonCode, KubernetesClientConfig,
        Platform, ResourceDefinition, ResourceSpec, ResourceStatus, Sandbox, SandboxCode,
        SandboxEgress, SandboxLifecyclePolicy, Worker, WorkerCode,
    };

    use crate::container::KubernetesContainerController;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::ResourceController;
    use crate::daemon::KubernetesDaemonController;
    use crate::sandbox::{KubernetesSandboxController, KubernetesSandboxState};
    use crate::worker::KubernetesWorkerController;

    const IMAGE: &str = "registry.k8s.io/pause:3.10";
    const OLD_BROKER: &str = "http://broker-old.alien.svc:8080";
    const NEW_BROKER: &str = "http://broker-new.alien.svc:8080";

    fn kubeconfig() -> String {
        std::env::var("ALIEN_LINKED_BINDING_KUBECONFIG")
            .expect("set ALIEN_LINKED_BINDING_KUBECONFIG")
    }

    /// `kubectl` against the same cluster, as ground truth: asserting through the client under
    /// test would only prove it agrees with itself.
    fn kubectl(arguments: &[&str]) -> String {
        let output = std::process::Command::new("kubectl")
            .args(["--kubeconfig", &kubeconfig()])
            .args(arguments)
            .output()
            .expect("kubectl runs");
        assert!(
            output.status.success(),
            "kubectl {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn sandbox() -> Sandbox {
        Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: IMAGE.to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(600),
                idle_pause_seconds: None,
            })
            .build()
    }

    /// The linked sandbox, Running and publishing `broker`, as its controller holds it after
    /// its Ready read the operator's broker address.
    fn sandbox_serving(namespace: &str, broker: &str) -> KubernetesSandboxController {
        KubernetesSandboxController {
            state: KubernetesSandboxState::Ready,
            sandbox_id: Some("agent".to_string()),
            namespace: Some(namespace.to_string()),
            runtime_class: Some("gvisor".to_string()),
            broker_url: Some(broker.to_string()),
            deployment_label_key: Some("alien.dev/deployment".to_string()),
            deployment_label_value: Some("test".to_string()),
            egress: Some(SandboxEgress::None.into()),
            ..Default::default()
        }
    }

    async fn executor<R: ResourceDefinition>(
        namespace: &str,
        resource: R,
        controller: impl ResourceController + 'static,
        broker: &str,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .real_delays()
            .resource(resource)
            .controller(controller)
            .platform(Platform::Kubernetes)
            .client_config(ClientConfig::Kubernetes(Box::new(
                KubernetesClientConfig::Kubeconfig {
                    kubeconfig_path: Some(kubeconfig()),
                    context: None,
                    cluster: None,
                    user: None,
                    namespace: Some(namespace.to_string()),
                    additional_headers: None,
                },
            )))
            .with_dependency(sandbox(), sandbox_serving(namespace, broker))
            .build()
            .await
            .expect("the executor builds with a real kubeconfig")
    }

    /// The workload's pod template generation and its sandbox binding, read with kubectl.
    fn observed(namespace: &str, kind: &str, name: &str) -> (i64, String) {
        let object: serde_json::Value = serde_json::from_str(&kubectl(&[
            "-n", namespace, "get", kind, name, "-o", "json",
        ]))
        .expect("kubectl JSON");
        let generation = object["metadata"]["generation"]
            .as_i64()
            .expect("a generation");
        let binding = object["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .expect("an environment")
            .iter()
            .find(|var| var["name"] == "ALIEN_AGENT_BINDING")
            .and_then(|var| var["value"].as_str())
            .expect("the sandbox binding")
            .to_string();
        let broker = serde_json::from_str::<serde_json::Value>(&binding).expect("binding JSON")
            ["brokerUrl"]
            .as_str()
            .expect("a broker URL")
            .to_string();
        (generation, broker)
    }

    /// Creates the workload against `OLD_BROKER`, then checks, against the live object:
    /// - a Ready tick with nothing moved changes nothing (no new generation, so no roll);
    /// - once the sandbox publishes `NEW_BROKER` with no update planned, one Ready tick reports
    ///   Updating and the refresh rolls the template exactly once, with the new binding;
    /// - the next Ready tick changes nothing.
    async fn a_moved_binding_rolls_once<R, C>(
        namespace: &str,
        kind: &str,
        resource: R,
        controller: C,
        workload_name: impl Fn(&C) -> String,
    ) where
        R: ResourceDefinition + Clone,
        C: ResourceController + Clone + 'static,
    {
        kubectl(&["create", "namespace", namespace]);
        kubectl(&["-n", namespace, "create", "serviceaccount", "test-app-sa"]);

        let mut created = executor(namespace, resource.clone(), controller, OLD_BROKER).await;
        created
            .run_until_terminal()
            .await
            .expect("the create flow completes on a real cluster");
        assert_eq!(created.status(), ResourceStatus::Running);
        let ready: C = created.internal_state::<C>().expect("controller").clone();
        let name = workload_name(&ready);
        let (created_generation, broker) = observed(namespace, kind, &name);
        assert_eq!(broker, OLD_BROKER);

        created.step().await.expect("a Ready tick");
        assert_eq!(created.status(), ResourceStatus::Running);
        assert_eq!(
            observed(namespace, kind, &name),
            (created_generation, OLD_BROKER.to_string()),
            "a Ready tick with nothing moved must leave the workload alone"
        );

        let ready: C = created.internal_state::<C>().expect("controller").clone();
        let mut moved = executor(namespace, resource.clone(), ready, NEW_BROKER).await;
        moved
            .step()
            .await
            .expect("the Ready tick that sees the drift");
        assert_eq!(
            moved.status(),
            ResourceStatus::Updating,
            "the refresh reports Updating, so the planner waits for it"
        );
        moved
            .run_until_terminal()
            .await
            .expect("the refresh completes on a real cluster");
        assert_eq!(moved.status(), ResourceStatus::Running);
        assert_eq!(
            observed(namespace, kind, &name),
            (created_generation + 1, NEW_BROKER.to_string()),
            "the refresh applies the moved binding in one rollout"
        );

        moved.step().await.expect("a Ready tick after the refresh");
        assert_eq!(moved.status(), ResourceStatus::Running);
        assert_eq!(
            observed(namespace, kind, &name),
            (created_generation + 1, NEW_BROKER.to_string()),
            "the refreshed workload matches, so the next tick leaves it alone"
        );

        kubectl(&["delete", "namespace", namespace, "--wait=false"]);
    }

    #[tokio::test]
    #[ignore = "requires a Kubernetes cluster"]
    async fn a_container_rolls_once_when_its_linked_binding_moves() {
        let container = Container::new("app".to_string())
            .code(ContainerCode::Image {
                image: IMAGE.to_string(),
            })
            .cpu(ResourceSpec {
                min: "50m".to_string(),
                desired: "50m".to_string(),
            })
            .memory(ResourceSpec {
                min: "32Mi".to_string(),
                desired: "32Mi".to_string(),
            })
            .permissions("app".to_string())
            .link(&sandbox())
            .build();
        a_moved_binding_rolls_once(
            "alien1709-container",
            "deployment",
            container,
            KubernetesContainerController::default(),
            |controller| controller.workload_name.clone().expect("workload name"),
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a Kubernetes cluster"]
    async fn a_worker_rolls_once_when_its_linked_binding_moves() {
        let worker = Worker::new("app".to_string())
            .code(WorkerCode::Image {
                image: IMAGE.to_string(),
            })
            .permissions("app".to_string())
            .link(&sandbox())
            .build();
        a_moved_binding_rolls_once(
            "alien1709-worker",
            "deployment",
            worker,
            KubernetesWorkerController::default(),
            |controller| controller.deployment_name.clone().expect("deployment name"),
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a Kubernetes cluster"]
    async fn a_daemon_rolls_once_when_its_linked_binding_moves() {
        let daemon = Daemon::new("app".to_string())
            .code(DaemonCode::Image {
                image: IMAGE.to_string(),
            })
            .link(&sandbox())
            .build();
        a_moved_binding_rolls_once(
            "alien1709-daemon",
            "daemonset",
            daemon,
            KubernetesDaemonController::default(),
            |controller| controller.daemon_set_name.clone().expect("daemonset name"),
        )
        .await;
    }
}
