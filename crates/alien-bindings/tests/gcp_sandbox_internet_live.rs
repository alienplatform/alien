//! GCP `internet` egress, proven live against the setup's gateway VMs. Ignored: the proof must run
//! **inside the deployment's VPC** (the gateway admin port is reachable only from there), against a
//! deployment whose gateway configuration has been published.
//!
//! Two tests: `prepare_template` creates the sandbox template that names the setup's network
//! attachment (from anywhere; it connects the attachment, which Google then pins to the tenant for
//! good) and writes the sandbox binding; `live_internet_proof` runs the rows below from a VM on
//! the workload subnet.
//!
//! Env: `GOOGLE_TARGET_PROJECT_ID`, `GOOGLE_TARGET_REGION`, `GOOGLE_TARGET_SERVICE_ACCOUNT_KEY`
//! (the identity that may use the sandbox API, read the vault and stop the gateway VMs);
//! `G_VAULT_PREFIX`, `G_GATEWAY_IPS` (comma separated), `G_BINDING` (the sandbox binding JSON
//! `prepare_template` wrote), `G_GATEWAY_VMS` (comma separated `zone/name`, same order as the
//! addresses), `G_RESULT` (optional file receiving the log). `prepare_template` also takes
//! `G_ENGINE`, `G_IMAGE`, `G_ATTACHMENT`, `G_KID`, `G_AUDIENCE`, `G_BINDING_OUT`.
//!
//! Rows: (a) the allowed `host:443` connects, another host and the same host on 8443 are refused
//! against a positive control; (b) `update_egress` adds `host:8443`, which works on the next
//! connection of the same sandbox; (c) the metadata server, direct IPs and the gateway admin port
//! never connect; (e) an unprivileged process in the sandbox cannot reach the agent's control
//! operations; (f) one gateway stopped: the sandbox keeps reaching its host through the other;
//! both stopped: refused; started again: back.
#![cfg(feature = "gcp")]

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{ClientConfig, EgressPolicy, GcpClientConfig, GcpCredentials};
use alien_gcp_clients::compute::{ComputeApi, ComputeClient};
use futures::StreamExt;

/// Open on 443 and 8443, so a refusal on either is the policy and nothing else.
const HOST: &str = "www.cloudflare.com";
const OTHER: &str = "example.com";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn list(name: &str) -> Vec<String> {
    env(name)
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

fn gcp_config() -> GcpClientConfig {
    GcpClientConfig {
        project_id: env("GOOGLE_TARGET_PROJECT_ID"),
        region: env("GOOGLE_TARGET_REGION"),
        credentials: GcpCredentials::ServiceAccountKey {
            json: env("GOOGLE_TARGET_SERVICE_ACCOUNT_KEY"),
        },
        service_overrides: None,
        project_number: None,
    }
}

/// One CONNECT through the sandbox's own proxy variables (or through `proxy`, host:port, with the
/// same credentials). Prints `PROXY <status line> <seconds>` or `PROXY-NONE <error> <seconds>`.
const PROXY_PROBE: &str = r#"
import base64, os, socket, sys, time, urllib.parse
host, port, proxy = sys.argv[1], int(sys.argv[2]), sys.argv[3]
env = urllib.parse.urlparse(os.environ["HTTPS_PROXY"])
authority = proxy or f"{env.hostname}:{env.port}"
ph, pp = authority.rsplit(":", 1)
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

/// A direct TCP connection with the proxy variables ignored: `DIRECT-NONE <error>` when it cannot
/// connect, `DIRECT-OPEN` when it connects.
const DIRECT_PROBE: &str = r#"
import socket, sys, time
host, port = sys.argv[1], int(sys.argv[2])
t = time.time()
try:
    s = socket.create_connection((host, port), timeout=8)
    print("DIRECT-OPEN", round(time.time() - t, 1))
except Exception as e:
    print("DIRECT-NONE", type(e).__name__, round(time.time() - t, 1))
"#;

/// The agent's control operations from inside the sandbox, as the unprivileged exec user.
const CONTROL_PROBE: &str = r#"
import json, os, sys, urllib.request, urllib.error
port = os.environ.get("ALIEN_SANDBOX_PORT", "8971")
def call(method, path, body):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode(),
                                 method=method, headers={"content-type": "application/json"})
    try:
        return urllib.request.urlopen(req, timeout=10).status
    except urllib.error.HTTPError as e:
        return e.code
    except Exception as e:
        return type(e).__name__
