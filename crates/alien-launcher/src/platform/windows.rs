//! Windows service host — the `ServiceHost` boundary over the Service Control
//! Manager (SCM), mirroring the Linux/macOS hosts (`platform/linux.rs`,
//! `platform/macos.rs`). Control signals arrive on an internal cell drained by
//! `poll_control`; `report_ready` / `report_stopping` / `heartbeat` drive the
//! SCM status (`SERVICE_RUNNING` / `SERVICE_STOP_PENDING` / checkpoint bumps).
//!
//! A `--console` mode (what the E2E suite drives) swaps the SCM control handler
//! for a Ctrl-C handler and makes the status calls no-ops — no SCM to report to.
//!
//! `main.rs`'s Windows `run_supervisor` constructs this host — via `service()`
//! under the SCM dispatcher, or `console()` for the E2E suite — and drives it
//! through `core::run`.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use alien_error::AlienError;
use tracing::warn;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};

use crate::core::traits::{Control, ServiceHost};
use crate::error::{ErrorData, Result};

/// Fixed heartbeat cadence — SCM checkpoint bumps while in a `*Pending` state.
/// Mirrors the macOS host's 20 s; SCM has no `WATCHDOG_USEC` equivalent to read.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

/// Wait hint on a `*Pending` status — must exceed the heartbeat interval so the
/// SCM doesn't time the transition out between checkpoints.
const PENDING_WAIT_HINT: Duration = Duration::from_secs(30);

// Pending control request. At-most-once per delivery burst (like the Unix flag):
// repeated controls before a poll collapse into the latest.
const CONTROL_NONE: u8 = 0;
const CONTROL_STOP: u8 = 1;
const CONTROL_SHUTDOWN: u8 = 2;

// Current SCM state, tracked so `heartbeat` only bumps the checkpoint while
// pending (SCM ignores the checkpoint once the service is `Running`).
const STATE_START_PENDING: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_STOP_PENDING: u8 = 2;

/// The console Ctrl-C handler is an `extern "system"` callback that cannot
/// capture, so the console host parks its control cell here for it to reach.
static CONSOLE_CONTROL: OnceLock<Arc<AtomicU8>> = OnceLock::new();

/// Up-facing host over the SCM (service) or a Ctrl-C handler (console).
pub struct WindowsHost {
    /// SCM status handle; `None` in console mode (status calls are no-ops).
    status_handle: Option<ServiceStatusHandle>,
    /// Pending control request, set by the SCM handler / Ctrl-C handler and
    /// drained by `poll_control`.
    control: Arc<AtomicU8>,
    /// Current SCM state, so `heartbeat` bumps the checkpoint only while pending.
    state: AtomicU8,
    /// Monotonic SCM checkpoint for `*Pending` transitions.
    checkpoint: AtomicU32,
}

