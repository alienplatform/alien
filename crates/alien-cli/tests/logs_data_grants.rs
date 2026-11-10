#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien logs` against a platform that issues data grants: the CLI asks the
//! platform for a grant, then queries the data service with it.

use httpmock::{
    Method::{GET, POST},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000001";
const MANAGER: &str = "mgr_0000000000000000000000000001";
const DEPLOYMENT: &str = "dep_0000000000000000000000000001";
const KEY: &str = "test-key";
const FROM: &str = "2026-11-09T10:00:00Z";
const TO: &str = "2026-11-09T11:00:00Z";
const T0: i64 = 1_794_000_000_000_000_000;
const SECOND: i64 = 1_000_000_000;

fn command(api: &MockServer, args: &[&str]) -> (Command, TempDir, TempDir) {
    let config = TempDir::new().unwrap();
    let checkout = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .args(["--base-url", &api.base_url(), "--workspace", "demo"])
        .env("XDG_CONFIG_HOME", config.path())
        .env("ALIEN_API_KEY", KEY);
    for name in [
        "ALIEN_MANAGER_URL",
        "ALIEN_PROJECT",
        "ALIEN_BASE_URL",
        "ALIEN_WORKSPACE",
        "ALIEN_LOG",
        "ALIEN_LOG_FILE",
        "RUST_LOG",
    ] {
        command.env_remove(name);
    }
    command.args(args);
    (command, config, checkout)
}

fn run(api: &MockServer, args: &[&str]) -> Output {
    let (mut command, _config, _checkout) = command(api, args);
    command.output().unwrap()
}

fn stdout_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn mock_project<'a>(api: &'a MockServer) -> Mock<'a> {
    api.mock(|when, then| {
        when.method(GET)
            .path("/v1/projects/demo-project")
            .header("authorization", format!("Bearer {KEY}"));
        then.status(200).json_body(json!({
            "id": PROJECT, "name": "demo-project",
            "workspaceId": "ws_000000000000000000000000", "createdAt": "2026-01-01T00:00:00Z"
        }));
    })
}

fn mock_grant<'a>(api: &'a MockServer, manager: &str, project: &str) -> Mock<'a> {
    let data_url = api.base_url();
    api.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/managers/{manager}/data-grants"))
            .query_param("workspace", "demo")
            .header("authorization", format!("Bearer {KEY}"))
            .json_body(json!({"projectId": project, "table": "logs", "action": "query"}));
        then.status(200).json_body(json!({
            "grant": "data-grant-token", "expiresAt": "2999-01-01T00:00:00Z",
            "dataUrl": data_url, "tenantId": "ws_000000000000000000000000",
            "databaseId": project, "tableId": "logs"
        }));
    })
}

fn row(nanos: i64, message: &str) -> Value {
    json!({
        "id": format!("auto_{nanos}"),
        "timestampNanos": nanos.to_string(),
        "severityText": "WARN",
        "severityNumber": 13,
        "body": message,
        "resource": {"service.name": "api", "alien.deployment_id": DEPLOYMENT},
        "attributes": {},
    })
}

fn rows_response(rows: Vec<Value>, cursor: Option<&str>) -> Value {
    json!({
        "rows": rows, "rowMetadata": [], "coverage": {"complete": true},
        "numHits": 42, "nextCursor": cursor,
    })
}

fn query_path(project: &str) -> String {
    format!("/v2/databases/{project}/tables/logs/query")
}

