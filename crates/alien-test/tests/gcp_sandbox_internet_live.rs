//! Google Cloud sandbox egress `internet` through the egress gateway, proven against a real
//! project. This is the check that decides when `INTERNET_EGRESS_AVAILABLE` flips: until it
//! passes, every plan-time path refuses a Google Cloud `internet` sandbox.
//!
//! The test turns the switch on for its own process (the override exists only in test builds),
//! then deploys `test-apps/sandbox-internet` with direct setup through the in-process manager:
//! the network attachment, the gateway subnet, NAT, firewall rules, both gateway VMs and the
//! template that names the attachment are all created by the product's own setup. It then opens
//! sandboxes and proves, from inside them:
//!
//! - an allowed host connects through the gateway;
//! - another host, and the allowed host on another port, are refused;
//! - `update_egress` adds a port, applied on the next connection of the same sandbox;
//! - nothing leaves outside the gateway: direct connections to public addresses, the metadata
//!   server and the gateway admin port never connect;
//! - a process in the sandbox cannot use the agent's control operations.
//!
//! A template with a network attachment has private ingress: its sandboxes answer only through
//! the deployment's Private Service Connect endpoint, an internal address, and the gateway admin
//! port answers only inside the VPC. The test therefore creates a small relay VM in the
//! deployment's gateway subnet (no public address) and reaches it through an IAP tunnel that
//! `gcloud` opens. The relay and its firewall rule carry the run's resource prefix and are
//! deleted before the deployment is torn down, on every exit path the test controls.
//!
//! Run by hand, against the test project only (it creates network resources):
//!
//! ```text
//! GOOGLE_TARGET_PROJECT_ID=... GOOGLE_TARGET_REGION=us-central1 \
//! GOOGLE_TARGET_ACCESS_TOKEN=<short-lived token> \
//! ALIEN_E2E_RESOURCE_PREFIX=<run prefix> \
//! ALIEN_TEST_PROOF_LOG=/path/to/proof.log \
//!   cargo test -p alien-test --test gcp_sandbox_internet_live -- --ignored --nocapture
//! ```
//!
//! plus the variables the rest of the GCP E2E suite needs (manager, registry);
//! `GOOGLE_TARGET_SERVICE_ACCOUNT_KEY` works in place of the token. `gcloud` must be on the
//! PATH. With a token it opens the IAP tunnels with that token; with a key it uses its own login,
//! which must be a principal that may open IAP tunnels in the project.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::gcp_sandbox_gateway as plan;
use alien_core::{
    BindingValue, ClientConfig, ContainerBinding, EgressPolicy, GcpClientConfig, GcpCredentials,
    HorizonContainerBinding, Platform,
};
use alien_gcp_clients::gcp::compute::{
    AttachedDisk, AttachedDiskInitializeParams, ComputeApi, ComputeClient, Firewall,
    FirewallAllowed, FirewallDirection, Instance, Metadata, MetadataItem, NetworkInterface, Tags,
};
use alien_test::{DeploymentModel, TestApp};
use anyhow::{bail, Context};
use futures::StreamExt;
use test_context::test_context;

/// Open on 443 and 8443, so a refusal on either is the policy and nothing else.
const HOST: &str = "www.cloudflare.com";
const OTHER: &str = "example.com";
const SANDBOX: &str = "agents";
const RELAY_TAG: &str = "alien-live-relay";
/// Google's documented source range for IAP TCP forwarding.
const IAP_RANGE: &str = "35.235.240.0/20";
const RELAY_ENDPOINT_PORT: u16 = 10000;
const RELAY_ADMIN_PORT: u16 = 10001;

struct GcpInternetSandbox {
    ctx: alien_test::TestContext,
}

impl test_context::AsyncTestContext for GcpInternetSandbox {
    async fn setup() -> Self {
        // Build, manager and controllers run on this process's worker threads.
        plan::enable_internet_egress_for_process();
        alien_test::e2e::init_tracing();
        let ctx = alien_test::e2e::setup(
            Platform::Gcp,
            DeploymentModel::Push,
            TestApp::SandboxInternet,
        )
        .await
        .expect("the sandbox-internet stack deploys on GCP");
        Self { ctx }
    }

    async fn teardown(self) {
        self.ctx
            .cleanup()
            .await
            .expect("E2E cleanup must reach a safe terminal state");
    }
}

