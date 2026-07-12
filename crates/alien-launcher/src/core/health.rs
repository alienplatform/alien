//! The readiness-gate client: a blocking GET against the operator's local
//! `/readyz` endpoint, polled by the state machine during probation.
//!
//! Deliberately minimal: one request, one timeout, no retries — the state
//! machine's gate loop IS the retry policy. Anything that is not a clean
//! `200` (connection refused, timeout, 5xx, transport error) is simply
//! "not ready yet".

// Skeleton staging: constructed by the CLI wiring in the Linux phase;
// consumed by tests until then.
#![allow(dead_code)]

use std::time::Duration;

use super::traits::HealthProbe;

/// Default per-request timeout. Well under the probe interval × 2 so a
/// hanging endpoint can never starve the gate loop.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Blocking `HealthProbe` over `ureq` — tiny, synchronous, no async runtime,
/// matching the launcher's deliberately-synchronous design. Shared by all
/// three OSes.
pub struct UreqProbe {
    agent: ureq::Agent,
}

impl UreqProbe {
    /// A probe with an explicit per-request timeout (covers connect + read).
    pub fn new(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // 4xx/5xx are readiness answers, not transport errors — we want
            // to see the status, not an Err.
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl Default for UreqProbe {
    fn default() -> Self {
        Self::new(DEFAULT_PROBE_TIMEOUT)
    }
}

impl HealthProbe for UreqProbe {
    fn is_ready(&self, url: &str) -> bool {
        match self.agent.get(url).call() {
            Ok(response) => response.status().as_u16() == 200,
            // Refused / timed out / reset / malformed — all mean "not ready
            // yet" during probation; the gate keeps polling.
            Err(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    /// One-shot HTTP responder on 127.0.0.1: reads the request head, writes
    /// `response`, closes. Returns the URL to probe.
    fn one_shot_server(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind should succeed");
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept should succeed");
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf); // consume the request head
            stream
                .write_all(response.as_bytes())
                .expect("response write should succeed");
        });
        format!("http://{addr}/readyz")
    }

    #[test]
    fn ready_on_200() {
        let url = one_shot_server("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok");
        assert!(UreqProbe::default().is_ready(&url));
    }

    #[test]
    fn not_ready_on_503() {
        let url =
            one_shot_server("HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n");
        assert!(!UreqProbe::default().is_ready(&url));
    }

    #[test]
    fn not_ready_on_connection_refused() {
        // Bind to learn a free port, then drop the listener so the connect
        // is refused.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let started = Instant::now();
        assert!(!UreqProbe::default().is_ready(&format!("http://{addr}/readyz")));
        // A refused connection resolves to "not ready" within the probe timeout.
        // On Unix the OS RSTs immediately (near-instant); on Windows a connect to
        // a just-released loopback port can instead run to the connect timeout
        // before failing — still not-ready, just not instant. Assert the probe's
        // actual contract (bounded by `timeout_global`), not the OS's fast-fail
        // optimization, so this holds cross-platform while still catching a
        // never-returns regression.
        assert!(
            started.elapsed() < DEFAULT_PROBE_TIMEOUT + Duration::from_secs(1),
            "refused connection must resolve within the probe timeout, took {:?}",
            started.elapsed()
        );
    }

    /// A hanging endpoint (accepts, never responds) returns false within the
    /// timeout — the done-when bound is ≤ 3 s with the 2 s default.
    #[test]
    fn not_ready_on_hang_within_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept should succeed");
            // Hold the socket open, never respond.
            std::thread::sleep(Duration::from_secs(5));
            drop(stream);
        });

        let started = Instant::now();
        let ready = UreqProbe::default().is_ready(&format!("http://{addr}/readyz"));
        let elapsed = started.elapsed();
        assert!(!ready, "a hanging endpoint is not ready");
        assert!(
            elapsed <= Duration::from_secs(3),
            "probe must give up within 3s on a hang, took {elapsed:?}"
        );
    }

    /// A garbage response (not HTTP) is "not ready", never a panic.
    #[test]
    fn not_ready_on_malformed_response() {
        let url = one_shot_server("definitely not http\r\n\r\n");
        assert!(!UreqProbe::default().is_ready(&url));
    }
}
