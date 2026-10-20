//! Kubernetes sandbox gateway hosting, installed and reconciled on a disposable Kind cluster.
//!
//! `#[ignore]` because it needs a Kind cluster whose CNI enforces NetworkPolicy (Calico), a
//! RuntimeClass named `gvisor`, and the gateway and session images loaded into the node:
//!
//! ```text
//! KUBECONFIG=/path/to/kind/kubeconfig \
//! ALIEN_GATEWAY_KIND_CONTEXT=kind-<name> \
//! ALIEN_GATEWAY_KIND_NAMESPACE=alien-gateway-<suffix> \
//! ALIEN_GATEWAY_KIND_SESSION_IMAGE=<image that keeps running> \
//! [ALIEN_GATEWAY_KIND_APP_IMAGE=<application image, default: the session image>] \
//! ALIEN_KUBERNETES_CLUSTER_DOMAIN=<the cluster DNS domain> \
//!   cargo test -p alien-infra --features all-platforms --test kubernetes_sandbox_gateway_kind \
//!   -- --ignored --nocapture
//! ```
//!
//! The test runs the same preparation `alien render` runs, installs the generated chart (whose
//! pre-install hook refuses a CNI that does not enforce NetworkPolicy), and drives the real
//! controllers until the gateway Deployment, its Services and NetworkPolicy exist and the warm
//! pool has a session pod. It leaves the namespace in place so traffic probes can run against
//! it; the caller deletes the namespace. Outside the cluster the controller cannot read the
//! cluster DNS domain from its resolver search path, so the test passes it in. The controllers
//! run in this process rather than in the operator pod, so the test hands them the broker
//! address the installed chart gave the operator, as the operator's own environment would.

#![cfg(feature = "kubernetes")]

use std::process::Command;
use std::time::Duration;

use alien_core::{
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, ClientConfig, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, KubernetesClientConfig,
    Platform, ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings, StackState,
};

const PREFIX: &str = "gwkind";

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
        // Images outside the manager's registry are pulled directly, as in a deployment.
        .manager_url("https://manager.example.test".to_string())
        .build()
}

async fn prepared_stack(session_image: &str, app_image: &str) -> Stack {
    let sandbox = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: session_image.to_string(),
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(3600),
            idle_pause_seconds: None,
        })
        .build();
    // The workload that administers session policies, as an application would.
    let manager = Container::new("app".to_string())
        .code(ContainerCode::Image {
            image: app_image.to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.1".to_string(),
            desired: "0.1".to_string(),
        })
        .memory(ResourceSpec {
            min: "128Mi".to_string(),
            desired: "128Mi".to_string(),
        })
        .permissions("app".to_string())
        .link(&sandbox)
        .build();
    let mut profile = alien_core::PermissionProfile::new();
    profile.0.insert(
        "agent".to_string(),
        vec![alien_core::permissions::PermissionSetReference::Name(
            "sandbox/management".to_string(),
        )],
    );
    let stack = Stack::new("gateway-kind".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile("app", profile))
        .add(sandbox, ResourceLifecycle::Frozen)
        .add(manager, ResourceLifecycle::Live)
        .build();
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .expect("Kubernetes accepts internet sandboxes");
    let mut prepared = runner
        .apply_mutations(
            stack,
            &StackState::new(Platform::Kubernetes),
            &deployment_config(),
        )
        .await
        .expect("preparation adds the gateway");
    // Three replicas exercise peer synchronization beyond the default pair.
    prepared
        .resources
        .get_mut(SANDBOX_EGRESS_GATEWAY_ID)
        .expect("gateway")
        .config
        .downcast_mut::<Container>()
        .expect("gateway Container")
        .replicas = Some(3);
    prepared
}

