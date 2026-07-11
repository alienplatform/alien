//! Platform shims — one module per OS, each implementing the
//! `crate::core::traits` boundary. This module is the single place where a
//! `#[cfg(...)]` selects the concrete implementation; nothing else in the
//! crate branches on the target OS.
//!
//! Wired for real: **Linux** (systemd host) and **macOS** (launchd host), both
//! over the shared Unix child supervisor and Unix symlink store — the host is
//! the only per-OS piece; supervision and the version store are identical Unix
//! code. The Windows shim (SCM + Job Object + junction store) lands in its own
//! phase.
//!
//! On any other target (no supported host) the `unix_*` modules are exercised
//! by tests only, hence the narrowed dead-code staging below.

#[cfg(unix)]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
pub mod unix_child;
#[cfg(unix)]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
pub mod unix_signals;
#[cfg(unix)]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
pub mod unix_store;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub mod windows_child;

// The host is per-OS; the child supervisor and version store are shared Unix.
#[cfg(target_os = "linux")]
pub use linux::LinuxHost as ActiveHost;
#[cfg(target_os = "macos")]
pub use macos::MacosHost as ActiveHost;
// Windows aliases `ActiveHost` (→ `WindowsHost`) together with its
// `ActiveChildSupervisor` (T3.2, Job Object) and `ActiveVersionStore` (T3.4,
// junctions) once main.rs's Windows `run_supervisor` consumes them — aliasing
// the host alone now would be an unused re-export. The T3.1 host lives in
// `windows.rs` and is exercised by its unit test.

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix_child::UnixChildSupervisor as ActiveChildSupervisor;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix_store::UnixVersionStore as ActiveVersionStore;
