use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use alien_sandbox_conformance::{evaluate, Expectation, Observation, Probe, Verdict, GUEST_PROBE};

#[test]
fn tls_sni_is_independent_of_connect_authority_and_certificate_validation_still_succeeds() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg(root.join("tests/tls_probe.py"))
        .arg(root.join("src/probe.py"))
        .output()
        .expect("TLS fixture runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn observe(probe: &Probe, proxy: Option<&str>) -> Observation {
    let mut command = Command::new("python3");
    command
        .args(["-c", GUEST_PROBE, &serde_json::to_string(probe).unwrap()])
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy");
    if let Some(proxy) = proxy {
        command.env("HTTPS_PROXY", proxy);
    }
    let output = command.output().expect("Python probe runs");
    assert!(
        output.status.success(),
        "probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("one structured observation")
}

fn serve(response: &'static [u8]) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let task = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "probe never connected"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("listener failed: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            assert!(request.len() < 8192, "request header exceeded bound");
        }
        socket.write_all(response).unwrap();
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}"), task)
}

fn http(url: String) -> Probe {
    Probe::Http {
        url,
        use_proxy: false,
        server_name: None,
        host_header: None,
    }
}

#[test]
fn successful_control_and_subject_return_exact_fixture_bytes() {
    let (url, server) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
    let control = observe(&http(url), None);
    server.join().unwrap();
    assert_eq!(
        control,
        Observation::Response {
            status: 200,
            body_sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            bytes: 5,
        }
    );
    let (url, server) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
    let subject = observe(&http(url), None);
    server.join().unwrap();
    assert_eq!(
        evaluate(
            &Expectation::Response {
                body_sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .into()
            },
            &control,
            &subject
        ),
        Verdict::Pass
    );
}

#[test]
fn an_origin_forbidden_response_proves_network_access() {
    let (url, server) = serve(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
    let subject = observe(&http(url), None);
    server.join().unwrap();
    assert_eq!(
        evaluate(
            &Expectation::NetworkDenied,
            &Observation::Connected,
            &subject
        ),
        Verdict::Fail
    );
    assert_eq!(
        evaluate(&Expectation::ProxyDenied, &Observation::Connected, &subject),
        Verdict::Fail
    );
}

#[test]
fn connect_rejection_is_distinct_from_an_origin_response() {
    let (proxy, server) = serve(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
    let probe = Probe::Http {
        url: "https://example.invalid/probe".into(),
        use_proxy: true,
        server_name: None,
        host_header: None,
    };
    let subject = observe(&probe, Some(&proxy));
    let request = server.join().unwrap();
    assert!(request.starts_with("CONNECT example.invalid:443 HTTP/1.1\r\n"));
    assert_eq!(subject, Observation::ProxyDenied { status: 403 });
    assert_eq!(
        evaluate(&Expectation::ProxyDenied, &Observation::Connected, &subject),
        Verdict::Pass
    );
}

#[test]
fn missing_proxy_configuration_cannot_silently_probe_a_direct_connection() {
    let probe = Probe::Http {
        url: "https://127.0.0.1:1/".into(),
        use_proxy: true,
        server_name: None,
        host_header: None,
    };
    let output = Command::new("python3")
        .args(["-c", GUEST_PROBE, &serde_json::to_string(&probe).unwrap()])
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "configuration errors are not observations"
    );
}

#[test]
fn a_dead_control_cannot_turn_a_denied_subject_into_a_pass() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let control = observe(
        &Probe::Tcp {
            host: "127.0.0.1".into(),
            port,
        },
        None,
    );
    assert_eq!(control, Observation::Refused);
    assert_eq!(
        evaluate(&Expectation::NetworkDenied, &control, &Observation::Timeout),
        Verdict::Unproven
    );
}

#[test]
fn dns_or_tls_failure_is_not_a_network_policy_pass() {
    for subject in [Observation::DnsFailed, Observation::TlsFailed] {
        assert_eq!(
            evaluate(
                &Expectation::NetworkDenied,
                &Observation::Connected,
                &subject
            ),
            Verdict::Unproven
        );
    }
}

#[test]
fn host_mismatch_reaches_the_listener_unchanged() {
    let (url, server) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    let probe = Probe::Http {
        url,
        use_proxy: false,
        server_name: None,
        host_header: Some("other.example.invalid".into()),
    };
    assert!(matches!(
        observe(&probe, None),
        Observation::Response { status: 200, .. }
    ));
    assert!(server
        .join()
        .unwrap()
        .contains("\r\nHost: other.example.invalid\r\n"));
}
