//! A Kubernetes sandbox that links a Container reaches it through the egress gateway, and
//! nothing else private, on a disposable Kind cluster.
//!
//! `#[ignore]` because it needs a Kind cluster whose CNI enforces NetworkPolicy (Kind's kindnet
//! does), a worker node besides the control plane, a RuntimeClass named `gvisor`, and these
//! images loaded into the nodes. The controller accepts only a sandboxed handler, and Kind has
//! none, so the nodes' containerd gets a test-only handler named `runsc` that runs `runc`
//! (`runtime_type = "io.containerd.runc.v2"` under `runtimes.runsc`, then restart containerd),
//! and the RuntimeClass names it. This test is about the network path, which is the same.
//! Images:
//! - a gateway image built from this tree (the pinned release image predates links);
//! - a session image that keeps running and has `curl`, with a numeric `USER` so `runAsNonRoot`
//!   admits it (for example `curlimages/curl` with `USER 100:101` and `sleep infinity`);
//! - a busybox image (`ALIEN_LINKS_KIND_HTTP_IMAGE`, default `busybox:1.36`), which serves HTTP for
//!   the linked and the unlinked Container.
//!
//! ```text
//! KUBECONFIG=/path/to/kind/kubeconfig \
//! ALIEN_LINKS_KIND_CONTEXT=kind-<name> \
//! ALIEN_LINKS_KIND_NAMESPACE=alien-links-<suffix> \
//! ALIEN_LINKS_KIND_GATEWAY_IMAGE=<gateway image built from this tree> \
//! ALIEN_LINKS_KIND_SESSION_IMAGE=<session image with curl> \
//! ALIEN_KUBERNETES_CLUSTER_DOMAIN=cluster.local \
//! ALIEN_LINKS_KIND_HTTP_IMAGE=<busybox image loaded into the nodes> \
//!   cargo test -p alien-infra --features all-platforms --test kubernetes_sandbox_links_kind \
//!   -- --ignored --nocapture
//! ```
//!
//! The test prepares and installs the stack as `kubernetes_sandbox_gateway_kind` does, swaps in
//! the gateway image built from this tree, and drives the real controllers. It then plays the
//! application: it reads the deployment's signing key, publishes grants through a
//! port-forward to the gateway's admin listener, and runs `curl` in a warm-pool session pod with
//! each session's token. With `internet` the linked Container answers through the gateway; its
//! other ports, the unlinked Container, the node, the Service's ClusterIP by address, metadata
//! and loopback are refused. An allow list reaches the link only when it names `link:api`, and
//! an empty one refuses it. A direct connection does not leave the pod. The caller deletes the
//! namespace.

#![cfg(feature = "kubernetes")]

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::sandbox_egress::SandboxEgressManager;
use alien_bindings::{Binding, SecretPresence, Vault};
use alien_core::{
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, ClientConfig, Container, ContainerCode,
    DeploymentConfig, EgressPolicy, EnvironmentVariablesSnapshot, ExternalBindings,
    KubernetesClientConfig, Platform, ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings, StackState,
};

const PREFIX: &str = "lnkkind";
const SESSION: &str = "kind-links-session";

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

/// A busybox HTTP server on `port`, reachable inside the cluster only.
fn http_service(id: &str, port: u16) -> Container {
    Container::new(id.to_string())
        .code(ContainerCode::Image {
            image: std::env::var("ALIEN_LINKS_KIND_HTTP_IMAGE")
                .unwrap_or_else(|_| "busybox:1.36".to_string()),
        })
        .command(vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("mkdir -p /tmp/www && echo {id} > /tmp/www/index.html && exec httpd -f -p {port} -h /tmp/www"),
        ])
        .cpu(ResourceSpec {
            min: "0.05".to_string(),
            desired: "0.05".to_string(),
        })
        .memory(ResourceSpec {
            min: "32Mi".to_string(),
            desired: "32Mi".to_string(),
        })
        .permissions("app".to_string())
        .port(port)
        .build()
}

