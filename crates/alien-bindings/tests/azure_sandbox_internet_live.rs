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
//! (peer sync) and a write during the restart is not silently dropped; (i) no cloud credential is
//! in the guest environment.
#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig, EgressPolicy};
use futures::StreamExt;

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
    s.settimeout(3)
    s.sendall(b"HEAD / HTTP/1.0\r\n\r\n")
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
    let stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: command.to_string(),
                args: args.iter().map(|arg| arg.to_string()).collect(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(90),
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
            serde_json::json!({
                "service": "horizon",
                "containerName": "sandbox-egress-gateway",
                "internalUrl": format!("http://{}:3128", gateway_ips[0]),
                "internalUrls": {
                    "3128": format!("http://{}:3128", gateway_ips[0]),
                    "3129": format!("http://{}:3129", gateway_ips[0]),
                },
            }),
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

        // (c) direct to a public IP: only the native interposer may answer, never an origin.
        for port in [443, 80] {
            let seen = direct(sandbox, &id, "1.1.1.1", port).await;
            proof.check(
                seen.starts_with("DIRECT-NONE") || seen.contains("403"),
                &format!("c direct 1.1.1.1:{port} meets the interposer"),
                &seen,
            );
        }
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

        // (i) no cloud credential in the guest.
        let seen = run(
            sandbox,
            &id,
            "sh",
            &["-c", "env | grep -c -E '^(IDENTITY_ENDPOINT|IDENTITY_HEADER|MSI_ENDPOINT|AZURE_)' || true"],
        )
        .await;
        proof.check(seen == "0", "i no cloud credential in the environment", &seen);

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

        // (g) restart a gateway VM: a write during the restart is not silently dropped, and the
        // restarted replica comes back holding the policy its peer holds.
        let token = arm_token(&tenant, &client_id, &client_secret).await;
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
        let verdict = if proof.failures.is_empty() { "PASS" } else { "FAIL" };
        let body = format!("{verdict}\n{}", proof.log.join("\n"));
        vault.set_secret(&name, &body).await.expect("publish the proof log");
    }
    assert!(proof.failures.is_empty(), "failed: {:#?}", proof.failures);
}
