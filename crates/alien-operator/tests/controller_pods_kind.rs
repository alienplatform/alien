//! The Operator's extension controller pods against a real API server. Run explicitly against a
//! disposable Kind cluster; never against the default kube context.

use std::process::Command;

use alien_k8s_clients::{KubernetesClient, KubernetesClientConfigExt};
use alien_operator::controller_pods::{Cluster, ControllerPod, KubernetesCluster};

const PREFIX: &str = "kindpods";

fn require_kind_context() {
    let expected =
        std::env::var("ALIEN_CONTROLLER_PODS_KIND_CONTEXT").expect("set explicit Kind context");
    assert!(
        expected.starts_with("kind-"),
        "only Kind contexts are accepted"
    );
    let kubeconfig = std::env::var("KUBECONFIG").expect("set explicit disposable kubeconfig");
    assert!(!kubeconfig.is_empty());
    let output = Command::new("kubectl")
        .args(["config", "current-context"])
        .output()
        .expect("kubectl current-context");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
}

fn kubectl(namespace: &str, stdin: &str) {
    use std::io::Write as _;
    let mut child = Command::new("kubectl")
        .args(["-n", namespace, "apply", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("kubectl apply");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success(), "kubectl apply failed");
}

fn pod(sha256: &str) -> ControllerPod {
    ControllerPod {
        extension: "acme".to_string(),
        sha256: sha256.repeat(64),
        deployment_id: "dep_kind_pods".to_string(),
        identity: None,
        env: Default::default(),
        trusted_key: "a2V5".to_string(),
    }
}

/// The Operator creates a controller's Deployment and Service only where the chart's
/// NetworkPolicy exists, makes them from its own pod, writes nothing on a pass that changes
/// nothing, replaces them when the pinned binary changes, and removes them when the policy goes
/// or none is wanted.
#[tokio::test]
#[ignore = "requires explicit disposable Kind kubeconfig"]
async fn controller_pods_follow_what_the_operator_wants() {
    require_kind_context();
    let namespace =
        std::env::var("ALIEN_CONTROLLER_PODS_KIND_NAMESPACE").expect("set Kind namespace");
    assert!(namespace.starts_with("alien-controller-pods-"));
    let config = alien_infra::resolve_kubeconfig(
        &alien_k8s_clients::KubernetesClientConfig::try_kubeconfig()
            .await
            .expect("Kind kubeconfig"),
    )
    .await
    .expect("resolve Kind kubeconfig");
    let client = KubernetesClient::new(config.clone())
        .await
        .expect("Kubernetes client");
    // The Operator's own pod, read for its image. It need not run.
    kubectl(
        &namespace,
        r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"operator"},"spec":{
            "imagePullSecrets":[{"name":"operator-registry"}],
            "containers":[{"name":"operator","image":"registry.invalid/operator:1"}]}}"#,
    );
    let cluster = KubernetesCluster::new(
        config,
        namespace.clone(),
        alien_infra::kubernetes_deployment_scope_label_for(None, PREFIX),
        PREFIX.to_string(),
        Some("operator".to_string()),
    );
    let first = pod("a");
    let second = pod("b");

    let refused = cluster
        .run(std::slice::from_ref(&first))
        .await
        .expect_err("no NetworkPolicy bounds the pod yet");
    assert!(
        refused
            .message
            .contains("'kindpods-extension-controllers' that bounds an extension controller's pod is not installed"),
        "{refused}"
    );
    assert!(client
        .get_deployment(&namespace, &first.name())
        .await
        .is_err());

    kubectl(
        &namespace,
        &format!(
            r#"{{"apiVersion":"networking.k8s.io/v1","kind":"NetworkPolicy",
            "metadata":{{"name":"{}"}},
            "spec":{{"podSelector":{{"matchLabels":{{"component":"extension-controller"}}}},"policyTypes":["Ingress","Egress"]}}}}"#,
            alien_core::kubernetes_extension_controllers_policy_name(PREFIX)
        ),
    );
    assert!(!cluster
        .has_service_account("kindpods-acme-ext-sa")
        .await
        .unwrap());
    kubectl(
        &namespace,
        r#"{"apiVersion":"v1","kind":"ServiceAccount","metadata":{"name":"kindpods-acme-ext-sa"}}"#,
    );
    assert!(cluster
        .has_service_account("kindpods-acme-ext-sa")
        .await
        .unwrap());

    let urls = cluster.run(std::slice::from_ref(&first)).await.unwrap();
    assert_eq!(
        urls,
        [format!("http://{}.{namespace}.svc:8080", first.name())]
    );
    let created = client
        .get_deployment(&namespace, &first.name())
        .await
        .expect("the Deployment exists");
    let spec = created.spec.clone().unwrap().template.spec.unwrap();
    assert_eq!(spec.automount_service_account_token, Some(false));
    assert_eq!(spec.service_account_name.as_deref(), Some("default"));
    assert_eq!(
        spec.containers[0].image.as_deref(),
        Some("registry.invalid/operator:1")
    );
    assert_eq!(
        spec.image_pull_secrets.unwrap()[0].name,
        "operator-registry"
    );
    client
        .get_service(&namespace, &first.name())
        .await
        .expect("the Service exists");

    cluster.run(std::slice::from_ref(&first)).await.unwrap();
    let unchanged = client
        .get_deployment(&namespace, &first.name())
        .await
        .unwrap();
    assert_eq!(unchanged.metadata.generation, created.metadata.generation);

    cluster.run(std::slice::from_ref(&second)).await.unwrap();
    assert!(client
        .get_deployment(&namespace, &first.name())
        .await
        .is_err());
    assert!(client.get_service(&namespace, &first.name()).await.is_err());
    client
        .get_deployment(&namespace, &second.name())
        .await
        .expect("the pod of the newly pinned binary");

    // A pod that is running when its policy goes is removed, not left unbounded.
    let policy = alien_core::kubernetes_extension_controllers_policy_name(PREFIX);
    assert!(Command::new("kubectl")
        .args(["-n", &namespace, "delete", "networkpolicy", &policy])
        .status()
        .unwrap()
        .success());
    cluster
        .run(std::slice::from_ref(&second))
        .await
        .expect_err("no NetworkPolicy bounds the pod any more");
    assert!(client
        .get_deployment(&namespace, &second.name())
        .await
        .is_err());
    assert!(client
        .get_service(&namespace, &second.name())
        .await
        .is_err());

    cluster.run(&[]).await.unwrap();
    assert!(client
        .get_deployment(&namespace, &second.name())
        .await
        .is_err());
    assert!(client
        .get_service(&namespace, &second.name())
        .await
        .is_err());
}
