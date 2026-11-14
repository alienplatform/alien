//! Fail-closed check that the pod's egress restriction is in force before the workload starts.
//!
//! A pod's NetworkPolicy can load a moment after its container starts (measured at about one
//! second on EKS Auto Mode). Anything the workload connects to in that window is unrestricted.
//! So on Kubernetes the agent, before it starts the ENTRYPOINT, tries to connect to destinations
//! the policy refuses: the cluster API in every egress mode, and a public address where the declared
//! egress forbids direct public traffic (`none` and `internet`; an `allow` pod may reach it). The metadata address is not a target: some CNIs leave it
//! reachable, so credential exposure there is a separate check.
//! A drop looks like a timeout, and so does a pod network that is not ready yet, so a round only
//! counts when it also proves the network works:
//! - `internet`: the gateway connects in the same round (a positive control);
//! - `none`: nothing may be used as a control, so the pod must have an address and a default
//!   route, and every blocked attempt must end in a timeout (not ENETUNREACH and the like).
//!
//! It starts the workload only after several such rounds in a row. If they do not come within a
//! bound, it refuses to start at all. After launch the same check re-runs periodically for the
//! sandbox's lifetime; a blocked destination that connects ends the sandbox.

use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

/// Set by the Kubernetes backend on every sandbox pod. Unset elsewhere: nothing to prove.
pub const ENV_STARTUP_GATE: &str = "ALIEN_SANDBOX_STARTUP_EGRESS_GATE";

/// How often the check re-runs after launch.
const RECHECK_INTERVAL: Duration = Duration::from_secs(30);
const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(300);
const ROUND_INTERVAL: Duration = Duration::from_millis(200);
/// Consecutive all-dropped rounds required.
const REQUIRED_ROUNDS: u32 = 3;
/// How long the destinations may stay reachable before the agent gives up.
const BOUND: Duration = Duration::from_secs(30);

/// Result of one connection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// The connection was established.
    Connected,
    /// The connect timed out: a policy drop, or a network that is not up yet.
    Dropped,
    /// Anything else (refused, unreachable, resolution failure): says nothing about the policy.
    Failed,
}

/// What a round checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Must be refused: the cluster API always, a public address only where the policy forbids it.
    pub blocked: Vec<String>,
    /// `internet`: the gateway, which must connect in the same round.
    pub control: Option<String>,
}