async fn prepared_stack(gateway_image: &str, session_image: &str) -> Stack {
    let api = http_service("api", 8080);
    let other = http_service("other", 8080);
    let sandbox = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: session_image.to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(3600),
            idle_pause_seconds: None,
            ..Default::default()
        })
        .link(&api)
        .build();
    let app = Container::new("app".to_string())
        .code(ContainerCode::Image {
            image: session_image.to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.05".to_string(),
            desired: "0.05".to_string(),
        })
        .memory(ResourceSpec {
            min: "64Mi".to_string(),
            desired: "64Mi".to_string(),
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
    let stack = Stack::new("links-kind".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile("app", profile))
        .add(sandbox, ResourceLifecycle::Frozen)
        .add(api, ResourceLifecycle::Live)
        .add(other, ResourceLifecycle::Live)
        .add(app, ResourceLifecycle::Live)
        .build();
    let runner = alien_preflights::runner::PreflightRunner::new();
    let mut prepared = runner
        .apply_mutations(
            stack,
            &StackState::new(Platform::Kubernetes),
            &deployment_config(),
        )
        .await
        .expect("preparation adds the gateway");
    {
        let gateway = prepared
            .resources
            .get_mut(SANDBOX_EGRESS_GATEWAY_ID)
            .expect("gateway")
            .config
            .downcast_mut::<Container>()
            .expect("gateway Container");
        // The release's pinned gateway predates links; this tree's gateway enforces them.
        gateway.code = ContainerCode::Image {
            image: gateway_image.to_string(),
        };
        gateway.replicas = Some(1);
    }
    // With the gateway that enforces links planned, the deployment's link check accepts it.
    let sandbox = prepared.resources["agent"]
        .config
        .downcast_ref::<Sandbox>()
        .expect("sandbox")
        .clone();
    alien_core::sandbox_links::validate_sandbox_links(&prepared, &sandbox, Platform::Kubernetes)
        .expect("Kubernetes accepts a link with a gateway that enforces links");
    prepared
}

/// The deployment's signing key, read from its Secret as the application's vault would.
#[derive(Debug)]
struct SecretKey(String);

impl Binding for SecretKey {}

#[async_trait::async_trait]
impl Vault for SecretKey {
    async fn get_secret(&self, _secret_name: &str) -> alien_bindings::error::Result<String> {
        Ok(self.0.clone())
    }
    async fn secret_presence(
        &self,
        _secret_name: &str,
    ) -> alien_bindings::error::Result<SecretPresence> {
        Ok(SecretPresence::Present { version: None })
    }
    async fn set_secret(&self, _name: &str, _value: &str) -> alien_bindings::error::Result<()> {
        unreachable!("the test only reads the key")
    }
    async fn delete_secret(&self, _name: &str) -> alien_bindings::error::Result<()> {
        unreachable!("the test only reads the key")
    }
    async fn list_secrets(&self) -> alien_bindings::error::Result<Vec<String>> {
        unreachable!("the test only reads the key")
    }
}

struct PortForward(Child);

impl Drop for PortForward {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `curl` in the session pod; returns (exit code, HTTP status or body, stderr).
fn curl_in(namespace: &str, pod: &str, arguments: &[&str]) -> (i32, String, String) {
    let mut all = vec![
        "-n",
        namespace,
        "exec",
        pod,
        "--",
        "curl",
        "-sS",
        "--max-time",
        "8",
    ];
    all.extend(arguments);
    let output = Command::new("kubectl")
        .args(&all)
        .output()
        .expect("kubectl exec");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    )
}

#[tokio::test]
#[ignore = "requires a disposable Kind cluster; see the module docs"]
async fn a_linked_kubernetes_sandbox_reaches_its_link_through_the_gateway_and_nothing_else_private()
{
    let _ = tracing_subscriber::fmt::try_init();
    // The embedding process sets one before workload secrets are applied, as the operator does.
    alien_core::secrets_digest_key::set_if_unset(rand::random());
    let context = env("ALIEN_LINKS_KIND_CONTEXT");
    assert!(
        context.starts_with("kind-"),
        "only Kind contexts are accepted"
    );
    assert_eq!(
        run("kubectl", &["config", "current-context"]).trim(),
        context
    );
    let namespace = env("ALIEN_LINKS_KIND_NAMESPACE");
    assert!(namespace.starts_with("alien-links-"));
    let gateway_image = env("ALIEN_LINKS_KIND_GATEWAY_IMAGE");
    let session_image = env("ALIEN_LINKS_KIND_SESSION_IMAGE");

    let stack = prepared_stack(&gateway_image, &session_image).await;
    let chart = alien_helm::generate_helm_chart(
        &stack,
        alien_helm::HelmOptions {
            registry: &alien_helm::HelmRegistry::built_in(),
            stack_settings: StackSettings::default(),
            chart_name: "links-kind".to_string(),
        },
    )
    .expect("chart renders");
    let directory = tempfile::tempdir().expect("chart directory");
    for (path, contents) in &chart.files {
        let destination = directory.path().join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, contents).unwrap();
    }
    run(
        "helm",
        &[
            "install",
            "links-kind",
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
            "runtime.image.tag=1.36",
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
            "--set-string",
            "sandbox.runtimeClassName=gvisor",
        ],
    );
    let operator = kubectl_json(&namespace, &["get", "deployment", "links-kind"]);
    let broker_url = operator["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .expect("operator env")
        .iter()
        .find(|variable| variable["name"] == alien_core::ENV_ALIEN_SANDBOX_BROKER_URL)
        .and_then(|variable| variable["value"].as_str())
        .expect("the chart gives the operator its broker address")
        .to_string();
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
    let executor = |lifecycles: Vec<ResourceLifecycle>| {
        alien_infra::StackExecutor::builder(&stack, client_config.clone())
            .deployment_config(&deployment)
            .lifecycle_filter(lifecycles)
            .build()
            .expect("executor")
    };
    let mut state = StackState::with_resource_prefix(Platform::Kubernetes, PREFIX.to_string());
    for lifecycles in [
        vec![ResourceLifecycle::Frozen],
        vec![ResourceLifecycle::Live],
    ] {
        let result = executor(lifecycles).run_until_synced(state).await;
        assert!(result.success, "{:?}", result.error);
        state = result.final_state;
    }
    let executor = executor(vec![ResourceLifecycle::Frozen, ResourceLifecycle::Live]);

    // Health ticks fill the warm pool, and republish the gateway's links once the linked
    // Service has an address.
    let mut pool = Vec::new();
    for _ in 0..45 {
        state = executor.refresh(state).await.expect("refresh").next_state;
        pool = kubectl_json(
            &namespace,
            &[
                "get",
                "pods",
                "-l",
                "alien.dev/sandbox=agent",
                "--field-selector=status.phase=Running",
            ],
        )["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !pool.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(!pool.is_empty(), "the warm pool never ran a session pod");
    let session_pod = pool[0]["metadata"]["name"]
        .as_str()
        .expect("pod name")
        .to_string();

    let binding = {
        use alien_infra::StackResourceStateExt as _;
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
    println!(
        "LINK_BINDINGS {}",
        serde_json::to_string(&gateway.links).expect("links JSON")
    );
    let alien_core::bindings::ContainerBinding::Kubernetes(api) = &gateway.links["api"] else {
        panic!("a Kubernetes binding for the linked Container")
    };
    let alien_core::bindings::BindingValue::Value(api_url) = &api.internal_urls[&8080] else {
        panic!("a literal internal URL")
    };
    let api_url = api_url.clone();
    assert!(
        !gateway.links.contains_key("other"),
        "only linked Containers are given to the sandbox"
    );

    let gateway_service = kubectl_json(&namespace, &["get", "service", SANDBOX_EGRESS_GATEWAY_ID]);
    let gateway_ip = gateway_service["spec"]["clusterIP"]
        .as_str()
        .expect("gateway ClusterIP")
        .to_string();
    let mut ready = 0;
    for _ in 0..90 {
        let deployment = kubectl_json(
            &namespace,
            &["get", "deployment", SANDBOX_EGRESS_GATEWAY_ID],
        );
        ready = deployment["status"]["readyReplicas"].as_u64().unwrap_or(0);
        if ready == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert_eq!(ready, 1, "the gateway becomes ready");
    let policy = kubectl_json(
        &namespace,
        &["get", "networkpolicy", SANDBOX_EGRESS_GATEWAY_ID],
    );
    println!(
        "GATEWAY_NETWORK_POLICY {}",
        serde_json::to_string(&policy["spec"]).unwrap()
    );

    // The application's side: the signing key from the deployment's vault, and the gateway's
    // admin listener through a port-forward.
    let tail = alien_core::vault_naming::kubernetes_secret_name_tail(&gateway.signing_key.key);
    let secrets = kubectl_json(&namespace, &["get", "secrets"]);
    let key_secret = secrets["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|secret| {
            secret["metadata"]["name"]
                .as_str()
                .is_some_and(|name| name.ends_with(&tail))
        })
        .expect("the signing key Secret");
    use base64::Engine as _;
    let key = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(key_secret["data"]["value"].as_str().expect("key value"))
            .expect("base64"),
    )
    .expect("UTF-8 key");
    let local_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let _forward = PortForward(
        Command::new("kubectl")
            .args([
                "-n",
                &namespace,
                "port-forward",
                &format!("deployment/{SANDBOX_EGRESS_GATEWAY_ID}"),
                &format!("{local_port}:3129"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("port-forward"),
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut admin_binding = gateway.clone();
    // One replica behind the forwarded address.
    admin_binding.replicas = None;
    let egress = SandboxEgressManager::new(
        admin_binding,
        Arc::new(SecretKey(key)),
        &format!("http://{gateway_ip}:3128"),
        &format!("http://127.0.0.1:{local_port}"),
    )
    .expect("egress manager");
    let now = chrono::Utc::now().timestamp();
    let token = egress
        .mint_identity(SESSION, now + 3600)
        .await
        .expect("a session token");
    egress
        .publish_new_grant(SESSION, &EgressPolicy::Internet)
        .await
        .expect("the grant is published");
    let proxy = format!("http://alien:{token}@{gateway_ip}:3128");

    // Retried for a short while: the owner publishes a link once the Service has its ClusterIP.
    let mut linked = (0, String::new(), String::new());
    for _ in 0..20 {
        linked = curl_in(&namespace, &session_pod, &["-x", &proxy, &api_url]);
        if linked.0 == 0 && linked.1 == "api" {
            break;
        }
        state = executor.refresh(state).await.expect("refresh").next_state;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    println!("LINKED {api_url} -> {linked:?}");
    assert_eq!(
        (linked.0, linked.1.as_str()),
        (0, "api"),
        "the linked Container answers through the gateway: {linked:?}"
    );

    let api_service = kubectl_json(&namespace, &["get", "service", &api.service_name_value()]);
    let api_ip = api_service["spec"]["clusterIP"]
        .as_str()
        .unwrap()
        .to_string();
    let api_host = api_url
        .trim_start_matches("http://")
        .split(':')
        .next()
        .unwrap()
        .to_string();
    let other_host = format!(
        "{}.{}",
        other_service_name(&api.service_name_value()),
        api_host.split_once('.').expect("a qualified host").1
    );
    let node_ip = pool[0]["status"]["hostIP"].as_str().unwrap().to_string();
    let refused = [
        (
            "linked host, undeclared port",
            format!("http://{api_host}:8081/"),
        ),
        ("unlinked Container", format!("http://{other_host}:8080/")),
        (
            "linked ClusterIP by address",
            format!("http://{api_ip}:8080/"),
        ),
        ("node", format!("http://{node_ip}:10250/")),
        (
            "metadata",
            "http://169.254.169.254/latest/meta-data/".to_string(),
        ),
        ("loopback", "http://127.0.0.1:3129/".to_string()),
    ];
    for (what, url) in &refused {
        let result = curl_in(
            &namespace,
            &session_pod,
            &["-x", &proxy, "-o", "/dev/null", "-w", "%{http_code}", url],
        );
        println!("REFUSED? {what} {url} -> {result:?}");
        assert!(
            result.0 != 0 || result.1 != "200",
            "{what} must be refused: {result:?}"
        );
        assert_ne!(result.1, "200", "{what} must not answer: {result:?}");
    }
    // A session's own policy narrows its links: an allow list reaches a link only by naming it,
    // and an empty one refuses it.
    for (session, allow, reaches) in [
        ("kind-links-named", vec!["link:api".to_string()], true),
        ("kind-links-unnamed", vec!["example.com".to_string()], false),
        ("kind-links-deny-all", vec![], false),
    ] {
        let token = egress
            .mint_identity(session, now + 3600)
            .await
            .expect("a session token");
        egress
            .publish_new_grant(
                session,
                &EgressPolicy::Allow {
                    allow: allow.clone(),
                },
            )
            .await
            .expect("the grant is published");
        let proxy = format!("http://alien:{token}@{gateway_ip}:3128");
        let result = curl_in(
            &namespace,
            &session_pod,
            &[
                "-x",
                &proxy,
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &api_url,
            ],
        );
        println!("POLICY allow={allow:?} {api_url} -> {result:?}");
        assert_eq!(
            result.0 == 0 && result.1 == "200",
            reaches,
            "allow {allow:?}: {result:?}"
        );
    }
    // Without the gateway the session pod reaches nothing.
    let direct = curl_in(&namespace, &session_pod, &["--noproxy", "*", &api_url]);
    println!("DIRECT {api_url} -> {direct:?}");
    assert_ne!(direct.0, 0, "a direct connection must fail: {direct:?}");
    let direct = curl_in(
        &namespace,
        &session_pod,
        &["--noproxy", "*", &format!("http://{api_ip}:8080/")],
    );
    println!("DIRECT {api_ip}:8080 -> {direct:?}");
    assert_ne!(
        direct.0, 0,
        "a direct connection by address must fail: {direct:?}"
    );

    let logs = run(
        "kubectl",
        &[
            "-n",
            &namespace,
            "logs",
            &format!("deployment/{SANDBOX_EGRESS_GATEWAY_ID}"),
            "--tail",
            "200",
        ],
    );
    println!("GATEWAY_LOGS\n{logs}");
    println!("LINKS_KIND_PASSED namespace={namespace}");
}

/// The unlinked Container's Service, named as the linked one is.
fn other_service_name(api_service: &str) -> String {
    format!(
        "{}other",
        api_service.strip_suffix("api").expect("the api Service")
    )
}

trait ServiceName {
    fn service_name_value(&self) -> String;
}

impl ServiceName for alien_core::bindings::KubernetesContainerBinding {
    fn service_name_value(&self) -> String {
        match &self.service_name {
            alien_core::bindings::BindingValue::Value(name) => name.clone(),
            other => panic!("a literal Service name: {other:?}"),
        }
    }
}
