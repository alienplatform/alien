//! A stdio controller process is replaced whenever its pipe can no longer be trusted to answer
//! the next request, and stopping it stops what it started.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::Duration;

use alien_infra::step_protocol::{StdioTransport, Transport};

fn script(dir: &Path, body: &str) -> StdioTransport {
    let path = dir.join("controller");
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    StdioTransport::new(path.display().to_string(), Vec::new(), Vec::new())
}

fn request(step_id: &str) -> alien_infra::step_protocol::protocol::StepRequest {
    serde_json::from_value(serde_json::json!({
        "protocol": 1, "stepId": step_id, "deploymentId": "d", "resourceId": "zone",
        "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
        "initialSetupAuthority": "directSetup", "lifecycle": "live",
        "resource": { "type": "acme/dns-zone", "id": "zone" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" }
        },
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap()
}

/// A reply the transport can parse, for `step_id`.
fn refused_reply(step_id: &str) -> String {
    format!(
        r#"{{"protocol":1,"stepId":"{step_id}","result":"refused","error":{{"code":"NO","message":"no","retryable":false}}}}"#
    )
}

async fn gone(pid: u32) -> bool {
    for _ in 0..50 {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        if stat.trim().is_empty() || stat.starts_with('Z') {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
    false
}

/// A log line ahead of the reply fails that step, and the next step gets a new process, not
/// the reply the first one left in the pipe.
#[tokio::test]
async fn a_stray_line_on_stdout_fails_one_step_and_the_next_gets_a_new_process() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("logged");
    let transport = script(
        dir.path(),
        &format!(
            "while read line; do\n  case \"$line\" in *'\"s1\"'*) id=s1 ;; *) id=s2 ;; esac\n  \
             if [ ! -e {m} ]; then touch {m}; echo 'a library logged this'; fi\n  \
             case $id in s1) echo '{r1}' ;; *) echo '{r2}' ;; esac\ndone\n",
            m = marker.display(),
            r1 = refused_reply("s1"),
            r2 = refused_reply("s2"),
        ),
    );

    let first = transport
        .step(&request("s1"), Duration::from_secs(10))
        .await;
    assert!(first.is_err(), "the log line is not a reply");
    assert_eq!(transport.child_pid(), None, "the process is replaced");

    let second = transport
        .step(&request("s2"), Duration::from_secs(10))
        .await
        .expect("the new process answers its own request");
    assert_eq!(second.step_id, "s2");
}

/// A reply the transport can't use kills the process and what it started.
#[tokio::test]
async fn an_unusable_reply_stops_the_controller_and_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let child_pid = dir.path().join("child.pid");
    let transport = script(
        dir.path(),
        &format!(
            "sleep 300 &\necho $! > {}\nread line\necho '{{}}'\nexec sleep 30\n",
            child_pid.display()
        ),
    );
    let refused = transport
        .step(&request("s1"), Duration::from_secs(10))
        .await;
    assert!(refused.is_err());
    assert_eq!(transport.child_pid(), None);
    let child: u32 = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(gone(child).await, "process {child} outlived its controller");
}

/// Dropping the transport stops a controller that ignores its stdin closing, and what it
/// started.
#[tokio::test]
async fn dropping_the_transport_stops_the_controller_and_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let child_pid = dir.path().join("child.pid");
    let transport = script(
        dir.path(),
        &format!(
            "sleep 300 &\necho $! > {}\nread line\necho '{}'\nexec sleep 30\n",
            child_pid.display(),
            refused_reply("s1")
        ),
    );
    transport
        .step(&request("s1"), Duration::from_secs(10))
        .await
        .expect("the controller answers");
    let pid = transport.child_pid().expect("the controller still runs");
    let child: u32 = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    drop(transport);

    assert!(
        gone(pid).await,
        "process {pid} outlived its dropped transport"
    );
    assert!(gone(child).await, "process {child} outlived its controller");
}

/// A step that waits for the process while another runs gets its deadline counted from when it
/// is sent, not from when it was made.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_waiting_for_the_process_gets_its_deadline_when_sent() {
    let dir = tempfile::tempdir().unwrap();
    // Answers each step a second later, naming the deadline it got.
    let transport = script(
        dir.path(),
        r#"while read line; do
  sleep 1
  id=$(printf '%s' "$line" | sed -n 's/.*"stepId":"\([^"]*\)".*/\1/p')
  deadline=$(printf '%s' "$line" | sed -n 's/.*"deadline":"\([^"]*\)".*/\1/p')
  echo "{\"protocol\":1,\"stepId\":\"$id\",\"result\":\"refused\",\"error\":{\"code\":\"NO\",\"message\":\"$deadline\",\"retryable\":false}}"
done"#,
    );
    let deadline_of = |response: alien_infra::step_protocol::protocol::StepResponse| {
        let value = serde_json::to_value(response).unwrap();
        chrono::DateTime::parse_from_rfc3339(value["error"]["message"].as_str().unwrap()).unwrap()
    };
    let timeout = Duration::from_secs(10);
    let (s1, s2) = (request("s1"), request("s2"));
    let (first, second) = tokio::join!(transport.step(&s1, timeout), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        transport.step(&s2, timeout).await
    });
    let gap = deadline_of(second.unwrap()) - deadline_of(first.unwrap());
    assert!(gap >= chrono::Duration::milliseconds(800), "{gap}");
}
