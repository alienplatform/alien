//! Whole-contract conformance for a root-supervisor image, through the bindings applications use.
//!
//! The image is built from `contract/` (root ENTRYPOINT, workload server as uid 1001). Every
//! check reports a verdict and what it observed; one failing check never hides the others.
//! Network denials follow the egress method of this crate: a failed control is `unproven`.

use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use alien_bindings::{
    traits::{CommandOutput, CreateSandboxRequest, PreviewCapability, RunCommandRequest, Sandbox},
    Bindings,
};
use alien_core::EgressPolicy;
use alien_error::{Context, IntoAlienError};
use alien_sandbox_conformance::{evaluate, Expectation, Observation, Probe, Verdict};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use super::{cleanup, invalid, operation, probe, Result, Session};

const FIXTURE_DIR: &str = "/opt/contract-conformance";
const DEFAULT_TOKEN: &str = "conformance-bearer-token";
const WORKLOAD_UID: u64 = 1001;
const MESSAGE_BYTES: usize = 1024 * 1024;
const CONCURRENT_REQUESTS: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Suite {
    backend: String,
    revision: String,
    /// Binding that declares the fixture image.
    binding: String,
    /// Bearer token the fixture checks. Defaults to the fixture's built-in value.
    token: Option<String>,
    /// `aws`, `gcp` or `azure`: which metadata endpoints the workload uid probes.
    metadata_backend: String,
    /// Boot identity expected for ENTRYPOINT (0 for a root-supervisor image).
    #[serde(default)]
    boot_uid: u64,
    /// Seconds a WebSocket stays idle before it is used again.
    #[serde(default = "default_idle")]
    idle_seconds: u64,
    /// Ports to prove. Defaults to the contract's 8080, 8081 and 8090.
    #[serde(default = "default_ports")]
    ports: Vec<u16>,
    /// Paired egress checks against a controlled listener. Omit when no listener exists.
    egress: Option<EgressSuite>,
}

fn default_idle() -> u64 {
    60
}

fn default_ports() -> Vec<u16> {
    vec![8080, 8081, 8090]
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressSuite {
    /// Public name or address of the controlled HTTP listener.
    host: String,
    port: u16,
    /// Digest of the listener's known response body.
    body_sha256: String,
    /// Reach the listener through the sandbox's proxy environment. Backends whose limit sits
    /// outside the guest (Kubernetes: NetworkPolicy plus the gateway) drop a direct connect, so
    /// the paired checks must use the gateway path there.
    #[serde(default)]
    use_proxy: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    name: String,
    verdict: Verdict,
    detail: String,
    elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    backend: String,
    revision: String,
    pub passed: bool,
    checks: Vec<Check>,
    termination_calls_completed: bool,
}

type Outcome = std::result::Result<String, String>;

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Deterministic, non-repeating payload so truncation or reordering changes the digest.
fn payload(length: usize, salt: u8) -> Vec<u8> {
    (0..length)
        .map(|index| (index.wrapping_mul(31).wrapping_add(index >> 8) as u8) ^ salt)
        .collect()
}

pub async fn run(bindings: &Bindings, suite: Suite, journal_path: &Path) -> Result<Report> {
    if !matches!(suite.metadata_backend.as_str(), "aws" | "gcp" | "azure") {
        return Err(invalid("metadataBackend must be aws, gcp or azure"));
    }
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(journal_path)
        .into_alien_error()
        .context(operation("create a fresh contract cleanup journal"))?;
    let provider = bindings
        .sandbox(&suite.binding)
        .await
        .context(operation("load contract binding"))?;
    let providers = BTreeMap::from([(suite.binding.clone(), provider.clone())]);
    let mut sessions: Vec<Session> = Vec::new();
    let mut checks = Vec::new();
    let result = async {
        let started = Instant::now();
        let subject = super::create_with(
            provider.as_ref(),
            &suite.binding,
            &mut journal,
            &mut sessions,
            CreateSandboxRequest::default(),
        )
        .await?;
        let id = sessions[subject].sandbox_id.clone();
        checks.push(Check {
            name: "create".into(),
            verdict: Verdict::Pass,
            detail: format!("session {id} ready"),
            elapsed_ms: started.elapsed().as_millis(),
        });
        run_checks(&suite, provider.clone(), &id, &mut checks).await;
        if let Some(egress) = &suite.egress {
            egress_checks(
                &suite,
                egress,
                provider.clone(),
                &mut journal,
                &mut sessions,
                &mut checks,
            )
            .await?;
        }
        Ok::<(), alien_error::AlienError<super::ErrorData>>(())
    }
    .await;
    let cleaned = cleanup(&providers, &sessions).await;
    match (result, cleaned) {
        (Ok(()), Ok(())) => Ok(Report {
            backend: suite.backend,
            revision: suite.revision,
            passed: checks.iter().all(|check| check.verdict == Verdict::Pass),
            checks,
            termination_calls_completed: true,
        }),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(run_error), Err(cleanup_error)) => Err(cleanup_error).context(operation(format!(
            "contract check also failed with {}; cleanup remains incomplete",
            run_error.code
        ))),
    }
}

