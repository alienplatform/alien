//! Platform shims — one module per OS, each implementing the
//! `crate::core::traits` boundary. This module is the single place where a
//! `#[cfg(...)]` selects the concrete implementation; nothing else in the
//! crate branches on the target OS.
//!
//! Shims land with their phases:
//!
//! ```text
//! #[cfg(target_os = "linux")]   mod linux;    // systemd: sd-notify + process group
//! #[cfg(target_os = "macos")]   mod macos;    // launchd: no-op host + process group
//! #[cfg(windows)]               mod windows;  // SCM: dispatcher + Job Object
//! // pub use <selected>::… as Active;
//! ```
