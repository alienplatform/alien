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
                false,
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
        chrono::Utc::now().timestamp(),
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

fn kubectl(arguments: &[&str]) -> String {
    let output = std::process::Command::new("kubectl")
        .args(arguments)
        .output()
        .expect("run kubectl");
    assert!(
        output.status.success(),
        "kubectl {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// The real agent in a real pod serves a preview capability for its declared port, and for nothing
/// else. The capability is signed the way the broker signs one; the pod gets the environment the
/// controller gives a sandbox pod, and a second pod stands in for a workload that links it.
///
/// Needs the agent binary (`cargo build -p alien-sandbox-agent`) at `ALIEN_AGENT_BINARY` and a
/// Debian-based image the cluster can pull. `kubectl` must point at the Kind cluster.
#[tokio::test]
#[ignore = "needs a Kind cluster and the agent binary; see the module docs"]
async fn kubernetes_broker_kind_preview_reaches_only_a_declared_port() {
    use alien_core::sandbox_capability::{SandboxCapabilityClaims, SandboxOperationClass};
    use base64::Engine as _;

    let namespace = env("ALIEN_BROKER_KIND_NAMESPACE");
    let binary = env("ALIEN_AGENT_BINARY");
    let keys = ed25519_compact::KeyPair::generate();
    let public_key = base64::engine::general_purpose::STANDARD.encode(keys.pk.as_ref());
    let image = "python:3.12-slim";
    for name in ["preview-agent", "preview-client"] {
        let _ = std::process::Command::new("kubectl")
            .args([
                "--namespace",
                &namespace,
                "delete",
                "pod",
                name,
                "--ignore-not-found",
                "--wait=true",
            ])
            .output();
    }
    let pod = |name: &str, command: &str, environment: &str| {
        format!(
            r#"apiVersion: v1
kind: Pod
metadata: {{name: {name}, namespace: {namespace}}}
spec:
  securityContext: {{runAsUser: 1001, runAsGroup: 1001, runAsNonRoot: true}}
  containers:
    - name: p
      image: {image}
      command: ["sh", "-c", "{command}"]
      env: [{environment}]
"#
        )
    };
    let apply = |manifest: String| {
        use std::io::Write;
        let mut child = std::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("kubectl apply");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(manifest.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success(), "apply failed:\n{manifest}");
    };
    // The same names and values the controller puts on a sandbox pod, with a preview port.
    let agent_env = format!(
        "{{name: ALIEN_SANDBOX_AUTHORIZATION, value: capability}}, \
         {{name: ALIEN_SANDBOX_PUBLIC_KEY, value: \"{public_key}\"}}, \
         {{name: ALIEN_SANDBOX_SESSION_ID, valueFrom: {{fieldRef: {{fieldPath: metadata.uid}}}}}}, \
         {{name: ALIEN_SANDBOX_GENERATION, value: \"1\"}}, \
         {{name: ALIEN_SANDBOX_PREVIEW_PORTS, value: \"8080\"}}, \
         {{name: ALIEN_SANDBOX_PORT, value: \"8971\"}}, \
         {{name: ALIEN_SANDBOX_ISOLATION, value: platform}}, \
         {{name: ALIEN_SANDBOX_EXEC_UID, value: \"1001\"}}, \
         {{name: ALIEN_SANDBOX_EXEC_GID, value: \"1001\"}}, \
         {{name: ALIEN_SANDBOX_ROOT, value: /tmp/sbx}}"
    );
    apply(pod(
        "preview-agent",
        "mkdir -p /tmp/sbx; echo hello-from-the-sandbox > /tmp/index.html; cd /tmp; python3 -m http.server 8080 --bind 127.0.0.1 & python3 -m http.server 9090 --bind 127.0.0.1 & sleep 3600",
        &agent_env,
    ));
    apply(pod("preview-client", "sleep 3600", ""));
    for name in ["preview-agent", "preview-client"] {
        kubectl(&[
            "--namespace",
            &namespace,
            "wait",
            "--for=condition=Ready",
            &format!("pod/{name}"),
            "--timeout=240s",
        ]);
    }
    kubectl(&[
        "--namespace",
        &namespace,
        "cp",
        &binary,
        "preview-agent:/tmp/agent",
    ]);
    kubectl(&[
        "--namespace",
        &namespace,
        "exec",
        "preview-agent",
        "--",
        "sh",
        "-c",
        "chmod +x /tmp/agent; (/tmp/agent >/tmp/agent.log 2>&1 &); sleep 2",
    ]);
    let uid = kubectl(&[
        "--namespace",
        &namespace,
        "get",
        "pod",
        "preview-agent",
        "-o",
        "jsonpath={.metadata.uid}",
    ]);
    let ip = kubectl(&[
        "--namespace",
        &namespace,
        "get",
        "pod",
        "preview-agent",
        "-o",
        "jsonpath={.status.podIP}",
    ]);

    let token =
        |session: &str, class: SandboxOperationClass, port: Option<u16>, expires_in: i64| {
            alien_core::sandbox_capability_token::mint(
                &SandboxCapabilityClaims {
                    session_id: session.to_string(),
                    operation: class,
                    generation: 1,
                    expires_at: chrono::Utc::now().timestamp() + expires_in,
                    key_id: "kind".to_string(),
                    port,
                },
                &keys.sk,
            )
            .unwrap()
        };
    // What a linking workload does: call the agent's address with the capability.
    let call = |path: &str, token: &str| -> String {
        kubectl(&[
            "--namespace", &namespace, "exec", "preview-client", "--", "python3", "-c",
            &format!(
                "import urllib.request,urllib.error\n\
                 r=urllib.request.Request('http://{ip}:8971{path}',headers={{'Authorization':'Bearer {token}'}})\n\
                 try:\n  x=urllib.request.urlopen(r,timeout=10); print(x.status, x.read().decode().strip())\n\
                 except urllib.error.HTTPError as e: print(e.code)"
            ),
        ])
        .trim()
        .to_string()
    };
    let preview = SandboxOperationClass::Preview;
    let good = token(&uid, preview, Some(8080), 600);
    assert_eq!(
        call("/v1/preview/8080/index.html", &good),
        "200 hello-from-the-sandbox"
    );
    // A capability for another pod, for an undeclared port (a server really listens on 9090), for
    // the agent's own port, as a shell capability, and an expired one.
    for (label, refused) in [
        (
            "another pod's token",
            call(
                "/v1/preview/8080/",
                &token("another-pod", preview, Some(8080), 600),
            ),
        ),
        (
            "a port the sandbox did not declare",
            call("/v1/preview/9090/", &token(&uid, preview, Some(9090), 600)),
        ),
        (
            "the agent's own port",
            call("/v1/preview/8971/", &token(&uid, preview, Some(8971), 600)),
        ),
        (
            "a token for another port",
            call("/v1/preview/8080/", &token(&uid, preview, Some(9090), 600)),
        ),
        (
            "an execute token",
            call(
                "/v1/preview/8080/",
                &token(&uid, SandboxOperationClass::Execute, None, 600),
            ),
        ),
        (
            "an expired token",
            call("/v1/preview/8080/", &token(&uid, preview, Some(8080), -5)),
        ),
        ("no token", call("/v1/preview/8080/", "")),
    ] {
        assert!(
            ["401", "403"].contains(&refused.as_str()),
            "{label} must be refused: {refused}"
        );
    }
    let _ = good;
    // Clean up.
    for name in ["preview-agent"] {
        kubectl(&[
            "--namespace",
            &namespace,
            "delete",
            "pod",
            name,
            "--wait=false",
        ]);
    }
    for name in ["preview-client"] {
        kubectl(&[
            "--namespace",
            &namespace,
            "delete",
            "pod",
            name,
            "--wait=false",
        ]);
    }
}

/// A source-built sandbox on Kubernetes: the pod runs as the base image's non-root `USER`
/// (a `python:3.12-slim` base with `USER 65534`), and the agent starts with exactly the
/// environment the layered image carries and runs a command. The agent used to exit at start
/// when the image also carried `ALIEN_SANDBOX_IMAGE_COMMAND`, which it allows only for a root
/// agent with uid-split isolation.
#[tokio::test]
#[ignore = "needs a Kind cluster and the agent binary; see the module docs"]
async fn kubernetes_broker_kind_source_sandbox_runs_as_the_images_user() {
    use alien_core::sandbox_capability::{SandboxCapabilityClaims, SandboxOperationClass};
    use base64::Engine as _;

    let namespace = env("ALIEN_BROKER_KIND_NAMESPACE");
    let binary = env("ALIEN_AGENT_BINARY");
    let keys = ed25519_compact::KeyPair::generate();
    let public_key = base64::engine::general_purpose::STANDARD.encode(keys.pk.as_ref());
    let _ = std::process::Command::new("kubectl")
        .args([
            "--namespace",
            &namespace,
            "delete",
            "pod",
            "source-agent",
            "--ignore-not-found",
            "--wait=true",
        ])
        .output();
    // The contract the layered image sets, with the exec identity the base's USER replaces.
    let mut contract = alien_core::sandbox_image::KUBERNETES_SANDBOX
        .contract_env_vars()
        .to_vec();
    for (name, value) in contract.iter_mut() {
        match *name {
            "ALIEN_SANDBOX_EXEC_UID" | "ALIEN_SANDBOX_EXEC_GID" => *value = "65534".into(),
            "ALIEN_SANDBOX_ROOT" => *value = "/tmp/sbx".into(),
            _ => {}
        }
    }
    let environment = contract
        .iter()
        .map(|(name, value)| format!("{{name: {name}, value: \"{value}\"}}"))
        .chain([
            format!("{{name: ALIEN_SANDBOX_PUBLIC_KEY, value: \"{public_key}\"}}"),
            "{name: ALIEN_SANDBOX_SESSION_ID, valueFrom: {fieldRef: {fieldPath: metadata.uid}}}"
                .to_string(),
            "{name: ALIEN_SANDBOX_GENERATION, value: \"1\"}".to_string(),
        ])
        .collect::<Vec<_>>()
        .join(", ");
    let manifest = format!(
        r#"apiVersion: v1
kind: Pod
metadata: {{name: source-agent, namespace: {namespace}}}
spec:
  securityContext: {{runAsUser: 65534, runAsGroup: 65534, runAsNonRoot: true}}
  containers:
    - name: p
      image: python:3.12-slim
      command: ["sh", "-c", "mkdir -p /tmp/sbx; sleep 3600"]
      env: [{environment}]
"#
    );
    {
        use std::io::Write;
        let mut child = std::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("kubectl apply");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(manifest.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success(), "apply failed:\n{manifest}");
    }
    kubectl(&[
        "--namespace",
        &namespace,
        "wait",
        "--for=condition=Ready",
        "pod/source-agent",
        "--timeout=240s",
    ]);
    kubectl(&[
        "--namespace",
        &namespace,
        "cp",
        &binary,
        "source-agent:/tmp/agent",
    ]);
    // The agent runs under the pod's identity and its exit would show here.
    kubectl(&[
        "--namespace",
        &namespace,
        "exec",
        "source-agent",
        "--",
        "sh",
        "-c",
        "chmod +x /tmp/agent; (/tmp/agent >/tmp/agent.log 2>&1 &); sleep 2",
    ]);
    let uid = kubectl(&[
        "--namespace",
        &namespace,
        "get",
        "pod",
        "source-agent",
        "-o",
        "jsonpath={.metadata.uid}",
    ]);
    let token = alien_core::sandbox_capability_token::mint(
        &SandboxCapabilityClaims {
            session_id: uid.clone(),
            operation: SandboxOperationClass::Execute,
            generation: 1,
            expires_at: chrono::Utc::now().timestamp() + 600,
            key_id: "kind".to_string(),
            port: None,
        },
        &keys.sk,
    )
    .unwrap();
    let output = kubectl(&[
        "--namespace", &namespace, "exec", "source-agent", "--", "python3", "-c",
        &format!(
            "import urllib.request,json\n\
             r=urllib.request.Request('http://127.0.0.1:8971/v1/exec',data=json.dumps({{'command':['python3','-c','import os;print(1,os.getuid())'],'timeoutMs':20000}}).encode(),headers={{'Authorization':'Bearer {token}','Content-Type':'application/json'}})\n\
             print(urllib.request.urlopen(r,timeout=30).read().decode())"
        ),
    ]);
    // Output frames carry base64 data.
    let printed = output
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|frame| frame["t"] == "stdout")
        .filter_map(|frame| {
            base64::engine::general_purpose::STANDARD
                .decode(frame["data"].as_str()?)
                .ok()
        })
        .flatten()
        .collect::<Vec<u8>>();
    assert_eq!(
        String::from_utf8_lossy(&printed).trim(),
        "1 65534",
        "the command ran as the image's user: {output}"
    );
    kubectl(&[
        "--namespace",
        &namespace,
        "delete",
        "pod",
        "source-agent",
        "--wait=false",
    ]);
}

/// What the API server says about the two tokens a linking pod holds: the one projected for the
/// broker is accepted for the broker's audience, and the default one the kubelet mounts (issued
/// for the API server) is not. The pod's spec is the one the controllers build.
#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_broker_kind_token_audience_is_enforced() {
    use super::kubernetes_broker_token::{project_into, BROKER_AUDIENCE, TOKEN_PATH};
    use alien_core::ResourceRef;
    use alien_k8s_clients::kubernetes::token_reviews::{
        authenticated_for_audience, authenticated_user, review_for_audience, TokenReviewsApi,
    };

    let namespace = env("ALIEN_BROKER_KIND_NAMESPACE");
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
    let client = KubernetesClient::new(resolved)
        .await
        .expect("kubernetes client");

    let mut spec = k8s_openapi::api::core::v1::PodSpec {
        containers: vec![k8s_openapi::api::core::v1::Container {
            name: "p".to_string(),
            image: Some("alpine:3.20".to_string()),
            command: Some(vec!["sleep".to_string(), "600".to_string()]),
            ..Default::default()
        }],
        ..Default::default()
    };
    project_into(
        &mut spec,
        &[ResourceRef::new(
            alien_core::Sandbox::RESOURCE_TYPE,
            "agent",
        )],
    );
    let pod = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "token-audience", "namespace": namespace},
        "spec": spec,
    });
    let _ = std::process::Command::new("kubectl")
        .args([
            "--namespace",
            &namespace,
            "delete",
            "pod",
            "token-audience",
            "--ignore-not-found",
            "--wait=true",
        ])
        .output();
    {
        use std::io::Write;
        let mut child = std::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("kubectl apply");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(pod.to_string().as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }
    kubectl(&[
        "--namespace",
        &namespace,
        "wait",
        "--for=condition=Ready",
        "pod/token-audience",
        "--timeout=120s",
    ]);
    let read = |path: &str| {
        kubectl(&[
            "--namespace",
            &namespace,
            "exec",
            "token-audience",
            "--",
            "cat",
            path,
        ])
        .trim()
        .to_string()
    };
    let projected = read(TOKEN_PATH);
    let default = read("/var/run/secrets/kubernetes.io/serviceaccount/token");
    assert!(!projected.is_empty() && !default.is_empty() && projected != default);

    let review = |token: &str| {
        let client = &client;
        let review = review_for_audience(token, BROKER_AUDIENCE);
        async move { client.create_token_review(&review).await.expect("review") }
    };
    let verdict = review(&projected).await;
    assert!(
        authenticated_for_audience(&verdict, BROKER_AUDIENCE),
        "the projected token is for the broker: {verdict:?}"
    );
    assert_eq!(
        authenticated_user(&verdict).as_deref(),
        Some(format!("system:serviceaccount:{namespace}:default").as_str())
    );
    let refused = review(&default).await;
    assert!(
        !authenticated_for_audience(&refused, BROKER_AUDIENCE),
        "the default ServiceAccount token is not for the broker: {refused:?}"
    );
    let _ = std::process::Command::new("kubectl")
        .args([
            "--namespace",
            &namespace,
            "delete",
            "pod",
            "token-audience",
            "--wait=false",
        ])
        .output();
}