/// Outcome of one round: `Ok` or the reasons it does not count.
fn round<F, N>(probe: &Probe, attempt: &mut F, network_up: &mut N) -> Result<(), Vec<String>>
where
    F: FnMut(&str) -> Attempt,
    N: FnMut() -> bool,
{
    let mut problems = Vec::new();
    match &probe.control {
        Some(control) => {
            if attempt(control) != Attempt::Connected {
                problems.push(format!("{control} (the gateway) is not reachable"));
            }
        }
        None => {
            if !network_up() {
                problems.push("the pod network has no address or default route".to_string());
            }
        }
    }
    for target in &probe.blocked {
        match attempt(target) {
            Attempt::Dropped => {}
            Attempt::Connected => problems.push(format!("{target} is reachable")),
            Attempt::Failed => problems.push(format!("{target} failed without a timeout")),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Runs rounds until `REQUIRED_ROUNDS` in a row are fully proven, or `bound` passes.
/// Returns the problems of the last round on failure.
pub fn wait_until_blocked<F, N, S>(
    probe: &Probe,
    bound: Duration,
    interval: Duration,
    mut attempt: F,
    mut network_up: N,
    mut sleep: S,
) -> Result<(), Vec<String>>
where
    F: FnMut(&str) -> Attempt,
    N: FnMut() -> bool,
    S: FnMut(Duration),
{
    let start = Instant::now();
    let mut clean = 0;
    loop {
        match round(probe, &mut attempt, &mut network_up) {
            Ok(()) => {
                clean += 1;
                if clean >= REQUIRED_ROUNDS {
                    return Ok(());
                }
            }
            Err(problems) => {
                clean = 0;
                if start.elapsed() >= bound {
                    return Err(problems);
                }
            }
        }
        sleep(interval);
    }
}

/// After launch: the destinations among `probe.blocked` that now connect.
pub fn breaches<F>(probe: &Probe, mut attempt: F) -> Vec<String>
where
    F: FnMut(&str) -> Attempt,
{
    probe
        .blocked
        .iter()
        .filter(|target| attempt(target) == Attempt::Connected)
        .cloned()
        .collect()
}

fn connect(target: &str) -> Attempt {
    let Some(address) = target
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
    else {
        return Attempt::Failed;
    };
    match std::net::TcpStream::connect_timeout(&address, ATTEMPT_TIMEOUT) {
        Ok(_) => Attempt::Connected,
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Attempt::Dropped,
        Err(_) => Attempt::Failed,
    }
}

/// The pod has a source address and a route out. A connected UDP socket sends nothing.
fn network_up() -> bool {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("1.1.1.1:53")?;
            socket.local_addr()
        })
        .is_ok_and(|address| !address.ip().is_unspecified())
}

/// The blocked targets, or why there are none to prove anything with. The Kubernetes API server
/// is the destination that matters most, so a pod told to gate without being told where the API
/// server is does not start: skipping it would be a silent fail-open.
fn probe_from(
    kube_api: Option<&str>,
    public: Option<&str>,
    service_host: Option<&str>,
    service_port: Option<&str>,
    control: Option<String>,
) -> Result<Probe, String> {
    let mut blocked = Vec::new();
    for target in kube_api
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        if !blocked.iter().any(|known| known == target) {
            blocked.push(target.to_string());
        }
    }
    if let (Some(host), Some(port)) = (service_host, service_port) {
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let target = format!("{host}:{port}");
        if !blocked.contains(&target) {
            blocked.push(target);
        }
    }
    if blocked.is_empty() {
        return Err(format!(
            "{} is not set: the Kubernetes API server address is a required target of the egress \
             check, and without it the check would silently skip the destination that matters \
             most. The Operator sets it from the default/kubernetes Service and EndpointSlice",
            alien_core::sandbox_image::ENV_MUST_DROP_KUBE_API
        ));
    }
    // Only where the declared egress forbids direct public traffic; the Operator says so.
    for target in public
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        if !blocked.iter().any(|known| known == target) {
            blocked.push(target.to_string());
        }
    }
    Ok(Probe { blocked, control })
}

fn probe_from_env() -> Result<Probe, String> {
    probe_from(
        std::env::var(alien_core::sandbox_image::ENV_MUST_DROP_KUBE_API)
            .ok()
            .as_deref(),
        std::env::var(alien_core::sandbox_image::ENV_MUST_DROP_PUBLIC)
            .ok()
            .as_deref(),
        std::env::var("KUBERNETES_SERVICE_HOST").ok().as_deref(),
        std::env::var("KUBERNETES_SERVICE_PORT").ok().as_deref(),
        std::env::var("ALIEN_SANDBOX_EGRESS_GATEWAY").ok(),
    )
}

/// The gate for a pod that asked for it. Returns an error message when the restriction is not
/// proven within the bound.
pub fn enforce_from_env() -> Result<(), String> {
    if std::env::var_os(ENV_STARTUP_GATE).is_none() {
        return Ok(());
    }
    let probe = probe_from_env()?;
    wait_until_blocked(
        &probe,
        BOUND,
        ROUND_INTERVAL,
        connect,
        network_up,
        std::thread::sleep,
    )
    .map_err(|problems| {
        format!(
            "egress restriction not proven after {}s: {}. The cluster is not enforcing this \
                 pod's NetworkPolicy (or the pod network is not ready), so the workload was not \
                 started. Enable NetworkPolicy enforcement (EKS Auto Mode: ConfigMap \
                 amazon-vpc-cni in kube-system with enable-network-policy-controller \"true\"; \
                 GKE: dataplane v2 or network policy; AKS: Azure CNI powered by Cilium, or Calico)",
            BOUND.as_secs(),
            problems.join(", ")
        )
    })
}

