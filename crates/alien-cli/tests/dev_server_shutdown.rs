#![cfg(unix)]

use std::{
    fs,
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}

fn shutdown(signal: &str) {
    let directory = TempDir::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let status_file = directory.path().join("status.json");
    let log = fs::File::create(directory.path().join("server.log")).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_alien"))
            .current_dir(directory.path())
            .args(["dev", "--status-file"])
            .arg(&status_file)
            .args(["server", "--port", &port.to_string()])
            .args([
                "--workspace",
                "example",
                "--project",
                "example",
                "--no-browser",
            ])
            .env_remove("ALIEN_API_KEY")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = fs::read(&status_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        if status.as_ref().and_then(|value| value["status"].as_str()) == Some("ready") {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited: {}",
            fs::read_to_string(directory.path().join("server.log")).unwrap()
        );
        assert!(Instant::now() < deadline, "server did not become ready");
        thread::sleep(Duration::from_millis(20));
    }
    assert!(Command::new("kill")
        .args([signal, &server.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let exit = loop {
        if let Some(exit) = server.0.try_wait().unwrap() {
            break exit;
        }
        assert!(Instant::now() < deadline, "server did not stop");
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        exit.success(),
        "{signal} should drain the manager, got {exit}"
    );
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(status_file).unwrap()).unwrap();
    assert_eq!(status["status"], "shuttingDown");
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn dev_server_sigint_shuts_down_cleanly() {
    shutdown("-INT");
}

#[test]
fn dev_server_sigterm_shuts_down_cleanly() {
    shutdown("-TERM");
}
