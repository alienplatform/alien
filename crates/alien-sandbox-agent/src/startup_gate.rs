//! Fail-closed check that the pod's egress restriction is in force before the workload starts.
//!
//! A pod's NetworkPolicy can load a moment after its container starts (measured at about one
//! second on EKS Auto Mode). Anything the workload connects to in that window is unrestricted.
//! So on Kubernetes the agent, before it starts the ENTRYPOINT, tries to connect to destinations
//! the policy refuses in every egress mode (the cluster API and the instance metadata address).
//! It starts the workload only after several consecutive rounds in which every attempt was
//! dropped. If a destination stays reachable past a bound, it refuses to start at all.

use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Set by the Kubernetes backend on every sandbox pod. Unset elsewhere: nothing to prove.
pub const ENV_STARTUP_GATE: &str = "ALIEN_SANDBOX_STARTUP_EGRESS_GATE";

const METADATA_ADDRESS: &str = "169.254.169.254:80";
const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(300);
const ROUND_INTERVAL: Duration = Duration::from_millis(200);
/// Consecutive all-dropped rounds required.
const REQUIRED_ROUNDS: u32 = 3;
/// How long the destinations may stay reachable before the agent gives up.
const BOUND: Duration = Duration::from_secs(30);

/// Result of one connection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// The packet was dropped (the connect timed out): the policy is in force.
    Dropped,
    /// Anything else: accepted, reset, or an error that says nothing about the policy.
    NotDropped,
}

/// Runs the gate until `REQUIRED_ROUNDS` rounds in a row are fully dropped, or `bound` passes.
/// Returns the destinations still not proven dropped on failure.
pub fn wait_until_blocked<F, S>(
    targets: &[String],
    bound: Duration,
    interval: Duration,
    mut attempt: F,
    mut sleep: S,
) -> Result<(), Vec<String>>
where
    F: FnMut(&str) -> Attempt,
    S: FnMut(Duration),
{
    let start = Instant::now();
    let mut clean = 0;
    loop {
        let open: Vec<String> = targets
            .iter()
            .filter(|target| attempt(target) != Attempt::Dropped)
            .cloned()
            .collect();
        if open.is_empty() {
            clean += 1;
            if clean >= REQUIRED_ROUNDS {
                return Ok(());
            }
        } else {
            clean = 0;
            if start.elapsed() >= bound {
                return Err(open);
            }
        }
        sleep(interval);
    }
}

fn connect(target: &str) -> Attempt {
    let Some(address) = target
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
    else {
        return Attempt::NotDropped;
    };
    connect_address(address)
}

fn connect_address(address: SocketAddr) -> Attempt {
    match std::net::TcpStream::connect_timeout(&address, ATTEMPT_TIMEOUT) {
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Attempt::Dropped,
        _ => Attempt::NotDropped,
    }
}

fn targets_from_env() -> Vec<String> {
    let mut targets = vec![METADATA_ADDRESS.to_string()];
    if let (Ok(host), Ok(port)) = (
        std::env::var("KUBERNETES_SERVICE_HOST"),
        std::env::var("KUBERNETES_SERVICE_PORT"),
    ) {
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        targets.push(format!("{host}:{port}"));
    }
    targets
}

/// The gate for a pod that asked for it. Returns an error message when the restriction is not
/// in force within the bound.
pub fn enforce_from_env() -> Result<(), String> {
    if std::env::var_os(ENV_STARTUP_GATE).is_none() {
        return Ok(());
    }
    let targets = targets_from_env();
    wait_until_blocked(&targets, BOUND, ROUND_INTERVAL, connect, std::thread::sleep).map_err(
        |open| {
            format!(
                "egress is not restricted: {} still reachable after {}s. The cluster is not \
                 enforcing this pod's NetworkPolicy, so the workload was not started. Enable \
                 NetworkPolicy enforcement (EKS Auto Mode: ConfigMap amazon-vpc-cni in kube-system \
                 with enable-network-policy-controller \"true\"; GKE: dataplane v2 or network \
                 policy; AKS: Azure/Calico network policy)",
                open.join(", "),
                BOUND.as_secs()
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn t(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn starts_after_three_clean_rounds() {
        let rounds = Cell::new(0);
        let result = wait_until_blocked(
            &t(&["a", "b"]),
            Duration::from_secs(5),
            Duration::ZERO,
            |_| {
                rounds.set(rounds.get() + 1);
                Attempt::Dropped
            },
            |_| {},
        );
        assert!(result.is_ok());
        assert_eq!(rounds.get(), 6, "3 rounds x 2 targets");
    }

    #[test]
    fn a_reachable_target_resets_the_count() {
        let calls = Cell::new(0);
        // Round 1 open, then everything blocked: needs 3 clean rounds after the open one.
        let result = wait_until_blocked(
            &t(&["a"]),
            Duration::from_secs(5),
            Duration::ZERO,
            |_| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Attempt::NotDropped
                } else {
                    Attempt::Dropped
                }
            },
            |_| {},
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn a_clean_streak_broken_by_one_open_round_restarts() {
        let calls = Cell::new(0);
        let result = wait_until_blocked(
            &t(&["a"]),
            Duration::from_secs(5),
            Duration::ZERO,
            |_| {
                calls.set(calls.get() + 1);
                if calls.get() == 3 {
                    Attempt::NotDropped
                } else {
                    Attempt::Dropped
                }
            },
            |_| {},
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 6, "2 clean, 1 open, then 3 clean");
    }

    #[test]
    fn refuses_when_a_target_stays_reachable_past_the_bound() {
        let result = wait_until_blocked(
            &t(&["a", "b"]),
            Duration::from_millis(30),
            Duration::from_millis(5),
            |target| {
                if target == "b" {
                    Attempt::NotDropped
                } else {
                    Attempt::Dropped
                }
            },
            std::thread::sleep,
        );
        assert_eq!(result, Err(t(&["b"])));
    }

    #[test]
    fn an_accepting_or_refusing_listener_is_not_a_drop() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        assert_eq!(connect(&address), Attempt::NotDropped);
        drop(listener);
        assert_eq!(connect(&address), Attempt::NotDropped);
    }
}
