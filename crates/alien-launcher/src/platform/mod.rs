//! Platform shims — one module per OS, each implementing the
//! `crate::core::traits` boundary. This module is the single place where a
//! `#[cfg(...)]` selects the concrete implementation; nothing else in the
//! crate branches on the target OS.
//!
//! Currently wired: **Linux** (systemd host + shared Unix child supervisor +
//! shared Unix symlink store). The macOS host (launchd) and the Windows shim
//! (SCM + Job Object + junction store) land in their own phases; until then
//! the launcher binary only runs for real on Linux.
//!
//! The `unix_*` modules are shared by Linux and (later) macOS; on non-Linux
//! unix builds they are exercised by tests only until the macOS host lands,
//! hence the targeted dead-code staging below.

#[cfg(unix)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod unix_child;
#[cfg(unix)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod unix_signals;
#[cfg(unix)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod unix_store;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::LinuxHost as ActiveHost;
#[cfg(target_os = "linux")]
pub use unix_child::UnixChildSupervisor as ActiveChildSupervisor;
#[cfg(target_os = "linux")]
pub use unix_store::UnixVersionStore as ActiveVersionStore;