#[test]
fn json_output_comes_from_one_grant_and_one_query() {
    let api = MockServer::start();
    let project = mock_project(&api);
    let grant = mock_grant(&api, MANAGER, PROJECT);
    // Newest first, as the data service answers.
    let query = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .header("authorization", "Bearer data-grant-token")
            .json_body(json!({
                "search": "(timeout) AND (severityNumber:[13 TO 16]) AND NOT resource.alien.system:\"true\" AND NOT attributes.alien.system:\"true\"",
                "sortBy": "-timestampNanos",
                "start": "2026-11-09T10:00:00.000000000Z",
                "end": "2026-11-09T11:00:00.000000000Z",
                "limit": 25,
            }));
        then.status(200).json_body(rows_response(
            vec![row(T0 + SECOND, "second"), row(T0, "first")],
            None,
        ));
    });

    let output = run(
        &api,
        &[
            "logs",
            "--project",
            "demo-project",
            "--manager",
            MANAGER,
            "--json",
            "--query",
            "timeout",
            "--level",
            "warn",
            "--from",
            FROM,
            "--to",
            TO,
            "--limit",
            "25",
        ],
    );

    let result = stdout_json(&output);
    assert_eq!(result["numHits"], 42);
    assert_eq!(result["target"]["projectId"], PROJECT);
    assert_eq!(result["target"]["managerId"], MANAGER);
    assert_eq!(result["target"]["databaseId"], PROJECT);
    assert_eq!(
        result["query"].as_str().unwrap().starts_with("(timeout)"),
        true
    );
    let logs = result["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 2);
    // Oldest first.
    assert_eq!(logs[0]["message"], "first");
    assert_eq!(logs[1]["message"], "second");
    assert_eq!(logs[0]["level"], "WARN");
    assert_eq!(logs[0]["service"], "api");
    assert_eq!(logs[0]["deploymentId"], DEPLOYMENT);
    assert_eq!(logs[0]["timestampNanos"], T0);
    assert_eq!(logs[0]["raw"]["id"], format!("auto_{T0}"));
    assert!(result.get("errors").is_none());
    project.assert_hits(1);
    grant.assert_hits(1);
    query.assert_hits(1);
}

#[test]
fn human_output_lists_logs_oldest_first() {
    let api = MockServer::start();
    mock_project(&api);
    mock_grant(&api, MANAGER, PROJECT);
    api.mock(|when, then| {
        when.method(POST).path(query_path(PROJECT));
        then.status(200).json_body(rows_response(
            vec![row(T0 + SECOND, "second line"), row(T0, "first line")],
            None,
        ));
    });

    let output = run(
        &api,
        &["logs", "--project", "demo-project", "--manager", MANAGER],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(
        lines[0].contains("first line") && lines[0].contains("WARN") && lines[0].contains("api")
    );
    assert!(
        lines[0].starts_with("2026-11-0") || lines[0].contains("2026-"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("second line"));
}

#[test]
fn a_limit_beyond_one_page_follows_the_cursor() {
    let api = MockServer::start();
    mock_project(&api);
    mock_grant(&api, MANAGER, PROJECT);
    // Both pages ask for the same page size, and the second repeats the body with
    // the cursor. Registered first so it wins over the cursor-less page.
    let second = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .json_body_partial(json!({"cursor": "page-2", "limit": 4}).to_string());
        then.status(200).json_body(rows_response(
            vec![row(T0 + 2 * SECOND, "r3"), row(T0 + SECOND, "r2")],
            None,
        ));
    });
    let first = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .json_body_partial(json!({"limit": 4}).to_string());
        then.status(200).json_body(rows_response(
            vec![row(T0 + 4 * SECOND, "r5"), row(T0 + 3 * SECOND, "r4")],
            Some("page-2"),
        ));
    });

    let output = run(
        &api,
        &[
            "logs",
            "--project",
            "demo-project",
            "--manager",
            MANAGER,
            "--json",
            "--limit",
            "4",
        ],
    );

    let result = stdout_json(&output);
    let messages: Vec<_> = result["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|log| log["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, ["r2", "r3", "r4", "r5"]);
    first.assert_hits(1);
    second.assert_hits(1);
}

