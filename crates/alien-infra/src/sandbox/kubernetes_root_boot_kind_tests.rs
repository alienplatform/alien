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
//! # On a cluster that enforces NetworkPolicy (the startup gate needs it), also set
//! # ALIEN_ROOT_BOOT_KIND_ENFORCES_POLICY=1: the tests attach a deny policy to their pods.
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
    if std::env::var_os("ALIEN_ROOT_BOOT_KIND_ENFORCES_POLICY").is_some() {
        // The startup gate holds the ENTRYPOINT back until the cluster really drops egress, so on
        // a cluster that enforces NetworkPolicy the pod needs the deny policy the Operator would
        // attach. Deny all egress and ingress for these pods.
        let policy = format!(
            r#"{{"apiVersion":"networking.k8s.io/v1","kind":"NetworkPolicy","metadata":{{"name":"root-boot-kind-deny","namespace":"{namespace}"}},"spec":{{"podSelector":{{"matchLabels":{{"alien.dev/deployment":"root-boot-kind"}}}},"policyTypes":["Egress","Ingress"]}}}}"#
        );
        let applied = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "printf '%s' '{policy}' | kubectl --context {context} apply -f -"
            ))
            .output()
            .expect("kubectl apply");
        assert!(applied.status.success(), "deny policy applied");
    }
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

/// Run as root inside the pod (the same user and capability set as the image's ENTRYPOINT), this
/// tries everything that would let image code replace the next boot's agent or its metadata.
const ATTACK: &str = r#"
import ctypes, errno, os, sys
agent = "/usr/local/bin/alien-sandbox-agent"
meta = "/opt/alien"
assert os.getuid() == 0
libc = ctypes.CDLL(None, use_errno=True)
fails = []
def refused(name, call, allowed=(errno.EROFS, errno.EPERM, errno.EBUSY, errno.EACCES, errno.ETXTBSY)):
    try:
        result = call()
    except OSError as error:
        if error.errno not in allowed:
            fails.append(f"{name}: unexpected errno {error.errno}")
        return
    if result not in (None, -1):
        fails.append(f"{name}: succeeded")
    elif result == -1 and ctypes.get_errno() not in allowed:
        fails.append(f"{name}: unexpected errno {ctypes.get_errno()}")
    elif result is None:
        fails.append(f"{name}: succeeded")
def mount(source, target, fstype, flags):
    return libc.mount(source, target, fstype, flags, None)
MS_REMOUNT, MS_BIND = 32, 4096
refused("write agent", lambda: open(agent, "wb").close())
refused("append agent", lambda: open(agent, "ab").write(b"x"))
refused("truncate agent", lambda: os.truncate(agent, 0))
refused("chmod agent", lambda: os.chmod(agent, 0o777))
refused("rename agent", lambda: os.rename(agent, agent + ".old"))
refused("unlink agent", lambda: os.unlink(agent))
refused("create under /opt/alien", lambda: open(meta + "/new", "w").close())
refused("delete under /opt/alien", lambda: os.unlink(meta + "/image-command.json"))
refused("rename under /opt/alien", lambda: os.rename(meta + "/image-command.json", meta + "/x"))
refused("remount agent rw", lambda: mount(None, agent.encode(), None, MS_REMOUNT | MS_BIND))
refused("remount meta rw", lambda: mount(None, meta.encode(), None, MS_REMOUNT | MS_BIND))
refused("umount agent", lambda: libc.umount(agent.encode()))
refused("umount meta", lambda: libc.umount(meta.encode()))
refused("bind over agent", lambda: mount(b"/tmp", agent.encode(), None, MS_BIND))
refused("bind over meta", lambda: mount(b"/tmp", meta.encode(), None, MS_BIND))
# The agent is PID 1 of the container; a same-uid root without SYS_PTRACE must not attach.
PTRACE_ATTACH = 16
refused("ptrace attach to the agent", lambda: libc.ptrace(PTRACE_ATTACH, 1, 0, 0))
try:
    os.open("/proc/1/mem", os.O_RDWR)
    fails.append("opened the agent's /proc/1/mem")