macro_rules! check {
    ($checks:expr, $name:expr, $body:expr) => {{
        let started = Instant::now();
        let outcome: Outcome = $body.await;
        $checks.push(Check {
            name: $name.to_string(),
            verdict: if outcome.is_ok() {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            detail: outcome.unwrap_or_else(|error| error),
            elapsed_ms: started.elapsed().as_millis(),
        });
    }};
}

async fn run_checks(suite: &Suite, sandbox: Arc<dyn Sandbox>, id: &str, checks: &mut Vec<Check>) {
    let token = suite.token.clone().unwrap_or_else(|| DEFAULT_TOKEN.into());
    for port in &suite.ports {
        check!(
            checks,
            format!("http:{port}"),
            http_port(&*sandbox, id, *port, &token)
        );
    }
    // After the first HTTP answers: the ENTRYPOINT starts asynchronously after create, so a
    // census taken earlier would find no server yet.
    check!(
        checks,
        "censusJob",
        census_job(&*sandbox, id, suite.boot_uid)
    );
    check!(checks, "bootOnce", boot_once(&*sandbox, id, suite.boot_uid));
    for port in &suite.ports {
        check!(
            checks,
            format!("websocket:{port}"),
            websocket_port(&*sandbox, id, *port, &token, 0)
        );
    }
    check!(
        checks,
        format!("websocketIdle{}s", suite.idle_seconds),
        websocket_port(&*sandbox, id, suite.ports[0], &token, suite.idle_seconds)
    );
    check!(
        checks,
        format!("concurrent{CONCURRENT_REQUESTS}"),
        concurrent(&*sandbox, id, &suite.ports, &token)
    );
    check!(
        checks,
        "metadataAsWorkloadUid",
        metadata(
            &*sandbox,
            id,
            suite.ports[0],
            &token,
            &suite.metadata_backend
        )
    );
}

/// The caller's own Authorization replaces anything the capability carried.
async fn capability(
    sandbox: &dyn Sandbox,
    id: &str,
    port: u16,
) -> std::result::Result<PreviewCapability, String> {
    let capability = sandbox.preview(id, port).await.map_err(failed)?;
    if !capability.allowed_ports.contains(&port) {
        return Err(format!(
            "capability admits {:?}, not {port}",
            capability.allowed_ports
        ));
    }
    Ok(capability)
}

fn merged_headers(capability: &PreviewCapability, token: Option<&str>) -> BTreeMap<String, String> {
    let mut headers: BTreeMap<String, String> = capability
        .headers
        .iter()
        .filter(|(name, _)| !(token.is_some() && name.eq_ignore_ascii_case("authorization")))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if let Some(token) = token {
        headers.insert("Authorization".into(), format!("Bearer {token}"));
    }
    headers
}

fn http_client() -> std::result::Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(failed)
}

fn url(capability: &PreviewCapability, path: &str) -> String {
    format!("{}{path}", capability.endpoint.trim_end_matches('/'))
}

