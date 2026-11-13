//! Bounded root boot on a real API server (Kind): what the pod spec admits under Pod Security,
//! and what the agent really does with the capability set it is given.
//!
//! `#[ignore]` because it needs a cluster. The Kind workflow prepares three namespaces and runs
//! it against the root-supervisor contract image with the agent layered on
//! (`alien-sandbox-conformance/contract/layer-agent.sh`) loaded into the node, and a RuntimeClass
//! `alien-kind-runc` whose handler is `runc`:
//!
//! ```text
//! kubectl create namespace sbx-baseline && kubectl label namespace sbx-baseline \
//!   pod-security.kubernetes.io/enforce=baseline
//! kubectl create namespace sbx-restricted && kubectl label namespace sbx-restricted \
//!   pod-security.kubernetes.io/enforce=restricted
//! # The non-root test needs the same image layered with a non-root exec uid:
//! #   layer-agent.sh <agent> <contract-image> <tag> 1001
//! ALIEN_ROOT_BOOT_KIND_CONTEXT=kind-<name> ALIEN_ROOT_BOOT_KIND_IMAGE=<image> \
//!   ALIEN_ROOT_BOOT_KIND_NONROOT_IMAGE=<non-root image> \
//!   ALIEN_ROOT_BOOT_KIND_BASELINE_NAMESPACE=sbx-baseline \
//!   ALIEN_ROOT_BOOT_KIND_RESTRICTED_NAMESPACE=sbx-restricted \
//!   cargo test -p alien-infra --features all-platforms --lib -- --ignored kubernetes_root_boot_kind
//! ```

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Arc;

use alien_core::sandbox_capability::{SandboxCapabilityClaims, SandboxOperationClass};
use alien_core::{
    KubernetesClientConfig, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxLimits,
};
use alien_k8s_clients::kubernetes::kubernetes_client::KubernetesClient;
use alien_k8s_clients::PodApi;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use super::idle_pool_pod_with_labels;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn declared(id: &str, image_user: u32) -> Sandbox {
    let mut sandbox = Sandbox::new(id.to_string())
        .code(SandboxCode::Image {
            image: env("ALIEN_ROOT_BOOT_KIND_IMAGE"),
            bundled_from: None,
        })
        .limits(SandboxLimits {
            cpu: "250m".to_string(),
            memory: "256Mi".to_string(),
            disk: "512Mi".to_string(),
            max_processes: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(600),
            idle_pause_seconds: None,
        })
        .build();
    sandbox.image_user = Some(image_user);
    sandbox
}

fn kubectl(context: &str, args: &[&str]) -> std::process::Output {
    Command::new("kubectl")
        .arg("--context")
        .arg(context)
        .args(args)
        .output()
        .expect("kubectl")
}

async fn client(context: &str) -> Arc<dyn PodApi> {
    let resolved = crate::kubeconfig::resolve_kubeconfig(&KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: std::env::var("KUBECONFIG").ok(),
        context: Some(context.to_string()),
        cluster: None,
        user: None,
        namespace: None,
        additional_headers: None,
    })
    .await
    .expect("resolved kubeconfig");
    Arc::new(KubernetesClient::new(resolved).await.expect("client"))
}