except OSError:
    pass
print("FAILS:" + ";".join(fails) if fails else "ALL REFUSED")
"#;

#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_root_boot_kind_root_image_code_cannot_replace_the_agent_or_its_metadata() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let baseline = env("ALIEN_ROOT_BOOT_KIND_BASELINE_NAMESPACE");
    let pods = client(&context).await;
    let pair = ed25519_compact::KeyPair::generate();
    let public_key = BASE64.encode(pair.pk.as_ref());
    let sandbox = declared("rootprotect", 0);
    let (pod, _) = start(&pods, &context, &baseline, &sandbox, &public_key)
        .await
        .expect("a root-boot pod is admitted where Pod Security is baseline");
    let output = kubectl(
        &context,
        &["exec", "-n", &baseline, &pod, "--", "python3", "-c", ATTACK],
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let _ = kubectl(
        &context,
        &["delete", "pod", "-n", &baseline, &pod, "--wait=false"],
    );
    assert!(
        stdout.contains("ALL REFUSED"),
        "root image code changed the agent or its metadata: {stdout} {stderr}"
    );
}

/// The three preview ports of a root-supervisor workload, over HTTP and WebSocket, through the
/// agent's preview route with a signed capability. The workload checks its own bearer token, so
/// the caller's `Authorization` must reach it unchanged, and a 1 MiB WebSocket message must
/// round-trip. The capability for one port is refused on another.
#[tokio::test]
#[ignore = "needs a Kind cluster; see the module docs"]
async fn kubernetes_root_boot_kind_preview_serves_three_ports_over_http_and_websocket() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let baseline = env("ALIEN_ROOT_BOOT_KIND_BASELINE_NAMESPACE");
    let pods = client(&context).await;
    let pair = ed25519_compact::KeyPair::generate();
    let public_key = BASE64.encode(pair.pk.as_ref());

    let mut sandbox = declared("rootpreview", 0);
    sandbox.preview_ports = vec![8080, 8081, 8090];
    let (pod, uid) = start(&pods, &context, &baseline, &sandbox, &public_key)
        .await
        .expect("a root-boot pod is admitted where Pod Security is baseline");

    let token = |port: u16| {
        alien_core::sandbox_capability_token::mint(
            &SandboxCapabilityClaims {
                session_id: uid.clone(),
                operation: SandboxOperationClass::Preview,
                generation: 1,
                expires_at: chrono::Utc::now().timestamp() + 300,
                key_id: "kind".to_string(),
                port: Some(port),
            },
            &pair.sk,
        )
        .expect("token")
    };
    // The caller: HTTP `/whoami`, a 1 MiB WebSocket echo, then HTTP without the daemon's bearer.
    let client_script = r#"
import base64, hashlib, json, os, socket, struct, sys, time, urllib.request, urllib.error
port, capability = int(sys.argv[1]), sys.argv[2]
bearer = "Bearer conformance-bearer-token"
base = "127.0.0.1"
headers = {"X-Alien-Preview-Capability": capability, "X-Alien-Preview-Port": str(port),
           "Authorization": bearer}
def get(extra=None):
    h = dict(headers); h.update(extra or {})
    try:
        r = urllib.request.urlopen(urllib.request.Request(
            "http://%s:8971/v1/preview/%d/whoami" % (base, port), headers=h), timeout=20)
        return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, ""
# The startup egress gate runs before the agent listens; wait for the preview port.
deadline = time.time() + 90
while True:
    try:
        socket.create_connection((base, 8971), timeout=2).close()
        break
    except OSError:
        if time.time() > deadline:
            raise
        time.sleep(1)
# The workload starts after the agent listens: 502 until its port is open.
while True:
    status, body = get()
    if status != 502 or time.time() > deadline:
        break
    time.sleep(1)
out = {"http": status}
if status == 200:
    seen = json.loads(body)
    out["port"] = seen["port"]
    out["uid"] = seen["uid"]
    out["bearerUnchanged"] = seen["authorizationSha256"] == hashlib.sha256(bearer.encode()).hexdigest()