async fn get_json(
    client: &reqwest::Client,
    capability: &PreviewCapability,
    token: Option<&str>,
    path: &str,
) -> std::result::Result<(u16, serde_json::Value), String> {
    let mut request = client.get(url(capability, path));
    for (name, value) in merged_headers(capability, token) {
        request = request.header(name, value);
    }
    let response = request.send().await.map_err(failed)?;
    let status = response.status().as_u16();
    let body = response.bytes().await.map_err(failed)?;
    Ok((
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    ))
}

async fn http_port(sandbox: &dyn Sandbox, id: &str, port: u16, token: &str) -> Outcome {
    let capability = capability(sandbox, id, port).await?;
    let client = http_client()?;
    // The ENTRYPOINT starts asynchronously after create; the agent answers 502 until the daemon listens.
    let deadline = Instant::now() + Duration::from_secs(60);
    let (status, body) = loop {
        let (status, body) = get_json(&client, &capability, Some(token), "/whoami").await?;
        if status != 502 || Instant::now() >= deadline {
            break (status, body);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    if status != 200 {
        return Err(format!(
            "expected 200 with the caller's bearer, got {status}"
        ));
    }
    let expected = sha256(format!("Bearer {token}").as_bytes());
    if body["authorizationSha256"] != expected {
        return Err("the daemon received a different Authorization than the caller sent".into());
    }
    if body["port"] != port || body["uid"] != WORKLOAD_UID {
        return Err(format!("wrong daemon answered: {body}"));
    }
    // The daemon, not the broker, must be the one checking the bearer.
    let (denied, _) = get_json(&client, &capability, Some("wrong-token"), "/whoami").await?;
    if denied != 401 {
        return Err(format!(
            "a wrong bearer got {denied}, expected the daemon's 401"
        ));
    }
    Ok(format!(
        "200 as uid {WORKLOAD_UID}, Authorization unchanged, wrong bearer 401"
    ))
}

async fn websocket_port(
    sandbox: &dyn Sandbox,
    id: &str,
    port: u16,
    token: &str,
    idle_seconds: u64,
) -> Outcome {
    let capability = capability(sandbox, id, port).await?;
    let endpoint = capability.endpoint.trim_end_matches('/');
    let socket_url = match endpoint.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}/ws"),
        Some(("http", rest)) => format!("ws://{rest}/ws"),
        _ => {
            return Err(format!(
                "preview endpoint is not an http(s) URL: {endpoint}"
            ))
        }
    };
    let connect = |token: &str| {
        let mut request = socket_url.as_str().into_client_request().map_err(failed)?;
        for (name, value) in merged_headers(&capability, Some(token)) {
            let name =
                tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes())
                    .map_err(failed)?;
            request
                .headers_mut()
                .insert(name, HeaderValue::from_str(&value).map_err(failed)?);
        }
        Ok::<_, String>(request)
    };
    if tokio_tungstenite::connect_async(connect("wrong-token")?)
        .await
        .is_ok()
    {
        return Err("a WebSocket upgrade with a wrong bearer was accepted".into());
    }
    let (mut socket, _) = tokio_tungstenite::connect_async(connect(token)?)
        .await
        .map_err(failed)?;
    if idle_seconds > 0 {
        // Hold the stream open with no application traffic, then prove it still carries data.
        tokio::time::sleep(Duration::from_secs(idle_seconds)).await;
    }
    let outbound = payload(MESSAGE_BYTES, port as u8);
    socket
        .send(Message::Binary(outbound.clone().into()))
        .await
        .map_err(failed)?;
    let reply = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(data))) => return Ok(data),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(other)) => return Err(format!("unexpected frame {other:?}")),
                Some(Err(error)) => return Err(failed(error)),
                None => return Err("the stream closed before the echo".into()),
            }
        }
    })
    .await
    .map_err(|_| "no echo within 60 seconds".to_string())??;
    if sha256(&reply) != sha256(&outbound) {
        return Err(format!(
            "echo differs: {} bytes back for {} sent",
            reply.len(),
            outbound.len()
        ));
    }
    let _ = socket.close(None).await;
    Ok(format!(
        "{} bytes each way intact after {idle_seconds}s idle, wrong bearer refused",
        outbound.len()
    ))
}

