//! alien-launcher — supervisor for the alien-operator OS-service packaging.
//!
//! Sits between the OS init system (systemd / launchd / SCM) and the operator:
//! the init system keeps the launcher alive; the launcher keeps a healthy
//! operator running and owns version swaps + rollback over the on-disk
//! version store. The launcher itself is frozen — it never rewrites its own
//! binary; it is only replaced by a state-preserving reinstall.
//!
//! Layout:
//! - `core/` — the OS-agnostic update state machine, health gate, and the
//!   trait boundary. Must stay platform-blind (see `core`'s module docs);
//!   enforced mechanically by `tests/platform_blind.rs`.
//! - `platform/` — one shim per OS implementing the `core::traits` boundary.

mod core;
mod error;
mod platform;

use std::process::ExitCode;

fn main() -> ExitCode {
    // The supervisor run loop lands with the core state machine. Until it is
    // wired, starting the launcher is a hard, loud error — never a silent
    // no-op that an init system would happily respawn forever.
    eprintln!(
        "alien-launcher {}: the supervisor run loop is not implemented yet",
        env!("CARGO_PKG_VERSION")
    );
    ExitCode::FAILURE
}
