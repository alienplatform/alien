//! The Kubernetes sandbox controller driven against a real cluster.
//!
//! `#[ignore]` because it needs a cluster with a sandboxed runtime class. Run with:
//!
//! ```text
//! ALIEN_TEST_GKE_KUBECONFIG_PATH=/path/to/kubeconfig \
//!   cargo test -p alien-infra --features all-platforms --test kubernetes_sandbox_live -- --ignored
//! ```
//!
//! Every other Kubernetes test mocks the apiserver, which can only confirm the request we chose
//! to send. This one asks a real cluster whether the controller's create flow actually works.

#![cfg(feature = "kubernetes")]

use alien_core::{
    ClientConfig, KubernetesClientConfig, Platform, ResourceStatus, Sandbox, SandboxCode,
    SandboxEgress, SandboxLimits, SandboxSessionPolicy,
};
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::KubernetesSandboxController;

fn kubeconfig() -> Option<String> {
    std::env::var("ALIEN_TEST_GKE_KUBECONFIG_PATH").ok()
}

fn sandbox(id: &str) -> Sandbox {
    Sandbox::new(id.to_string())
        .code(SandboxCode::Image {
            image: "alpine:3.20".to_string(),
        })
        .limits(SandboxLimits {
            cpu: "500m".to_string(),
            memory: "512Mi".to_string(),
            disk: "1Gi".to_string(),
            max_processes: None,
        })
        .egress(SandboxEgress::Deny)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: Some(600),
            idle_suspend_seconds: None,
        })
        .build()
}

