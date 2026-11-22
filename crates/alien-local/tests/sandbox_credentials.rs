//! Real Docker coverage for credential rules in `alien dev`: a session holds a placeholder, and
//! its sandbox's egress gateway puts the real value on requests to a linked service (plain
//! HTTP) and to an internet host (HTTPS through the session's CA).
//!
//! Needs a gateway image that serves credentials, named by `ALIEN_TEST_EGRESS_GATEWAY_IMAGE`,
//! and public HTTPS access to `httpbin.org`. Writes what it saw to
//! `ALIEN_TEST_PROOF_DIR` when set:
//!
//! `ALIEN_TEST_EGRESS_GATEWAY_IMAGE=<image> cargo test -p alien-local --test sandbox_credentials -- --ignored --test-threads=1`

use std::collections::HashMap;
use std::process::Command;

use alien_core::SandboxCredentialRule;
use alien_local::{
    LocalSandboxManager, SandboxEgressMode, SandboxLink, SandboxOutput, SandboxSessionConfig,
};
use tempfile::TempDir;

/// Python for urllib, which honours the proxy variables and `SSL_CERT_FILE`.
const IMAGE: &str = "python:3.12-alpine";
const NETWORK: &str = "deployment-network";
const SANDBOX: &str = "creds-a";
const FIRST: &str = "first-turn-local-6e0c41";
const SECOND: &str = "second-turn-local-a93d27";

/// Answers every request with its headers as JSON.
const ECHO: &str = r#"
import http.server, json
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({k.lower(): v for k, v in self.headers.items()}).encode()
        self.send_response(200); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
http.server.ThreadingHTTPServer(("0.0.0.0", 8080), H).serve_forever()
"#;

/// Prints the body of a GET through the session's proxy, sending a placeholder in the header.
const FETCH: &str = r#"
import sys, urllib.request
request = urllib.request.Request(sys.argv[1], headers={sys.argv[2]: "placeholder"})
try:
    print(urllib.request.urlopen(request, timeout=20).read().decode())
except Exception as error:
    print("FAILED", type(error).__name__, error); sys.exit(1)
"#;

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

fn config() -> SandboxSessionConfig {
    SandboxSessionConfig {
        image: IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 256 * 1024 * 1024,
        pids_limit: Some(64),
        scratch_bytes: 16 * 1024 * 1024,
        egress: SandboxEgressMode::Allow,
        preview_ports: Vec::new(),
        env: HashMap::from([("API_KEY".to_string(), "placeholder".to_string())]),
        links: vec![SandboxLink {
            id: "connector".into(),
            ports: vec![8080],
        }],
    }
}

