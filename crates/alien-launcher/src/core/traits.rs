//! The trait boundary between the OS-agnostic core and the platform shims.
//!
//! Four traits isolate every platform difference; the core state machine is
//! written once against them and never names a syscall, a signal, a symlink,
//! or a Job Object. Signatures here are deliberately reviewed against the
//! constraints of ALL THREE OSes (no symlink assumption, no PDEATHSIG
//! assumption, Job-Object-compatible stop semantics, launchd's lack of a
//! notify protocol) so that no platform shim ever forces a signature change.
//!
//! Per-OS realization of each method:
//!
//! | Trait method | Linux (systemd) | macOS (launchd) | Windows (SCM) |
//! |---|---|---|---|
//! | `ServiceHost::poll_control` | signal thread → channel, drained non-blocking | same | SCM control handler → channel, drained non-blocking |
//! | `ServiceHost::report_ready` | `sd-notify` READY=1 | no-op | `SERVICE_RUNNING` |
//! | `ServiceHost::heartbeat` | `sd-notify` WATCHDOG=1 | no-op | checkpoint bump |
//! | `ChildSupervisor::spawn` | process group + parent-death signal | process group + operator-side parent watch | Job Object (kill-on-close) |
//! | `ChildSupervisor::stop` | SIGTERM then SIGKILL | SIGTERM then SIGKILL | CTRL_BREAK then Job terminate |
//! | `VersionStore::set_current` | atomic rename of a symlink | atomic rename of a symlink | directory-junction swap |
//! | `VersionStore::gc` | unlink | unlink | delayed delete for locked files |
//! | `HealthProbe::is_ready` | HTTP GET (shared impl) | shared | shared |

// Skeleton staging: the env consts, ExitStatus/Control variants, and a few
// helpers are constructed by the platform shims; until the first shim lands
// the non-test build sees them as dead. Remove once wired.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

// The pieces of the launcher↔operator protocol that BOTH binaries serialize
// (marker shapes, env-var names, the exit-code contract, the semver Version)
// live in the shared crate — `alien_core::self_update` — and are re-exported
// here so launcher code keeps its natural paths. `ProbationMarker` below is
// deliberately NOT shared: it is launcher-private bookkeeping the operator
// never reads.
pub use alien_core::self_update::{
    FailureRecord, PendingMarker, Version, EXIT_CODE_UPDATE_HANDOFF,
};
// (The spawn env-var names — ENV_SELF_UPDATE / ENV_LAUNCHER_VERSION /
// ENV_HEALTH_ADDR — also live in `alien_core::self_update`; the platform
// shims import them from there when they map `UpdateEnv` onto the child's
// environment.)

/// Opaque handle to a spawned operator child. Only meaningful to the
/// `ChildSupervisor` that produced it.
#[derive(Debug, Clone)]
pub struct OperatorHandle {
    /// OS process id of the operator child (valid on all three OSes).
    pub pid: u32,
}

/// Environment the launcher passes to the operator on spawn. The
/// `ChildSupervisor` maps these onto the boundary's environment variables
/// (`ENV_SELF_UPDATE`, `ENV_LAUNCHER_VERSION`, `ENV_HEALTH_ADDR`).
#[derive(Debug, Clone)]
pub struct UpdateEnv {
    /// Where the operator must serve `/readyz` + `/livez`, and where the
    /// launcher probes during probation. Always a loopback address — the
    /// endpoints are consumed only by the local launcher.
    pub health_addr: SocketAddr,
    /// The launcher's own version, reported by the operator on sync so the
    /// manager can gate targets on `min_launcher_version`.
    pub launcher_version: String,
}

/// A lifecycle request from the OS supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Stop the service (systemctl stop / launchctl bootout / SCM Stop).
    Stop,
    /// The host is shutting down (SCM Shutdown; folded into SIGTERM on Unix).
    Shutdown,
}

/// How the operator child exited.
///
/// Our own enum rather than `std::process::ExitStatus` so the same type fits
/// all three OSes: Windows has exit codes but no signals, Unix has both. The
/// state machine's exit-code contract (0 = clean stop, 10 = update handoff,
/// other = crash) is decided on `Code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// Exited with a code.
    Code(i32),
    /// Killed by a signal (Unix only; never produced on Windows).
    Signal,
    /// The platform could not report how the process ended.
    Unknown,
}