out["noBearer"] = get({"Authorization": "Bearer wrong"})[0]
# WebSocket: 1 MiB each way.
s = socket.create_connection((base, 8971), timeout=30)
key = base64.b64encode(os.urandom(16)).decode()
lines = ["GET /v1/preview/%d/ws HTTP/1.1" % port, "Host: x", "Upgrade: websocket",
         "Connection: Upgrade", "Sec-WebSocket-Key: " + key, "Sec-WebSocket-Version: 13"]
lines += ["%s: %s" % kv for kv in headers.items()]
s.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
buf = b""
while b"\r\n\r\n" not in buf:
    chunk = s.recv(4096)
    if not chunk:
        break
    buf += chunk
out["upgrade"] = buf.split(b"\r\n", 1)[0].decode()
if b" 101 " in buf.split(b"\r\n", 1)[0]:
    rest = buf.split(b"\r\n\r\n", 1)[1]
    payload = os.urandom(1 << 20)
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    s.sendall(bytes([0x82, 0x80 | 127]) + struct.pack(">Q", len(payload)) + mask + masked)
    def need(n):
        global rest
        while len(rest) < n:
            chunk = s.recv(1 << 16)
            if not chunk:
                raise EOFError
            rest += chunk
        data, rest = rest[:n], rest[n:]
        return data
    first, second = need(2)
    length = second & 0x7F
    if length == 126:
        length = struct.unpack(">H", need(2))[0]
    elif length == 127:
        length = struct.unpack(">Q", need(8))[0]
    echoed = need(length)
    out["wsEcho"] = echoed == payload
print(json.dumps(out))
"#;
    for port in [8080u16, 8081, 8090] {
        let capability = token(port);
        let output = kubectl(
            &context,
            &[
                "exec",
                "-n",
                &baseline,
                &pod,
                "--",
                "python3",
                "-c",
                client_script,
                &port.to_string(),
                &capability,
            ],
        );
        assert!(
            output.status.success(),
            "client for {port}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let seen: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("client result");
        assert_eq!(seen["http"], 200, "port {port}: {seen}");
        assert_eq!(seen["port"], port, "port {port}: {seen}");
        assert_eq!(
            seen["uid"], 1001,
            "the workload runs as the dropped uid: {seen}"
        );
        assert_eq!(seen["bearerUnchanged"], true, "port {port}: {seen}");
        assert_eq!(seen["noBearer"], 401, "port {port}: {seen}");
        assert!(
            seen["upgrade"]
                .as_str()
                .unwrap_or_default()
                .contains(" 101 "),
            "port {port}: {seen}"
        );
        assert_eq!(seen["wsEcho"], true, "port {port}: {seen}");
    }
    // A capability for one port does not open another.
    let output = kubectl(
        &context,
        &[
            "exec",
            "-n",
            &baseline,
            &pod,
            "--",
            "python3",
            "-c",
            client_script,
            "8081",
            &token(8080),
        ],
    );
    let seen: serde_json::Value = serde_json::from_slice(&output.stdout).expect("client result");
    assert_ne!(
        seen["http"], 200,
        "a capability for 8080 must not open 8081: {seen}"
    );
    let _ = kubectl(
        &context,
        &["delete", "pod", "-n", &baseline, &pod, "--wait=false"],
    );
}

