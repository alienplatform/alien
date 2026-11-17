//! Azure `internet` egress, proven live against the setup's gateway VMs. Ignored: it must run
//! **inside the deployment's VNet** (the gateway admin port is reachable only from there), against
//! a deployment whose gateway config the owner controller has published.
//!
//! The pieces are produced by `scripts/azure-internet-live.sh`, which also builds and runs this
//! test as a Container Apps job on the setup's container-apps subnet. Run by hand with the same
//! environment. Everything comes from the environment; nothing is hardcoded.
//!
//! Env (all required unless marked optional):
//! - `AZURE_TARGET_SUBSCRIPTION_ID`, `AZURE_TARGET_TENANT_ID`, `AZURE_TARGET_CLIENT_ID`,
//!   `AZURE_TARGET_CLIENT_SECRET`: the identity holding the sandbox-group data-plane role and
//!   Key Vault secret read on the deployment vault.
//! - `AZURE_RESOURCE_GROUP`, `AZURE_SANDBOX_REGION`, `AZURE_INTERNET_VAULT_NAME`.
//! - `AZURE_INTERNET_GATEWAY_BINDING`: the `sandbox-egress-gateway` Container binding its own controller published.
//! - `AZURE_INTERNET_BINDING`: the sandbox binding JSON the owner controller published (its
//!   `egress` carries the gateway handoff, kid, audience and replica addresses).
//! - `AZURE_INTERNET_GATEWAY_IPS`: comma separated gateway replica addresses (proxy 3128, admin 3129).
//! - `AZURE_INTERNET_GATEWAY_VMS`: comma separated ARM resource ids of those VMs, in the same
//!   order, for the restart proof (g).
//! - `AZURE_INTERNET_PRIVATE_PROBES`: optional comma separated private `ip` addresses probed on
//!   port 8443 (d2); defaults to none.
//! - `AZURE_INTERNET_RESULT_SECRET`: optional vault secret name that receives the proof log.
//!
//! ```text
//! cargo test -p alien-bindings --features azure --test azure_sandbox_internet_live -- --ignored --nocapture
//! ```
//!
//! Proofs, in the order they run: (a) the allowed `host:443` connects and the unlisted host and
//! the wrong port do not, against a positive control that reaches the same hosts; (b) live
//! `update_egress` adds and narrows rules; (c) a direct connection to a public IP on 80/443 meets
//! the native interposer, never the origin; (d) a direct connection on any other port, and (d2) to
//! a private address, times out; (neg) the admin port is unreachable from a sandbox; (f) ten
//! parallel creates all come up behind the gateway; (g) restarting one gateway VM loses no policy
//! (peer sync) and a write during the restart is not silently dropped; (g3) with one gateway VM powered off, a running sandbox keeps
//! reaching its allowed host through the agent's failover; (i) no cloud credential is
//! in the guest environment.
#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig, EgressPolicy};
use futures::{FutureExt, StreamExt};

const ALLOWED: &str = "en.wikipedia.org";
const OTHER: &str = "example.com";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn list(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

/// One CONNECT through `proxy` (host:port) with the sandbox's own proxy credentials, or the
/// environment's proxy when `proxy` is empty. Prints `PROXY <status line> <seconds>` or `PROXY-NONE`.
const PROXY_PROBE: &str = r#"
import os, socket, sys, time, urllib.parse
host, port, proxy = sys.argv[1], int(sys.argv[2]), sys.argv[3]
env = urllib.parse.urlparse(os.environ["HTTPS_PROXY"])
authority = proxy or f"{env.hostname}:{env.port}"
ph, pp = authority.rsplit(":", 1)
auth = urllib.parse.quote(env.username or "", safe="") + ":" + (env.password or "")
import base64
basic = base64.b64encode(f"{env.username}:{env.password}".encode()).decode()
t = time.time()
try:
    s = socket.create_connection((ph, int(pp)), timeout=30)
    s.settimeout(30)
    s.sendall(f"CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Authorization: Basic {basic}\r\n\r\n".encode())
    line = s.recv(200).split(b"\r\n")[0].decode(errors="replace")
    print("PROXY", line, round(time.time() - t, 1))
except Exception as e:
    print("PROXY-NONE", type(e).__name__, round(time.time() - t, 1))
"#;

/// A direct TCP connection: `DIRECT-DATA <first bytes>` when something answers, `DIRECT-OPEN` when
/// it connects but says nothing, `DIRECT-NONE <error>` when it cannot connect.
const DIRECT_PROBE: &str = r#"
import socket, sys, time
host, port = sys.argv[1], int(sys.argv[2])
t = time.time()
try:
    s = socket.create_connection((host, port), timeout=8)
except Exception as e:
    print("DIRECT-NONE", type(e).__name__, round(time.time() - t, 1)); sys.exit(0)
try:
    s.settimeout(15)
    s.sendall(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    data = s.recv(60)
    print("DIRECT-DATA", data[:60] if data else b"")
except Exception as e:
    print("DIRECT-OPEN", type(e).__name__)
"#;

struct Proof {
    log: Vec<String>,
    failures: Vec<String>,
}

impl Proof {
    fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        eprintln!("{line}");
        self.log.push(line);
    }
    fn check(&mut self, ok: bool, label: &str, observed: &str) {
        self.note(format!(
            "[{}] {label}: {observed}",
            if ok { "ok" } else { "FAIL" }
        ));
        if !ok {
            self.failures.push(format!("{label}: {observed}"));
        }
    }
}

async fn run(
    sandbox: &dyn alien_bindings::traits::Sandbox,
    id: &str,
    command: &str,
    args: &[&str],
) -> String {
    run_for(sandbox, id, command, args, Duration::from_secs(90)).await
}

async fn run_for(
    sandbox: &dyn alien_bindings::traits::Sandbox,
    id: &str,
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> String {
    let stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: command.to_string(),
                args: args.iter().map(|arg| arg.to_string()).collect(),
                cwd: None,
                env: BTreeMap::new(),
                timeout,
            },
        )
        .await;
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(error) => return format!("RUN-ERROR {error}"),
    };
    let mut out = Vec::new();
    while let Some(frame) = stream.next().await {
        match frame {
            Ok(CommandOutput::Stdout { data, .. }) => out.extend(data),
            Ok(_) => {}
            Err(error) => return format!("RUN-ERROR {error}"),
        }
    }
    String::from_utf8_lossy(&out).trim().to_string()
}