fn gcp_config() -> GcpClientConfig {
    let gcp = alien_test::config::TestConfig::from_env()
        .gcp_target
        .expect("GOOGLE_TARGET_* is set");
    GcpClientConfig {
        project_id: gcp.project_id.clone(),
        region: gcp.region.clone(),
        credentials: gcp
            .credentials()
            .expect("GOOGLE_TARGET_ACCESS_TOKEN or GOOGLE_TARGET_SERVICE_ACCOUNT_KEY is set"),
        service_overrides: None,
        project_number: None,
    }
}

// ---- Probes, run as commands in the sandbox ----------------------------------------------------

/// One CONNECT through the sandbox's own proxy variables. Prints `PROXY <status line> <seconds>`
/// or `PROXY-NONE <error> <seconds>`.
const PROXY_PROBE: &str = r#"
import base64, os, socket, sys, time, urllib.parse
host, port = sys.argv[1], int(sys.argv[2])
env = urllib.parse.urlparse(os.environ["HTTPS_PROXY"])
basic = base64.b64encode(f"{env.username}:{env.password}".encode()).decode()
t = time.time()
try:
    s = socket.create_connection((env.hostname, env.port), timeout=30)
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
import json, os, urllib.request, urllib.error
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
print("setEgressProxy", call("POST", "/", {"v": 1, "op": "setEgressProxy", "replicas": ["192.168.251.10:22"]}))
print("updateEgress-op", call("POST", "/", {"v": 1, "op": "updateEgress", "token": "x", "policy": "internet"}))
print("PUT-v1-egress", call("PUT", "/v1/egress", {"token": "x", "policy": "internet"}))
"#;

/// The relay's boot script: forwards each local port to one address inside the VPC.
fn relay_script(endpoint: &str, admin: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
import socket, threading
def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except Exception:
        pass
    finally:
        a.close(); b.close()
def serve(port, target):
    host, tport = target.rsplit(":", 1)
    listener = socket.socket(); listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("0.0.0.0", port)); listener.listen(64)
    while True:
        client, _ = listener.accept()
        try:
            upstream = socket.create_connection((host, int(tport)), timeout=30)
        except Exception:
            client.close(); continue
        threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
        threading.Thread(target=pipe, args=(upstream, client), daemon=True).start()
threading.Thread(target=serve, args=({RELAY_ENDPOINT_PORT}, "{endpoint}"), daemon=True).start()
serve({RELAY_ADMIN_PORT}, "{admin}")
"#
    )
}

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

