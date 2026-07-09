//! Linux / systemd `ServiceHost`.
//!
//! The unit runs with `Type=notify` + `WatchdogSec` (see the installer's unit
//! template): the launcher signals `READY=1` once a healthy operator is up and
//! pings `WATCHDOG=1` from the core's dedicated heartbeat thread — including
//! through probation windows longer than the watchdog interval, or systemd
//! would kill the launcher mid-swap.
//!
//! Every notify call is a silent no-op when `NOTIFY_SOCKET` is unset (running
//! outside systemd — tests, the E2E harness, manual runs), so the same binary
//! behaves identically under and outside the init system.

use std::time::Duration;

use tracing::warn;

use super::unix_signals::SignalControls;
use crate::core::traits::{Control, ServiceHost};
use crate::error::Result;

/// Fallback heartbeat interval when systemd did not provide `WATCHDOG_USEC`
/// (watchdog disabled or running outside systemd).
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

pub struct LinuxHost {
    signals: SignalControls,
}

impl LinuxHost {
    pub fn new() -> Result<Self> {
        Ok(Self {
            signals: SignalControls::register()?,
        })
    }

    /// The interval the core's heartbeat thread should tick at:
    /// `WATCHDOG_USEC / 3` when systemd supervises with a watchdog, else the
    /// default. A third of the budget tolerates two missed/slow ticks before
    /// systemd declares the launcher hung.
    pub fn heartbeat_interval(&self) -> Duration {
        heartbeat_interval_from(std::env::var("WATCHDOG_USEC").ok().as_deref())
    }
}

/// Pure decision core, unit-tested: parse systemd's `WATCHDOG_USEC` (µs).
fn heartbeat_interval_from(watchdog_usec: Option<&str>) -> Duration {
    match watchdog_usec.and_then(|raw| raw.parse::<u64>().ok()) {
        Some(usec) if usec > 0 => Duration::from_micros(usec / 3),
        Some(_) => DEFAULT_HEARTBEAT_INTERVAL,
        None => {
            if watchdog_usec.is_some() {
                warn!("WATCHDOG_USEC is set but unparseable; using the default heartbeat interval");
            }
            DEFAULT_HEARTBEAT_INTERVAL
        }
    }
}

impl ServiceHost for LinuxHost {
    fn poll_control(&self) -> Option<Control> {
        self.signals.poll()
    }

    fn report_ready(&self) {
        // No-op (Ok) without NOTIFY_SOCKET; a genuine socket error is worth a
        // log but never worth failing the launcher over.
        if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Ready]) {
            warn!(error = %e, "failed to send READY=1 to systemd");
        }
    }

    fn heartbeat(&self) {
        if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]) {
            warn!(error = %e, "failed to send WATCHDOG=1 to systemd");
        }
    }

    fn report_stopping(&self) {
        if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Stopping]) {
            warn!(error = %e, "failed to send STOPPING=1 to systemd");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `WATCHDOG_USEC / 3`, defaulting when absent, zero, or garbage.
    #[test]
    fn heartbeat_interval_is_a_third_of_the_watchdog_budget() {
        // systemd's WatchdogSec=60 arrives as 60_000_000 µs → 20 s ticks.
        assert_eq!(
            heartbeat_interval_from(Some("60000000")),
            Duration::from_secs(20)
        );
        assert_eq!(
            heartbeat_interval_from(Some("30000000")),
            Duration::from_secs(10)
        );
        assert_eq!(heartbeat_interval_from(None), DEFAULT_HEARTBEAT_INTERVAL);
        assert_eq!(
            heartbeat_interval_from(Some("garbage")),
            DEFAULT_HEARTBEAT_INTERVAL
        );
        assert_eq!(heartbeat_interval_from(Some("0")), DEFAULT_HEARTBEAT_INTERVAL);
    }

    /// Outside systemd (no NOTIFY_SOCKET in the test env) every notify call
    /// is a silent no-op — the host must never panic or error the launcher.
    #[test]
    fn notify_calls_are_noops_without_systemd() {
        let host = LinuxHost::new().expect("host should construct");
        host.report_ready();
        host.heartbeat();
        host.report_stopping();
        // Reaching here without panic is the assertion; poll_control still works.
        assert_eq!(host.poll_control(), None);
    }
}