async fn proxy(
    sandbox: &dyn alien_bindings::traits::Sandbox,
    id: &str,
    host: &str,
    port: u16,
    via: &str,
) -> String {
    run(
        sandbox,
        id,
        "python3",
        &["-c", PROXY_PROBE, host, &port.to_string(), via],
    )
    .await
}

async fn direct(
    sandbox: &dyn alien_bindings::traits::Sandbox,
    id: &str,
    host: &str,
    port: u16,
) -> String {
    run(
        sandbox,
        id,
        "python3",
        &["-c", DIRECT_PROBE, host, &port.to_string()],
    )
    .await
}

fn connected(observed: &str) -> bool {
    observed.contains(" 200 ")
}

fn refused(observed: &str) -> bool {
    observed.contains(" 403 ")
}

fn allow(hosts: &[&str]) -> EgressPolicy {
    EgressPolicy::Allow {
        allow: hosts.iter().map(|host| host.to_string()).collect(),
    }
}

async fn arm_token(tenant: &str, client: &str, secret: &str) -> String {
    let response: serde_json::Value = reqwest::Client::new()
        .post(format!(
            "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
        ))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", client),
            ("client_secret", secret),
            ("scope", "https://management.azure.com/.default"),
        ])
        .send()
        .await
        .expect("token request")
        .json()
        .await
        .expect("token body");
    response["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("no ARM token: {response}"))
        .to_string()
}