#[test]
fn a_deployment_reads_the_logs_of_its_own_project_and_manager() {
    let api = MockServer::start();
    let detail = api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT}"))
            .header("authorization", format!("Bearer {KEY}"));
        then.status(200).json_body(json!({
            "id": DEPLOYMENT, "name": "production", "platform": "aws", "status": "running",
            "deploymentGroupId": "dg_0000000000000000000000000001",
            "deploymentProtocolVersion": 1, "projectId": PROJECT,
            "workspaceId": "ws_000000000000000000000000", "managerId": MANAGER,
            "retryRequested": false, "updateBlocked": false,
            "purpose": "application", "stackSettings": {}, "releaseChannel": "stable",
            "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
        }));
    });
    let grant = mock_grant(&api, MANAGER, PROJECT);
    let query = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .header("authorization", "Bearer data-grant-token")
            .json_body_partial(
                json!({"filter": {"resource.alien.deployment_id": DEPLOYMENT}}).to_string(),
            );
        then.status(200)
            .json_body(rows_response(vec![row(T0, "from the deployment")], None));
    });

    let output = run(&api, &["logs", "--deployment", DEPLOYMENT, "--json"]);

    let result = stdout_json(&output);
    assert_eq!(result["target"]["projectId"], PROJECT);
    assert_eq!(result["target"]["deploymentId"], DEPLOYMENT);
    assert_eq!(result["filter"]["resource.alien.deployment_id"], DEPLOYMENT);
    assert_eq!(result["logs"][0]["message"], "from the deployment");
    detail.assert_hits(1);
    grant.assert_hits(1);
    query.assert_hits(1);
}

#[test]
fn without_a_project_the_command_lists_what_can_be_read() {
    let api = MockServer::start();
    let projects = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/projects")
            .query_param("workspace", "demo");
        then.status(200).json_body(json!({
            "items": [
                {"id": PROJECT, "name": "alpha", "workspaceId": "ws_000000000000000000000000",
                 "createdAt": "2026-01-01T00:00:00Z"},
                {"id": "prj_0000000000000000000000000002", "name": "beta",
                 "workspaceId": "ws_000000000000000000000000", "createdAt": "2026-01-01T00:00:00Z"}
            ],
            "nextCursor": null
        }));
    });
    let anything = api.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(json!({}));
    });

    let output = run(&api, &["logs", "--manager", MANAGER, "--json"]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], "LOGS_PROJECT_REQUIRED");
    let message = error["message"].as_str().unwrap();
    for expected in [
        "--project",
        "--deployment",
        &format!("alpha ({PROJECT})"),
        "beta (prj_0000000000000000000000000002)",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    projects.assert_hits(1);
    anything.assert_hits(0);
}

#[test]
fn grant_failures_explain_what_to_do() {
    let api_error = |code: &str, status: u16| {
        json!({"code": code, "message": format!("{code} from the server"),
            "retryable": false, "internal": false, "httpStatusCode": status})
    };
    let cases = [
        // An older server has no data grant route.
        // The status of a reply that is not an API error is not visible to the CLI.
        (404, None, "LOG_DATA_GRANT_UNREADABLE", "could not be read"),
        (503, None, "LOG_DATA_GRANT_UNREADABLE", "could not be read"),
        (
            404,
            Some(api_error("NOT_FOUND", 404)),
            "LOG_DATA_GRANTS_UNSUPPORTED",
            "update the server or use an older CLI",
        ),
        (
            403,
            Some(api_error("FORBIDDEN", 403)),
            "LOG_DATA_ACCESS_DENIED",
            "don't have access to the logs of project 'prj_0000000000000000000000000001'",
        ),
        (
            409,
            Some(api_error("DATA_SERVICE_UNAVAILABLE", 409)),
            "LOG_DATA_SERVICE_NOT_CONFIGURED",
            "no log data service configured",
        ),
        (
            503,
            Some(api_error("MANAGER_OFFLINE", 503)),
            "LOG_MANAGER_OFFLINE",
            "is offline",
        ),
    ];
    for (status, body, code, expected) in cases {
        let api = MockServer::start();
        mock_project(&api);
        let grant = api.mock(|when, then| {
            when.method(POST)
                .path(format!("/v1/managers/{MANAGER}/data-grants"));
            let then = then.status(status);
            if let Some(body) = &body {
                then.json_body(body.clone());
            }
        });
        let query = api.mock(|when, then| {
            when.method(POST).path(query_path(PROJECT));
            then.status(200).json_body(rows_response(vec![], None));
        });

        let output = run(
            &api,
            &[
                "logs",
                "--project",
                "demo-project",
                "--manager",
                MANAGER,
                "--json",
            ],
        );

        assert!(!output.status.success(), "{status}");
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["code"], code, "{status}: {error}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains(expected), "{status}: {message}");
        grant.assert_hits(1);
        query.assert_hits(0);
    }
}