async fn concurrent(sandbox: &dyn Sandbox, id: &str, ports: &[u16], token: &str) -> Outcome {
    let client = http_client()?;
    let mut capabilities = Vec::new();
    for port in ports {
        capabilities.push(capability(sandbox, id, *port).await?);
    }
    let requests = (0..CONCURRENT_REQUESTS).map(|index| {
        let capability = &capabilities[index % capabilities.len()];
        let client = client.clone();
        async move {
            let body = payload(4096 + index, index as u8);
            let mut request = client.post(url(capability, "/echo")).body(body.clone());
            for (name, value) in merged_headers(capability, Some(token)) {
                request = request.header(name, value);
            }
            let response = request.send().await.map_err(failed)?;
            let status = response.status().as_u16();
            let reply: serde_json::Value =
                serde_json::from_slice(&response.bytes().await.map_err(failed)?).map_err(failed)?;
            if status == 200 && reply["sha256"] == sha256(&body) {
                Ok(())
            } else {
                Err(format!("request {index}: status {status}, body {reply}"))
            }
        }
    });
    let results = futures::future::join_all(requests).await;
    let failures: Vec<String> = results
        .into_iter()
        .filter_map(std::result::Result::err)
        .collect();
    if failures.is_empty() {
        Ok(format!(
            "{CONCURRENT_REQUESTS} concurrent requests answered with intact bodies"
        ))
    } else {
        Err(format!(
            "{} of {CONCURRENT_REQUESTS} failed: {}",
            failures.len(),
            failures[0]
        ))
    }
}

async fn metadata(
    sandbox: &dyn Sandbox,
    id: &str,
    port: u16,
    token: &str,
    backend: &str,
) -> Outcome {
    let capability = capability(sandbox, id, port).await?;
    let (status, body) = get_json(
        &http_client()?,
        &capability,
        Some(token),
        &format!("/metadata/{backend}"),
    )
    .await?;
    if status != 200 {
        return Err(format!("metadata probe endpoint answered {status}"));
    }
    if body["uid"] != WORKLOAD_UID || body["exitCode"] != 0 {
        return Err(format!(
            "credential exclusion unproven: {}",
            body["failure"]
        ));
    }
    Ok(format!(
        "uid {WORKLOAD_UID} obtained no {backend} credential or credential endpoint"
    ))
}

/// Runs a guest command and returns stdout when it exits 0.
async fn run_guest(
    sandbox: &dyn Sandbox,
    id: &str,
    script: &str,
    args: &[&str],
) -> std::result::Result<String, String> {
    let script_path = format!("{FIXTURE_DIR}/{script}");
    let args = std::iter::once(script_path.as_str()).chain(args.iter().copied());
    run_python(sandbox, id, args, script).await
}

/// Runs `python3 <args>` in the guest and returns stdout; `label` names it in errors.
async fn run_python<'a>(
    sandbox: &dyn Sandbox,
    id: &str,
    args: impl Iterator<Item = &'a str>,
    label: &str,
) -> std::result::Result<String, String> {
    let request = RunCommandRequest {
        command: "python3".into(),
        args: args.map(str::to_string).collect(),
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(60),
    };
    let mut stream = sandbox.run_command(id, request).await.map_err(failed)?;
    let (mut stdout, mut exit) = (Vec::new(), None);
    while let Some(frame) = stream.next().await {
        match frame.map_err(failed)? {
            CommandOutput::Stdout { data, .. } if stdout.len() < 65536 => stdout.extend(data),
            CommandOutput::Exit { code, truncated } => exit = Some((code, truncated)),
            _ => {}
        }
    }
    if exit != Some((0, false)) {
        return Err(format!("{label} ended with {exit:?}"));
    }
    String::from_utf8(stdout).map_err(failed)
}

