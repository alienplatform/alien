//! The on-disk handoff protocol between `alien-launcher` and `alien-operator`
//! for the os-service self-update flow.
//!
//! The two binaries coordinate exclusively through the operator's data dir
//! and an exit code: the operator downloads + verifies + stages a new binary,
//! writes `pending.json`, and exits with [`EXIT_CODE_UPDATE_HANDOFF`]; the
//! launcher validates the stage, performs the health-gated swap, and — on a
//! failed probation — rolls back and records the failure in
//! `failed/<version>.json`, which the operator translates into
//! `SyncRequest.operator_update` on its next sync (the launcher has no
//! network path to the manager).
//!
//! These types ARE the protocol: both sides must serialize identically, so
//! they live here in the shared crate rather than in either binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::sync::OperatorUpdatePhase;

// ---------------------------------------------------------------------------
// Environment + exit-code contract
// ---------------------------------------------------------------------------

/// Set to `1` by the launcher on spawn; enables the operator's os-service
/// self-update actuator. Kubernetes detection (`KUBERNETES_SERVICE_HOST`)
/// takes precedence even if this is somehow present.
pub const ENV_SELF_UPDATE: &str = "ALIEN_SELF_UPDATE";
/// The launcher's version, set by the launcher on spawn and reported by the
/// operator on sync (the `min_launcher_version` gate input). The launcher is
/// frozen — reported, never driven.
pub const ENV_LAUNCHER_VERSION: &str = "ALIEN_LAUNCHER_VERSION";
/// The `127.0.0.1:<port>` address the operator must bind its `/readyz` +
/// `/livez` endpoints to; the launcher probes it during probation.
pub const ENV_HEALTH_ADDR: &str = "ALIEN_HEALTH_ADDR";

/// Exit code by which the operator requests an update handoff: it has staged
/// a new version and written `pending.json`; the launcher validates and
/// swaps. `0` = clean stop; anything else = crash (launcher respawns with
/// backoff).
pub const EXIT_CODE_UPDATE_HANDOFF: i32 = 10;

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// An operator version — a validated SemVer value.
///
/// Newtype over `semver::Version` so every comparison in the update flow uses
/// spec-correct SemVer *precedence* (`1.10.0 > 1.9.0`, `1.4.0-rc.1 < 1.4.0`,
/// prerelease segments compared numerically-then-lexically) instead of string
/// ordering. Serializes as the plain version string, which is also the
/// on-disk directory name under `versions/`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(semver::Version);

impl Version {
    /// Parse a SemVer string (e.g. `"1.4.0"`, `"1.5.0-rc.1"`).
    pub fn parse(s: &str) -> Result<Self, semver::Error> {
        semver::Version::parse(s).map(Self)
    }

    /// The canonical string form — used on the wire and as the `versions/`
    /// directory name.
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl FromStr for Version {
    type Err = semver::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ---------------------------------------------------------------------------
// Marker files
// ---------------------------------------------------------------------------

/// `pending.json` — written by the OPERATOR after staging a new binary,
/// immediately before exiting with [`EXIT_CODE_UPDATE_HANDOFF`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMarker {
    /// The staged version (present under `versions/<version>/`).
    pub version: Version,
    /// SHA-256 (lowercase hex) of the staged binary; the launcher re-hashes
    /// and validates before swapping.
    pub sha256: String,
    /// When staging completed.
    pub staged_at: DateTime<Utc>,
}

/// `failed/<version>.json` — written by the LAUNCHER on a health-gate
/// rollback (or a pre-swap failure). Doubles as the report handoff: the
/// OPERATOR translates the newest record into `SyncRequest.operator_update`
/// on every sync, and applies exponential backoff before re-acting on a
/// matching target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureRecord {
    /// The version whose update failed.
    pub version: Version,
    /// SHA-256 of the artifact that failed — a target with a different digest
    /// ignores this record (new artifact = fresh start).
    pub sha256: String,
    /// Which stage failed, in the sync-wire vocabulary.
    pub phase: OperatorUpdatePhase,
    /// Human-readable detail (probe timeout, crash exit status, …).
    pub message: String,
    /// How many attempts for this version have failed so far (1-based).
    pub attempts: u32,
    /// Completion time of the most recent failed attempt — the backoff clock.
    pub last_failed_at: DateTime<Utc>,
}

/// Exponential backoff between failed update attempts, keyed on the record's
/// `attempts`: `30 s · 2^(attempts−1)`, capped at 5 min. Mirrors the
/// Kubernetes actuator's Job backoff so both packagings converge at the same
/// pace.
pub fn backoff_delay(attempts: u32) -> Duration {
    const BASE_SECS: u64 = 30;
    const CAP_SECS: u64 = 300;
    let factor = 2u64.saturating_pow(attempts.saturating_sub(1));
    Duration::from_secs(BASE_SECS.saturating_mul(factor).min(CAP_SECS))
}

// ---------------------------------------------------------------------------
// Atomic marker I/O (normative: write temp → fsync → rename)
// ---------------------------------------------------------------------------

/// Atomically write a JSON marker: serialize → write `<path>.tmp` → fsync →
/// rename over `path`. The rename is the commit point; a crash before it
/// leaves only a `.tmp` file, which readers ignore — the protocol then sees
/// the marker as absent, which is always an older, classifiable state.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;

    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(&tmp, path)
}

