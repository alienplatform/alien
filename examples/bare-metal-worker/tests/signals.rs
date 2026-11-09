//! Runs the real binary and stops it with a signal, to prove the drain end to end.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(20);

struct Server {
    child: Child,
    port: u16,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Server {
    fn start() -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_job-worker"))
            .env("PORT", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start job-worker");
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });

        let mut server = Server {
            child,
            port: 0,
            lines,
            seen: Vec::new(),
        };
        let listening = server.wait_for("listening on 0.0.0.0:");
        let port = listening.rsplit(':').next().expect("port in the log line");
        server.port = port.parse().expect("numeric port");
        server
    }

    fn wait_for(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(remaining).unwrap_or_else(|_| {
                panic!("no log line containing {needle:?}; saw {:?}", self.seen)
            });
            self.seen.push(line.clone());
            if line.contains(needle) {
                return line;
            }
        }
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.child.id().to_string())
            .status()
            .expect("run kill");
        assert!(status.success(), "kill -{name} failed");
    }

    fn request(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("send request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status code");
        let (_, payload) = response.split_once("\r\n\r\n").expect("response body");
        (status, serde_json::from_str(payload).expect("json body"))
    }

    fn wait_for_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll the process") {
                return status;
            }
            assert!(Instant::now() < deadline, "the process never exited");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn drain_on(signal: &str) {
    let mut server = Server::start();

    let (status, body) = server.request("GET", "/healthz", "");
    assert_eq!(status, 200);
    assert_eq!(body, json!({ "status": "ok", "version": "0.1.0" }));

    let (status, body) = server.request("POST", "/jobs", r#"{"task":"wait","seconds":2}"#);
    assert_eq!(status, 202);
    assert_eq!(body, json!({ "id": "j-1", "status": "queued" }));
    server.wait_for("job j-1 started (wait 2s)");

    server.signal(signal);
    server.wait_for(&format!("received SIG{signal}"));
    server.wait_for("draining: 1 job running, new jobs refused");

    let (status, body) = server.request("GET", "/healthz", "");
    assert_eq!(status, 503);
    assert_eq!(body, json!({ "status": "draining", "version": "0.1.0" }));
    let (status, body) = server.request("POST", "/jobs", r#"{"task":"wait","seconds":1}"#);
    assert_eq!(status, 503);
    assert_eq!(body["status"], "draining");
    let (status, job) = server.request("GET", "/jobs/j-1", "");
    assert_eq!(status, 200);
    assert_eq!(job["status"], "running", "the job is not cut short");

    server.wait_for("job j-1 finished in 2s");
    server.wait_for("drained, exiting");
    let exit = server.wait_for_exit();
    assert!(exit.success(), "exit status {exit:?}");
}

#[test]
fn sigterm_drains_then_exits() {
    drain_on("TERM");
}

#[test]
fn sigint_drains_then_exits() {
    drain_on("INT");
}

#[test]
fn a_signal_with_nothing_running_exits_at_once() {
    let mut server = Server::start();

    server.signal("TERM");

    server.wait_for("draining: 0 jobs running, new jobs refused");
    server.wait_for("drained, exiting");
    assert!(server.wait_for_exit().success());
}
