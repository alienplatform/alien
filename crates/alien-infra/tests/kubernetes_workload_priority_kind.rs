//! Workload priority on a disposable Kind cluster: the chart maps levels to PriorityClasses, the
//! controllers put the mapped class on pods, and an upgrade that sets nothing restarts nothing.
//!
//! `#[ignore]` because it needs a Kind cluster, `helm` and `kubectl`:
//!
//! ```text
//! KUBECONFIG=/path/to/kind/kubeconfig \
//! ALIEN_PRIORITY_KIND_CONTEXT=kind-<name> \
//! ALIEN_PRIORITY_KIND_NAMESPACE=alien-priority-<suffix> \
//! [ALIEN_PRIORITY_KIND_OLD_CHART=<directory of a chart generated before this feature>] \
//!   cargo test -p alien-infra --features all-platforms --test kubernetes_workload_priority_kind \
//!   -- --ignored --nocapture
//! ```
//!
//! With `ALIEN_PRIORITY_KIND_OLD_CHART` the install starts from that chart and the test then
//! upgrades to the current one with the same values, proving no pod restarts. The controllers run
//! in this process, so the test hands them the priority map the installed chart gave the operator,
//! as the operator's own environment would.

#![cfg(feature = "kubernetes")]

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use alien_core::{
    ClientConfig, Container, ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, KubernetesClientConfig, Platform, ResourceLifecycle, ResourceSpec, Stack,
    StackSettings, StackState, WorkloadPriority,
};

const PREFIX: &str = "priokind";
const RELEASE: &str = "priority-kind";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

fn run(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("{program} should run: {error}"));
    assert!(
        output.status.success(),
        "{program} {arguments:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn kubectl_json(namespace: &str, arguments: &[&str]) -> serde_json::Value {
    let mut all = vec!["-n", namespace];
    all.extend(arguments);
    all.extend(["-o", "json"]);
    serde_json::from_str(&run("kubectl", &all)).expect("kubectl JSON")
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .manager_url("https://manager.example.test".to_string())
        .build()
}

fn container(id: &str, priority: Option<WorkloadPriority>) -> Container {
    let mut container = Container::new(id.to_string())
        .code(ContainerCode::Image {
            image: "busybox:1.37.0".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.05".to_string(),
            desired: "0.05".to_string(),
        })
        .memory(ResourceSpec {
            min: "32Mi".to_string(),
            desired: "32Mi".to_string(),
        })
        .permissions("app".to_string())
        .command(vec!["sleep".to_string(), "3600".to_string()])
        .build();
    container.priority = priority;
    container
}

fn stack(priorities: bool) -> Stack {
    let level = |level| priorities.then_some(level);
    let mut profile = alien_core::PermissionProfile::new();
    profile.0.insert("*".to_string(), vec![]);
    Stack::new("priority-kind".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile("app", profile))
        .add(
            container("core", level(WorkloadPriority::Critical)),
            ResourceLifecycle::Live,
        )
        .add(
            container("batch", level(WorkloadPriority::Low)),
            ResourceLifecycle::Live,
        )
        .add(container("plain", None), ResourceLifecycle::Live)
        .build()
}

fn write_chart(stack: &Stack) -> tempfile::TempDir {
    let chart = alien_helm::generate_helm_chart(
        stack,
        alien_helm::HelmOptions {
            registry: &alien_helm::HelmRegistry::built_in(),
            stack_settings: StackSettings::default(),
            chart_name: RELEASE.to_string(),
        },
    )
    .expect("chart renders");
    let directory = tempfile::tempdir().expect("chart directory");
    for (path, contents) in &chart.files {
        let destination = directory.path().join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, contents).unwrap();
    }
    directory
}

fn helm(verb: &str, chart: &std::path::Path, namespace: &str, extra: &[&str]) {
    let mut arguments = vec![
        verb,
        RELEASE,
        chart.to_str().unwrap(),
        "--namespace",
        namespace,
        "--timeout",
        "5m",
        "--set-string",
        "serviceAccountPrefix=priokind",
        "--set-string",
        "runtime.image.repository=busybox",
        "--set-string",
        "runtime.image.tag=1.37.0",
        "--set",
        "runtime.probes.liveness.enabled=false",
        "--set",
        "runtime.probes.readiness.enabled=false",
        "--set",
        "heartbeat.collection.nodes.enabled=false",
        "--set",
        "runtime.cleanup.onUninstall.enabled=false",
        "--set-string",
        "management.defaultUrl=https://manager.example.test",
    ];
    if verb == "install" {
        arguments.push("--create-namespace");
    }
    arguments.extend_from_slice(extra);
    run("helm", &arguments);
}

/// (uid, restart-relevant generation) of every pod and workload, keyed by name.
fn snapshot(namespace: &str) -> BTreeMap<String, String> {
    let mut seen = BTreeMap::new();
    for kind in ["deployment", "pod"] {
        for item in kubectl_json(namespace, &["get", kind])["items"]
            .as_array()
            .unwrap()
        {
            seen.insert(
                format!("{kind}/{}", item["metadata"]["name"].as_str().unwrap()),
                format!(
                    "uid={} generation={}",
                    item["metadata"]["uid"], item["metadata"]["generation"]
                ),
            );
        }
    }
    seen
}