/// Read a JSON marker. Absent file → `Ok(None)`. A file that exists but does
/// not parse is genuine corruption (atomic writes rule out torn markers) and
/// surfaces as `InvalidData` — never silently treated as absent.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> std::io::Result<Option<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Remove a marker. Idempotent — an absent marker is success.
pub fn remove_json(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .expect("marker paths always have a file name")
        .to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

// ---------------------------------------------------------------------------
// Store paths (shared layout knowledge)
// ---------------------------------------------------------------------------

/// `pending.json` inside a data dir.
pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("pending.json")
}

/// `failed/<version>.json` inside a data dir.
pub fn failure_path(data_dir: &Path, version: &Version) -> PathBuf {
    data_dir.join("failed").join(format!("{version}.json"))
}

/// `failed/` directory inside a data dir.
pub fn failed_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("failed")
}

/// `versions/<version>/` inside a data dir.
pub fn version_dir(data_dir: &Path, version: &Version) -> PathBuf {
    data_dir.join("versions").join(version.as_str())
}

/// `download/` staging area inside a data dir.
pub fn download_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("download")
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

    /// `Version` must order by SemVer precedence, not string order.
    #[test]
    fn version_orders_by_semver_precedence() {
        assert!(version("1.10.0") > version("1.9.0"), "numeric, not lexical");
        assert!(version("1.4.0-rc.1") < version("1.4.0"), "prerelease < release");
        assert!(version("1.4.0-rc.2") < version("1.4.0-rc.10"), "numeric prerelease segments");
        assert_eq!(version("1.4.0"), version("1.4.0"));
        assert!(Version::parse("not-a-version").is_err());
        assert!(Version::parse("01.2.3").is_err(), "leading zeros rejected");
    }

    /// `Version` serializes as the plain string (wire form + dir name).
    #[test]
    fn version_serializes_transparent() {
        let v = version("1.5.0-rc.1");
        assert_eq!(serde_json::to_value(&v).unwrap(), "1.5.0-rc.1");
        let back: Version = serde_json::from_value(serde_json::json!("1.5.0-rc.1")).unwrap();
        assert_eq!(back, v);
        assert_eq!(v.to_string(), "1.5.0-rc.1");
    }

    /// `pending.json` round-trips with exact camelCase keys.
    #[test]
    fn pending_marker_roundtrip_exact_keys() {
        let marker = PendingMarker {
            version: version("1.4.0"),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            staged_at: "2026-07-08T12:00:00Z".parse().unwrap(),
        };
        let json = serde_json::to_value(&marker).unwrap();
        assert_eq!(json["version"], "1.4.0");
        assert_eq!(json["stagedAt"], "2026-07-08T12:00:00Z");
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["sha256", "stagedAt", "version"], "exact key set");
        let back: PendingMarker = serde_json::from_value(json).unwrap();
        assert_eq!(back, marker);
    }

    /// `failed/<version>.json` round-trips with exact camelCase keys and the
    /// sync-wire kebab-case phase, so the operator can translate the record
    /// into `SyncRequest.operator_update` without re-mapping.
    #[test]
    fn failure_record_roundtrip_exact_keys() {
        let record = FailureRecord {
            version: version("1.4.0"),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            phase: OperatorUpdatePhase::Apply,
            message: "readyz never returned 200 within the probation window".to_string(),
            attempts: 3,
            last_failed_at: "2026-07-08T12:05:00Z".parse().unwrap(),
        };
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["version"], "1.4.0");
        assert_eq!(json["phase"], "apply", "kebab-case wire vocabulary");
        assert_eq!(json["attempts"], 3);
        assert_eq!(json["lastFailedAt"], "2026-07-08T12:05:00Z");
        let back: FailureRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, record);
    }

    /// Backoff: 30s · 2^(n−1), capped at 5 min, mirroring the K8s actuator.
    #[test]
    fn backoff_delay_doubles_and_caps() {
        assert_eq!(backoff_delay(1), Duration::from_secs(30));
        assert_eq!(backoff_delay(2), Duration::from_secs(60));
        assert_eq!(backoff_delay(3), Duration::from_secs(120));
        assert_eq!(backoff_delay(4), Duration::from_secs(240));
        assert_eq!(backoff_delay(5), Duration::from_secs(300), "capped");
        assert_eq!(backoff_delay(50), Duration::from_secs(300), "no overflow");
        assert_eq!(backoff_delay(0), Duration::from_secs(30), "0 treated as first");
    }

    /// Atomic write leaves no temp residue; absent reads None; corruption is
    /// loud; removal is idempotent.
    #[test]
    fn marker_io_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.json");

        let marker = PendingMarker {
            version: version("1.4.0"),
            sha256: "aa".repeat(32),
            staged_at: Utc::now(),
        };
        write_json_atomic(&path, &marker).unwrap();
        let back: Option<PendingMarker> = read_json(&path).unwrap();
        assert_eq!(back, Some(marker));
        assert!(
            !dir.path().join("pending.json.tmp").exists(),
            "no temp residue after commit"
        );

        std::fs::write(&path, b"{ not json").unwrap();
        let err = read_json::<PendingMarker>(&path).expect_err("corrupt marker must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

        remove_json(&path).unwrap();
        remove_json(&path).expect("removing an absent marker is Ok");
        assert_eq!(read_json::<PendingMarker>(&path).unwrap(), None);
    }

    /// The shared path layout matches the version-store layout.
    #[test]
    fn store_paths_layout() {
        let data = Path::new("/var/lib/alien-operator");
        assert_eq!(pending_path(data), data.join("pending.json"));
        assert_eq!(
            failure_path(data, &version("1.4.0")),
            data.join("failed/1.4.0.json")
        );
        assert_eq!(version_dir(data, &version("1.4.0")), data.join("versions/1.4.0"));
        assert_eq!(download_dir(data), data.join("download"));
    }
}