// ---------------------------------------------------------------------------
// Marker files (the on-disk handoff protocol)
// ---------------------------------------------------------------------------

/// `probation.json` — written by the LAUNCHER at the start of a swap, before
/// flipping `current`, so a crash at any later step is classifiable on
/// restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbationMarker {
    /// The version being promoted.
    pub new: Version,
    /// The version we swap away from (and roll back to on failure).
    pub old: Version,
    /// Wall-clock start of the probation window. A restarted launcher resumes
    /// with the remaining time; negative elapsed (NTP step) clamps to zero.
    pub started_at: DateTime<Utc>,
    /// 1-based swap attempt for this target version.
    pub attempt: u32,
}

// ---------------------------------------------------------------------------
// The four boundary traits
// ---------------------------------------------------------------------------

/// Up-facing: how the launcher reports to, and receives control from, the OS
/// init system.
///
/// | | Linux (systemd) | macOS (launchd) | Windows (SCM) |
/// |---|---|---|---|
/// | `poll_control` | signal thread → channel | same | control handler → channel |
/// | `report_ready` | READY=1 | no-op | `SERVICE_RUNNING` |
/// | `heartbeat` | WATCHDOG=1 | no-op | checkpoint bump |
/// | `report_stopping` | STOPPING=1 | no-op | `STOP_PENDING` → `STOPPED` |
pub trait ServiceHost {
    /// Non-blocking: has the supervisor requested a lifecycle change since the
    /// last poll? The run loop polls this every tick. (Deliberately not a
    /// blocking wait: a blocked receive with no cancellation would leave the
    /// control listener unjoinable on fatal-error paths. Platform impls
    /// deliver signals / SCM controls onto an internal channel and this
    /// method drains it.)
    fn poll_control(&self) -> Option<Control>;

    /// A healthy operator is up.
    fn report_ready(&self);

    /// Liveness ping. MUST be callable from a dedicated heartbeat thread that
    /// ticks for the launcher's whole life — including during a probation
    /// window longer than the watchdog interval — and must never be starved
    /// by the update loop.
    fn heartbeat(&self);

    /// We are shutting down.
    fn report_stopping(&self);
}

/// Down-facing: spawn/stop/observe the operator child.
///
/// Die-with-parent is NORMATIVE, not hardening: an orphaned operator severs
/// the exit-code handoff channel, deadlocks the respawned launcher against
/// the operator's instance lock, and breaks stop/redeploy semantics. Every
/// implementation must guarantee the operator dies when the launcher dies
/// (kill-group / Job Object / parent watch), with the operator's instance
/// lock as the correctness backstop.
///
/// | | Linux | macOS | Windows |
/// |---|---|---|---|
/// | `spawn` | process group + parent-death signal | process group + operator-side parent watch | Job Object, kill-on-close |
/// | `stop` | SIGTERM → SIGKILL | SIGTERM → SIGKILL | CTRL_BREAK → Job terminate |
pub trait ChildSupervisor {
    /// Spawn the operator binary inside a kill-group, with the `UpdateEnv`
    /// mapped onto `ENV_SELF_UPDATE=1`, `ENV_LAUNCHER_VERSION`, and
    /// `ENV_HEALTH_ADDR`.
    fn spawn(&mut self, binary: &Path, env: &UpdateEnv) -> Result<OperatorHandle>;

    /// Graceful stop, escalating to force-kill after `grace`.
    fn stop(&mut self, handle: &OperatorHandle, grace: Duration) -> Result<()>;

    /// Non-blocking: has the child exited, and how?
    fn try_wait(&mut self, handle: &OperatorHandle) -> Result<Option<ExitStatus>>;
}

/// The on-disk version store: `versions/`, the `current` / `last-stable`
/// pointers, `state/` snapshots, the protocol marker files, and gc.
///
/// Hides symlink-vs-junction and locked-file deletion. ALL marker writes MUST
/// be atomic — write `<name>.tmp`, fsync, rename — so a crash never leaves a
/// half-written marker for startup classification to trip on.
///
/// | | Linux / macOS | Windows |
/// |---|---|---|
/// | pointer flip | atomic `rename` of a symlink | directory-junction swap |
/// | `gc` | unlink | delayed delete for locked `.exe`s |
pub trait VersionStore {
    /// Directory where a version's binary is staged: `versions/<v>/`.
    fn stage_dir(&self, version: &Version) -> PathBuf;