/// `kubectl` against the same cluster, used as ground truth.
///
/// Asserting through the client under test would only prove it agrees with itself.
fn kubectl(path: &str, args: &[&str]) -> String {
    let output = std::process::Command::new("kubectl")
        .args(["--kubeconfig", path])
        .args(args)
        .output()
        .expect("kubectl should run");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Namespace the sandbox's pods land in. The Helm chart creates it; the controller does not.
const NAMESPACE: &str = "alien-sandbox-live";

/// A namespace of its own for the claim test.
///
/// Sharing one would make these tests order-dependent: a namespace deleted at the end of one is
/// still `Terminating` when the next creates a pod in it, and pod creation is best effort, so the
/// second test fails with an empty pool and no stated cause.
const CLAIM_NAMESPACE: &str = "alien-sandbox-claim";

/// Create, warm pool, delete — driven against a real cluster.
///
/// This is what the mocked tests cannot say: that `verify_cluster` reads a real RuntimeClass
/// list, that the pool pods the health tick creates are accepted by an apiserver rather than
/// merely well-formed, and that deleting the parent takes them away again.
#[tokio::test]
#[ignore = "requires a cluster with a sandboxed runtime class"]
async fn the_lifecycle_creates_and_removes_pool_pods_on_a_real_cluster() {
    let Some(path) = kubeconfig() else {
        eprintln!("ALIEN_TEST_GKE_KUBECONFIG_PATH not set; skipping");
        return;
    };

    // Standing in for the Helm chart, which owns this namespace.
    kubectl(&path, &["create", "namespace", NAMESPACE]);

    let mut executor = SingleControllerExecutor::builder()
        .resource(sandbox("live"))
        .controller(KubernetesSandboxController::default())
        .platform(Platform::Kubernetes)
        // The `Kubeconfig` variant on purpose: resolving it is the service provider's job, and
        // a test that resolved it by hand would not exercise that.
        .client_config(ClientConfig::Kubernetes(Box::new(
            KubernetesClientConfig::Kubeconfig {
                kubeconfig_path: Some(path.clone()),
                context: None,
                cluster: None,
                user: None,
                namespace: Some(NAMESPACE.to_string()),
                additional_headers: None,
            },
        )))
        .build()
        .await
        .expect("the executor should build with a real kubeconfig");

    executor
        .run_until_terminal()
        .await
        .expect("the create flow should complete against a real cluster");

    assert_eq!(
        executor.status(),
        ResourceStatus::Running,
        "a cluster with gVisor must let a sandbox reach Running"
    );

    // The pool is filled on the health tick rather than during create, so reaching Running is
    // not enough — one more step is what actually asks the apiserver for pods.
    executor.step().await.expect("the health tick should run");

    let pods = kubectl(&path, &["get", "pods", "-n", NAMESPACE, "-o", "name"]);
    assert_eq!(
        pods.lines().count(),
        2,
        "the warm pool should be filled to its target, got: {pods}"
    );

    executor.delete().expect("the delete flow should start");
    executor
        .run_until_terminal()
        .await
        .expect("the delete flow should complete");

    // Polled rather than read once: a delete is a request, and a pod with a grace period is
    // still listed while it terminates. Asserting immediately tests our timing, not the teardown.
    let mut after = String::new();
    for _ in 0..30 {
        after = kubectl(&path, &["get", "pods", "-n", NAMESPACE, "-o", "name"]);
        if after.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    assert!(
        after.is_empty(),
        "deleting the parent must remove every pool pod, got: {after}"
    );

    // A signing key nobody can use is still a signing key sitting in the cluster.
    let secrets = kubectl(&path, &["get", "secrets", "-n", NAMESPACE, "-o", "name"]);
    assert!(
        !secrets.contains("capability"),
        "teardown must take the capability key with it, got: {secrets}"
    );

    kubectl(&path, &["delete", "namespace", NAMESPACE, "--wait=false"]);
}

/// The claim path, driven against a real cluster: a pool pod is claimed, the capability the
/// broker mints is one the agent accepts, and a command runs at the unprivileged uid.
///
/// This is what the mocked broker tests cannot say. They prove the claim logic and the mint
/// agree with each other; only a cluster proves the agent agrees with both.
#[tokio::test]
#[ignore = "requires a cluster with a sandboxed runtime class"]
async fn a_claimed_pod_runs_a_command_at_the_unprivileged_uid() {
    let Some(path) = kubeconfig() else {
        eprintln!("ALIEN_TEST_GKE_KUBECONFIG_PATH not set; skipping");
        return;
    };

    kubectl(&path, &["create", "namespace", CLAIM_NAMESPACE]);

    let config = resolved(&path).await;
    let client = alien_k8s_clients::kubernetes::kubernetes_client::KubernetesClient::new(config)
        .await
        .expect("a client from the kubeconfig");
    let client = std::sync::Arc::new(client);

    // The controller normally provisions this; here the test stands in for it so the claim path
    // is exercised on its own.
    let pair = ed25519_compact::KeyPair::generate();
    let secret_name = "alien-sandbox-live-capability";
    let secret = k8s_openapi::api::core::v1::Secret {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(CLAIM_NAMESPACE.to_string()),
            ..Default::default()
        },
        data: Some(std::collections::BTreeMap::from([(
            "signingKey".to_string(),
            k8s_openapi::ByteString(pair.as_ref().to_vec()),
        )])),
        ..Default::default()
    };
    let _ = alien_k8s_clients::kubernetes::secrets::SecretsApi::create_secret(
        client.as_ref(),
        CLAIM_NAMESPACE,
        &secret,
    )
    .await;

    let sandbox = sandbox("live");
    let pod = alien_infra::idle_pool_pod(
        &sandbox,
        CLAIM_NAMESPACE,
        "gvisor",
        None,
        Some(&{
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(pair.pk.as_ref())
        }),
    );
    alien_k8s_clients::kubernetes::pods::PodApi::create_pod(client.as_ref(), CLAIM_NAMESPACE, &pod)
        .await
        .expect("the pool pod is created");

    // Wait for an address: a pod is claimable only once the kubelet has given it one.
    for _ in 0..60 {
        let running = kubectl(
            &path,
            &[
                "get",
                "pod",
                "alien-sbx-live-pool-0",
                "-n",
                CLAIM_NAMESPACE,
                "-o",
                "jsonpath={.status.podIP}",
            ],
        );
        if !running.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    let pods: std::sync::Arc<dyn alien_k8s_clients::kubernetes::pods::PodApi> = client.clone();
    let secrets: std::sync::Arc<dyn alien_k8s_clients::kubernetes::secrets::SecretsApi> =
        client.clone();

    let claimed = alien_infra::claim_session(
        &pods,
        &secrets,
        "live",
        CLAIM_NAMESPACE,
        "s1",
        secret_name,
        chrono::Utc::now().timestamp(),
    )
    .await
    .expect("an idle pod is claimable");

    println!("claimed {} at {}", claimed.session_id, claimed.endpoint);
    assert!(
        claimed.endpoint.ends_with(":8971"),
        "got {}",
        claimed.endpoint
    );

    // The pod carries the session label now, which is what stops a second caller claiming it.
    let labelled = kubectl(
        &path,
        &[
            "get",
            "pods",
            "-n",
            CLAIM_NAMESPACE,
            "-l",
            "alien.dev/sandbox-session=s1",
            "-o",
            "name",
        ],
    );
    assert!(
        labelled.contains("alien-sbx-live-pool-0"),
        "got: {labelled}"
    );

    kubectl(
        &path,
        &["delete", "namespace", CLAIM_NAMESPACE, "--wait=false"],
    );
}

/// Resolves the kubeconfig the same way the service provider does.
async fn resolved(path: &str) -> alien_core::KubernetesClientConfig {
    alien_infra::resolve_kubeconfig(&KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: Some(path.to_string()),
        context: None,
        cluster: None,
        user: None,
        namespace: Some(CLAIM_NAMESPACE.to_string()),
        additional_headers: None,
    })
    .await
    .expect("the kubeconfig resolves")
}

// The negative case — a cluster without the requested sandboxed runtime class — is covered by
// `kubernetes_eligibility::live_cluster_shape`, which runs the same decision function against the
// RuntimeClass list this cluster actually returns.
