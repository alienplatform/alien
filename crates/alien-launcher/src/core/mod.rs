//! OS-agnostic core of the launcher: the update state machine, the health
//! gate, failure reporting, and the trait boundary the platform shims
//! implement.
//!
//! HARD RULE — this module tree is platform-blind. Nothing under `src/core/`
//! may import platform modules or crates, name a syscall or a signal, or know
//! whether a version pointer is a symlink or a junction. All platform behavior
//! goes through `traits` and lives in `crate::platform`. The rule is enforced
//! mechanically by the `tests/platform_blind.rs` integration test, which scans
//! this directory's sources for forbidden tokens.

pub mod health;
pub mod report;
pub mod state_machine;
pub mod store_common;
pub mod traits;

#[cfg(test)]
pub mod testing;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::error::Result;
use state_machine::{classify_startup, LoopExit, Machine, RunConfig};
use traits::{ChildSupervisor, HealthProbe, ServiceHost, VersionStore};

/// The launcher entry point: classify the store, execute the startup action,
/// then supervise — with a dedicated watchdog-heartbeat thread that ticks for
/// the launcher's whole life (including through probation windows longer than
/// the watchdog interval; see the rule on `ServiceHost::heartbeat`).
///
/// Store errors propagate out: the OS init respawns the launcher and startup
/// classification recovers from whatever the crash left on disk.
// Wired to the CLI in the Linux phase; consumed by tests until then.
#[allow(dead_code)]
pub fn run<S, C, P, H>(
    store: &S,
    child: &mut C,
    probe: &P,
    host: &H,
    config: &RunConfig,
) -> Result<LoopExit>
where
    S: VersionStore,
    C: ChildSupervisor,
    P: HealthProbe,
    H: ServiceHost + Sync,
{
    let action = classify_startup(store, config)?;
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    std::thread::scope(|scope| {
        scope.spawn(|| heartbeat_loop(host, config.heartbeat_interval, shutdown_rx));

        let mut machine = Machine {
            store,
            child,
            probe,
            host,
            config,
        };
        let result = machine
            .execute_startup(action)
            .and_then(|handle| machine.supervise(handle));

        // Wake the heartbeat thread immediately so the scope can close.
        drop(shutdown_tx);
        result
    })
}

/// Tick `host.heartbeat()` every `interval` until the shutdown channel closes.
/// `recv_timeout` doubles as the sleep, so a shutdown wakes it instantly.
fn heartbeat_loop<H: ServiceHost>(host: &H, interval: Duration, shutdown: mpsc::Receiver<()>) {
    loop {
        host.heartbeat();
        match shutdown.recv_timeout(interval) {
            Err(RecvTimeoutError::Timeout) => continue,
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod run_tests {
    use super::state_machine::RunConfig;
    use super::testing::{SpawnOutcome, StubChild, StubHost, StubProbe, StubStore};
    use super::traits::{Control, PendingMarker, Version};
    use super::*;
    use crate::core::store_common;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    fn version(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// The watchdog heartbeat must keep ticking DURING a probation
    /// window (probation 150 ms >> heartbeat 10 ms), and READY must be
    /// reported after the promote.
    #[test]
    fn watchdog_ticks_through_probation_and_run_stops_on_control() {
        let dir = tempfile::tempdir().unwrap();
        let store = StubStore::new(dir.path());
        store.install_version(&version("1.3.5"));
        store.set_current(&version("1.3.5")).unwrap();
        store.set_last_stable(&version("1.3.5")).unwrap();
        std::fs::write(store.state_dir().join("db"), b"state-v1").unwrap();

        let config = RunConfig {
            probation_window: Duration::from_millis(150),
            probe_interval: Duration::from_millis(5),
            poll_interval: Duration::from_millis(5),
            heartbeat_interval: Duration::from_millis(10),
            stop_grace: Duration::from_millis(50),
            restart_backoff_base: Duration::from_millis(5),
            restart_backoff_cap: Duration::from_millis(40),
            healthy_reset: Duration::from_millis(150),
            max_swap_attempts: 3,
            operator_binary: "alien-operator".to_string(),
            ..RunConfig::default()
        };

        // Stage a valid 1.4.0 so run() starts with a swap whose probation
        // only passes near the end of the window.
        store.install_version(&version("1.4.0"));
        let binary = store.stage_dir(&version("1.4.0")).join("alien-operator");
        let pending = PendingMarker {
            version: version("1.4.0"),
            sha256: store_common::file_sha256(&binary).unwrap(),
            staged_at: chrono::Utc::now(),
        };
        store.write_pending(&pending).unwrap();

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::ReadyAt(Instant::now() + Duration::from_millis(100));
        let host = StubHost::new();
        let controls = host.controls_tx.clone();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            controls.send(Control::Stop).unwrap();
        });

        let exit = run(&store, &mut child, &probe, &host, &config).unwrap();
        assert_eq!(exit, LoopExit::ControlStop(Control::Stop));

        // ≥ 5 heartbeats must have landed during the ~100 ms probation alone;
        // require a conservative floor to avoid timing flake.
        let beats = host.heartbeat_calls.load(Ordering::SeqCst);
        assert!(beats >= 5, "heartbeat starved during probation: {beats} ticks");
        assert!(host.ready_calls.load(Ordering::SeqCst) >= 1, "READY after promote");
        assert!(host.stopping_calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(store.current().unwrap(), Some(version("1.4.0")));
        assert_eq!(store.last_stable().unwrap(), Some(version("1.4.0")));
    }
}