async fn run(sandbox: &dyn Sandbox, id: &str, args: &[&str]) -> String {
    let stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: "python3".to_string(),
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

async fn proxy(sandbox: &dyn Sandbox, id: &str, host: &str, port: u16) -> String {
    run(sandbox, id, &["-c", PROXY_PROBE, host, &port.to_string()]).await
}

async fn direct(sandbox: &dyn Sandbox, id: &str, host: &str, port: u16) -> String {
    run(sandbox, id, &["-c", DIRECT_PROBE, host, &port.to_string()]).await
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

// ---- The deployment, as the cloud and the stack state describe it -----------------------------

struct Deployed {
    /// The sandbox binding the manager published for remote access.
    binding: serde_json::Value,
    resource_prefix: String,
    gateway_ips: Vec<String>,
    private_endpoint: String,
    /// Zone, network and subnetwork of the first gateway VM.
    zone: String,
    network: String,
    gateway_subnet: String,
}

async fn deployed(
    ctx: &alien_test::TestContext,
    compute: &ComputeClient,
) -> anyhow::Result<Deployed> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
    let state: alien_core::StackState = serde_json::from_value(
        response
            .into_inner()
            .stack_state
            .context("deployment is missing stack_state")?,
    )
    .context("parse stack_state")?;
    let binding = state
        .resources
        .get(SANDBOX)
        .and_then(|resource| resource.remote_binding_params.clone())
        .context("the sandbox published no binding")?;
    let private_endpoint = binding
        .pointer("/privateEndpoint")
        .and_then(|value| value.as_str())
        .context("the binding names no private endpoint")?
        .to_string();
    let gateway_ips: Vec<String> = binding
        .pointer("/egress/gateway/replicas/addresses")
        .and_then(|value| value.as_array())
        .context("the binding names no gateway replicas")?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    let resource_prefix = state.resource_prefix.clone();
    let vm = plan::resource_name(&resource_prefix, &plan::suffix::vm(0));
    let region = gcp_config().region;
    let mut zones: Vec<String> = compute
        .list_zones(Some("status=UP".into()))
        .await
        .context("list zones")?
        .items
        .into_iter()
        .filter_map(|zone| zone.name)
        .filter(|name| name.starts_with(&format!("{region}-")))
        .collect();
    zones.sort();
    for zone in zones {
        if let Ok(instance) = compute.get_instance(zone.clone(), vm.clone()).await {
            let nic = instance
                .network_interfaces
                .first()
                .context("the gateway VM has no interface")?;
            return Ok(Deployed {
                binding,
                resource_prefix,
                gateway_ips,
                private_endpoint,
                zone,
                network: nic.network.clone().context("no network")?,
                gateway_subnet: nic.subnetwork.clone().context("no subnetwork")?,
            });
        }
    }
    bail!("gateway VM {vm} is not in any zone of {region}")
}

// ---- The relay ---------------------------------------------------------------------------------

struct Relay {
    vm: String,
    firewall: String,
    zone: String,
}

fn relay_names(deployed: &Deployed) -> Relay {
    Relay {
        vm: plan::resource_name(&deployed.resource_prefix, "live-relay"),
        firewall: plan::resource_name(&deployed.resource_prefix, "live-relay-iap"),
        zone: deployed.zone.clone(),
    }
}

async fn create_relay(
    compute: &ComputeClient,
    deployed: &Deployed,
    relay: &Relay,
    proof: &mut Proof,
) -> anyhow::Result<()> {
    let firewall = Firewall {
        name: Some(relay.firewall.clone()),
        network: Some(deployed.network.clone()),
        direction: Some(FirewallDirection::Ingress),
        priority: Some(1000),
        source_ranges: vec![IAP_RANGE.to_string()],
        target_tags: vec![RELAY_TAG.to_string()],
        allowed: vec![FirewallAllowed {
            ip_protocol: Some("tcp".into()),
            ports: vec![
                RELAY_ENDPOINT_PORT.to_string(),
                RELAY_ADMIN_PORT.to_string(),
            ],
        }],
        ..Default::default()
    };
    let operation = compute
        .insert_firewall(firewall)
        .await
        .context("create the relay firewall rule")?;
    if let Some(name) = operation.name {
        compute.wait_global_operation(name).await?;
    }
    proof.note(format!("[relay] created firewall rule {}", relay.firewall));
    let vm = Instance {
        name: Some(relay.vm.clone()),
        machine_type: Some(format!("zones/{}/machineTypes/e2-small", relay.zone)),
        tags: Some(Tags {
            items: vec![RELAY_TAG.to_string()],
            ..Default::default()
        }),
        disks: vec![AttachedDisk {
            boot: Some(true),
            auto_delete: Some(true),
            initialize_params: Some(AttachedDiskInitializeParams {
                source_image: Some(
                    "projects/debian-cloud/global/images/family/debian-12".to_string(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }],
        network_interfaces: vec![NetworkInterface {
            subnetwork: Some(deployed.gateway_subnet.clone()),
            ..Default::default()
        }],
        metadata: Some(Metadata {
            items: vec![MetadataItem {
                key: Some("startup-script".into()),
                value: Some(relay_script(
                    &deployed.private_endpoint,
                    &format!("{}:{}", deployed.gateway_ips[0], plan::ADMIN_PORT),
                )),
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let operation = compute
        .insert_instance(relay.zone.clone(), vm)
        .await
        .context("create the relay VM")?;
    if let Some(name) = operation.name {
        compute
            .wait_zone_operation(relay.zone.clone(), name)
            .await?;
    }
    proof.note(format!("[relay] created VM {} in {}", relay.vm, relay.zone));
    Ok(())
}

async fn delete_relay(compute: &ComputeClient, relay: &Relay, proof: &mut Proof) {
    match compute
        .delete_instance(relay.zone.clone(), relay.vm.clone())
        .await
    {
        Ok(operation) => {
            if let Some(name) = operation.name {
                let _ = compute.wait_zone_operation(relay.zone.clone(), name).await;
            }
            proof.note(format!("[relay] deleted VM {}", relay.vm));
        }
        Err(error) => proof.note(format!("[relay] FAILED to delete VM {}: {error}", relay.vm)),
    }
    match compute.delete_firewall(relay.firewall.clone()).await {
        Ok(operation) => {
            if let Some(name) = operation.name {
                let _ = compute.wait_global_operation(name).await;
            }
            proof.note(format!("[relay] deleted firewall rule {}", relay.firewall))
        }
        Err(error) => proof.note(format!(
            "[relay] FAILED to delete firewall rule {}: {error}",
            relay.firewall
        )),
    }
}

/// An IAP tunnel from a local port to one relay port, open until dropped.
async fn tunnel(relay: &Relay, port: u16) -> anyhow::Result<(tokio::process::Child, u16)> {
    let local = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let mut gcloud = tokio::process::Command::new("gcloud");
    if let Some(path) = gcloud_token_file() {
        gcloud.arg(format!("--access-token-file={}", path.display()));
    }
    let child = gcloud
        .args([
            "compute",
            "start-iap-tunnel",
            &relay.vm,
            &port.to_string(),
            &format!("--local-host-port=127.0.0.1:{local}"),
            &format!("--zone={}", relay.zone),
            &format!("--project={}", gcp_config().project_id),
        ])
        .kill_on_drop(true)
        .spawn()
        .context("start gcloud compute start-iap-tunnel")?;
    // The relay boots and the tunnel opens; wait until the local port accepts.
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", local))
            .await
            .is_ok()
        {
            return Ok((child, local));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    bail!("the tunnel to {}:{port} did not open", relay.vm)
}

/// With `GOOGLE_TARGET_ACCESS_TOKEN` set, gcloud opens the tunnels with that same token, from a
/// private temporary file that lives as long as the test process; no gcloud login is used.
fn gcloud_token_file() -> Option<&'static std::path::Path> {
    static FILE: std::sync::OnceLock<Option<tempfile::NamedTempFile>> = std::sync::OnceLock::new();
    FILE.get_or_init(|| {
        let gcp = alien_test::config::TestConfig::from_env().gcp_target?;
        let token = gcp.access_token?;
        let file = tempfile::NamedTempFile::new().expect("a temporary token file");
        std::fs::write(file.path(), token).expect("write the token file");
        Some(file)
    })
    .as_ref()
    .map(|file| file.path())
}

/// The provider, as a caller in the VPC would load it: the published sandbox binding, the
/// gateway Container and the secrets vault, with the two VPC-only addresses reached through the
/// tunnels.
fn provider(
    deployed: &Deployed,
    endpoint_port: u16,
    admin_port: u16,
) -> anyhow::Result<BindingsProvider> {
    let mut binding = deployed.binding.clone();
    binding["privateEndpoint"] = serde_json::json!(format!("127.0.0.1:{endpoint_port}"));
    let admin = format!("http://127.0.0.1:{admin_port}");
    let proxy = format!(
        "http://{}:{}",
        deployed.gateway_ips[0],
        plan::CONTAINER_PROXY_PORT
    );
    let gateway = serde_json::to_value(ContainerBinding::Horizon(HorizonContainerBinding {
        container_name: BindingValue::Value(
            alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string(),
        ),
        internal_url: BindingValue::Value(proxy.clone()),
        public_url: None,
        internal_urls: HashMap::from([
            (plan::CONTAINER_PROXY_PORT, BindingValue::Value(proxy)),
            (plan::ADMIN_PORT, BindingValue::Value(admin)),
        ]),
    }))?;
    let bindings = HashMap::from([
        (SANDBOX.to_string(), binding),
        (
            alien_core::SECRETS_VAULT_ID.to_string(),
            serde_json::json!({
                "service": "secret-manager",
                "vaultPrefix": format!("{}-{}", deployed.resource_prefix, alien_core::SECRETS_VAULT_ID),
            }),
        ),
        (
            alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string(),
            gateway,
        ),
    ]);
    BindingsProvider::new(ClientConfig::Gcp(Box::new(gcp_config())), bindings)
        .map_err(|error| anyhow::anyhow!("bindings: {error}"))
}

async fn prove(
    sandbox: &dyn Sandbox,
    deployed: &Deployed,
    proof: &mut Proof,
    created: &mut Vec<String>,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let subject = sandbox
        .create(CreateSandboxRequest {
            egress: Some(allow(&[&format!("{HOST}:443")])),
            ..Default::default()
        })
        .await
        .map_err(|error| anyhow::anyhow!("create: {error}"))?;
    let id = subject.sandbox_id.clone();
    created.push(id.clone());
    proof.note(format!("[create] {id} in {:?}", started.elapsed()));

    // An allowed host works; another host, and the same host on another port, are refused.
    let seen = proxy(sandbox, &id, HOST, 443).await;
    proof.check(connected(&seen), "allowed host:443 connects", &seen);
    let seen = proxy(sandbox, &id, HOST, 8443).await;
    proof.check(refused(&seen), "allowed host on 8443 is refused", &seen);
    let seen = proxy(sandbox, &id, OTHER, 443).await;
    proof.check(refused(&seen), "another host is refused", &seen);

    // update_egress, applied on the next connection of the same sandbox.
    sandbox
        .update_egress(
            &id,
            allow(&[&format!("{HOST}:443"), &format!("{HOST}:8443")]),
        )
        .await
        .map_err(|error| anyhow::anyhow!("update_egress: {error}"))?;
    let seen = proxy(sandbox, &id, HOST, 8443).await;
    proof.check(
        connected(&seen),
        "host:8443 connects after update_egress",
        &seen,
    );
    let seen = proxy(sandbox, &id, OTHER, 443).await;
    proof.check(refused(&seen), "another host is still refused", &seen);

    // Nothing leaves outside the gateway.
    for (host, port) in [
        ("1.1.1.1", 443),
        ("1.1.1.1", 80),
        ("8.8.8.8", 443),
        (HOST, 443),
        ("169.254.169.254", 80),
        ("metadata.google.internal", 80),
    ] {
        let seen = direct(sandbox, &id, host, port).await;
        proof.check(
            seen.starts_with("DIRECT-NONE"),
            &format!("direct {host}:{port} never connects"),
            &seen,
        );
    }
    for ip in &deployed.gateway_ips {
        let seen = direct(sandbox, &id, ip, plan::ADMIN_PORT).await;
        proof.check(
            seen.starts_with("DIRECT-NONE"),
            &format!(
                "gateway admin {ip}:{} is closed to the sandbox",
                plan::ADMIN_PORT
            ),
            &seen,
        );
    }
    for (host, port) in [("169.254.169.254", 80), ("10.0.0.1", 443)] {
        let seen = proxy(sandbox, &id, host, port).await;
        proof.check(
            !connected(&seen),
            &format!("{host}:{port} through the gateway is refused"),
            &seen,
        );
    }

    // The control channel is closed to code in the sandbox.
    let seen = run(sandbox, &id, &["-c", CONTROL_PROBE]).await;
    let codes: Vec<&str> = seen.lines().collect();
    proof.check(
        codes.len() == 3
            && codes.iter().all(|line| {
                line.rsplit(' ')
                    .next()
                    .and_then(|code| code.parse::<u16>().ok())
                    .is_some_and(|code| matches!(code, 401 | 403))
            }),
        "every control operation from the sandbox is refused (401/403)",
        &seen.replace('\n', " | "),
    );
    Ok(())
}

#[test_context(GcpInternetSandbox)]
#[tokio::test]
#[ignore = "creates network resources and sandboxes in the GCP test project; see the module docs"]
async fn gcp_internet_sandbox_egress_goes_through_the_gateway_only(ctx: &mut GcpInternetSandbox) {
    let compute = ComputeClient::new(reqwest::Client::new(), gcp_config());
    let mut proof = Proof {
        log: Vec::new(),
        failures: Vec::new(),
    };
    let deployed = deployed(&ctx.ctx, &compute)
        .await
        .expect("the deployment's gateway wiring is readable");
    proof.note(format!(
        "[deployment] prefix {} gateways {:?} endpoint {}",
        deployed.resource_prefix, deployed.gateway_ips, deployed.private_endpoint
    ));

    let mut created = Vec::new();
    let relay = relay_names(&deployed);
    let created_relay = create_relay(&compute, &deployed, &relay, &mut proof).await;
    let outcome = match &created_relay {
        Ok(()) => {
            async {
                let (_endpoint_tunnel, endpoint_port) = tunnel(&relay, RELAY_ENDPOINT_PORT).await?;
                let (_admin_tunnel, admin_port) = tunnel(&relay, RELAY_ADMIN_PORT).await?;
                let provider = provider(&deployed, endpoint_port, admin_port)?;
                let sandbox = provider
                    .load_sandbox(SANDBOX)
                    .await
                    .map_err(|error| anyhow::anyhow!("load the sandbox binding: {error}"))?;
                let outcome = prove(sandbox.as_ref(), &deployed, &mut proof, &mut created).await;
                for id in &created {
                    if let Err(error) = sandbox.terminate(id).await {
                        proof.note(format!("[cleanup] terminate {id}: {error}"));
                    }
                }
                outcome
            }
            .await
        }
        Err(error) => Err(anyhow::anyhow!("relay: {error:#}")),
    };
    // The relay sits in the gateway subnet: it goes before the deployment's teardown. Both
    // deletes run even when creation stopped half way (a rule without its VM still blocks the
    // network's deletion); a delete of a resource that was never created only logs a 404.
    delete_relay(&compute, &relay, &mut proof).await;
    if let Err(error) = &outcome {
        proof.note(format!("[error] {error:#}"));
    }
    if let Ok(path) = std::env::var("ALIEN_TEST_PROOF_LOG") {
        std::fs::write(&path, proof.log.join("\n") + "\n").expect("write the proof log");
    }
    outcome.expect("the live proof ran to the end");
    assert!(
        proof.failures.is_empty(),
        "failed rows: {:#?}",
        proof.failures
    );
}
