//! The session broker against a real API server (Kind), which the mocked unit tests cannot
//! stand in for: label selectors, `resourceVersion` conflicts, annotations that survive a round
//! trip, and a pod that really starts.
//!
//! `#[ignore]` because it needs a cluster. The Kind workflow runs it with a RuntimeClass named
//! `alien-kind-runc` (handler `runc`, which is fine here because the broker functions are driven
//! directly and the controller's sandboxed-handler check is not part of this path) and the
//! `alpine:3.20` image loaded into the node:
//!
//! ```text
//! ALIEN_BROKER_KIND_CONTEXT=kind-<name> ALIEN_BROKER_KIND_NAMESPACE=<existing namespace> \
//!   cargo test -p alien-infra --features all-platforms --lib -- --ignored kubernetes_broker_kind
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_core::{
    KubernetesClientConfig, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxLimits,
};
use alien_k8s_clients::kubernetes::kubernetes_client::KubernetesClient;
use alien_k8s_clients::{PodApi, SecretsApi};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use super::kubernetes_broker::{
    find_or_create_session_for_deployment, list_sessions_for_deployment,
    preview_session_for_deployment, reattach_session_for_deployment,
    release_session_for_deployment, PreviewError, SessionRequestError,
};
use super::{
    capability_secret_name, idle_pool_pod_with_labels, retire_stale_idle_pods, stale_idle_pods,
    ANNOTATION_SESSION_DEADLINE, ANNOTATION_TEMPLATE_HASH, LABEL_SANDBOX, LABEL_SESSION,
    POD_TEMPLATE_SECRET_KEY,
};

const SANDBOX: &str = "kindbroker";
const SCOPE_KEY: &str = "alien.dev/deployment";
const SCOPE_VALUE: &str = "kind-broker";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn declared() -> Sandbox {
    Sandbox::new(SANDBOX.to_string())
        .code(SandboxCode::Image {
            image: "alpine:3.20".to_string(),
            bundled_from: None,
        })
        .limits(SandboxLimits {
            cpu: "100m".to_string(),
            memory: "64Mi".to_string(),
            disk: "64Mi".to_string(),
            max_processes: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(3600),
            idle_pause_seconds: None,
        })
        .build()
}

/// The pod the controller would build, made runnable on Kind: a command that keeps alive, and a
/// pinned uid so `runAsNonRoot` admits the image (which declares no `USER`).
fn template(namespace: &str) -> k8s_openapi::api::core::v1::Pod {
    let mut sandbox = declared();
    sandbox.image_user = Some(1001);
    let mut pod = idle_pool_pod_with_labels(
        &sandbox,
        namespace,
        "alien-kind-runc",
        None,
        None,
        BTreeMap::from([(SCOPE_KEY.to_string(), SCOPE_VALUE.to_string())]),
    );
    let spec = pod.spec.as_mut().expect("a spec");
    spec.containers[0].command = Some(vec!["sleep".to_string(), "3600".to_string()]);
    pod
}