/// A pod is Ready only once its NetworkPolicy is proven in force: the real agent is given a
/// listening pod as the address the policy must refuse. With no policy the pod can reach it and
/// `/v1/network-ready` stays 503; once a deny-all egress policy selects the pod, the connection is
/// dropped and it answers 200. Needs a CNI that enforces NetworkPolicy, as the Kind workflow's.
#[tokio::test]
#[ignore = "needs a Kind cluster that enforces NetworkPolicy and the agent binary; see the module docs"]
async fn kubernetes_broker_kind_pod_is_ready_only_once_its_policy_is_enforced() {
    use std::io::Write;
    let namespace = env("ALIEN_BROKER_KIND_NAMESPACE");
    let binary = env("ALIEN_AGENT_BINARY");
    let ns = ["--namespace", namespace.as_str()];
    let kubectl_ns = |rest: &[&str]| {
        let mut all = ns.to_vec();
        all.extend_from_slice(rest);
        kubectl(&all)
    };
    let apply = |manifest: &str| {
        let mut child = std::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("kubectl apply");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(manifest.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success(), "apply failed:\n{manifest}");
    };
    for pod in ["gate-listener", "gate-agent"] {
        kubectl_ns(&["delete", "pod", pod, "--ignore-not-found", "--wait=true"]);
    }
    kubectl_ns(&[
        "delete",
        "networkpolicy",
        "gate-agent-deny",
        "--ignore-not-found",
    ]);

    apply(&format!(
        r#"apiVersion: v1
kind: Pod
metadata: {{name: gate-listener, namespace: {namespace}}}
spec:
  containers:
    - name: p
      image: python:3.12-slim
      command: ["python3", "-m", "http.server", "8080"]
"#
    ));
    kubectl_ns(&[
        "wait",
        "--for=condition=Ready",
        "pod/gate-listener",
        "--timeout=240s",
    ]);
    let listener_ip = kubectl_ns(&[
        "get",
        "pod",
        "gate-listener",
        "-o",
        "jsonpath={.status.podIP}",
    ]);

    // The pod applied is the one the code under test builds, readiness probe included: only the
    // image, the command and the environment a layered image would carry are filled in.
    let mut pod = template(&namespace);
    super::kubernetes_spec::require_enforced_policy(&mut pod, Some(&format!("{listener_ip}:8080")));
    {
        let mut contract = alien_core::sandbox_image::KUBERNETES_SANDBOX
            .contract_env_vars()
            .to_vec();
        for (name, value) in contract.iter_mut() {
            match *name {
                "ALIEN_SANDBOX_EXEC_UID" | "ALIEN_SANDBOX_EXEC_GID" => *value = "1001".into(),
                "ALIEN_SANDBOX_ROOT" => *value = "/tmp/sbx".into(),
                _ => {}
            }
        }
        let public_key = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .encode(ed25519_compact::KeyPair::generate().pk.as_ref())
        };
        let spec = pod.spec.as_mut().unwrap();
        let container = &mut spec.containers[0];
        assert!(
            container.readiness_probe.is_some(),
            "the gate adds a readiness probe"
        );
        assert_eq!(
            container
                .readiness_probe
                .as_ref()
                .and_then(|probe| probe.http_get.as_ref())
                .and_then(|get| get.path.as_deref()),
            Some("/v1/network-ready")
        );
        container.image = Some("python:3.12-slim".into());
        container.image_pull_policy = Some("IfNotPresent".into());
        container.command = Some(vec![
            "sh".into(),
            "-c".into(),
            "mkdir -p /tmp/sbx; sleep 3600".into(),
        ]);
        let env = container.env.get_or_insert_default();
        let mut add = |name: &str, value: String| {
            env.retain(|var| var.name != name);
            env.push(k8s_openapi::api::core::v1::EnvVar {
                name: name.to_string(),
                value: Some(value),
                value_from: None,
            });
        };
        for (name, value) in contract {
            add(name, value.to_string());
        }
        add("ALIEN_SANDBOX_PUBLIC_KEY", public_key);
        add("ALIEN_SANDBOX_GENERATION", "1".into());
        // The agent identifies itself by the pod uid.
        if !env.iter().any(|var| var.name == "ALIEN_SANDBOX_SESSION_ID") {
            env.push(k8s_openapi::api::core::v1::EnvVar {
                name: "ALIEN_SANDBOX_SESSION_ID".into(),
                value: None,
                value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                    field_ref: Some(k8s_openapi::api::core::v1::ObjectFieldSelector {
                        field_path: "metadata.uid".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            });
        }
    }
    pod.metadata.name = Some("gate-agent".into());
    pod.metadata.generate_name = None;
    pod.metadata.namespace = Some(namespace.clone());
    pod.metadata
        .labels
        .get_or_insert_default()
        .insert("gate".into(), "agent".into());
    pod.metadata.annotations = None;
    apply(&serde_json::to_string(&pod).unwrap());
    kubectl_ns(&[
        "wait",
        "--for=jsonpath={.status.phase}=Running",
        "pod/gate-agent",
        "--timeout=240s",
    ]);
    let ready = || {
        kubectl_ns(&[
            "get",
            "pod",
            "gate-agent",
            "-o",
            "jsonpath={.status.conditions[?(@.type==\"Ready\")].status}",
        ])
    };
    kubectl_ns(&["cp", &binary, "gate-agent:/tmp/agent"]);
    kubectl_ns(&[
        "exec",
        "gate-agent",
        "--",
        "sh",
        "-c",
        "chmod +x /tmp/agent; (/tmp/agent >/tmp/agent.log 2>&1 &); sleep 2",
    ]);
    // No policy: the listener answers, so the policy is not in force and kubelet's probe fails.
    for _ in 0..6 {
        assert_eq!(ready(), "False", "an unrestricted pod must not be Ready");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    apply(&format!(
        r#"apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: {{name: gate-agent-deny, namespace: {namespace}}}
spec:
  podSelector: {{matchLabels: {{gate: agent}}}}
  policyTypes: [Egress]
"#
    ));
    let mut became_ready = false;
    for _ in 0..40 {
        if ready() == "True" {
            became_ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    assert!(
        became_ready,
        "Ready once the deny-all egress policy is enforced"
    );

    kubectl_ns(&[
        "delete",
        "pod",
        "gate-agent",
        "gate-listener",
        "--wait=false",
    ]);
    kubectl_ns(&["delete", "networkpolicy", "gate-agent-deny"]);
}
