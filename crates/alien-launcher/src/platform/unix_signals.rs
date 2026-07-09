//! Unix signal → `poll_control` bridge, shared by the Linux and macOS hosts.
//!
//! `signal-hook` (rather than a `sigwait` thread) because its handler
//! registration is async-signal-safe by construction and needs no process-wide
//! signal-mask coordination: the handler just flips an atomic, and the run
//! loop's `poll_control` drains it once per delivery. SIGTERM and SIGINT both
//! map to `Control::Stop` — on Unix a host shutdown arrives as SIGTERM, so
//! there is no separate `Shutdown` signal to distinguish.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::traits::Control;
use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};

/// Registered stop-signal flag. Cheap to clone-share with a host struct.
pub struct SignalControls {
    stop_requested: Arc<AtomicBool>,
}

impl SignalControls {
    /// Register SIGTERM + SIGINT handlers. Call once, early in `main`.
    pub fn register() -> Result<Self> {
        let stop_requested = Arc::new(AtomicBool::new(false));
        for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
            signal_hook::flag::register(signal, stop_requested.clone())
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("failed to register handler for signal {signal}"),
                })?;
        }
        Ok(Self { stop_requested })
    }

    /// Drain the pending stop request, if any (at-most-once per delivery
    /// burst — repeated signals before a poll collapse into one Stop, which
    /// is exactly what the run loop wants).
    pub fn poll(&self) -> Option<Control> {
        if self.stop_requested.swap(false, Ordering::SeqCst) {
            Some(Control::Stop)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for SignalControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalControls")
            .field("stop_requested", &self.stop_requested.load(Ordering::SeqCst))
            .finish()
    }
}

/// Serializes tests that deliver a real signal to our own process. Signals are
/// process-wide, so two self-`SIGTERM` tests running concurrently would each
/// see the other's signal and break their strict assertions; taking this guard
/// makes them run one at a time. Test-only.
#[cfg(test)]
pub(crate) fn signal_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real SIGTERM to our own process lands as exactly one `Control::Stop`
    /// on the next poll, and the flag drains.
    #[test]
    fn sigterm_delivers_stop_once() {
        let _guard = signal_test_guard();
        let controls = SignalControls::register().expect("registration should succeed");
        assert_eq!(controls.poll(), None, "no signal yet");

        // Deliver SIGTERM to ourselves; the signal-hook handler flips the flag.
        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGTERM)
            .expect("self-signal should succeed");

        // The handler runs asynchronously; poll until it lands (bounded).
        let mut seen = None;
        for _ in 0..100 {
            if let Some(control) = controls.poll() {
                seen = Some(control);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(seen, Some(Control::Stop), "SIGTERM must map to Stop");
        assert_eq!(controls.poll(), None, "drained after one poll");
    }
}