async fn wait_running(pods: &Arc<dyn PodApi>, namespace: &str, session: &str) {
    for _ in 0..90 {
        let listed = pods
            .list_pods(namespace, Some(format!("{LABEL_SESSION}={session}")), None)
            .await
            .expect("list");
        if listed.items.iter().any(|pod| {
            pod.status
                .as_ref()
                .is_some_and(|status| status.phase.as_deref() == Some("Running"))
        }) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!("the pod of session {session} never ran");
}

#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_broker_kind_sessions_live_on_the_pod() {
    let namespace = env("ALIEN_BROKER_KIND_NAMESPACE");
    // The client takes a resolved configuration; kubeconfig files are resolved here, as the
    // service provider does for every controller.
    let resolved = crate::kubeconfig::resolve_kubeconfig(&KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: std::env::var("KUBECONFIG").ok(),
        context: Some(env("ALIEN_BROKER_KIND_CONTEXT")),
        cluster: None,
        user: None,
        namespace: Some(namespace.clone()),
        additional_headers: None,
    })
    .await
    .expect("resolved kubeconfig");
    let client = Arc::new(
        KubernetesClient::new(resolved)
            .await
            .expect("kubernetes client"),
    );
    let pods: Arc<dyn PodApi> = client.clone();
    let secrets: Arc<dyn SecretsApi> = client.clone();
    let secret_name = capability_secret_name(SANDBOX);
    // A failed earlier run leaves its pods behind.
    for leftover in pods
        .list_pods(&namespace, Some(format!("{LABEL_SANDBOX}={SANDBOX}")), None)
        .await
        .expect("list leftovers")
        .items
    {
        let _ = pods
            .delete_pod(&namespace, leftover.metadata.name.as_deref().unwrap())
            .await;
    }
    let key = ed25519_compact::KeyPair::generate();
    let template_bytes = serde_json::to_vec(&template(&namespace)).expect("template");
    let _ = secrets.delete_secret(&namespace, &secret_name).await;
    secrets
        .create_secret(
            &namespace,
            &Secret {
                metadata: ObjectMeta {
                    name: Some(secret_name.clone()),
                    namespace: Some(namespace.clone()),
                    labels: Some(BTreeMap::from([
                        (LABEL_SANDBOX.to_string(), SANDBOX.to_string()),
                        (SCOPE_KEY.to_string(), SCOPE_VALUE.to_string()),
                    ])),
                    ..Default::default()
                },
                data: Some(BTreeMap::from([
                    (
                        "signingKey".to_string(),
                        k8s_openapi::ByteString(key.sk.as_ref().to_vec()),
                    ),
                    (
                        POD_TEMPLATE_SECRET_KEY.to_string(),
                        k8s_openapi::ByteString(template_bytes),
                    ),
                ])),
                ..Default::default()
            },
        )
        .await
        .expect("capability Secret");

    let now = || chrono::Utc::now().timestamp();
    let ask = |session: &'static str, tenant: Option<&'static str>, timeout: Option<u64>| {
        let (pods, secrets, namespace, secret_name) = (
            pods.clone(),
            secrets.clone(),
            namespace.clone(),
            secret_name.clone(),
        );
        async move {
            find_or_create_session_for_deployment(
                &pods,
                &secrets,
                SANDBOX,
                &namespace,
                session,
                &secret_name,
                chrono::Utc::now().timestamp(),
                SCOPE_KEY,
                SCOPE_VALUE,
                tenant,
                timeout,
            )
            .await
        }
    };

    // R4: with no idle pod at all, a create still succeeds, from the published template.
    let first = ask("s1", Some("tenant-a"), Some(600))
        .await
        .unwrap_or_else(|_| panic!("a create on an empty pool must succeed"));
    assert!(first.created);
    assert_eq!(first.tenant_key.as_deref(), Some("tenant-a"));

    // R2: another process (no shared memory, a fresh lookup) reaches the same session by id and
    // is told it did not create it; the pod carries what the first call wrote.
    let again = ask("s1", Some("tenant-a"), None).await.ok().expect("found");
    assert!(!again.created);
    assert_eq!(again.agent_session_id, first.agent_session_id);
    assert_eq!(again.generation, 1);
    assert_eq!(again.created_at, first.created_at);
    assert!(matches!(
        ask("s1", Some("tenant-b"), None).await,
        Err(SessionRequestError::TenantMismatch)
    ));

    // An unknown id is a clean not-found, not an error.
    let unknown = reattach_session_for_deployment(
        &pods,
        &secrets,
        SANDBOX,
        &namespace,
        "probe-id",
        &secret_name,
        now(),
        SCOPE_KEY,
        SCOPE_VALUE,
    )
    .await
    .expect("lookup works");
    assert!(unknown.is_none());

    // R9: a timeout beyond the declared lifetime is refused; one within it sets the deadline.
    assert!(matches!(
        ask("too-long", None, Some(3601)).await,
        Err(SessionRequestError::LifetimeTooLong(_))
    ));
    let listed = pods
        .list_pods(&namespace, Some(format!("{LABEL_SESSION}=s1")), None)
        .await
        .expect("list");
    let pod = &listed.items[0];
    let annotations = pod.metadata.annotations.as_ref().expect("annotations");
    let deadline: i64 = annotations[ANNOTATION_SESSION_DEADLINE].parse().unwrap();
    assert_eq!(deadline, first.created_at + 600);

    // R6: the pod runs under the pinned non-root uid with a sized, writable session root.
    wait_running(&pods, &namespace, "s1").await;
    let running = pods
        .get_pod(&namespace, pod.metadata.name.as_deref().unwrap())
        .await
        .expect("pod");
    let spec = running.spec.as_ref().unwrap();
    let pod_security = spec.security_context.as_ref().unwrap();
    assert_eq!(pod_security.run_as_non_root, Some(true));
    assert_eq!(pod_security.run_as_user, Some(1001));
    let container = spec.containers[0].security_context.as_ref().unwrap();
    assert_eq!(container.read_only_root_filesystem, Some(true));
    // The process really runs as that uid: the kubelet would have refused a root start.
    assert_eq!(
        running.status.as_ref().and_then(|s| s.phase.as_deref()),
        Some("Running")
    );

    // A warm pod is claimed by label write instead of a second create.
    let mut idle = template(&namespace);
    idle.metadata.name = Some("alien-kind-idle".to_string());
    idle.metadata.generate_name = None;
    pods.create_pod(&namespace, &idle).await.expect("idle pod");
    for _ in 0..60 {
        let idle_now = pods.get_pod(&namespace, "alien-kind-idle").await.unwrap();
        if idle_now
            .status
            .as_ref()
            .and_then(|s| s.pod_ip.clone())
            .is_some()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let warm = ask("warm", None, None).await.ok().expect("warm claim");
    assert!(warm.created);
    let claimed = pods.get_pod(&namespace, "alien-kind-idle").await.unwrap();
    assert_eq!(
        claimed.metadata.labels.as_ref().unwrap()[LABEL_SESSION],
        "warm",
        "the idle pod was claimed, not a new one created"
    );

    // list matches get: both sessions, nothing else.
    let mut listed = list_sessions_for_deployment(
        &pods,
        &secrets,
        SANDBOX,
        &namespace,
        &secret_name,
        SCOPE_KEY,
        SCOPE_VALUE,
    )
    .await
    .expect("list sessions");
    listed.sort();
    assert_eq!(listed, ["s1", "warm"]);

    // R8: a preview capability is minted for a declared port of a session of this deployment and
    // for nothing else. The ports come from the Secret the controller publishes.
    let mut secret = secrets
        .get_secret(&namespace, &secret_name)
        .await
        .expect("secret");
    secret.data.get_or_insert_default().insert(
        super::PREVIEW_PORTS_SECRET_KEY.to_string(),
        k8s_openapi::ByteString(serde_json::to_vec(&[8080u16]).unwrap()),
    );
    secrets
        .update_secret(&namespace, &secret_name, &secret)
        .await
        .expect("publish the declared ports");
    let preview = |session: &'static str, scope: &'static str, port: u16| {
        let (pods, secrets, namespace, secret_name) = (
            pods.clone(),
            secrets.clone(),
            namespace.clone(),
            secret_name.clone(),
        );
        async move {
            preview_session_for_deployment(
                &pods,
                &secrets,
                SANDBOX,
                &namespace,
                session,
                &secret_name,
                chrono::Utc::now().timestamp(),
                SCOPE_KEY,
                scope,
                port,
            )
            .await
        }
    };
    let grant = preview("s1", SCOPE_VALUE, 8080)
        .await
        .ok()
        .expect("a declared port of a live session");
    assert!(
        grant.endpoint.ends_with(":8971/v1/preview/8080"),
        "{}",
        grant.endpoint
    );
    assert!(grant.expires_at <= chrono::Utc::now().timestamp() + 900 + 5);
    assert!(matches!(
        preview("s1", SCOPE_VALUE, 9999).await,
        Err(PreviewError::PortNotDeclared)
    ));
    assert!(matches!(
        preview("s1", SCOPE_VALUE, alien_core::sandbox_image::AGENT_PORT).await,
        Err(PreviewError::PortNotDeclared)
    ));
    // Another deployment's label finds nothing (the Secret's own scope also refuses it).
    assert!(preview("s1", "another-deployment", 8080).await.is_err());
    assert!(matches!(
        preview("nobody", SCOPE_VALUE, 8080).await,
        Err(PreviewError::NotFound)
    ));

    // R7: a new release changes the template. Idle pods built from the old one are retired, but a
    // pod a session has already claimed keeps running, and a claim that lands between the listing
    // and the retire wins: the retire's write carries the listed resourceVersion and conflicts.
    let stale_idle = |name: &str| {
        let mut pod = template(&namespace);
        pod.metadata.name = Some(name.to_string());
        pod.metadata.generate_name = None;
        pod.metadata
            .annotations
            .get_or_insert_default()
            .insert(ANNOTATION_TEMPLATE_HASH.to_string(), "old".to_string());
        pod
    };
    let list_stale = || async {
        let idle = pods
            .list_pods(&namespace, Some(super::idle_selector(SANDBOX)), None)
            .await
            .expect("list idle");
        stale_idle_pods(idle.items, "new", &mut 0)
    };
    let wait_ip = |name: &'static str| {
        let (pods, namespace) = (pods.clone(), namespace.clone());
        async move {
            for _ in 0..60 {
                if pods
                    .get_pod(&namespace, name)
                    .await
                    .unwrap()
                    .status
                    .as_ref()
                    .and_then(|s| s.pod_ip.clone())
                    .is_some()
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            panic!("{name} never got an address");
        }
    };
    pods.create_pod(&namespace, &stale_idle("alien-kind-raced"))
        .await
        .expect("idle pod");
    wait_ip("alien-kind-raced").await;
    let listed_before_claim = list_stale().await;
    assert_eq!(listed_before_claim.len(), 1);
    let racer = ask("racer", None, None).await.ok().expect("claim");
    assert!(racer.created);
    retire_stale_idle_pods(&pods, &namespace, SANDBOX, listed_before_claim).await;
    let raced = pods
        .get_pod(&namespace, "alien-kind-raced")
        .await
        .expect("claimed pod survives");
    assert_eq!(
        raced.metadata.labels.as_ref().unwrap()[LABEL_SESSION],
        "racer"
    );
    assert!(
        raced.metadata.deletion_timestamp.is_none(),
        "a claimed pod is never retired"
    );

    pods.create_pod(&namespace, &stale_idle("alien-kind-stale"))
        .await
        .expect("idle pod");
    wait_ip("alien-kind-stale").await;
    let to_retire = list_stale().await;
    assert_eq!(
        to_retire.len(),
        1,
        "only the idle pod built from the old template"
    );
    retire_stale_idle_pods(&pods, &namespace, SANDBOX, to_retire).await;
    for _ in 0..30 {
        match pods.get_pod(&namespace, "alien-kind-stale").await {
            Err(_) => break,
            Ok(pod) if pod.metadata.deletion_timestamp.is_some() => break,
            Ok(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
        }
    }
    let gone = pods.get_pod(&namespace, "alien-kind-stale").await;
    assert!(
        gone.as_ref()
            .map_or(true, |pod| pod.metadata.deletion_timestamp.is_some()),
        "the stale idle pod is retired"
    );
    for session in ["s1", "warm"] {
        let still = pods
            .list_pods(&namespace, Some(format!("{LABEL_SESSION}={session}")), None)
            .await
            .expect("list");
        assert_eq!(
            still.items.len(),
            1,
            "claimed session {session} survives the roll"
        );
        assert!(still.items[0].metadata.deletion_timestamp.is_none());
    }

    for session in ["s1", "warm", "racer"] {
        release_session_for_deployment(
            &pods,
            &secrets,
            &namespace,
            SANDBOX,
            session,
            &secret_name,
            SCOPE_KEY,
            SCOPE_VALUE,
        )
        .await
        .expect("release");
    }
    let _ = secrets.delete_secret(&namespace, &secret_name).await;
}