async fn run(manager: &LocalSandboxManager, container: &str, command: &str) -> (i64, String) {
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
    let text = result
        .output
        .iter()
        .map(|frame| match frame {
            SandboxOutput::Stdout(bytes) | SandboxOutput::Stderr(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
        })
        .collect();
    (result.exit_code, text)
}

async fn fetch(manager: &LocalSandboxManager, container: &str, url: &str, header: &str) -> String {
    let (code, body) = run(
        manager,
        container,
        &format!("python3 -c '{}' {url} {header}", FETCH.replace('\'', "\"")),
    )
    .await;
    assert_eq!(code, 0, "{url}: {body}");
    body.to_ascii_lowercase()
}

fn rules(value: &str) -> Vec<SandboxCredentialRule> {
    serde_json::from_value(serde_json::json!([
        {"link":"connector","header":"Authorization","valuePrefix":"Bearer ","value":value},
        {"host":"httpbin.org","header":"X-Api-Key","value":value}
    ]))
    .unwrap()
}

#[tokio::test]
#[ignore = "requires a real Docker daemon, a gateway image that serves credentials, and public HTTPS"]
async fn a_session_holds_a_placeholder_and_its_gateway_sends_the_current_value() {
    let image = std::env::var("ALIEN_TEST_EGRESS_GATEWAY_IMAGE")
        .expect("ALIEN_TEST_EGRESS_GATEWAY_IMAGE names the gateway image to test");
    let proof = std::env::var("ALIEN_TEST_PROOF_DIR").ok();
    let mut log = String::new();
    let mut note = |line: String| {
        eprintln!("{line}");
        log.push_str(&line);
        log.push('\n');
    };
    let dir = TempDir::new().unwrap();
    let manager = LocalSandboxManager::new(dir.path().to_path_buf())
        .expect("Docker must be reachable")
        .with_gateway_image(image);
    manager.reap(SANDBOX).await.expect("clean slate");
    if !docker_ok(&["network", "inspect", NETWORK]) {
        docker(&["network", "create", NETWORK]);
    }
    let _ = docker_ok(&["rm", "-f", "creds-test-connector"]);
    docker(&[
        "run",
        "-d",
        "--name",
        "creds-test-connector",
        "--network",
        NETWORK,
        "--network-alias",
        "connector.svc",
        IMAGE,
        "python3",
        "-c",
        ECHO,
    ]);

    let session = manager
        .create_session(SANDBOX, "s1", &config())
        .await
        .expect("a session starts");
    let id = session.container_id.clone();
    let other = manager
        .create_session(SANDBOX, "s2", &config())
        .await
        .expect("a second session starts");

    // Before any rule: the placeholder goes out as the session sent it.
    let link = "http://connector.svc:8080/turn";
    let internet = "https://httpbin.org/headers";
    let body = fetch(&manager, &id, link, "Authorization").await;
    note(format!("no rules, link -> {body}"));
    assert!(body.contains(r#""authorization": "placeholder""#), "{body}");

    manager
        .update_credentials(SANDBOX, "s1", &rules(FIRST))
        .await
        .expect("the gateway acknowledges the rules");
    let body = fetch(&manager, &id, link, "Authorization").await;
    note(format!("first rules, link (plain HTTP) -> {body}"));
    assert!(
        body.contains(&format!(r#""authorization": "bearer {FIRST}""#)),
        "{body}"
    );
    let body = fetch(&manager, &id, internet, "X-Api-Key").await;
    note(format!(
        "first rules, internet host (HTTPS via session CA) -> {body}"
    ));
    assert!(
        body.contains(&format!(r#""x-api-key": "{FIRST}""#)),
        "{body}"
    );

    // The other session of the same sandbox gets nothing.
    let body = fetch(&manager, &other.container_id, link, "Authorization").await;
    note(format!("other session, link -> {body}"));
    assert!(body.contains(r#""authorization": "placeholder""#), "{body}");

    // The value is nowhere in the session: not in its environment, not in a file.
    let (_, env) = run(&manager, &id, "env").await;
    note(format!("session env:\n{env}"));
    assert!(!env.contains(FIRST));
    assert!(
        env.contains("SSL_CERT_FILE=/etc/alien-egress/bundle.pem"),
        "{env}"
    );
    let (code, found) = run(
        &manager,
        &id,
        &format!("grep -rl {FIRST} / --exclude-dir=proc --exclude-dir=sys 2>/dev/null; true"),
    )
    .await;
    note(format!(
        "files containing the value (exit {code}): [{}]",
        found.trim()
    ));
    assert_eq!(found.trim(), "", "a file holds the value");
    let (_, ca) = run(&manager, &id, "cat /etc/alien-egress/ca.pem").await;
    assert!(
        ca.contains("BEGIN CERTIFICATE") && !ca.contains("PRIVATE KEY"),
        "{ca}"
    );

    // Replace: the next request carries the new value.
    manager
        .update_credentials(SANDBOX, "s1", &rules(SECOND))
        .await
        .unwrap();
    let body = fetch(&manager, &id, link, "Authorization").await;
    note(format!("replaced rules, link -> {body}"));
    assert!(
        body.contains(&format!(r#""authorization": "bearer {SECOND}""#)),
        "{body}"
    );
    assert!(!body.contains(FIRST));
    let body = fetch(&manager, &id, internet, "X-Api-Key").await;
    note(format!("replaced rules, internet host -> {body}"));
    assert!(
        body.contains(&format!(r#""x-api-key": "{SECOND}""#)),
        "{body}"
    );

    // An empty list: the header is the session's own again.
    manager
        .update_credentials(SANDBOX, "s1", &[])
        .await
        .unwrap();
    let body = fetch(&manager, &id, link, "Authorization").await;
    note(format!("empty rules, link -> {body}"));
    assert!(body.contains(r#""authorization": "placeholder""#), "{body}");
    let body = fetch(&manager, &id, internet, "X-Api-Key").await;
    note(format!("empty rules, internet host -> {body}"));
    assert!(body.contains(r#""x-api-key": "placeholder""#), "{body}");

    // The gateway's logs and connection audit never carry a value.
    let logs = docker(&["logs", &format!("alien-sbx-gw-{SANDBOX}")]);
    let leaked: Vec<_> = [FIRST, SECOND]
        .into_iter()
        .filter(|value| logs.contains(value))
        .collect();
    note(format!(
        "gateway log lines: {}, values found in them: {leaked:?}",
        logs.lines().count()
    ));
    if let Some(proof) = &proof {
        std::fs::create_dir_all(proof).unwrap();
        std::fs::write(format!("{proof}/gateway.log"), &logs).unwrap();
        std::fs::write(format!("{proof}/session-and-echo.txt"), &log).unwrap();
    }
    assert!(
        leaked.is_empty(),
        "a credential value reached the gateway logs"
    );

    manager.reap(SANDBOX).await.unwrap();
    let _ = docker_ok(&["rm", "-f", "creds-test-connector"]);
}