/// What a running pod's agent reports about a command it ran, read the way a caller would: a
/// signed capability, the agent's own `/v1/exec`, from inside the pod's network namespace.
fn run_through_agent(
    context: &str,
    namespace: &str,
    pod: &str,
    secret_key: &ed25519_compact::SecretKey,
    session: &str,
    script: &str,
) -> String {
    let claims = SandboxCapabilityClaims {
        session_id: session.to_string(),
        operation: SandboxOperationClass::Execute,
        generation: 1,
        expires_at: chrono::Utc::now().timestamp() + 300,
        key_id: "kind".to_string(),
        port: None,
    };
    let token = alien_core::sandbox_capability_token::mint(&claims, secret_key).expect("token");
    let request = serde_json::json!({
        "command": ["sh", "-c", script],
        "timeoutMs": 10_000,
    })
    .to_string();
    let client = r#"
import base64, json, sys, urllib.request
token, body = sys.argv[1], sys.argv[2].encode()
request = urllib.request.Request(
    "http://127.0.0.1:8971/v1/exec", data=body,
    headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
out = b""
for line in urllib.request.urlopen(request, timeout=20).read().splitlines():
    frame = json.loads(line)
    if frame.get("t") == "stdout":
        out += base64.b64decode(frame["data"])
    elif frame.get("t") == "stderr":
        out += b"stderr: " + base64.b64decode(frame["data"])
    elif frame.get("t") in ("error", "exit") and frame.get("code"):
        out += ("frame: %s\n" % json.dumps(frame)).encode()
sys.stdout.write(out.decode())
"#;
    let output = kubectl(
        context,
        &[
            "exec", "-n", namespace, pod, "--", "python3", "-c", client, &token, &request,
        ],
    );
    assert!(
        output.status.success(),
        "exec through the agent failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn field(status: &str, name: &str) -> String {
    status
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}:")))
        .unwrap_or_else(|| panic!("{name} missing from:\n{status}"))
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

async fn start(
    pods: &Arc<dyn PodApi>,
    context: &str,
    namespace: &str,
    sandbox: &Sandbox,
    public_key: &str,
) -> Result<(String, String), alien_error::AlienError<alien_client_core::ErrorData>> {
    let mut pod = idle_pool_pod_with_labels(
        sandbox,
        namespace,
        "alien-kind-runc",
        None,
        Some(public_key),
        BTreeMap::from([(
            "alien.dev/deployment".to_string(),
            "root-boot-kind".to_string(),
        )]),
    );
    pod.metadata.name = Some(format!("{}-{:x}", sandbox.id, rand::random::<u32>()));
    let created = pods.create_pod(namespace, &pod).await?;
    let name = created.metadata.name.clone().unwrap();
    let uid = created.metadata.uid.clone().unwrap();
    for _ in 0..90 {
        let phase = kubectl(
            context,
            &[
                "get",
                "pod",
                "-n",
                namespace,
                &name,
                "-o",
                "jsonpath={.status.phase}",
            ],
        );
        if String::from_utf8_lossy(&phase.stdout) == "Running" {
            return Ok((name, uid));
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!("pod {name} never ran");
}

#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_root_boot_kind_boots_root_with_the_bounded_set_and_drops_commands() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let baseline = env("ALIEN_ROOT_BOOT_KIND_BASELINE_NAMESPACE");
    let pods = client(&context).await;
    let pair = ed25519_compact::KeyPair::generate();
    let public_key = BASE64.encode(pair.pk.as_ref());

    let sandbox = declared("rootboot", 0);
    let (pod, uid) = start(&pods, &context, &baseline, &sandbox, &public_key)
        .await
        .expect("a root-boot pod is admitted where Pod Security is baseline");

    // The image's ENTRYPOINT ran once, as root, with exactly the boot capability set.
    let mut boots = String::new();
    for _ in 0..30 {
        let read = kubectl(
            &context,
            &[
                "exec",
                "-n",
                &baseline,
                &pod,
                "--",
                "cat",
                "/tmp/alien-contract/boots.jsonl",
            ],
        );
        boots = String::from_utf8_lossy(&read.stdout).to_string();
        if !boots.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let lines: Vec<serde_json::Value> = boots
        .lines()
        .map(|line| serde_json::from_str(line).expect("boot record"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "the ENTRYPOINT starts once per boot: {boots}"
    );
    assert_eq!(lines[0]["uid"], 0);
    assert_eq!(
        lines[0]["capabilities"]["CapEff"],
        u64::from(alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_MASK)
    );

    // A command run through the agent is the fixed non-root uid with nothing left.
    let status = run_through_agent(
        &context,
        &baseline,
        &pod,
        &pair.sk,
        &uid,
        "grep -E '^(Uid|CapEff|NoNewPrivs):' /proc/self/status",
    );
    assert_eq!(field(&status, "Uid"), "1000", "{status}");
    assert_eq!(field(&status, "CapEff"), "0000000000000000", "{status}");
    assert_eq!(field(&status, "NoNewPrivs"), "1", "{status}");
    let _ = kubectl(
        &context,
        &["delete", "pod", "-n", &baseline, &pod, "--wait=false"],
    );
}

#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_root_boot_kind_a_restricted_namespace_refuses_with_the_fix() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let restricted = env("ALIEN_ROOT_BOOT_KIND_RESTRICTED_NAMESPACE");
    let pods = client(&context).await;
    let sandbox = declared("rootrefused", 0);
    let refusal = start(&pods, &context, &restricted, &sandbox, "AAAA")
        .await
        .expect_err("a restricted namespace refuses a root-boot pod");
    let message = super::kubernetes::pod_security_rejection("rootrefused", &restricted, &refusal)
        .unwrap_or_else(|| panic!("not recognised as Pod Security: {refusal:?}"));
    assert!(message.contains("sandbox.namespace"), "{message}");
    assert!(message.contains(&restricted), "{message}");
}

#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_root_boot_kind_a_non_root_image_still_runs_restricted() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let restricted = env("ALIEN_ROOT_BOOT_KIND_RESTRICTED_NAMESPACE");
    let pods = client(&context).await;
    let pair = ed25519_compact::KeyPair::generate();
    let public_key = BASE64.encode(pair.pk.as_ref());
    let mut sandbox = declared("nonroot", 1001);
    sandbox.code = SandboxCode::Image {
        image: env("ALIEN_ROOT_BOOT_KIND_NONROOT_IMAGE"),
        bundled_from: None,
    };
    let (pod, uid) = start(&pods, &context, &restricted, &sandbox, &public_key)
        .await
        .expect("a non-root image is admitted where Pod Security is restricted");
    let status = run_through_agent(
        &context,
        &restricted,
        &pod,
        &pair.sk,
        &uid,
        "grep -E '^(Uid|CapEff|NoNewPrivs):' /proc/self/status",
    );
    assert_eq!(field(&status, "Uid"), "1001", "{status}");
    assert_eq!(field(&status, "CapEff"), "0000000000000000", "{status}");
    assert_eq!(field(&status, "NoNewPrivs"), "1", "{status}");
    let _ = kubectl(
        &context,
        &["delete", "pod", "-n", &restricted, &pod, "--wait=false"],
    );
}