/// Re-runs the blocked check every 30 s for the sandbox's lifetime. If a blocked destination
/// connects, it reports why and ends the process, which as PID 1 ends the workload.
pub fn spawn_recheck() {
    if std::env::var_os(ENV_STARTUP_GATE).is_none() {
        return;
    }
    // `enforce_from_env` has already refused a pod without a kube-api target; the pod did not
    // start. Fail closed here too rather than run unchecked.
    let probe = match probe_from_env() {
        Ok(probe) => probe,
        Err(message) => {
            eprintln!("{message}");
            let _ = std::fs::write("/dev/termination-log", &message);
            std::process::exit(1);
        }
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(RECHECK_INTERVAL);
        let open = breaches(&probe, connect);
        if !open.is_empty() {
            let message = format!(
                "egress restriction lost: {} now reachable; ending the sandbox",
                open.join(", ")
            );
            eprintln!("{message}");
            let _ = std::fs::write("/dev/termination-log", &message);
            std::process::exit(1);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn none(blocked: &[&str]) -> Probe {
        Probe {
            blocked: blocked.iter().map(|s| s.to_string()).collect(),
            control: None,
        }
    }

    fn internet(blocked: &[&str]) -> Probe {
        Probe {
            control: Some("gw".to_string()),
            ..none(blocked)
        }
    }

    fn run(
        probe: &Probe,
        bound_ms: u64,
        attempt: impl FnMut(&str) -> Attempt,
        up: bool,
    ) -> Result<(), Vec<String>> {
        wait_until_blocked(
            probe,
            Duration::from_millis(bound_ms),
            Duration::ZERO,
            attempt,
            || up,
            |_| {},
        )
    }

    #[test]
    fn a_gated_pod_without_a_kube_api_target_is_refused() {
        let error = probe_from(None, None, None, None, None).unwrap_err();
        assert!(
            error.contains("ALIEN_SANDBOX_MUST_DROP_KUBE_API"),
            "{error}"
        );
        assert!(probe_from(
            Some(" , "),
            Some("1.1.1.1:443"),
            None,
            None,
            Some("gw".into())
        )
        .is_err());
    }

    #[test]
    fn the_operator_targets_are_probed_with_the_public_address() {
        let probe = probe_from(
            Some("10.0.0.1:443,10.1.2.3:6443"),
            Some("1.1.1.1:443"),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            probe.blocked,
            vec!["10.0.0.1:443", "10.1.2.3:6443", "1.1.1.1:443"]
        );
        // Service-link variables, when present, are probed too and never duplicated.
        let probe = probe_from(
            Some("10.0.0.1:443"),
            Some("1.1.1.1:443"),
            Some("10.0.0.1"),
            Some("443"),
            None,
        )
        .unwrap();
        assert_eq!(probe.blocked.len(), 2);
    }

    #[test]
    fn allow_does_not_probe_the_public_address_so_a_reachable_one_starts() {
        // `allow`: the Operator sets no public target, only the API server.
        let probe = probe_from(Some("10.0.0.1:443"), None, None, None, None).unwrap();
        assert_eq!(probe.blocked, vec!["10.0.0.1:443"]);
        let result = run(
            &probe,
            5000,
            |target| {
                if target == "10.0.0.1:443" {
                    Attempt::Dropped
                } else {
                    Attempt::Connected // 1.1.1.1 reachable, never asked
                }
            },
            true,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn none_and_internet_refuse_a_reachable_public_address() {
        let none_probe =
            probe_from(Some("10.0.0.1:443"), Some("1.1.1.1:443"), None, None, None).unwrap();
        let reachable = |target: &str| {
            if target == "1.1.1.1:443" {
                Attempt::Connected
            } else {
                Attempt::Dropped
            }
        };
        assert_eq!(
            run(&none_probe, 20, reachable, true),
            Err(vec!["1.1.1.1:443 is reachable".to_string()])
        );
        let internet_probe = probe_from(
            Some("10.0.0.1:443"),
            Some("1.1.1.1:443"),
            None,
            None,
            Some("gw".into()),
        )
        .unwrap();
        let result = run(
            &internet_probe,
            20,
            |target| {
                if target == "gw" {
                    Attempt::Connected
                } else {
                    reachable(target)
                }
            },
            true,
        );
        assert_eq!(result, Err(vec!["1.1.1.1:443 is reachable".to_string()]));
        // And the re-check ends such a sandbox.
        assert_eq!(
            breaches(&none_probe, reachable),
            vec!["1.1.1.1:443".to_string()]
        );
    }

    #[test]
    fn none_starts_after_three_clean_rounds() {
        let calls = Cell::new(0);
        let result = run(
            &none(&["a", "b"]),
            5000,
            |_| {
                calls.set(calls.get() + 1);
                Attempt::Dropped
            },
            true,
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 6, "3 rounds x 2 targets");
    }

    #[test]
    fn none_does_not_count_rounds_before_the_network_is_up() {
        let result = run(&none(&["a"]), 20, |_| Attempt::Dropped, false);
        assert!(result.unwrap_err()[0].contains("no address or default route"));
    }

    #[test]
    fn an_unreachable_error_is_not_a_drop() {
        let result = run(&none(&["a"]), 20, |_| Attempt::Failed, true);
        assert_eq!(result, Err(vec!["a failed without a timeout".to_string()]));
    }

    #[test]
    fn internet_needs_the_gateway_to_connect_in_the_same_round() {
        // Everything times out, including the gateway: a network that is not ready, not a policy.
        let result = run(&internet(&["a"]), 20, |_| Attempt::Dropped, true);
        assert!(result.unwrap_err()[0].contains("gateway"));
    }

    #[test]
    fn internet_starts_when_gateway_connects_and_the_rest_drop() {
        let result = run(
            &internet(&["a", "b"]),
            5000,
            |target| {
                if target == "gw" {
                    Attempt::Connected
                } else {
                    Attempt::Dropped
                }
            },
            true,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn a_reachable_target_resets_the_count() {
        let calls = Cell::new(0);
        let result = run(
            &none(&["a"]),
            5000,
            |_| {
                calls.set(calls.get() + 1);
                if calls.get() == 3 {
                    Attempt::Connected
                } else {
                    Attempt::Dropped
                }
            },
            true,
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 6, "2 clean, 1 open, then 3 clean");
    }

    #[test]
    fn refuses_when_a_target_stays_reachable_past_the_bound() {
        let result = wait_until_blocked(
            &none(&["a", "b"]),
            Duration::from_millis(30),
            Duration::from_millis(5),
            |target| {
                if target == "b" {
                    Attempt::Connected
                } else {
                    Attempt::Dropped
                }
            },
            || true,
            std::thread::sleep,
        );
        assert_eq!(result, Err(vec!["b is reachable".to_string()]));
    }

    #[test]
    fn recheck_reports_only_a_clean_connect() {
        let probe = none(&["a", "b", "c"]);
        let open = breaches(&probe, |target| match target {
            "a" => Attempt::Connected,
            "b" => Attempt::Failed,
            _ => Attempt::Dropped,
        });
        assert_eq!(open, vec!["a".to_string()]);
    }

    #[test]
    fn a_listener_is_connected_and_a_closed_port_is_failed_not_dropped() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        assert_eq!(connect(&address), Attempt::Connected);
        drop(listener);
        assert_eq!(connect(&address), Attempt::Failed);
    }
}
