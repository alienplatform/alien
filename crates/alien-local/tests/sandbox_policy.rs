//! Real Docker coverage for the outbound policy a local session runs under: the declaration's
//! default, a policy named at create time, and a sandbox declared closed.
//!
//! Needs a gateway image that serves links, named by `ALIEN_TEST_EGRESS_GATEWAY_IMAGE`:
//!
//! `ALIEN_TEST_EGRESS_GATEWAY_IMAGE=<image> cargo test -p alien-local --test sandbox_policy -- --ignored --test-threads=1`

use std::collections::HashMap;
use std::process::Command;

use alien_core::EgressPolicy;
use alien_local::{
    LocalSandboxManager, SandboxEgressMode, SandboxLink, SandboxOutput, SandboxSessionConfig,
};
use tempfile::TempDir;

const IMAGE: &str = "alpine:3.20";
/// Has busybox's `httpd`, which Alpine's busybox leaves out.
const SERVICE_IMAGE: &str = "busybox:1.36";
const NETWORK: &str = "deployment-network";

fn docker(args: &[&str]) -> String {
    let output = Command::new("docker")
        .args(args)
        .output()
        .expect("docker CLI");
    assert!(
        output.status.success(),
        "docker {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn docker_ok(args: &[&str]) -> bool {
    Command::new("docker")
        .args(args)
        .output()
        .expect("docker CLI")
        .status
        .success()
}

/// A container on `deployment-network` answering as `<name>.svc` on 8080 and 8081.
fn service(name: &str) -> String {
    let _ = docker_ok(&["rm", "-f", &format!("links-test-{name}")]);
    docker(&[
        "run",
        "-d",
        "--name",
        &format!("links-test-{name}"),
        "--network",
        NETWORK,
        "--network-alias",
        &format!("{name}.svc"),
        SERVICE_IMAGE,
        "sh",
        "-c",
        &format!(
            "mkdir /w && echo {name}-ok > /w/index.html && \
             (httpd -f -p 8081 -h /w &) && httpd -f -p 8080 -h /w"
        ),
    ]);
    docker(&[
        "inspect",
        "-f",
        &format!("{{{{(index .NetworkSettings.Networks \"{NETWORK}\").IPAddress}}}}"),
        &format!("links-test-{name}"),
    ])
}

fn output(result: &alien_local::SandboxExecResult) -> String {
    result
        .output
        .iter()
        .map(|frame| match frame {
            SandboxOutput::Stdout(bytes) | SandboxOutput::Stderr(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
        })
        .collect()
}

async fn fetch(manager: &LocalSandboxManager, container: &str, command: &str) -> (i64, String) {
    let result = manager
        .exec(
            container,
            &[
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!("{command} 2>&1"),
            ],
        )
        .await
        .expect("exec");
    (result.exit_code, output(&result))
}

fn config(
    egress: SandboxEgressMode,
    policy: Option<EgressPolicy>,
    links: Vec<SandboxLink>,
) -> SandboxSessionConfig {
    SandboxSessionConfig {
        image: IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 256 * 1024 * 1024,
        pids_limit: Some(64),
        scratch_bytes: 16 * 1024 * 1024,
        egress,
        preview_ports: Vec::new(),
        env: HashMap::new(),
        links,
        max_lifetime: None,
        egress_policy: policy,
    }
}

fn api_link() -> Vec<SandboxLink> {
    vec![SandboxLink {
        id: "api".into(),
        ports: vec![8080],
    }]
}

fn allow(targets: &[&str]) -> Option<EgressPolicy> {
    Some(EgressPolicy::Allow {
        allow: targets.iter().map(|target| target.to_string()).collect(),
    })
}

async fn manager(sandbox: &str, dir: &TempDir) -> LocalSandboxManager {
    let image = std::env::var("ALIEN_TEST_EGRESS_GATEWAY_IMAGE")
        .expect("ALIEN_TEST_EGRESS_GATEWAY_IMAGE names the gateway image to test");
    let manager = LocalSandboxManager::new(dir.path().to_path_buf())
        .expect("Docker must be reachable")
        .with_gateway_image(image);
    manager.reap(sandbox).await.expect("clean slate");
    if !docker_ok(&["network", "inspect", NETWORK]) {
        docker(&["network", "create", NETWORK]);
    }
    service("api");
    manager
}

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image"]
async fn the_default_policy_reaches_its_link_and_not_the_internet() {
    let dir = TempDir::new().unwrap();
    let sandbox = "policy-default";
    let manager = manager(sandbox, &dir).await;
    let session = manager
        .create_session(
            sandbox,
            "s1",
            &config(SandboxEgressMode::Allow, allow(&["link:api"]), api_link()),
        )
        .await
        .expect("session starts");
    let id = &session.container_id;

    let (code, body) = fetch(&manager, id, "wget -q -T 10 -O- http://api.svc:8080/").await;
    assert_eq!((code, body.trim()), (0, "api-ok"), "{body}");
    let (code, body) = fetch(&manager, id, "wget -q -T 10 -O- http://example.com/").await;
    assert_ne!(code, 0, "the internet is outside the policy: {body}");
    assert!(!body.contains("Example Domain"), "{body}");
    manager.reap(sandbox).await.unwrap();
}

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image"]
async fn a_policy_named_at_create_time_replaces_the_default() {
    let dir = TempDir::new().unwrap();
    let sandbox = "policy-override";
    let manager = manager(sandbox, &dir).await;
    // The route puts a create-time policy into the session's config; here that is `allow: []`
    // over a default that allowed the link.
    let session = manager
        .create_session(
            sandbox,
            "s1",
            &config(SandboxEgressMode::Allow, allow(&[]), api_link()),
        )
        .await
        .expect("session starts");
    let (code, body) = fetch(
        &manager,
        &session.container_id,
        "wget -q -T 10 -O- http://api.svc:8080/",
    )
    .await;
    assert_ne!(code, 0, "an empty allowlist refuses links too: {body}");
    assert!(!body.contains("api-ok"), "{body}");
    manager.reap(sandbox).await.unwrap();
}

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image"]
async fn a_sandbox_declared_none_stays_closed() {
    let dir = TempDir::new().unwrap();
    let sandbox = "policy-none";
    let manager = manager(sandbox, &dir).await;
    let session = manager
        .create_session(
            sandbox,
            "s1",
            &config(SandboxEgressMode::Deny, None, Vec::new()),
        )
        .await
        .expect("session starts");
    let (code, body) = fetch(
        &manager,
        &session.container_id,
        "wget -q -T 5 -O- http://example.com/",
    )
    .await;
    assert_ne!(code, 0, "no interface, no way out: {body}");
    manager.reap(sandbox).await.unwrap();
}