fn assert_census(census: &serde_json::Value, boot_uid: u64) -> Outcome {
    let processes = census["processes"]
        .as_array()
        .ok_or("census has no process list")?;
    let find = |script: &str| {
        processes.iter().find(|process| {
            process["argv"].as_array().is_some_and(|argv| {
                argv.iter()
                    .any(|part| part.as_str().is_some_and(|p| p.ends_with(script)))
            })
        })
    };
    let boot = find("boot.py").ok_or("the image ENTRYPOINT is not running")?;
    let server = find("server.py").ok_or("the workload server is not running")?;
    if boot["uid"] != boot_uid {
        return Err(format!(
            "ENTRYPOINT runs as uid {}, expected {boot_uid}",
            boot["uid"]
        ));
    }
    if server["uid"] != WORKLOAD_UID || server["capEff"] != 0 {
        return Err(format!(
            "workload server is uid {} CapEff {}",
            server["uid"], server["capEff"]
        ));
    }
    let census_job = find("census.py").ok_or("the census command is not in its own census")?;
    if census_job["uid"] == 0 || census_job["capEff"] != 0 {
        return Err(format!(
            "a command runs as uid {} CapEff {}",
            census_job["uid"], census_job["capEff"]
        ));
    }
    Ok(format!(
        "ENTRYPOINT uid {} CapEff {:#x}; workload uid {} CapEff 0; command uid {} CapEff 0",
        boot["uid"],
        boot["capEff"].as_u64().unwrap_or(0),
        server["uid"],
        census_job["uid"]
    ))
}