#[tokio::test]
#[ignore = "requires a disposable Kind cluster with Calico and a gvisor RuntimeClass"]
async fn kubernetes_installs_and_reconciles_a_replicated_sandbox_gateway() {
    let _ = tracing_subscriber::fmt::try_init();
    let context = env("ALIEN_GATEWAY_KIND_CONTEXT");
    assert!(context.starts_with("kind-"), "only Kind contexts are accepted");
    assert_eq!(run("kubectl", &["config", "current-context"]).trim(), context);
    let namespace = env("ALIEN_GATEWAY_KIND_NAMESPACE");
    assert!(namespace.starts_with("alien-gateway-"));
    let session_image = env("ALIEN_GATEWAY_KIND_SESSION_IMAGE");
    let app_image =
        std::env::var("ALIEN_GATEWAY_KIND_APP_IMAGE").unwrap_or_else(|_| session_image.clone());

    let stack = prepared_stack(&session_image, &app_image).await;
    let chart = alien_helm::generate_helm_chart(
        &stack,
        alien_helm::HelmOptions {
            registry: &alien_helm::HelmRegistry::built_in(),
            stack_settings: StackSettings::default(),
            chart_name: "gateway-kind".to_string(),
        },
    )
    .expect("chart renders");
    let directory = tempfile::tempdir().expect("chart directory");
    for (path, contents) in &chart.files {
        let destination = directory.path().join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, contents).unwrap();
    }
    // Helm runs the NetworkPolicy enforcement hook before creating anything else and fails
    // the install if the CNI ignores policy. The runtime itself is not exercised here.
    run(
        "helm",
        &[
            "install",
            "gateway-kind",
            directory.path().to_str().unwrap(),
            "--namespace",
            &namespace,
            "--create-namespace",
            "--timeout",
            "5m",
            "--set-string",
            &format!("serviceAccountPrefix={PREFIX}"),
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
        ],
    );

    // The address the chart gave the operator, which must name the Service in front of it.
    let operator = kubectl_json(&namespace, &["get", "deployment", "gateway-kind"]);
    let broker_url = operator["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .expect("operator env")
        .iter()
        .find(|variable| variable["name"] == alien_core::ENV_ALIEN_SANDBOX_BROKER_URL)
        .and_then(|variable| variable["value"].as_str())
        .expect("the chart gives the operator its broker address")
        .to_string();
    assert_eq!(
        broker_url,
        format!("http://gateway-kind.{namespace}.svc:8080"),
        "the broker address names the operator Service"
    );
    let operator_service = kubectl_json(&namespace, &["get", "service", "gateway-kind"]);
    assert_eq!(operator_service["spec"]["ports"][0]["port"], 8080);
    std::env::set_var(alien_core::ENV_ALIEN_SANDBOX_BROKER_URL, &broker_url);

    let client_config = ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: Some(env("KUBECONFIG")),
        context: Some(context.clone()),
        cluster: None,
        user: None,
        namespace: Some(namespace.clone()),
        additional_headers: None,
    }));
    let deployment = deployment_config();
    // As a deployment does: setup-owned resources first, then runtime-owned ones.
    let executor = |lifecycles: Vec<ResourceLifecycle>| {
        alien_infra::StackExecutor::builder(&stack, client_config.clone())
            .deployment_config(&deployment)
            .lifecycle_filter(lifecycles)
            .build()
            .expect("executor")
    };
    let mut state = StackState::with_resource_prefix(Platform::Kubernetes, PREFIX.to_string());
    for lifecycles in [vec![ResourceLifecycle::Frozen], vec![ResourceLifecycle::Live]] {
        let result = executor(lifecycles).run_until_synced(state).await;
        assert!(result.success, "{:?}", result.error);
        state = result.final_state;
    }
    for (id, resource) in &state.resources {
        assert_eq!(
            resource.status,
            alien_core::ResourceStatus::Running,
            "{id}: {:?}",
            resource.error
        );
    }
    let executor = executor(vec![ResourceLifecycle::Frozen, ResourceLifecycle::Live]);

    // Health ticks fill the warm pool once the gateway Service has an address.
    let mut pool = Vec::new();
    for _ in 0..30 {
        state = executor.refresh(state).await.expect("refresh").next_state;
        pool = kubectl_json(&namespace, &["get", "pods", "-l", "alien.dev/sandbox=agent"])
            ["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !pool.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(!pool.is_empty(), "the warm pool never created a session pod");
    // The binding a policy writer loads: writes must reach all three replicas.
    let binding = {
        use alien_infra::{ResourceController as _, StackResourceStateExt as _};
        state.resources["agent"]
            .get_internal_controller()
            .expect("sandbox controller")
            .expect("sandbox controller state")
            .get_binding_params()
            .expect("sandbox binding")
            .expect("sandbox binding params")
    };
    let alien_core::bindings::SandboxBinding::Kubernetes(binding) =
        serde_json::from_value(binding).expect("sandbox binding")
    else {
        panic!("Kubernetes sandbox binding")
    };
    let Some(alien_core::bindings::SandboxEgressBinding::Gateway(gateway)) = binding.egress else {
        panic!("gateway egress binding")
    };
    assert_eq!(
        gateway.replicas.as_ref().map(|replicas| replicas.count),
        Some(3)
    );
    println!(
        "GATEWAY_BINDING {}",
        serde_json::to_string(&gateway).expect("binding JSON")
    );
    // The linked application is given the sandbox binding in its own environment, pointing at
    // the operator's broker, so the SDK can load the sandbox from that workload alone.
    let app = kubectl_json(&namespace, &["get", "deployment", "app"]);
    let app_environment = app["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .expect("app env")
        .clone();
    let app_binding = app_environment
        .iter()
        .find(|variable| variable["name"] == "ALIEN_AGENT_BINDING")
        .and_then(|variable| variable["value"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "the linked app has no sandbox binding: {:?}",
                app_environment
                    .iter()
                    .map(|variable| variable["name"].clone())
                    .collect::<Vec<_>>()
            )
        });
    let app_binding: alien_core::bindings::SandboxBinding =
        serde_json::from_str(app_binding).expect("the app's sandbox binding parses");
    let alien_core::bindings::SandboxBinding::Kubernetes(app_binding) = app_binding else {
        panic!("Kubernetes sandbox binding in the app")
    };
    assert_eq!(
        app_binding.broker_url,
        alien_core::bindings::BindingValue::Value(broker_url.clone())
    );
    let environment = &pool[0]["spec"]["containers"][0]["env"];
    let gateway_service = kubectl_json(
        &namespace,
        &["get", "service", SANDBOX_EGRESS_GATEWAY_ID],
    );
    let address = format!(
        "{}:3128",
        gateway_service["spec"]["clusterIP"].as_str().expect("ClusterIP")
    );
    assert!(
        environment
            .as_array()
            .unwrap()
            .iter()
            .any(|variable| variable["name"] == "ALIEN_SANDBOX_EGRESS_GATEWAY"
                && variable["value"] == address.as_str()),
        "session pods are given the gateway Service address: {environment}"
    );

    let mut ready = 0;
    for _ in 0..90 {
        let gateway = kubectl_json(
            &namespace,
            &["get", "deployment", SANDBOX_EGRESS_GATEWAY_ID],
        );
        ready = gateway["status"]["readyReplicas"].as_u64().unwrap_or(0);
        if ready == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert_eq!(ready, 3, "three gateway replicas become ready");
    let peers = kubectl_json(
        &namespace,
        &[
            "get",
            "service",
            &format!("{SANDBOX_EGRESS_GATEWAY_ID}-peers"),
        ],
    );
    assert_eq!(peers["spec"]["clusterIP"], "None");
    let policy = kubectl_json(
        &namespace,
        &[
            "get",
            "networkpolicy",
            SANDBOX_EGRESS_GATEWAY_ID,
        ],
    );
    assert!(policy["spec"]["policyTypes"]
        .as_array()
        .unwrap()
        .contains(&"Egress".into()));
    println!("GATEWAY_KIND_READY namespace={namespace} prefix={PREFIX}");
}
