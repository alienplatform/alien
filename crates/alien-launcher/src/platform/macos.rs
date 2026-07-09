//! macOS / launchd `ServiceHost`.
//!
//! launchd has no readiness or watchdog protocol: a daemon is considered "up"
//! while its process is alive, and supervision is `KeepAlive` + exit codes (see
//! the installer's plist). So there is nothing for the launcher to notify —
//! every notify method is an intentional no-op, and the host reduces to the
//! shared Unix stop-signal bridge (`unix_signals`) for `poll_control`.
//!
//! `report_ready`/`heartbeat`/`report_stopping` exist only to satisfy the
//! `ServiceHost` contract the OS-agnostic core drives; on macOS they do nothing
//! and return immediately.

use std::time::Duration;

use super::unix_signals::SignalControls;
use crate::core::traits::{Control, ServiceHost};
use crate::error::Result;

/// Tick cadence for the core's heartbeat thread. launchd has no watchdog, so
/// `heartbeat` is a no-op; this only bounds how often that no-op wakes. Kept in
/// the same order of magnitude as the Linux default to avoid needless wakeups.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

pub struct MacosHost {
    signals: SignalControls,
}

impl MacosHost {
    pub fn new() -> Result<Self> {
        Ok(Self {
            signals: SignalControls::register()?,
        })
    }

    /// launchd provides no watchdog budget, so unlike the Linux host there is
    /// nothing to derive — the core still runs a heartbeat thread, so hand it a
    /// fixed, sane cadence for its no-op ping.
    pub fn heartbeat_interval(&self) -> Duration {
        HEARTBEAT_INTERVAL
    }
}

impl ServiceHost for MacosHost {
    fn poll_control(&self) -> Option<Control> {
        self.signals.poll()
    }

    // launchd has no notify protocol; the following are intentional no-ops that
    // return immediately (see the module docs). Supervision is KeepAlive + exit
    // codes, so there is no READY / WATCHDOG / STOPPING to signal.
    fn report_ready(&self) {}
    fn heartbeat(&self) {}
    fn report_stopping(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real SIGTERM to our own process surfaces as `Control::Stop` on the
    /// host's `poll_control` (which delegates to the shared Unix signal bridge).
    /// Serialized against the other self-signal test so concurrent SIGTERMs
    /// can't perturb either assertion.
    #[test]
    fn sigterm_maps_to_stop() {
        let _guard = crate::platform::unix_signals::signal_test_guard();
        let host = MacosHost::new().expect("host should construct");

        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGTERM)
            .expect("self-signal should succeed");

        // The handler runs asynchronously; poll until it lands (bounded). Extra
        // SIGTERMs would only make Stop appear sooner, so this stays robust.
        let mut seen = None;
        for _ in 0..100 {
            if let Some(control) = host.poll_control() {
                seen = Some(control);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(seen, Some(Control::Stop), "SIGTERM must map to Stop");
    }

    /// launchd has no notify protocol: every notify method is a no-op that must
    /// return immediately without panicking, and the heartbeat cadence is sane.
    #[test]
    fn notify_methods_are_noops_and_dont_panic() {
        let host = MacosHost::new().expect("host should construct");
        host.report_ready();
        host.heartbeat();
        host.report_stopping();
        assert!(
            host.heartbeat_interval() > Duration::ZERO,
            "heartbeat cadence must be positive"
        );
    }
}