/// Read the CLI's stdout lines as they arrive, so a test can stop a command that
/// never exits on its own.
struct Streaming {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
}

impl Streaming {
    fn start(mut command: Command) -> Self {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    fn collect_until(&mut self, mut done: impl FnMut(&[String]) -> bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut lines = Vec::new();
        while Instant::now() < deadline && !done(&lines) {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(100)) {
                lines.push(line);
            }
        }
        lines
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn follow_continues_after_the_newest_log_and_shows_each_log_once() {
    let api = MockServer::start();
    mock_project(&api);
    let grant = mock_grant(&api, MANAGER, PROJECT);
    // Later polls read oldest first from one second before the newest log seen
    // (T0 + 1s). They return that log again next to a new one.
    let newest_seen_start = rfc3339(T0);
    let later = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .json_body_partial(
                json!({"sortBy": "timestampNanos", "start": newest_seen_start}).to_string(),
            );
        then.status(200).json_body(rows_response(
            vec![row(T0 + SECOND, "second"), row(T0 + 2 * SECOND, "third")],
            None,
        ));
    });
    let later_again = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .json_body_partial(
                json!({"sortBy": "timestampNanos", "start": rfc3339(T0 + SECOND)}).to_string(),
            );
        then.status(200)
            .json_body(rows_response(vec![row(T0 + 2 * SECOND, "third")], None));
    });
    let first = api.mock(|when, then| {
        when.method(POST)
            .path(query_path(PROJECT))
            .json_body_partial(json!({"sortBy": "-timestampNanos"}).to_string());
        then.status(200).json_body(rows_response(
            vec![row(T0 + SECOND, "second"), row(T0, "first")],
            None,
        ));
    });

    let (command, _config, _checkout) = command(
        &api,
        &[
            "logs",
            "--project",
            "demo-project",
            "--manager",
            MANAGER,
            "--follow",
            "--json",
            "--interval",
            "1s",
        ],
    );
    let mut stream = Streaming::start(command);
    let lines = stream
        .collect_until(|lines| lines.iter().filter(|line| line.contains("\"log\"")).count() >= 3);
    // Let at least one more poll happen: it must add nothing.
    std::thread::sleep(Duration::from_millis(2500));
    let mut lines = lines;
    while let Ok(line) = stream.lines.try_recv() {
        lines.push(line);
    }
    drop(stream);

    let parsed: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(parsed[0]["type"], "metadata");
    let messages: Vec<_> = parsed[1..]
        .iter()
        .map(|line| {
            assert_eq!(line["type"], "log");
            line["log"]["message"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(messages, ["first", "second", "third"]);
    first.assert_hits(1);
    later.assert_hits(1);
    assert!(
        later_again.hits() >= 1,
        "polling continued from the newest log"
    );
    assert_eq!(grant.hits(), 1, "one grant serves the whole run");
}

fn rfc3339(nanos: i64) -> String {
    chrono::DateTime::from_timestamp(nanos.div_euclid(SECOND), nanos.rem_euclid(SECOND) as u32)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}