/// The ENTRYPOINT must not start before the pod's egress restriction is proven, including under
/// `none` egress. The cluster API address is given to the pod the way the Operator gives it
/// (`ALIEN_SANDBOX_MUST_DROP_KUBE_API`, from the `default/kubernetes` Service), which makes the
/// gate see a reachable blocked target until a NetworkPolicy that drops it is attached late.
#[tokio::test]
#[ignore = "needs a Kind cluster; with ALIEN_ROOT_BOOT_KIND_ENFORCES_POLICY set it also attaches a policy late"]
async fn kubernetes_root_boot_kind_the_entrypoint_waits_for_a_late_network_policy() {
    let context = env("ALIEN_ROOT_BOOT_KIND_CONTEXT");
    let baseline = env("ALIEN_ROOT_BOOT_KIND_BASELINE_NAMESPACE");
    let pods = client(&context).await;
    let pair = ed25519_compact::KeyPair::generate();
    let public_key = BASE64.encode(pair.pk.as_ref());

    let api = kubectl(
        &context,
        &[
            "get",
            "service",
            "kubernetes",
            "-n",
            "default",
            "-o",
            "jsonpath={.spec.clusterIP}",
        ],
    );
    let api = String::from_utf8_lossy(&api.stdout).to_string();
    assert!(!api.is_empty(), "the cluster API service address");

    let sandbox = declared("gatewait", 0);
    let mut pod = idle_pool_pod_with_labels(
        &sandbox,
        &baseline,
        "alien-kind-runc",
        None,
        Some(&public_key),
        BTreeMap::from([(
            "alien.dev/deployment".to_string(),
            "root-boot-kind".to_string(),
        )]),
    );
    let name = format!("gatewait-{:x}", rand::random::<u32>());
    pod.metadata.name = Some(name.clone());
    pod.metadata
        .labels
        .get_or_insert_default()
        .insert("a2657-gate-test".to_string(), name.clone());
    let container = &mut pod.spec.as_mut().unwrap().containers[0];
    let target = format!("{api}:443");
    container
        .env
        .get_or_insert_default()
        .push(k8s_openapi::api::core::v1::EnvVar {
            name: alien_core::sandbox_image::ENV_MUST_DROP_KUBE_API.to_string(),
            value: Some(target.clone()),
            value_from: None,
        });
    pods.create_pod(&baseline, &pod).await.expect("create pod");

    // No policy yet: the gate must hold the ENTRYPOINT back. Look while it still waits.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    let early = kubectl(
        &context,
        &[
            "exec",
            "-n",
            &baseline,
            &name,
            "--",
            "ls",
            "/tmp/alien-contract/boots.jsonl",
        ],
    );
    assert!(
        !early.status.success(),
        "the ENTRYPOINT ran before any NetworkPolicy was in force"
    );

    if std::env::var_os("ALIEN_ROOT_BOOT_KIND_ENFORCES_POLICY").is_none() {
        // A CNI that does not enforce NetworkPolicy (Kind's default) never proves the drop. The
        // gate must then refuse to start the workload at all, and say why.
        let mut logs = String::new();
        for _ in 0..30 {
            let read = kubectl(&context, &["logs", "-n", &baseline, &name, "-c", "sandbox"]);
            logs = String::from_utf8_lossy(&read.stdout).to_string();
            if logs.contains("egress restriction not proven") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        assert!(
            logs.contains("egress restriction not proven"),
            "the gate refused to start the workload: {logs:?}"
        );
        assert!(
            logs.contains(&target),
            "the refusal names the API server target {target}: {logs:?}"
        );
        let _ = kubectl(
            &context,
            &["delete", "pod", "-n", &baseline, &name, "--wait=false"],
        );
        return;
    }

    // Attach the policy late: no egress at all for this pod.
    let policy = format!(
        "apiVersion: networking.k8s.io/v1\nkind: NetworkPolicy\nmetadata:\n  name: {name}\n  namespace: {baseline}\nspec:\n  podSelector:\n    matchLabels:\n      a2657-gate-test: {name}\n  policyTypes: [Egress]\n"
    );
    let applied = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "printf '%b' '{policy}' | kubectl --context {context} apply -f -"
        ))
        .output()
        .expect("kubectl apply");
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );

    // After the drops are proven in a row, the ENTRYPOINT starts, once.
    let mut boots = String::new();
    for _ in 0..30 {
        let read = kubectl(
            &context,
            &[
                "exec",
                "-n",
                &baseline,
                &name,
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
    assert_eq!(
        boots.lines().count(),
        1,
        "one boot after the gate passed: {boots:?}"
    );
    let _ = kubectl(
        &context,
        &["delete", "networkpolicy", "-n", &baseline, &name],
    );
    let _ = kubectl(
        &context,
        &["delete", "pod", "-n", &baseline, &name, "--wait=false"],
    );
}
