//! The interceptor hook through Alien's Kubernetes client, against a local server that records
//! the bytes on the wire.

use alien_client_core::interceptor::{CallInterceptor, Cloud, HookRequest, InterceptorChain};
use alien_client_core::testing::capture::{self, Reply};
use alien_client_core::testing::TestInterceptor;
use alien_core::KubernetesClientConfig;
use alien_k8s_clients::KubernetesClient;
use k8s_openapi::api::core::v1::Pod;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

async fn client(server: &capture::Server, interceptor: TestInterceptor) -> KubernetesClient {
    KubernetesClient::new(KubernetesClientConfig::Manual {
        server_url: server.base_url.clone(),
        certificate_authority_data: None,
        insecure_skip_tls_verify: Some(true),
        client_certificate_data: None,
        client_key_data: None,
        token: Some("k8s-test-token".into()),
        username: None,
        password: None,
        namespace: Some("acme".into()),
        additional_headers: HashMap::new(),
    })
    .await
    .unwrap()
    .with_interceptors(InterceptorChain::new(vec![
        Arc::new(interceptor) as Arc<dyn CallInterceptor>
    ]))
}

fn pod() -> Pod {
    serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": { "name": "worker", "labels": { "app": "worker" } },
        "spec": { "containers": [{ "name": "main", "image": "busybox" }] }
    }))
    .unwrap()
}

/// The pod Alien creates carries the label the interceptor added, the request keeps the bearer
/// token, and the interceptor never saw it.
#[tokio::test]
async fn a_created_pod_carries_the_extension_label() {
    let server =
        capture::start(|request| Reply::new(201, "application/json", request.body.clone())).await;
    let seen: Arc<Mutex<Vec<HookRequest>>> = Arc::default();
    let spy = seen.clone();
    let label =
        TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes").on_request(move |request| {
            spy.lock().unwrap().push(request.clone());
            let mut pod: serde_json::Value =
                serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
            pod["metadata"]["labels"]["team"] = "payments".into();
            request.body = Some(pod.to_string());
            Ok(())
        });
    let created = client(&server, label)
        .await
        .create_pod("acme", &pod())
        .await
        .unwrap();
    assert_eq!(created.metadata.labels.unwrap()["team"], "payments");

    let sent = server.requests();
    assert_eq!(sent[0].target, "/api/v1/namespaces/acme/pods");
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer k8s-test-token")
    );
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(body["metadata"]["labels"]["team"], "payments");
    assert_eq!(body["metadata"]["labels"]["app"], "worker");
    assert!(seen.lock().unwrap()[0]
        .headers
        .get("authorization")
        .is_none());
}

/// Alien reads the object the API server returns to decide its next calls on the cluster, so an
/// interceptor that changes it fails the call.
#[tokio::test]
async fn a_returned_object_may_not_be_changed() {
    let server =
        capture::start(|request| Reply::new(201, "application/json", request.body.clone())).await;
    let annotate =
        TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes").on_response(|_, response| {
            let mut pod: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            pod["metadata"]["annotations"] = serde_json::json!({ "acme.dev/seen": "true" });
            response.body = pod.to_string();
            Ok(())
        });
    let error = client(&server, annotate)
        .await
        .create_pod("acme", &pod())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
    assert!(!error.retryable);
}

/// A Secret carries the deployment's registry credential, so an interceptor that matches every
/// call is never asked about one, and the Secret is sent as Alien built it.
#[tokio::test]
async fn a_secret_is_never_offered() {
    use k8s_openapi::api::core::v1::Secret;

    let server =
        capture::start(|request| Reply::new(201, "application/json", request.body.clone())).await;
    let every_call = TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes")
        .when(|request| panic!("{} {} was offered", request.method, request.url));
    let secret: Secret = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": { "name": "registry" },
        "type": "kubernetes.io/dockerconfigjson",
        "stringData": { ".dockerconfigjson": "{}" }
    }))
    .unwrap();
    client(&server, every_call)
        .await
        .create_secret("acme", &secret)
        .await
        .unwrap();
    let sent = server.requests();
    assert_eq!(sent[0].target, "/api/v1/namespaces/acme/secrets");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&sent[0].body).unwrap(),
        serde_json::to_value(&secret).unwrap()
    );
}

/// A privileged container reaches the node other deployments run on, so an interceptor may not
/// make one, and the pod is never sent.
#[tokio::test]
async fn a_privileged_container_is_refused() {
    let server =
        capture::start(|request| Reply::new(201, "application/json", request.body.clone())).await;
    let privileged =
        TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes").on_request(|request| {
            let mut pod: serde_json::Value =
                serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
            pod["spec"]["containers"][0]["securityContext"] =
                serde_json::json!({ "privileged": true });
            request.body = Some(pod.to_string());
            Ok(())
        });
    let error = client(&server, privileged)
        .await
        .create_pod("acme", &pod())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
    assert!(server.requests().is_empty());
}