#[tokio::test]
#[ignore = "runs inside the setup's VNet against live gateway VMs; see the module docs"]
async fn live_internet_proof() {
    let region = env("AZURE_SANDBOX_REGION");
    // AZURE_INTERNET_ONLY=a,c,g3 runs only those sections (empty: all). Create, a and c always run.
    let only = std::env::var("AZURE_INTERNET_ONLY").unwrap_or_default();
    let sel = |name: &str| only.is_empty() || only.split(',').any(|part| part.trim() == name);
    let tenant = env("AZURE_TARGET_TENANT_ID");
    let client_id = env("AZURE_TARGET_CLIENT_ID");
    let client_secret = env("AZURE_TARGET_CLIENT_SECRET");
    let gateway_ips = list("AZURE_INTERNET_GATEWAY_IPS");
    let gateway_vms = list("AZURE_INTERNET_GATEWAY_VMS");
    assert!(gateway_ips.len() >= 2, "two gateway replicas are required");
    assert_eq!(gateway_ips.len(), gateway_vms.len());
    let private_probes = list("AZURE_INTERNET_PRIVATE_PROBES");

    let binding: serde_json::Value =
        serde_json::from_str(&env("AZURE_INTERNET_BINDING")).expect("binding JSON");
    let bindings = HashMap::from([
        ("box".to_string(), binding),
        (
            "secrets".to_string(),
            serde_json::json!({"service": "key-vault", "vaultName": env("AZURE_INTERNET_VAULT_NAME")}),
        ),
        (
            "sandbox-egress-gateway".to_string(),
            serde_json::from_str(&env("AZURE_INTERNET_GATEWAY_BINDING"))
                .expect("gateway binding JSON"),
        ),
    ]);
    let config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: tenant.clone(),
        region: Some(region),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        },
        service_overrides: None,
    }));
    let provider = BindingsProvider::new(config, bindings).expect("bindings");
    let sandbox = provider.load_sandbox("box").await.expect("loads");
    let sandbox: &dyn alien_bindings::traits::Sandbox = sandbox.as_ref();

    let mut proof = Proof {
        log: Vec::new(),
        failures: Vec::new(),
    };
    let mut created: Vec<String> = Vec::new();

    let outcome = async {
        // The positive control: an open `internet` sandbox reaches both hosts on 443, so a
        // denial below can only be the policy.
        let started = Instant::now();
        let control = sandbox
            .create(CreateSandboxRequest {
                egress: Some(EgressPolicy::Internet),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("control create: {error}"))?;
        created.push(control.sandbox_id.clone());
        proof.note(format!(
            "[create] control {} in {:?}",
            control.sandbox_id,
            started.elapsed()
        ));
        for host in [ALLOWED, OTHER] {
            let seen = proxy(sandbox, &control.sandbox_id, host, 443, "").await;
            proof.check(connected(&seen), &format!("control reaches {host}:443"), &seen);
        }

        let subject = sandbox
            .create(CreateSandboxRequest {
                egress: Some(allow(&[&format!("{ALLOWED}:443")])),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("subject create: {error}"))?;
        let id = subject.sandbox_id.clone();
        created.push(id.clone());
        proof.note(format!("[create] subject {id}"));

        // (a) allowed, unlisted and wrong port.
        let seen = proxy(sandbox, &id, ALLOWED, 443, "").await;
        proof.check(connected(&seen), "a subject allowed host:443", &seen);
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(refused(&seen), "a subject unlisted host", &seen);
        let seen = proxy(sandbox, &id, ALLOWED, 8443, "").await;
        proof.check(refused(&seen), "a subject allowed host, wrong port", &seen);

        if sel("b") {
        // (b) live rule writes.
        sandbox
            .update_egress(&id, allow(&[&format!("{ALLOWED}:443"), &format!("{OTHER}:443")]))
            .await
            .map_err(|error| format!("update_egress add: {error}"))?;
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(connected(&seen), "b after adding the second host", &seen);
        sandbox
            .update_egress(&id, allow(&[&format!("{ALLOWED}:443")]))
            .await
            .map_err(|error| format!("update_egress narrow: {error}"))?;
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(refused(&seen), "b after narrowing, removed host", &seen);
        let seen = proxy(sandbox, &id, ALLOWED, 443, "").await;
        proof.check(connected(&seen), "b after narrowing, kept host", &seen);

        }
        // (c) direct to a public IP: only the native interposer may answer, never an origin.
        for port in [443, 80] {
            let seen = direct(sandbox, &id, "1.1.1.1", port).await;
            proof.check(
                seen.starts_with("DIRECT-NONE") || seen.contains("403"),
                &format!("c direct 1.1.1.1:{port} meets the interposer"),
                &seen,
            );
        }
        if sel("d") {
        // (d) any other port, and (d2) private addresses, never connect.
        let seen = direct(sandbox, &id, "1.1.1.1", 8443).await;
        proof.check(seen.starts_with("DIRECT-NONE"), "d direct 1.1.1.1:8443", &seen);
        for ip in &private_probes {
            let seen = direct(sandbox, &id, ip, 8443).await;
            proof.check(
                seen.starts_with("DIRECT-NONE"),
                &format!("d2 private {ip}:8443"),
                &seen,
            );
        }
        for ip in &gateway_ips {
            let seen = direct(sandbox, &id, ip, 3129).await;
            proof.check(
                seen.starts_with("DIRECT-NONE"),
                &format!("neg gateway admin {ip}:3129 from the guest"),
                &seen,
            );
        }
        let seen = direct(sandbox, &id, "169.254.169.254", 80).await;
        proof.check(
            seen.starts_with("DIRECT-NONE"),
            "neg metadata 169.254.169.254:80",
            &seen,
        );

        }
        if sel("i") {
        // (i) no cloud credential in the guest.
        let seen = run(
            sandbox,
            &id,
            "sh",
            &["-c", "env | grep -E '^(IDENTITY_ENDPOINT|IDENTITY_HEADER|MSI_ENDPOINT|AZURE_)' | cut -d= -f1 | tr '\\n' ' ' || true"],
        )
        .await;
        proof.check(
            seen.is_empty(),
            "i no cloud credential in the environment (names found)",
            &seen,
        );

        }
        if sel("f") {
        // (f) ten parallel creates all come up behind the gateway.
        let started = Instant::now();
        let creates = futures::future::join_all((0..10).map(|_| {
            sandbox.create(CreateSandboxRequest {
                egress: Some(allow(&[&format!("{ALLOWED}:443")])),
                ..Default::default()
            })
        }))
        .await;
        let mut parallel = Vec::new();
        let mut errors = Vec::new();
        for result in creates {
            match result {
                Ok(instance) => {
                    created.push(instance.sandbox_id.clone());
                    parallel.push(instance.sandbox_id);
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        proof.check(
            errors.is_empty(),
            "f parallel creates",
            &format!("ok={}/10 in {:?}; errors={errors:?}", parallel.len(), started.elapsed()),
        );
        for parallel_id in &parallel {
            let seen = proxy(sandbox, parallel_id, ALLOWED, 443, "").await;
            proof.check(connected(&seen), &format!("f {parallel_id} through the gateway"), &seen);
        }

        }
        // (g) restart a gateway VM: a write during the restart is not silently dropped, and the
        // restarted replica comes back holding the policy its peer holds.
        let token = arm_token(&tenant, &client_id, &client_secret).await;
        if sel("g") {
        // (g2) a sandbox that is deleted while a replica is away leaves no grant on either replica.
        // Its own proxy credentials are read inside it (never logged), then replayed from the subject.
        let doomed = sandbox
            .create(CreateSandboxRequest {
                egress: Some(allow(&[&format!("{ALLOWED}:443")])),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("doomed create: {error}"))?;
        let doomed_id = doomed.sandbox_id.clone();
        created.push(doomed_id.clone());
        let doomed_proxy = run(sandbox, &doomed_id, "printenv", &["HTTPS_PROXY"])
            .await
            .trim()
            .to_string();
        proof.check(
            doomed_proxy.starts_with("http://"),
            "g2 doomed sandbox has proxy credentials",
            "(not logged)",
        );
        let replay = |via: String| {
            let doomed_proxy = doomed_proxy.clone();
            let probe_host = ALLOWED.to_string();
            let subject_id = id.clone();
            async move {
                run(
                    sandbox,
                    &subject_id,
                    "env",
                    &[
                        &format!("HTTPS_PROXY={doomed_proxy}"),
                        "python3",
                        "-c",
                        PROXY_PROBE,
                        &probe_host,
                        "443",
                        &via,
                    ],
                )
                .await
            }
        };
        for (index, ip) in gateway_ips.iter().enumerate() {
            let seen = replay(format!("{ip}:3128")).await;
            proof.check(connected(&seen), &format!("g2 replica {index} serves the doomed grant before delete"), &seen);
        }
        let restarted = &gateway_vms[0];
        let response = reqwest::Client::new()
            .post(format!(
                "https://management.azure.com{restarted}/restart?api-version=2024-07-01"
            ))
            .bearer_auth(&token)
            .header("content-length", "0")
            .send()
            .await
            .map_err(|error| format!("restart request: {error}"))?;
        proof.check(
            response.status().is_success(),
            "g restart accepted",
            &response.status().to_string(),
        );
        // The write races the restart. It must succeed on every replica or fail loudly; either
        // way the sandbox ends holding the policy the last successful write named.
        let during = sandbox
            .update_egress(&id, allow(&[&format!("{ALLOWED}:443"), &format!("{OTHER}:443")]))
            .await;
        proof.note(format!(
            "[g] write during restart: {}",
            match &during {
                Ok(()) => "acknowledged by every replica".to_string(),
                Err(error) => format!("refused: {error}"),
            }
        ));
        // (g2) delete the doomed sandbox while the replica is still away.
        let deleted = sandbox.terminate(&doomed_id).await;
        proof.note(format!(
            "[g2] terminate during restart: {}",
            match &deleted {
                Ok(()) => "acknowledged by every replica".to_string(),
                Err(error) => format!("reported (retryable={}): {error}", error.retryable),
            }
        ));
        created.retain(|existing| existing != &doomed_id);
        let wanted_other = during.is_ok();
        // Wait for the restarted replica to answer again, through the sandbox's own credentials.
        let deadline = Instant::now() + Duration::from_secs(600);
        let via = format!("{}:3128", gateway_ips[0]);
        let mut back = String::new();
        while Instant::now() < deadline {
            back = proxy(sandbox, &id, ALLOWED, 443, &via).await;
            if !back.contains("PROXY-NONE") {
                break;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        proof.check(!back.contains("PROXY-NONE"), "g restarted replica answers again", &back);
        for (index, ip) in gateway_ips.iter().enumerate() {
            let via = format!("{ip}:3128");
            let seen = proxy(sandbox, &id, ALLOWED, 443, &via).await;
            proof.check(connected(&seen), &format!("g replica {index} kept host"), &seen);
            let seen = proxy(sandbox, &id, OTHER, 443, &via).await;
            proof.check(
                connected(&seen) == wanted_other && (wanted_other || refused(&seen)),
                &format!("g replica {index} holds the last acknowledged policy (second host open={wanted_other})"),
                &seen,
            );
        }
        // (g2) neither replica serves the deleted sandbox's grant, including the one that was away.
        for (index, ip) in gateway_ips.iter().enumerate() {
            let seen = replay(format!("{ip}:3128")).await;
            proof.check(!connected(&seen), &format!("g2 replica {index} serves no grant for the deleted sandbox"), &seen);
        }
        // A fresh write reaches both replicas once both are back.
        sandbox
            .update_egress(&id, allow(&[&format!("{ALLOWED}:443")]))
            .await
            .map_err(|error| format!("update_egress after restart: {error}"))?;
        for (index, ip) in gateway_ips.iter().enumerate() {
            let via = format!("{ip}:3128");
            let seen = proxy(sandbox, &id, OTHER, 443, &via).await;
            proof.check(refused(&seen), &format!("g replica {index} after the new write"), &seen);
        }
        }
        if sel("g3") {
        // (g3) gateway VM 0 stopped: egress continues through the agent's loopback failover. A
        // running sandbox keeps reaching its allowed host through VM 1 (the proxy variables point
        // at the agent's forwarder, not at either VM), and a new sandbox is tried against the
        // gate. Writes need every replica, so the new sandbox's create may be refused with a
        // retryable gateway error while VM 0 is down; that is recorded, not hidden.
        let stopped = &gateway_vms[0];
        let response = reqwest::Client::new()
            .post(format!(
                "https://management.azure.com{stopped}/powerOff?api-version=2024-07-01"
            ))
            .bearer_auth(&token)
            .header("content-length", "0")
            .send()
            .await
            .map_err(|error| format!("power off request: {error}"))?;
        proof.check(
            response.status().is_success(),
            "g3 power off accepted",
            &response.status().to_string(),
        );
        let via0 = format!("{}:3128", gateway_ips[0]);
        let mut seen = String::new();
        for _ in 0..60 {
            seen = proxy(sandbox, &id, ALLOWED, 443, &via0).await;
            if !connected(&seen) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        proof.check(!connected(&seen), "g3 VM 0 no longer serves direct", &seen);
        let started = Instant::now();
        let seen = proxy(sandbox, &id, ALLOWED, 443, "").await;
        proof.check(
            connected(&seen),
            "g3 running sandbox reaches its allowed host with VM 0 stopped",
            &format!("{seen} in {:?}", started.elapsed()),
        );
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(refused(&seen), "g3 running sandbox still denied the unlisted host", &seen);
        let started = Instant::now();
        let mut g3_new: Option<String> = None;
        match sandbox
            .create(CreateSandboxRequest {
                egress: Some(allow(&[&format!("{ALLOWED}:443")])),
                ..Default::default()
            })
            .await
        {
            Ok(new) => {
                created.push(new.sandbox_id.clone());
                g3_new = Some(new.sandbox_id.clone());
                proof.check(
                    started.elapsed() < Duration::from_secs(150),
                    "g3 create with VM 0 stopped is bounded",
                    &format!("{:?}", started.elapsed()),
                );
                let seen = proxy(sandbox, &new.sandbox_id, ALLOWED, 443, "").await;
                proof.check(
                    connected(&seen),
                    "g3 a new sandbox passes its gate and reaches its allowed host with VM 0 stopped",
                    &format!("{seen} created in {:?}", started.elapsed()),
                );
            }
            Err(error) => proof.check(
                false,
                "g3 a new sandbox is created with VM 0 stopped",
                &format!(
                    "refused after {:?} (retryable={}): {error}",
                    started.elapsed(),
                    error.retryable
                ),
            ),
        }
        let response = reqwest::Client::new()
            .post(format!(
                "https://management.azure.com{stopped}/start?api-version=2024-07-01"
            ))
            .bearer_auth(&token)
            .header("content-length", "0")
            .send()
            .await
            .map_err(|error| format!("start request: {error}"))?;
        proof.check(response.status().is_success(), "g3 start accepted", &response.status().to_string());
        let deadline = Instant::now() + Duration::from_secs(900);
        let mut back = String::new();
        while Instant::now() < deadline {
            back = proxy(sandbox, &id, ALLOWED, 443, &via0).await;
            if connected(&back) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        proof.check(connected(&back), "g3 VM 0 serves again after start", &back);
        if let Some(new_id) = &g3_new {
            // The replica that was away starts empty and takes the grant from its peer.
            let mut seen = String::new();
            for _ in 0..30 {
                seen = proxy(sandbox, new_id, ALLOWED, 443, &via0).await;
                if connected(&seen) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            proof.check(connected(&seen), "g3 replica 0 serves the grant created while it was away", &seen);
        }
        }
        Ok::<(), String>(())
    }
    .await;

    if let Err(error) = outcome {
        proof.failures.push(error.clone());
        proof.note(format!("[error] {error}"));
    }
    for sandbox_id in &created {
        if let Err(error) = sandbox.terminate(sandbox_id).await {
            proof.note(format!("[cleanup] terminate {sandbox_id}: {error}"));
        }
    }
    proof.note(format!("[cleanup] {} sandboxes terminated", created.len()));

    if let Ok(name) = std::env::var("AZURE_INTERNET_RESULT_SECRET") {
        let vault = provider.load_vault("secrets").await.expect("vault");
        let verdict = if proof.failures.is_empty() {
            "PASS"
        } else {
            "FAIL"
        };
        let body = format!("{verdict}\n{}", proof.log.join("\n"));
        vault
            .set_secret(&name, &body)
            .await
            .expect("publish the proof log");
    }
    assert!(proof.failures.is_empty(), "failed: {:#?}", proof.failures);
}

/// Leak probes from inside an `internet` sandbox to a listener the operator controls
/// (`AZURE_INTERNET_LISTENER_IP`, ports 80 and 443, TCP and UDP). The sandbox allows only
/// `example.com`; the listener is neither allowed by IP nor by name. Each probe prints one
/// `PROBE <label> <outcome>` line, one at a time; whether anything reached the listener is read
/// from the listener's own log and packet capture, keyed by the nonce and the time. The guest has
/// only python3, so every probe is a python socket: an `ssl` handshake without SNI, raw bytes, a
/// hand-written HTTP/1.0 request, the h2c prior-knowledge preface, an SSH banner, UDP datagrams, SNI
/// of an allowed name against the listener's address, and a plain request with an allowed Host.
/// After each TLS handshake a full HTTP request goes inside the session (verification off) and the
/// answer is recorded. The wireserver is probed with the connect and the read told apart.
const LEAK_PROBE: &str = r#"
import socket, ssl, sys, time
ip, nonce = sys.argv[1], sys.argv[2]
def out(label, outcome): print("PROBE", label, outcome, flush=True)
def tcp(label, port, payload=b"", tls=None, inner=None):
    t = time.time()
    try:
        s = socket.create_connection((ip, port), timeout=8)
    except Exception as e:
        return out(label, "CONNECT-" + type(e).__name__ + " " + str(round(time.time() - t, 1)) + "s")
    try:
        if tls is not None:
            c = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT); c.check_hostname = False; c.verify_mode = ssl.CERT_NONE
            s.settimeout(8)
            s = c.wrap_socket(s, server_hostname=tls or None)
            seen = "TLS-HANDSHAKE-OK " + str(s.version()) + " peer-cert-der-bytes=" + str(len(s.getpeercert(True) or b""))
            for host in (inner or []):
                s.settimeout(8)
                s.sendall(("GET /" + nonce + " HTTP/1.1\r\nHost: " + host + "\r\nConnection: close\r\n\r\n").encode())
                try:
                    d = s.recv(120); seen += " | host=" + host + " reply=" + repr(d[:100])
                except Exception as e:
                    seen += " | host=" + host + " no-reply " + type(e).__name__
                    break
            return out(label, seen)
        s.settimeout(6); s.sendall(payload)
        try:
            d = s.recv(80); out(label, "CONNECTED reply=" + repr(d[:60]))
        except Exception as e:
            out(label, "CONNECTED no-reply " + type(e).__name__)
    except Exception as e:
        out(label, "CONNECTED-then-" + type(e).__name__ + " " + str(e)[:60])
    finally:
        try: s.close()
        except Exception: pass
def udp(label, port):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(4)
    try:
        s.sendto(("udp-" + label + "-" + nonce).encode(), (ip, port))
        try: d = s.recv(100); out(label, "UDP-REPLY " + repr(d[:40]))
        except Exception as e: out(label, "UDP-SENT no-reply " + type(e).__name__)
    except Exception as e:
        out(label, "UDP-ERR " + type(e).__name__)
tcp("1-tls-nosni-443", 443, tls="", inner=[ip])
tcp("2a-raw-443", 443, ("hello-raw-" + nonce).encode())
tcp("2b-raw-80", 80, ("hello-raw-" + nonce).encode())
tcp("3-http10-nohost-80", 80, ("GET /" + nonce + " HTTP/1.0\r\n\r\n").encode())
tcp("4-h2c-prior-80", 80, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" + b"\x00\x00\x00\x04\x00\x00\x00\x00\x00")
tcp("5-ssh-banner-443", 443, ("SSH-2.0-test-" + nonce + "\r\n").encode())
udp("6a-udp-443", 443)
udp("6b-udp-80", 80)
tcp("7-tls-sni-allowed-443", 443, tls="example.com", inner=[ip, "example.com"])
tcp("8-http-host-allowed-80", 80, ("GET /" + nonce + " HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n").encode())
# Wireserver (finding 3): the connect and the read are told apart.
def wire(label, port, path):
    t = time.time()
    try:
        s = socket.create_connection(("168.63.129.16", port), timeout=5)
    except Exception as e:
        return out(label, "CONNECT-FAILED " + type(e).__name__ + " " + str(e)[:50] + " after " + str(round(time.time() - t, 1)) + "s")
    t = time.time()
    try:
        s.settimeout(5)
        s.sendall(("GET " + path + " HTTP/1.1\r\nHost: 168.63.129.16\r\nConnection: close\r\n\r\n").encode())
        d = s.recv(200); out(label, "CONNECT-OK READ-OK " + repr(d[:150]))
    except Exception as e:
        out(label, "CONNECT-OK READ-FAILED " + type(e).__name__ + " " + str(e)[:50] + " after " + str(round(time.time() - t, 1)) + "s")
wire("wire-80-versions", 80, "/?comp=versions")
wire("wire-32526-vmSettings", 32526, "/vmSettings")
"#;

fn live_bindings(extra: &[(&str, serde_json::Value)]) -> BindingsProvider {
    let binding: serde_json::Value =
        serde_json::from_str(&env("AZURE_INTERNET_BINDING")).expect("binding JSON");
    let mut bindings = HashMap::from([
        ("box".to_string(), binding),
        (
            "secrets".to_string(),
            serde_json::json!({"service": "key-vault", "vaultName": env("AZURE_INTERNET_VAULT_NAME")}),
        ),
        (
            "sandbox-egress-gateway".to_string(),
            serde_json::from_str(&env("AZURE_INTERNET_GATEWAY_BINDING"))
                .expect("gateway binding JSON"),
        ),
    ]);
    for (name, value) in extra {
        bindings.insert(name.to_string(), value.clone());
    }
    let config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(env("AZURE_SANDBOX_REGION")),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    }));
    BindingsProvider::new(config, bindings).expect("bindings")
}

async fn publish(provider: &BindingsProvider, body: &str) {
    eprintln!("{body}");
    if let Ok(name) = std::env::var("AZURE_INTERNET_RESULT_SECRET") {
        let vault = provider.load_vault("secrets").await.expect("vault");
        vault
            .set_secret(&name, body)
            .await
            .expect("publish the proof log");
    }
}

#[tokio::test]
#[ignore = "live Azure: run inside the deployment's VNet via scripts/azure-internet-live.sh"]
async fn live_leak_probes() {
    let listener = env("AZURE_INTERNET_LISTENER_IP");
    let provider = live_bindings(&[]);
    let sandbox = provider.load_sandbox("box").await.expect("loads");
    let sandbox: &dyn alien_bindings::traits::Sandbox = sandbox.as_ref();

    let body = async {
        let created = sandbox
            .create(CreateSandboxRequest {
                egress: Some(allow(&["example.com"])),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("create: {error}"))?;
        let id = created.sandbox_id.clone();
        let mut log = Vec::new();
        // The whole matrix twice, each time with a fresh nonce and the probes one at a time.
        for round in 1..=2 {
            let started = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let nonce = format!("{started:x}");
            log.push(format!(
                "round {round}: nonce={nonce} start_utc_epoch={started} listener={listener} sandbox={id}"
            ));
            log.push(
                run_for(
                    sandbox,
                    &id,
                    "python3",
                    &["-c", LEAK_PROBE, &listener, &nonce],
                    Duration::from_secs(600),
                )
                .await,
            );
            log.push(format!(
                "round {round}: end_utc_epoch={}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ));
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let _ = sandbox.terminate(&id).await;
        Ok::<String, String>(log.join("\n"))
    }
    .await;
    let body = match body {
        Ok(log) => format!("DONE\n{log}"),
        Err(error) => format!("ERROR\n{error}"),
    };
    publish(&provider, &body).await;
}

/// A long transfer through the proxy: CONNECT to the host, TLS, GET `/__down?bytes=N`, read
/// throttled (about 100 KB/s, so one 99 MB response spans the roll) for `seconds`, and print one `XFER` line per event with epoch times:
/// connect, a progress line every 10 s, the cut (what error, at which byte) and each reconnect.
/// A cut is not hidden: the transfer reconnects with a new request and the lines show the gap.
const TRANSFER_PROBE: &str = r#"
import base64, os, socket, ssl, sys, time, urllib.parse
host, seconds = sys.argv[1], int(sys.argv[2])
env = urllib.parse.urlparse(os.environ["HTTPS_PROXY"])
basic = base64.b64encode(f"{env.username}:{env.password}".encode()).decode()
ctx = ssl.create_default_context()
end = time.time() + seconds
total = 0
def now(): return int(time.time())
def connect():
    s = socket.create_connection((env.hostname, env.port), timeout=30)
    s.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\nProxy-Authorization: Basic {basic}\r\n\r\n".encode())
    head = b""
    while b"\r\n\r\n" not in head:
        d = s.recv(512)
        if not d: raise EOFError("proxy closed before the CONNECT answer")
        head += d
    line = head.split(b"\r\n")[0].decode(errors="replace")
    if " 200 " not in line: raise RuntimeError("CONNECT " + line)
    t = ctx.wrap_socket(s, server_hostname=host)
    t.settimeout(30)
    t.sendall(f"GET /__down?bytes=99000000 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n".encode())
    head = b""
    while b"\r\n\r\n" not in head:
        d = t.recv(1024)
        if not d: raise EOFError("closed before the response head")
        head += d
    status = head.split(b"\r\n")[0].decode(errors="replace")
    if " 200 " not in status: raise RuntimeError("response " + status)
    return t
attempt = 0
while time.time() < end:
    attempt += 1
    got, last = 0, time.time()
    try:
        t = connect()
        print("XFER", now(), "connected attempt", attempt, flush=True)
        while time.time() < end:
            d = t.recv(16384)
            if not d: raise EOFError("closed by peer")
            got += len(d); total += len(d)
            if time.time() - last >= 10:
                last = time.time(); print("XFER", now(), "progress attempt", attempt, "bytes", got, flush=True)
            time.sleep(0.15)
        print("XFER", now(), "finished attempt", attempt, "bytes", got, flush=True)
    except Exception as e:
        print("XFER", now(), "CUT attempt", attempt, type(e).__name__, str(e)[:80], "after bytes", got, flush=True)
        time.sleep(1)
print("XFER", now(), "end total bytes", total, "attempts", attempt, flush=True)
"#;

/// A running sandbox across a setup re-run that installs a new gateway image. The operator runs the
/// re-run from outside and drives three stages through the vault secret `gw-replace-signal`
/// (`ready` -> `vm0-back` -> `done`). Measured, all with times on one clock:
/// - the SAME sandbox probes an allowed and an unlisted host every second (each probe logs its own
///   latency; a gap or a failure is a line, not hidden);
/// - a long throttled download runs in that sandbox for the whole roll (its cut and reconnects);
/// - an IDLE second sandbox makes its first connection only when the stage becomes `vm0-back`
///   (the operator sets it when VM0's run command has finished: VM1 restarts next), and again at
///   `done`;
/// - finally a NEW sandbox connects.
#[tokio::test]
#[ignore = "live Azure: run inside the deployment's VNet via scripts/azure-internet-live.sh"]
async fn live_gateway_replace() {
    let provider = live_bindings(&[]);
    let sandbox = provider.load_sandbox("box").await.expect("loads");
    let sandbox: &dyn alien_bindings::traits::Sandbox = sandbox.as_ref();
    let vault = provider.load_vault("secrets").await.expect("vault");
    let log = std::cell::RefCell::new(Vec::<String>::new());
    let created = std::cell::RefCell::new(Vec::<String>::new());
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let note = |line: String| log.borrow_mut().push(line);
    let policy = || {
        Some(allow(&[
            "example.com",
            "speed.cloudflare.com",
        ]))
    };

    let outcome = std::panic::AssertUnwindSafe(async {
        let make = || async {
            sandbox
                .create(CreateSandboxRequest {
                    egress: policy(),
                    ..Default::default()
                })
                .await
                .map(|created| created.sandbox_id)
                .map_err(|error| format!("create: {error}"))
        };
        let id = make().await?;
        created.borrow_mut().push(id.clone());
        let mut idles = Vec::new();
        for _ in 0..3 {
            let idle = make().await?;
            created.borrow_mut().push(idle.clone());
            idles.push(idle);
        }
        let idle = idles[0].clone();
        for host in ["example.com", "www.google.com"] {
            let seen = proxy(sandbox, &id, host, 443, "").await;
            note(format!("{} before: {host} {seen}", now()));
        }
        vault
            .set_secret("gw-replace-signal", "ready")
            .await
            .map_err(|error| format!("signal: {error}"))?;

        // The long download, streamed: lines are logged as they arrive, and it is dropped
        // when the operator says `done` (the probes set the flag).
        let finished = std::cell::Cell::new(false);
        let transfer = async {
            let stream = sandbox
                .run_command(
                    &id,
                    RunCommandRequest {
                        command: "python3".to_string(),
                        args: ["-c", TRANSFER_PROBE, "speed.cloudflare.com", "3000"]
                            .iter()
                            .map(|arg| arg.to_string())
                            .collect(),
                        cwd: None,
                        env: BTreeMap::new(),
                        timeout: Duration::from_secs(3100),
                    },
                )
                .await;
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(error) => return note(format!("transfer: RUN-ERROR {error}")),
            };
            let mut pending = String::new();
            loop {
                tokio::select! {
                    frame = stream.next() => match frame {
                        Some(Ok(CommandOutput::Stdout { data, .. })) => {
                            pending.push_str(&String::from_utf8_lossy(&data));
                            while let Some(at) = pending.find('\n') {
                                let line: String = pending.drain(..=at).collect();
                                note(format!("transfer: {}", line.trim_end()));
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => return note(format!("transfer: stream error {error}")),
                        None => return note("transfer: command ended".to_string()),
                    },
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        if finished.get() {
                            return note("transfer: stopped by the operator's `done`".to_string());
                        }
                    }
                }
            }
        };
        // Probes every second until the operator says `done`; then the idle sandbox's last
        // connection and a NEW sandbox.
        let probes = async {
            let mut stage = "ready".to_string();
            let mut last: HashMap<&str, String> = Default::default();
            let mut counts: HashMap<String, u32> = Default::default();
            let mut worst: HashMap<&str, f64> = Default::default();
            let mut idle_first = false;
            let mut tick = 0u32;
            for _ in 0..1700 {
                let signal = vault
                    .get_secret("gw-replace-signal")
                    .await
                    .unwrap_or_default();
                if signal != stage && !signal.is_empty() {
                    note(format!("{} signal now `{signal}`", now()));
                    stage = signal;
                }
                if stage == "vm0-back" && !idle_first {
                    idle_first = true;
                    // Idle sandboxes are paused by the platform: each is resumed, then makes its
                    // first connection, all at once. Up to 8 tries each, so the time to the first
                    // working connection is on the record; the first try is the one that matters.
                    let first = async |n: usize, sandbox_id: String| {
                        let resumed = sandbox.resume(&sandbox_id).await;
                        note(format!("{} [vm0-back] idle {n} resume: {resumed:?}", now()));
                        for attempt in 1..=8 {
                            let seen = proxy(sandbox, &sandbox_id, "example.com", 443, "").await;
                            note(format!(
                                "{} [vm0-back] idle {n} ({sandbox_id}) connection {attempt}: {seen}",
                                now()
                            ));
                            if connected(&seen) {
                                break;
                            }
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    };
                    futures::future::join_all(
                        idles.iter().cloned().enumerate().map(|(n, id)| first(n, id)),
                    )
                    .await;
                }
                for host in ["example.com", "www.google.com"] {
                    let seen = proxy(sandbox, &id, host, 443, "").await;
                    let seconds: f64 = seen
                        .rsplit(' ')
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let status: String = seen
                        .trim_end_matches(|c: char| c == '.' || c.is_ascii_digit() || c == ' ')
                        .to_string();
                    *counts.entry(format!("{host} {status}")).or_default() += 1;
                    let w = worst.entry(host).or_default();
                    if seconds > *w {
                        *w = seconds;
                    }
                    if last.get(host) != Some(&status) || seconds > 3.0 {
                        note(format!(
                            "{} [{stage}] the SAME sandbox, {host} now: {seen}",
                            now()
                        ));
                        last.insert(host, status);
                    }
                }
                if stage == "done" {
                    break;
                }
                tick += 1;
                if tick % 15 == 0 {
                    let text = log.borrow().join("\n");
                    let cut = text.len().saturating_sub(18_000);
                    let _ = vault
                        .set_secret(
                            "azure-internet-proof-log",
                            &format!("PARTIAL\n{}", &text[text.ceil_char_boundary(cut)..]),
                        )
                        .await;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            let mut totals: Vec<_> = counts.into_iter().collect();
            totals.sort();
            for (what, n) in totals {
                note(format!("total probes: {what} x{n}"));
            }
            for (host, w) in worst {
                note(format!("slowest probe {host}: {w}s"));
            }
            finished.set(true);
            stage
        };
        let (stage, ()) = futures::join!(probes, transfer);
        if stage != "done" {
            return Err("no `done` signal".to_string());
        }
        let seen = proxy(sandbox, &idle, "example.com", 443, "").await;
        note(format!("{} done: the IDLE sandbox again, example.com {seen}", now()));
        match make().await {
            Ok(new) => {
                created.borrow_mut().push(new.clone());
                let seen = proxy(sandbox, &new, "example.com", 443, "").await;
                note(format!("{} after: a NEW sandbox, example.com {seen}", now()));
            }
            Err(error) => note(format!("{} after: a NEW sandbox, {error}", now())),
        }
        Ok::<(), String>(())
    })
    .catch_unwind()
    .await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => note(format!("error: {error}")),
        Err(_) => note("error: the test panicked".to_string()),
    }
    for id in created.borrow().iter() {
        let _ = sandbox.terminate(id).await;
    }
    publish(&provider, &format!("DONE\n{}", log.borrow().join("\n"))).await;
}