impl WindowsHost {
    /// Bind to the SCM: register the control handler (routing Stop/Shutdown onto
    /// the control cell) and report `StartPending`. Call from the service main
    /// after `service_dispatcher::start` hands control to it.
    pub fn service(service_name: &str) -> Result<Self> {
        let control = Arc::new(AtomicU8::new(CONTROL_NONE));
        let handler_control = control.clone();
        let status_handle = service_control_handler::register(service_name, move |ctrl| {
            match map_service_control(ctrl) {
                Some(Control::Stop) => {
                    handler_control.store(CONTROL_STOP, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                Some(Control::Shutdown) => {
                    handler_control.store(CONTROL_SHUTDOWN, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                // Interrogate must be acknowledged so the SCM sees us alive.
                None if ctrl == ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                None => ServiceControlHandlerResult::NotImplemented,
            }
        })
        .map_err(|e| {
            AlienError::new(ErrorData::Other {
                message: format!("failed to register the SCM control handler: {e}"),
            })
        })?;

        let host = Self {
            status_handle: Some(status_handle),
            control,
            state: AtomicU8::new(STATE_START_PENDING),
            checkpoint: AtomicU32::new(0),
        };
        host.set_status(ServiceState::StartPending, ServiceControlAccept::empty());
        Ok(host)
    }

    /// Console mode (`--console`, driven by the E2E suite): install a Ctrl-C
    /// handler that requests Stop. No SCM, so the status calls are no-ops.
    pub fn console() -> Result<Self> {
        let control = Arc::new(AtomicU8::new(CONTROL_NONE));
        // Park the cell for the extern handler; ignore if already set (one
        // console launcher per process).
        let _ = CONSOLE_CONTROL.set(control.clone());
        install_console_ctrl_handler()?;
        Ok(Self {
            status_handle: None,
            control,
            state: AtomicU8::new(STATE_START_PENDING),
            checkpoint: AtomicU32::new(0),
        })
    }

    pub fn heartbeat_interval(&self) -> Duration {
        HEARTBEAT_INTERVAL
    }

    /// Report `SERVICE_STOPPED` — the service main calls this after `core::run`
    /// returns. `exit_code` 0 is a clean stop; nonzero trips the SCM recovery
    /// config (the doc-12 restart actions), the Windows analogue of systemd
    /// respawning us on a nonzero exit. No-op in console mode.
    pub fn report_stopped(&self, exit_code: u32) {
        self.state.store(STATE_STOP_PENDING, Ordering::SeqCst);
        let Some(handle) = self.status_handle.as_ref() else {
            return;
        };
        let status = ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(exit_code),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };
        if let Err(e) = handle.set_service_status(status) {
            warn!(error = %e, "failed to report SERVICE_STOPPED");
        }
    }

    /// Emit an SCM status (no-op in console mode). `checkpoint`/`wait_hint` only
    /// matter for `*Pending` states; `Running`/`Stopped` report checkpoint 0.
    fn set_status(&self, state: ServiceState, controls: ServiceControlAccept) {
        let Some(handle) = self.status_handle.as_ref() else {
            return;
        };
        let pending = matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        );
        let checkpoint = if pending {
            self.checkpoint.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            0
        };
        let status = ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: controls,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint,
            wait_hint: if pending {
                PENDING_WAIT_HINT
            } else {
                Duration::default()
            },
            process_id: None,
        };
        if let Err(e) = handle.set_service_status(status) {
            warn!(error = %e, ?state, "failed to set the SCM service status");
        }
    }
}

impl ServiceHost for WindowsHost {
    fn poll_control(&self) -> Option<Control> {
        match self.control.swap(CONTROL_NONE, Ordering::SeqCst) {
            CONTROL_STOP => Some(Control::Stop),
            CONTROL_SHUTDOWN => Some(Control::Shutdown),
            _ => None,
        }
    }

    fn report_ready(&self) {
        self.state.store(STATE_RUNNING, Ordering::SeqCst);
        self.set_status(
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        );
    }

    fn heartbeat(&self) {
        // SCM ignores the checkpoint once `Running`; only bump while pending.
        let state = match self.state.load(Ordering::SeqCst) {
            STATE_RUNNING => return,
            STATE_STOP_PENDING => ServiceState::StopPending,
            _ => ServiceState::StartPending,
        };
        self.set_status(state, ServiceControlAccept::empty());
    }

    fn report_stopping(&self) {
        self.state.store(STATE_STOP_PENDING, Ordering::SeqCst);
        self.set_status(ServiceState::StopPending, ServiceControlAccept::empty());
    }
}

/// Pure mapping: an SCM control → the launcher's `Control`. Stop and Shutdown are
/// the only lifecycle controls we act on; the rest (Interrogate, Pause, …) are
/// not ours to translate.
fn map_service_control(control: ServiceControl) -> Option<Control> {
    match control {
        ServiceControl::Stop => Some(Control::Stop),
        ServiceControl::Shutdown => Some(Control::Shutdown),
        _ => None,
    }
}

/// Console Ctrl-C handler — `extern "system"`, can't capture, so it reads the
/// static control cell. Any close-ish console event requests Stop.
unsafe extern "system" fn console_ctrl_handler(
    ctrl_type: u32,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };
    let requested = ctrl_type == CTRL_C_EVENT
        || ctrl_type == CTRL_BREAK_EVENT
        || ctrl_type == CTRL_CLOSE_EVENT
        || ctrl_type == CTRL_LOGOFF_EVENT
        || ctrl_type == CTRL_SHUTDOWN_EVENT;
    if requested {
        if let Some(control) = CONSOLE_CONTROL.get() {
            control.store(CONTROL_STOP, Ordering::SeqCst);
        }
        1 // TRUE — handled
    } else {
        0 // FALSE — pass to the next handler
    }
}

fn install_console_ctrl_handler() -> Result<()> {
    // SAFETY: `console_ctrl_handler` is a static function that only touches the
    // static atomic control cell — safe to run on the OS's Ctrl-C thread.
    let ok = unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(console_ctrl_handler), 1)
    };
    if ok == 0 {
        return Err(AlienError::new(ErrorData::Other {
            message: "SetConsoleCtrlHandler failed".to_string(),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scm_controls_map_to_lifecycle() {
        assert_eq!(
            map_service_control(ServiceControl::Stop),
            Some(Control::Stop)
        );
        assert_eq!(
            map_service_control(ServiceControl::Shutdown),
            Some(Control::Shutdown)
        );
        assert_eq!(map_service_control(ServiceControl::Interrogate), None);
    }
}