print("uid", os.getuid())
print("setEgressProxy", call("POST", "/", {"v": 1, "op": "setEgressProxy", "replicas": ["192.168.251.10:22"]}))
print("updateEgress-op", call("POST", "/", {"v": 1, "op": "updateEgress", "token": "x", "policy": "internet"}))
print("PUT-v1-egress", call("PUT", "/v1/egress", {"token": "x", "policy": "internet"}))
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

async fn run(sandbox: &dyn Sandbox, id: &str, command: &str, args: &[&str]) -> String {
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

async fn proxy(sandbox: &dyn Sandbox, id: &str, host: &str, port: u16, via: &str) -> String {
    run(
        sandbox,
        id,
        "python3",
        &["-c", PROXY_PROBE, host, &port.to_string(), via],
    )
    .await
}

async fn direct(sandbox: &dyn Sandbox, id: &str, host: &str, port: u16) -> String {
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

async fn instance_status(compute: &ComputeClient, vm: &str) -> String {
    let (zone, name) = vm.split_once('/').expect("zone/name");
    compute
        .get_instance(zone.to_string(), name.to_string())
        .await
        .ok()
        .and_then(|instance| instance.status.map(|status| format!("{status:?}")))
        .unwrap_or_else(|| "unknown".into())
}

async fn power(compute: &ComputeClient, vm: &str, on: bool) -> Result<(), String> {
    let (zone, name) = vm.split_once('/').expect("zone/name");
    let (zone, name) = (zone.to_string(), name.to_string());
    if on {
        compute.start_instance(zone, name).await
    } else {
        compute.stop_instance(zone, name).await
    }
    .map(|_| ())
    .map_err(|error| error.to_string())?;
    let want = if on { "Running" } else { "Terminated" };
    let deadline = Instant::now() + Duration::from_secs(240);
    while Instant::now() < deadline {
        if instance_status(compute, vm).await == want {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Err(format!("{vm} did not reach {want}"))
}

fn providers() -> BindingsProvider {
    let binding: serde_json::Value = serde_json::from_str(&env("G_BINDING")).expect("binding JSON");
    let ips = list("G_GATEWAY_IPS");
    let url = |port: u16| format!("http://{}:{port}", ips[0]);
    let gateway = serde_json::json!({
        "service": "api",
        "containerName": alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
        "internalUrl": url(3128),
        "internalUrls": {"3128": url(3128), "3129": url(3129)},
    });
    let bindings = HashMap::from([
        ("box".to_string(), binding),
        (
            "secrets".to_string(),
            serde_json::json!({"service": "secret-manager", "vaultPrefix": env("G_VAULT_PREFIX")}),
        ),
        (
            alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string(),
            gateway,
        ),
    ]);
    BindingsProvider::new(ClientConfig::Gcp(Box::new(gcp_config())), bindings).expect("bindings")
}

#[tokio::test]
#[ignore = "runs inside the setup's VPC against live gateway VMs; see the module docs"]
async fn live_internet_proof() {
    let provider = providers();
    let sandbox = provider
        .load_sandbox("box")
        .await
        .expect("the binding loads");
    let sandbox: &dyn Sandbox = sandbox.as_ref();
    let vms = list("G_GATEWAY_VMS");
    assert_eq!(vms.len(), 2, "two gateway VMs");
    let compute = ComputeClient::new(reqwest::Client::new(), gcp_config());

    let mut proof = Proof {
        log: Vec::new(),
        failures: Vec::new(),
    };
    let mut created: Vec<String> = Vec::new();
    let outcome: Result<(), String> = async {
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
        for host in [HOST, OTHER] {
            let seen = proxy(sandbox, &control.sandbox_id, host, 443, "").await;
            proof.check(
                connected(&seen),
                &format!("control reaches {host}:443"),
                &seen,
            );
        }

        let subject = sandbox
            .create(CreateSandboxRequest {
                egress: Some(allow(&[&format!("{HOST}:443")])),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("subject create: {error}"))?;
        let id = subject.sandbox_id.clone();
        created.push(id.clone());
        proof.note(format!("[create] subject {id}"));

        // (a)
        let seen = proxy(sandbox, &id, HOST, 443, "").await;
        proof.check(connected(&seen), "a allowed host:443", &seen);
        let seen = proxy(sandbox, &id, HOST, 8443, "").await;
        proof.check(refused(&seen), "a same host on 8443", &seen);
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(refused(&seen), "a another host", &seen);

        // (b) live rule write, same sandbox.
        sandbox
            .update_egress(
                &id,
                allow(&[&format!("{HOST}:443"), &format!("{HOST}:8443")]),
            )
            .await
            .map_err(|error| format!("update_egress: {error}"))?;
        let seen = proxy(sandbox, &id, HOST, 8443, "").await;
        proof.check(connected(&seen), "b host:8443 after update_egress", &seen);
        let seen = proxy(sandbox, &id, OTHER, 443, "").await;
        proof.check(refused(&seen), "b another host still refused", &seen);

        // (c) metadata, direct IPs, the admin port.
        for (host, port) in [("169.254.169.254", 80), ("metadata.google.internal", 80)] {
            let seen = proxy(sandbox, &id, host, port, "").await;
            proof.check(
                !connected(&seen),
                &format!("c metadata {host}:{port} through the proxy"),
                &seen,
            );
            let seen = direct(sandbox, &id, host, port).await;
            proof.check(
                seen.starts_with("DIRECT-NONE"),
                &format!("c metadata {host}:{port} direct"),
                &seen,
            );
        }
        for (ip, port) in [
            ("1.1.1.1", 443),
            ("1.1.1.1", 80),
            ("1.1.1.1", 8443),
            ("8.8.8.8", 443),
        ] {
            let seen = direct(sandbox, &id, ip, port).await;
            proof.check(
                seen.starts_with("DIRECT-NONE"),
                &format!("c direct {ip}:{port}"),
                &seen,
            );
        }
        for ip in list("G_GATEWAY_IPS") {
            let seen = direct(sandbox, &id, &ip, 3129).await;
            proof.check(
                seen.starts_with("DIRECT-NONE"),
                &format!("c gateway admin {ip}:3129 from the guest"),
                &seen,
            );
        }
        let seen = proxy(sandbox, &id, "10.0.0.1", 443, "").await;
        proof.check(
            !connected(&seen),
            "c a private address through the proxy",
            &seen,
        );

        // (e) the control channel from inside.
        let seen = run(sandbox, &id, "python3", &["-c", CONTROL_PROBE]).await;
        proof.note(format!(
            "[e] control probe output: {}",
            seen.replace('\n', " | ")
        ));
        let codes: Vec<&str> = seen.lines().filter(|l| !l.starts_with("uid")).collect();
        proof.check(
            codes.len() == 3
                && codes.iter().all(|line| {
                    line.rsplit(' ')
                        .next()
                        .and_then(|code| code.parse::<u16>().ok())
                        .is_some_and(|code| matches!(code, 401 | 403))
                }),
            "e every control operation from the exec user is refused (401/403)",
            &seen.replace('\n', " | "),
        );
        let seen = proxy(sandbox, &id, HOST, 443, "").await;
        proof.check(
            connected(&seen),
            "e the forwarder is unchanged afterwards",
            &seen,
        );

        // (f) failover.
        proof.note(format!(
            "[f] gateway states: {} {}",
            instance_status(&compute, &vms[0]).await,
            instance_status(&compute, &vms[1]).await
        ));
        power(&compute, &vms[0], false).await?;
        let mut seen = String::new();
        for _ in 0..6 {
            seen = proxy(sandbox, &id, HOST, 443, "").await;
            if connected(&seen) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        proof.check(
            connected(&seen),
            "f one gateway stopped: still reaches the host",
            &seen,
        );
        power(&compute, &vms[1], false).await?;
        let seen = proxy(sandbox, &id, HOST, 443, "").await;
        proof.check(!connected(&seen), "f both gateways stopped: refused", &seen);
        let seen = direct(sandbox, &id, "1.1.1.1", 443).await;
        proof.check(
            seen.starts_with("DIRECT-NONE"),
            "f both stopped: a direct IP still never connects",
            &seen,
        );
        power(&compute, &vms[0], true).await?;
        power(&compute, &vms[1], true).await?;
        // The gateways come back on their own; wait for an answer of any kind.
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut seen = String::new();
        while Instant::now() < deadline {
            seen = proxy(sandbox, &id, HOST, 443, "").await;
            if seen.contains("HTTP/1.1") {
                break;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        // Both restarted together: no peer holds the policy, so the grant is gone and the
        // sandbox is denied (closed) until the manager writes it again.
        proof.check(
            seen.contains("HTTP/1.1"),
            "f both started again: the gateway answers",
            &seen,
        );
        proof.note(format!(
            "[f] both restarted together, before any write: {seen}"
        ));
        sandbox
            .update_egress(
                &id,
                allow(&[&format!("{HOST}:443"), &format!("{HOST}:8443")]),
            )
            .await
            .map_err(|error| format!("update_egress after the restart: {error}"))?;
        let seen = proxy(sandbox, &id, HOST, 8443, "").await;
        proof.check(
            connected(&seen),
            "f after both restarted and the rule was written again",
            &seen,
        );
        Ok(())
    }
    .await;
    if let Err(error) = &outcome {
        proof.failures.push(error.clone());
    }
    for id in &created {
        if let Err(error) = sandbox.terminate(id).await {
            proof.note(format!("[cleanup] terminate {id}: {error}"));
        }
    }
    // Leave both gateway VMs running whatever happened.
    for vm in &vms {
        let _ = power(&compute, vm, true).await;
    }
    if let Ok(path) = std::env::var("G_RESULT") {
        let _ = std::fs::write(path, proof.log.join("\n"));
    }
    assert!(
        proof.failures.is_empty(),
        "failed rows: {:#?}",
        proof.failures
    );
}

#[tokio::test]
#[ignore = "creates the sandbox template that connects the network attachment for good; see the module docs"]
async fn prepare_template() {
    use alien_core::bindings::{
        SandboxBinding, SandboxEgressBinding, SandboxEgressGateway, SandboxEgressReplicas,
        SandboxEgressSigningKey,
    };
    use alien_gcp_clients::gcp::agent_platform::{
        AgentPlatformApi, AgentPlatformClient, ContainerPort, ContainerResources,
        CustomContainerEnvironment, CustomContainerSpec, EgressControlConfig, IngressControlConfig,
        PollBudget, SandboxEnvironmentTemplate,
    };

    let config = gcp_config();
    let region = config.region.clone();
    let client = AgentPlatformClient::new(reqwest::Client::new(), config);
    let engine = env("G_ENGINE");
    let engine_segment = engine.rsplit('/').next().unwrap().to_string();
    let budget = PollBudget {
        interval: Duration::from_secs(4),
        max_attempts: 200,
    };
    let body = SandboxEnvironmentTemplate {
        name: None,
        display_name: Some("g2979-gateway-template".to_string()),
        custom_container_environment: Some(CustomContainerEnvironment {
            custom_container_spec: Some(CustomContainerSpec {
                image_uri: env("G_IMAGE"),
                extra: Default::default(),
            }),
            resources: Some(ContainerResources {
                requests: None,
                limits: Some(HashMap::from([
                    ("cpu".to_string(), "2".to_string()),
                    ("memory".to_string(), "4Gi".to_string()),
                ])),
            }),
            ports: vec![ContainerPort {
                port: 8971,
                protocol: Some("TCP".to_string()),
            }],
            extra: Default::default(),
        }),
        egress_control_config: Some(EgressControlConfig {
            internet_access: Some(false),
            network_attachment: Some(env("G_ATTACHMENT")),
            extra: Default::default(),
        }),
        ingress_control_config: Some(IngressControlConfig {
            enable_private_service_connect: Some(true),
            extra: Default::default(),
        }),
        state: None,
        extra: Default::default(),
    };
    let operation = client
        .create_template(&engine_segment, body)
        .await
        .expect("template create accepted");
    let created: SandboxEnvironmentTemplate = client
        .await_operation(&operation, budget.clone())
        .await
        .expect("template create operation resolves");
    let template = created.name.expect("a created template has a name");
    client
        .await_template_active(
            &engine_segment,
            template.rsplit('/').next().unwrap(),
            budget,
        )
        .await
        .expect("the template reaches ACTIVE (the attachment connected)");

    let mut binding =
        SandboxBinding::gcp_agent_platform(engine, template, region, Some(3600), false);
    if let SandboxBinding::GcpAgentPlatform(value) = &mut binding {
        value.egress = Some(SandboxEgressBinding::Gateway(SandboxEgressGateway {
            container: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into(),
            audience: env("G_AUDIENCE"),
            signing_key: SandboxEgressSigningKey {
                vault: alien_core::SECRETS_VAULT_ID.into(),
                key: "sandbox-egress-signing-key".into(),
            },
            kid: env("G_KID"),
            admin_ca_pem: None,
            replicas: Some(SandboxEgressReplicas {
                dns: String::new(),
                addresses: list("G_GATEWAY_IPS"),
                port: 3129,
                count: 2,
                min_protocol: alien_core::bindings::EGRESS_GATEWAY_ADMIN_PROTOCOL,
                proxy_port: 22,
            }),
        }));
    }
    std::fs::write(
        env("G_BINDING_OUT"),
        serde_json::to_string(&binding).unwrap(),
    )
    .expect("the binding is written");
}