    /// The version `current` points at. `None` on a store that has never had
    /// a version installed.
    fn current(&self) -> Result<Option<Version>>;

    /// The proven-good fallback `last-stable` points at.
    fn last_stable(&self) -> Result<Option<Version>>;

    /// Atomically repoint `current`.
    fn set_current(&self, version: &Version) -> Result<()>;

    /// Atomically repoint `last-stable`.
    fn set_last_stable(&self, version: &Version) -> Result<()>;

    /// Copy `state/` to `state-snapshots/<tag>/` (temp dir + rename). Called
    /// before every swap so rollback restores a (binary + state) pair the old
    /// binary can open.
    fn snapshot_state(&self, tag: &Version) -> Result<()>;

    /// Restore `state/` from `state-snapshots/<tag>/` during rollback.
    fn restore_state(&self, tag: &Version) -> Result<()>;

    /// Remove `state-snapshots/<tag>/` (idempotent) — promote's final step.
    fn drop_snapshot(&self, tag: &Version) -> Result<()>;

    /// Size of `state/` in bytes — logged with every swap so snapshot-copy
    /// cost growth is visible before it hurts.
    fn state_size(&self) -> Result<u64>;

    /// Delete versions not in `keep`. Implementations must never delete the
    /// versions `current` or `last-stable` point at, regardless of `keep`.
    fn gc(&self, keep: &[Version]) -> Result<()>;

    // --- protocol marker files (atomic temp+fsync+rename writes) ---

    /// Read `pending.json`; `None` when absent.
    fn read_pending(&self) -> Result<Option<PendingMarker>>;

    /// Atomically write `pending.json`.
    fn write_pending(&self, marker: &PendingMarker) -> Result<()>;

    /// Remove `pending.json` (idempotent — absent is not an error).
    fn clear_pending(&self) -> Result<()>;

    /// Read `probation.json`; `None` when absent.
    fn read_probation(&self) -> Result<Option<ProbationMarker>>;

    /// Atomically write `probation.json`.
    fn write_probation(&self, marker: &ProbationMarker) -> Result<()>;

    /// Remove `probation.json` (idempotent).
    fn clear_probation(&self) -> Result<()>;

    /// Read `failed/<version>.json`; `None` when absent.
    fn read_failure(&self, version: &Version) -> Result<Option<FailureRecord>>;

    /// Atomically write `failed/<version>.json` (overwrites an existing
    /// record — the caller increments `attempts`).
    fn write_failure(&self, record: &FailureRecord) -> Result<()>;

    /// All versions present under `versions/`.
    fn list_versions(&self) -> Result<Vec<Version>>;

    /// Disk-space preflight: succeed iff there is enough free space to
    /// snapshot `state/`. An out-of-space condition must abort the attempt
    /// cleanly (`ErrorData::DiskSpace`), never corrupt the store.
    fn free_space_for_snapshot(&self) -> Result<()>;
}

/// The readiness-gate client: one blocking GET against the operator's local
/// `/readyz`. Shared implementation on every OS.
pub trait HealthProbe {
    /// `true` iff `GET {url}` returned 200 within the probe timeout.
    /// Connection refused, timeouts, and non-200s are all `false` — during
    /// probation those simply mean "not ready yet".
    fn is_ready(&self, url: &str) -> bool;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn version(s: &str) -> Version {
        Version::parse(s).expect("test version should parse")
    }

    /// `probation.json` round-trips with exact camelCase keys.
    #[test]
    fn probation_marker_roundtrip_exact_keys() {
        let marker = ProbationMarker {
            new: version("1.4.0"),
            old: version("1.3.5"),
            started_at: "2026-07-08T12:00:00Z".parse().unwrap(),
            attempt: 2,
        };
        let json = serde_json::to_value(&marker).unwrap();
        assert_eq!(json["new"], "1.4.0");
        assert_eq!(json["old"], "1.3.5");
        assert_eq!(json["startedAt"], "2026-07-08T12:00:00Z");
        assert_eq!(json["attempt"], 2);
        let back: ProbationMarker = serde_json::from_value(json).unwrap();
        assert_eq!(back, marker);
    }

}
