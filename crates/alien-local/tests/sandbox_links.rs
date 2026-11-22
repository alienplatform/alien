//! Real Docker coverage for a local sandbox that links a service: its sessions reach the linked
//! service through the sandbox's egress gateway and nothing else private.
//!
//! Needs a gateway image that serves links, named by `ALIEN_TEST_EGRESS_GATEWAY_IMAGE`:
//!
//! `ALIEN_TEST_EGRESS_GATEWAY_IMAGE=<image> cargo test -p alien-local --test sandbox_links -- --ignored --test-threads=1`

use std::collections::HashMap;
use std::process::Command;

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

/// A container on `deployment-network` answering as `<name>.svc` on 8080 and 8081 (HTTP) and
/// 9000 (echoes every byte back).
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
             (httpd -f -p 8081 -h /w &) && (nc -lk -p 9000 -e cat &) && httpd -f -p 8080 -h /w"
        ),
    ]);
    docker(&[
        "inspect",
        "-f",
        &format!("{{{{(index .NetworkSettings.Networks \"{NETWORK}\").IPAddress}}}}"),
        &format!("links-test-{name}"),
    ])
}

fn config(links: Vec<SandboxLink>) -> SandboxSessionConfig {
    SandboxSessionConfig {
        image: IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 256 * 1024 * 1024,
        pids_limit: Some(64),
        scratch_bytes: 16 * 1024 * 1024,
        egress: SandboxEgressMode::Allow,
        preview_ports: Vec::new(),
        env: HashMap::new(),
        links,
    }
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

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image that serves links"]
async fn a_linked_session_reaches_its_link_through_the_gateway_and_nothing_else_private() {
    let image = std::env::var("ALIEN_TEST_EGRESS_GATEWAY_IMAGE")
        .expect("ALIEN_TEST_EGRESS_GATEWAY_IMAGE names the gateway image to test");
    let dir = TempDir::new().unwrap();
    let manager = LocalSandboxManager::new(dir.path().to_path_buf())
        .expect("Docker must be reachable")
        .with_gateway_image(image);
    let sandbox = "links-a";
    manager.reap(sandbox).await.expect("clean slate");
    if !docker_ok(&["network", "inspect", NETWORK]) {
        docker(&["network", "create", NETWORK]);
    }
    let api_ip = service("api");
    service("other");

    let session = manager
        .create_session(
            sandbox,
            "s1",
            &config(vec![SandboxLink {
                id: "api".into(),
                ports: vec![8080],
            }]),
        )
        .await
        .expect("a linked session starts");
    let id = &session.container_id;

    // Through the gateway, by the link's name and port.
    let (code, body) = fetch(&manager, id, "wget -q -T 10 -O- http://api.svc:8080/").await;
    eprintln!("http://api.svc:8080/ -> exit {code}: {}", body.trim());
    assert_eq!((code, body.trim()), (0, "api-ok"), "{body}");

    // Every other private destination stays refused by the gateway.
    for refused in [
        "http://api.svc:8081/".to_string(),
        "http://other.svc:8080/".to_string(),
        format!("http://{api_ip}:8080/"),
        "http://169.254.169.254/latest/meta-data/".to_string(),
        "http://127.0.0.1:8080/".to_string(),
    ] {
        let (code, body) = fetch(&manager, id, &format!("wget -q -T 10 -O- {refused}")).await;
        eprintln!("{refused} -> exit {code}: {}", body.trim());
        assert_ne!(code, 0, "{refused} must be refused: {body}");
        assert!(!body.contains("-ok"), "{refused} answered: {body}");
    }
    // Around the proxy there is no route at all: the session's network is internal.
    let (code, body) = fetch(
        &manager,
        id,
        &format!("wget -Y off -q -T 5 -O- http://{api_ip}:8080/"),
    )
    .await;
    assert_ne!(code, 0, "a direct connection must fail: {body}");

    eprintln!("direct {api_ip}:8080 -> exit {code}: {}", body.trim());

    let logs = docker(&["logs", "alien-sbx-gw-links-a"]);
    eprintln!("gateway log:\n{logs}");
    assert!(
        logs.contains(r#""host":"api.svc","port":8080,"link":"api","decision":"allow""#),
        "the gateway logs the link: {logs}"
    );
    // Each refusal above is the gateway's decision, not a missing service.
    for (host, port) in [
        ("api.svc", 8081),
        ("other.svc", 8080),
        (api_ip.as_str(), 8080),
        ("169.254.169.254", 80),
        ("127.0.0.1", 8080),
    ] {
        let denied = format!(r#""host":"{host}","port":{port},"decision":"deny""#);
        assert!(
            logs.contains(&denied),
            "the gateway refused {host}:{port}: {logs}"
        );
    }

    // A second session is on its own network.
    let second = manager
        .create_session(
            sandbox,
            "s2",
            &config(vec![SandboxLink {
                id: "api".into(),
                ports: vec![8080],
            }]),
        )
        .await
        .expect("a second linked session starts");
    let (code, body) = fetch(
        &manager,
        &second.container_id,
        "wget -q -T 10 -O- http://api.svc:8080/",
    )
    .await;
    assert_eq!((code, body.trim()), (0, "api-ok"), "{body}");

    // Removing the link from the stack reaches the running sessions: their gateway is replaced
    // and refuses the service, while the sessions keep their proxy. Replacing the container
    // also ends every tunnel the old one held open.
    let before = docker(&["inspect", "-f", "{{.Id}}", "alien-sbx-gw-links-a"]);
    manager
        .apply_links(sandbox, &[])
        .await
        .expect("the links change applies");
    let after = docker(&["inspect", "-f", "{{.Id}}", "alien-sbx-gw-links-a"]);
    assert_ne!(
        before, after,
        "the gateway holding the link's tunnels is gone"
    );
    let (code, body) = fetch(
        &manager,
        &second.container_id,
        "wget -q -T 10 -O- http://api.svc:8080/",
    )
    .await;
    eprintln!(
        "after unlink: http://api.svc:8080/ -> exit {code}: {}",
        body.trim()
    );
    assert_ne!(code, 0, "an unlinked service must be refused: {body}");
    assert!(
        !body.contains("-ok"),
        "the unlinked service answered: {body}"
    );
    let logs = docker(&["logs", "alien-sbx-gw-links-a"]);
    assert!(
        logs.contains(r#""host":"api.svc","port":8080,"decision":"deny""#),
        "the replaced gateway refused the unlinked service: {logs}"
    );
    // And linking it again restores it for the same session.
    manager
        .apply_links(
            sandbox,
            &[SandboxLink {
                id: "api".into(),
                ports: vec![8080],
            }],
        )
        .await
        .expect("the links change applies");
    let (code, body) = fetch(
        &manager,
        &second.container_id,
        "wget -q -T 10 -O- http://api.svc:8080/",
    )
    .await;
    eprintln!(
        "after relink: http://api.svc:8080/ -> exit {code}: {}",
        body.trim()
    );
    assert_eq!((code, body.trim()), (0, "api-ok"), "{body}");

    manager.terminate(sandbox, "s1").await.expect("terminate");
    assert!(!docker_ok(&[
        "network",
        "inspect",
        "alien-sbx-link.links-a.s1"
    ]));
    assert!(docker_ok(&[
        "network",
        "inspect",
        "alien-sbx-link.links-a.s2"
    ]));

    manager.reap(sandbox).await.expect("reap");
    assert!(!docker_ok(&["inspect", "alien-sbx-gw-links-a"]));
    // A sandbox without a running gateway has nothing to change.
    manager
        .apply_links(sandbox, &[])
        .await
        .expect("nothing to apply");
    assert!(!docker_ok(&["inspect", "alien-sbx-gw-links-a"]));
    assert!(!docker_ok(&[
        "network",
        "inspect",
        "alien-sbx-link.links-a.s2"
    ]));
    for name in ["api", "other"] {
        docker(&["rm", "-f", &format!("links-test-{name}")]);
    }
}

/// Sends `payload` through a CONNECT tunnel to `authority`, authenticated as the session's proxy
/// URL is, and prints what comes back: the gateway's answer, then the service's bytes.
fn tunnel_command(authority: &str, payload: &str) -> String {
    format!(
        "T=${{HTTP_PROXY#http://alien:}}; T=${{T%@*}}; A=$(printf 'alien:%s' \"$T\" | base64 -w0); \
         (printf 'CONNECT {authority} HTTP/1.1\\r\\nHost: {authority}\\r\\nProxy-Authorization: Basic %s\\r\\n\\r\\n' \"$A\"; \
         sleep 1; printf '{payload}'; sleep 2) | nc egress-gateway 3128"
    )
}

const WEBSOCKET_UPGRADE: &str = concat!(
    "GET /ws HTTP/1.1\\r\\nHost: relay-api.svc:9000\\r\\nConnection: Upgrade\\r\\n",
    "Upgrade: websocket\\r\\nSec-WebSocket-Version: 13\\r\\n",
    "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\\r\\n\\r\\n",
);

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image that serves links"]
async fn a_linked_tunnel_carries_websocket_and_other_protocols_to_the_link_only() {
    let image = std::env::var("ALIEN_TEST_EGRESS_GATEWAY_IMAGE")
        .expect("ALIEN_TEST_EGRESS_GATEWAY_IMAGE names the gateway image to test");
    let dir = TempDir::new().unwrap();
    let manager = LocalSandboxManager::new(dir.path().to_path_buf())
        .expect("Docker must be reachable")
        .with_gateway_image(image);
    let sandbox = "links-b";
    manager.reap(sandbox).await.expect("clean slate");
    if !docker_ok(&["network", "inspect", NETWORK]) {
        docker(&["network", "create", NETWORK]);
    }
    // Own service names and aliases, so this test runs beside the one above on the same
    // network.
    service("relay-api");
    service("relay-other");
    let session = manager
        .create_session(
            sandbox,
            "s1",
            &config(vec![SandboxLink {
                id: "relay-api".into(),
                ports: vec![8080, 9000],
            }]),
        )
        .await
        .expect("a linked session starts");
    let id = &session.container_id;

    // A WebSocket upgrade and a non-HTTP protocol both reach the linked service unchanged
    // (the service echoes them back).
    for (name, payload, expected) in [
        ("websocket", WEBSOCKET_UPGRADE, "Upgrade: websocket"),
        ("redis", "*1\\r\\n$4\\r\\nPING\\r\\n", "PING"),
    ] {
        let (_, body) = fetch(&manager, id, &tunnel_command("relay-api.svc:9000", payload)).await;
        eprintln!("{name} via CONNECT relay-api.svc:9000 ->\n{body}");
        assert!(
            body.starts_with("HTTP/1.1 200 Connection Established"),
            "{name}: {body}"
        );
        assert!(
            body.contains(expected),
            "{name} reached the service: {body}"
        );
    }
    // Anything that is not the link keeps the gateway's checks: an unlinked service and an
    // undeclared port are refused before any byte is sent.
    for authority in ["relay-other.svc:9000", "relay-api.svc:8081"] {
        let (_, body) = fetch(&manager, id, &tunnel_command(authority, WEBSOCKET_UPGRADE)).await;
        eprintln!("CONNECT {authority} -> {body}");
        assert!(body.starts_with("HTTP/1.1 403"), "{authority}: {body}");
        assert!(!body.contains("Upgrade: websocket"), "{authority}: {body}");
    }
    // Plain HTTP to the link still works through the proxy.
    let (code, body) = fetch(&manager, id, "wget -q -T 10 -O- http://relay-api.svc:8080/").await;
    assert_eq!((code, body.trim()), (0, "relay-api-ok"), "{body}");

    let logs = docker(&["logs", "alien-sbx-gw-links-b"]);
    eprintln!("gateway log:\n{logs}");
    assert!(
        logs.contains(r#""host":"relay-api.svc","port":9000,"link":"relay-api","decision":"allow""#),
        "the gateway logs the linked tunnel: {logs}"
    );
    manager.reap(sandbox).await.expect("reap");
    for name in ["relay-api", "relay-other"] {
        docker(&["rm", "-f", &format!("links-test-{name}")]);
    }
}