async fn census_job(sandbox: &dyn Sandbox, id: &str, boot_uid: u64) -> Outcome {
    let request = RunCommandRequest {
        command: "python3".into(),
        args: vec![format!("{FIXTURE_DIR}/census.py")],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(60),
    };
    let job = sandbox.start_job(id, request).await.map_err(failed)?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut stdout = Vec::new();
    let mut sequence = None;
    loop {
        let poll = sandbox
            .poll_job(id, &job.job_id, sequence)
            .await
            .map_err(failed)?;
        for frame in poll.frames {
            match frame {
                CommandOutput::Stdout { seq, data } => {
                    sequence = Some(seq);
                    stdout.extend(data);
                }
                CommandOutput::Stderr { seq, .. } => sequence = Some(seq),
                CommandOutput::Exit { .. } => {}
            }
        }
        if let Some(error) = poll.error {
            return Err(format!("job ended without exiting: {}", error.code));
        }
        if let Some(exit) = poll.exit {
            if exit.code != 0 || exit.truncated {
                return Err(format!("census job exit {exit:?}"));
            }
            break;
        }
        if Instant::now() > deadline {
            return Err("census job still running after 90 seconds".into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let census = serde_json::from_slice(&stdout).map_err(failed)?;
    assert_census(&census, boot_uid)
}

async fn boot_once(sandbox: &dyn Sandbox, id: &str, boot_uid: u64) -> Outcome {
    let output = run_guest(sandbox, id, "census.py", &[]).await?;
    let census: serde_json::Value = serde_json::from_str(&output).map_err(failed)?;
    let detail = assert_census(&census, boot_uid)?;
    // Read it from inside the guest: a backend's file API may not see the guest's /tmp.
    let boots = run_python(
        sandbox,
        id,
        [
            "-c",
            "print(open('/tmp/alien-contract/boots.jsonl').read(), end='')",
        ]
        .into_iter(),
        "boots.jsonl",
    )
    .await?;
    let boots: Vec<serde_json::Value> = boots
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()
        .map_err(failed)?;
    if boots.len() != 1 {
        return Err(format!(
            "ENTRYPOINT started {} times, expected once",
            boots.len()
        ));
    }
    Ok(format!(
        "one boot; {detail}; boot capabilities {}",
        boots[0]["capabilities"]
    ))
}

async fn observe(
    sandbox: &dyn Sandbox,
    id: &str,
    egress: &EgressSuite,
    use_proxy: bool,
) -> std::result::Result<Observation, String> {
    probe(
        sandbox,
        id,
        &Probe::Http {
            url: format!("http://{}:{}/", egress.host, egress.port),
            use_proxy,
            server_name: None,
            host_header: None,
        },
    )
    .await
    .map_err(failed)
}

fn verdict_check(
    name: &str,
    expected: &Expectation,
    control: &Observation,
    subject: &Observation,
) -> Check {
    let verdict = evaluate(expected, control, subject);
    Check {
        name: name.into(),
        verdict,
        detail: format!("control {control:?}; subject {subject:?}"),
        elapsed_ms: 0,
    }
}

/// A subject that starts with an empty allowlist is denied, then admitted by exactly the
/// host:port it is given, and denied again when the rule is removed. The control sandbox
/// reaches the same listener under `internet`, so a denial is never read without it.
async fn egress_checks(
    suite: &Suite,
    egress: &EgressSuite,
    provider: Arc<dyn Sandbox>,
    journal: &mut std::fs::File,
    sessions: &mut Vec<Session>,
    checks: &mut Vec<Check>,
) -> Result<()> {
    let control = super::create_with(
        provider.as_ref(),
        &suite.binding,
        journal,
        sessions,
        CreateSandboxRequest {
            egress: Some(EgressPolicy::Internet),
            ..Default::default()
        },
    )
    .await?;
    let subject = super::create_with(
        provider.as_ref(),
        &suite.binding,
        journal,
        sessions,
        CreateSandboxRequest {
            egress: Some(EgressPolicy::Allow { allow: vec![] }),
            ..Default::default()
        },
    )
    .await?;
    let control_id = sessions[control].sandbox_id.clone();
    let subject_id = sessions[subject].sandbox_id.clone();
    let denied = if egress.use_proxy {
        Expectation::GatewayRefused
    } else {
        Expectation::NetworkDenied
    };
    let reached = Expectation::Response {
        body_sha256: egress.body_sha256.clone(),
    };
    let control_seen = match observe(provider.as_ref(), &control_id, egress, egress.use_proxy).await
    {
        Ok(observation) => observation,
        Err(error) => {
            checks.push(Check {
                name: "egressControl".into(),
                verdict: Verdict::Unproven,
                detail: error,
                elapsed_ms: 0,
            });
            return Ok(());
        }
    };
    let steps: [(&str, Option<EgressPolicy>, Expectation); 4] = [
        ("egressEmptyAllowlistDenied", None, denied.clone()),
        (
            "egressLiveHostPortAdded",
            Some(EgressPolicy::Allow {
                allow: vec![format!("{}:{}", egress.host, egress.port)],
            }),
            reached.clone(),
        ),
        (
            "egressRuleRemovedDenied",
            Some(EgressPolicy::Allow { allow: vec![] }),
            denied,
        ),
        (
            "egressInternetReaches",
            Some(EgressPolicy::Internet),
            reached,
        ),
    ];
    for (name, update, expected) in steps {
        if let Some(policy) = update {
            if let Err(error) = provider.update_egress(&subject_id, policy).await {
                checks.push(Check {
                    name: name.into(),
                    verdict: Verdict::Fail,
                    detail: format!("update_egress refused: {error}"),
                    elapsed_ms: 0,
                });
                continue;
            }
        }
        match observe(provider.as_ref(), &subject_id, egress, egress.use_proxy).await {
            Ok(subject_seen) => {
                checks.push(verdict_check(name, &expected, &control_seen, &subject_seen))
            }
            Err(error) => checks.push(Check {
                name: name.into(),
                verdict: Verdict::Unproven,
                detail: error,
                elapsed_ms: 0,
            }),
        }
    }
    if egress.use_proxy {
        // The control may use the internet, yet ignoring the proxy environment gets nowhere:
        // the pod's NetworkPolicy drops a direct connect. The proxied control proves the listener.
        match observe(provider.as_ref(), &control_id, egress, false).await {
            Ok(direct) => checks.push(verdict_check(
                "egressDirectConnectRefused",
                &Expectation::NetworkDenied,
                &control_seen,
                &direct,
            )),
            Err(error) => checks.push(Check {
                name: "egressDirectConnectRefused".into(),
                verdict: Verdict::Unproven,
                detail: error,
                elapsed_ms: 0,
            }),
        }
    }
    Ok(())
}