fn priority_of(namespace: &str, container: &str) -> String {
    let pods = kubectl_json(namespace, &["get", "pods"]);
    let pod = pods["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pod| {
            pod["metadata"]["name"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{container}-"))
                && pod["metadata"]["deletionTimestamp"].is_null()
        })
        .unwrap_or_else(|| panic!("a pod for {container}"));
    pod["spec"]["priorityClassName"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

async fn converge(stack: &Stack, client_config: &ClientConfig, state: StackState) -> StackState {
    let deployment = deployment_config();
    let result = alien_infra::StackExecutor::builder(stack, client_config.clone())
        .deployment_config(&deployment)
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .build()
        .expect("executor")
        .run_until_synced(state)
        .await;
    assert!(result.success, "{:?}", result.error);
    result.final_state
}

async fn wait_ready(namespace: &str) {
    for _ in 0..60 {
        let deployments = kubectl_json(namespace, &["get", "deployments"]);
        let ready = deployments["items"].as_array().unwrap().iter().all(|item| {
            item["metadata"]["name"] == RELEASE
                || (item["status"]["updatedReplicas"] == item["spec"]["replicas"]
                    && item["status"]["readyReplicas"] == item["spec"]["replicas"]
                    && item["status"]["replicas"] == item["spec"]["replicas"])
        });
        if ready {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("workloads did not become ready");
}

fn show(title: &str, value: impl std::fmt::Debug) {
    println!("PRIORITY_KIND {title}: {value:?}");
}

#[tokio::test]
#[ignore = "requires a disposable Kind cluster, helm and kubectl"]
async fn mapped_priority_classes_reach_pods_and_an_unset_upgrade_restarts_nothing() {
    let _ = tracing_subscriber::fmt::try_init();
    let context = env("ALIEN_PRIORITY_KIND_CONTEXT");
    assert!(
        context.starts_with("kind-"),
        "only Kind contexts are accepted"
    );
    assert_eq!(
        run("kubectl", &["config", "current-context"]).trim(),
        context
    );
    let namespace = env("ALIEN_PRIORITY_KIND_NAMESPACE");
    assert!(namespace.starts_with("alien-priority-"));

    let unset = stack(false);
    let new_chart = write_chart(&unset);
    let old_chart = std::env::var("ALIEN_PRIORITY_KIND_OLD_CHART").ok();
    helm(
        "install",
        old_chart
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or(new_chart.path()),
        &namespace,
        &[],
    );

    let client_config = ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: Some(env("KUBECONFIG")),
        context: Some(context.clone()),
        cluster: None,
        user: None,
        namespace: Some(namespace.clone()),
        additional_headers: None,
    }));
    let mut state = StackState::with_resource_prefix(Platform::Kubernetes, PREFIX.to_string());
    state = converge(&unset, &client_config, state).await;
    wait_ready(&namespace).await;
    let before = snapshot(&namespace);
    show("before upgrade", &before);

    // Upgrade to the chart under test with the same values: nothing may restart.
    helm("upgrade", new_chart.path(), &namespace, &[]);
    state = converge(&unset, &client_config, state).await;
    wait_ready(&namespace).await;
    let after = snapshot(&namespace);
    show("after upgrade", &after);
    assert_eq!(before, after, "upgrading with nothing set restarts no pod");

    // Ask the chart to create the classes and map every level; only workloads that declare a
    // level change, and each change restarts only that workload.
    helm(
        "upgrade",
        new_chart.path(),
        &namespace,
        &["--set", "runtime.workloadPriority.create=true"],
    );
    let operator = kubectl_json(&namespace, &["get", "deployment", RELEASE]);
    let map = operator["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variable| variable["name"] == alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES)
        .and_then(|variable| variable["value"].as_str())
        .expect("the chart gives the operator its priority map")
        .to_string();
    show("operator ALIEN_KUBERNETES_PRIORITY_CLASSES", &map);
    std::env::set_var(alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES, &map);
    show(
        "PriorityClasses",
        run(
            "kubectl",
            &[
                "get",
                "priorityclass",
                "-l",
                &format!("app.kubernetes.io/instance={RELEASE}"),
                "--no-headers",
            ],
        ),
    );

    let prioritized = stack(true);
    let plain_before = snapshot(&namespace)
        .into_iter()
        .filter(|(name, _)| name.contains("-plain"))
        .collect::<BTreeMap<_, _>>();
    state = converge(&prioritized, &client_config, state).await;
    wait_ready(&namespace).await;
    let _ = state;
    for name in ["core", "batch", "plain"] {
        show(
            &format!("priorityClassName of {name}"),
            priority_of(&namespace, name),
        );
    }
    assert_eq!(
        priority_of(&namespace, "core"),
        format!("{RELEASE}-critical")
    );
    assert_eq!(priority_of(&namespace, "batch"), format!("{RELEASE}-low"));
    assert_eq!(priority_of(&namespace, "plain"), "");
    let plain_after = snapshot(&namespace)
        .into_iter()
        .filter(|(name, _)| name.contains("-plain"))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        plain_before, plain_after,
        "a workload with no level is not restarted"
    );
    println!("PRIORITY_KIND_OK");
}
