//! os-service self-update actuator: the operator's half of the on-disk
//! handoff protocol with `alien-launcher` (`alien_core::self_update`).
//!
//! When `/v1/sync` returns `operator_target.binary`, this module downloads
//! the artifact, verifies its SHA-256 while streaming, stages it under
//! `versions/<v>/`, writes `pending.json`, and requests a graceful exit with
//! the update-handoff code (10). The LAUNCHER then performs the health-gated
//! swap; on a failed probation it rolls back and records the failure in
//! `failed/<version>.json`, which this module (a) translates into
//! `SyncRequest.operator_update` on every sync — the launcher has no network
//! path to the manager — and (b) uses for exponential backoff before
//! re-acting on the same artifact.
//!
//! Enabled only when spawned by the launcher (`ALIEN_SELF_UPDATE=1`);
//! Kubernetes detection wins even if that env leaks into a pod.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Once;

use alien_core::self_update::{
    backoff_delay, download_dir, failed_dir, failure_path, pending_path, read_json, version_dir,
    write_json_atomic, FailureRecord, PendingMarker, Version, ENV_LAUNCHER_VERSION,
    ENV_SELF_UPDATE, EXIT_CODE_UPDATE_HANDOFF,
};
use alien_core::sync::{OperatorBinaryTarget, OperatorUpdatePhase, OperatorUpdateReport};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

/// File name of the operator binary inside `versions/<v>/`.
const OPERATOR_BINARY: &str = "alien-operator";
/// Disk-preflight headroom over the artifact size (20%).
const PREFLIGHT_HEADROOM_DIVISOR: u64 = 5;

// ---------------------------------------------------------------------------
// Process-exit plumbing (exit code 10 = update handoff)
// ---------------------------------------------------------------------------

/// 0 = no special exit requested.
static REQUESTED_EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Ask the process to exit with the update-handoff code once the runtime
/// shuts down gracefully (the CLI checks this after `run` returns).
pub fn request_update_handoff_exit() {
    REQUESTED_EXIT_CODE.store(EXIT_CODE_UPDATE_HANDOFF, Ordering::SeqCst);
}

/// The exit code a graceful shutdown should use, if a handoff was staged.
pub fn requested_exit_code() -> Option<i32> {
    match REQUESTED_EXIT_CODE.load(Ordering::SeqCst) {
        0 => None,
        code => Some(code),
    }
}

// ---------------------------------------------------------------------------
// Environment gates
// ---------------------------------------------------------------------------

/// Is the os-service actuator enabled for this process?
pub fn actuator_enabled() -> bool {
    enabled_from(
        std::env::var_os("KUBERNETES_SERVICE_HOST").is_some(),
        std::env::var(ENV_SELF_UPDATE).ok().as_deref(),
    )
}

/// Pure decision core: Kubernetes detection wins; otherwise the launcher's
/// explicit opt-in (`ALIEN_SELF_UPDATE=1`) is required.
fn enabled_from(in_kubernetes: bool, self_update_env: Option<&str>) -> bool {
    !in_kubernetes && self_update_env == Some("1")
}

/// The supervising launcher's version (from `ALIEN_LAUNCHER_VERSION`, set on
/// spawn), reported on every sync for the `min_launcher_version` gate.
pub fn launcher_version() -> Option<String> {
    launcher_version_from(std::env::var(ENV_LAUNCHER_VERSION).ok())
}

fn launcher_version_from(env: Option<String>) -> Option<String> {
    env.filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// The actuator
// ---------------------------------------------------------------------------

/// Outcome of acting on an `operator_target.binary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryActuation {
    /// New binary staged + `pending.json` written — the caller must trigger a
    /// graceful shutdown; the process must exit with code 10.
    Staged,
    /// The target names the version we are already running.
    AlreadyCurrent,
    /// Our version is below the target's `min_supported_version` floor
    /// (stepping-stone upgrade required); recorded, not retried.
    RefusedFloor,
    /// A previous attempt for this exact artifact failed and its backoff
    /// window has not elapsed; nothing was downloaded this tick.
    SkippedBackoff,
    /// This attempt failed (download / digest / staging); a failure record
    /// was written and the next sync after backoff will retry.
    Failed,
}

/// Act on a binary target. `own_version` is injected (the caller passes
/// `CARGO_PKG_VERSION`) so the flow is testable with arbitrary versions.
pub async fn apply_binary_target(
    data_dir: &Path,
    own_version: &str,
    target_version: &str,
    min_supported_version: &str,
    binary: &OperatorBinaryTarget,
) -> Result<BinaryActuation> {
    log_signature_posture();

    if target_version == own_version {
        return Ok(BinaryActuation::AlreadyCurrent);
    }
    let target = Version::parse(target_version).map_err(|e| {
        AlienError::new(ErrorData::SelfUpdateFailed {
            message: format!("target version '{target_version}' is not valid semver: {e}"),
        })
    })?;

    // Floor: refuse a target our version is too old to jump to. Recorded
    // once per artifact (idempotent — no attempt-count churn while floored).
    if below_floor(own_version, min_supported_version) {
        let message = format!(
            "operator {own_version} is below the min_supported_version floor \
             {min_supported_version} for target {target_version}; a stepping-stone \
             upgrade is required"
        );
        warn!(%message, "refusing operator_target");
        ensure_floor_record(data_dir, &target, &binary.sha256, &message)?;
        return Ok(BinaryActuation::RefusedFloor);
    }

    // Backoff: an identical artifact that failed recently is not retried
    // until its exponential window elapses.
    if let Some(record) = read_failure(data_dir, &target)? {
        if record.sha256 == binary.sha256 {
            let retry_at = record.last_failed_at
                + chrono::Duration::from_std(backoff_delay(record.attempts))
                    .unwrap_or(chrono::Duration::zero());
            if Utc::now() < retry_at {
                return Ok(BinaryActuation::SkippedBackoff);
            }
        }
    }

    match download_verify_stage(data_dir, &target, binary).await {
        Ok(()) => {
            info!(version = %target, "staged new operator binary; requesting update handoff");
            Ok(BinaryActuation::Staged)
        }
        Err(StageFailure { message }) => {
            warn!(%message, version = %target, "self-update staging attempt failed");
            record_attempt_failure(data_dir, &target, &binary.sha256, &message)?;
            Ok(BinaryActuation::Failed)
        }
    }
}

/// `own < floor`, with unparseable inputs treated as "no floor" (the manager
/// validates these fields; blocking all updates on a malformed optional floor
/// would be worse than skipping the check — logged for diagnosis).
fn below_floor(own_version: &str, min_supported_version: &str) -> bool {
    match (
        Version::parse(own_version),
        Version::parse(min_supported_version),
    ) {
        (Ok(own), Ok(floor)) => own < floor,
        (own, floor) => {
            warn!(
                own_parse_ok = own.is_ok(),
                floor_parse_ok = floor.is_ok(),
                "unparseable version in floor check; skipping the floor gate"
            );
            false
        }
    }
}

/// A staging failure that should be recorded + retried after backoff (all of
/// these are `Spawn`-phase in the wire vocabulary: the swap never started).
struct StageFailure {
    message: String,
}

async fn download_verify_stage(
    data_dir: &Path,
    target: &Version,
    binary: &OperatorBinaryTarget,
) -> std::result::Result<(), StageFailure> {
    let fail = |message: String| StageFailure { message };

    let response = reqwest::Client::new()
        .get(&binary.url)
        .send()
        .await
        .map_err(|e| fail(format!("download request failed: {e}")))?;
    if !response.status().is_success() {
        return Err(fail(format!(
            "artifact download returned HTTP {}",
            response.status()
        )));
    }

    // Disk preflight: artifact size + 20% headroom must fit the free space.
    if let Some(length) = response.content_length() {
        let required = length.saturating_add(length / PREFLIGHT_HEADROOM_DIVISOR);
        let available = fs2::available_space(data_dir)
            .map_err(|e| fail(format!("disk-space preflight failed: {e}")))?;
        if available < required {
            return Err(fail(format!(
                "not enough free disk space for the artifact: need {required} bytes \
                 (incl. headroom), {available} available"
            )));
        }
    }

    // Stream to download/<v>.partial, hashing as we go.
    let staging_dir = download_dir(data_dir);
    tokio::fs::create_dir_all(&staging_dir)
        .await
        .map_err(|e| fail(format!("failed to create download dir: {e}")))?;
    let partial = staging_dir.join(format!("{target}.partial"));

    let stream_result = stream_to_file(response, &partial).await;
    let sha256 = match stream_result {
        Ok(sha256) => sha256,
        Err(message) => {
            remove_best_effort(&partial).await;
            return Err(fail(message));
        }
    };
    if sha256 != binary.sha256 {
        remove_best_effort(&partial).await;
        return Err(fail(format!(
            "artifact digest mismatch: manifest says {}, downloaded bytes hash to {sha256}",
            binary.sha256
        )));
    }

    #[cfg(feature = "enforce-signature")]
    if let Err(message) = signature::verify_file(&partial, &binary.signature).await {
        remove_best_effort(&partial).await;
        return Err(fail(format!("signature verification failed: {message}")));
    }

    // Promote the verified download into versions/<v>/.
    let dest_dir = version_dir(data_dir, target);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| fail(format!("failed to create version dir: {e}")))?;
    let dest = dest_dir.join(OPERATOR_BINARY);
    tokio::fs::rename(&partial, &dest)
        .await
        .map_err(|e| fail(format!("failed to move staged binary into place: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .await
            .map_err(|e| fail(format!("failed to mark staged binary executable: {e}")))?;
    }

    // The handoff marker — the launcher validates this against the staged
    // bytes before swapping.
    let marker = PendingMarker {
        version: target.clone(),
        sha256,
        staged_at: Utc::now(),
    };
    write_json_atomic(&pending_path(data_dir), &marker)
        .map_err(|e| fail(format!("failed to write pending.json: {e}")))?;
    Ok(())
}

/// Stream the response body to `path`, returning the SHA-256 (lowercase hex)
/// of the bytes written.
async fn stream_to_file(
    mut response: reqwest::Response,
    path: &Path,
) -> std::result::Result<String, String> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| format!("failed to create '{}': {e}", path.display()))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download stream failed: {e}"))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("failed to write '{}': {e}", path.display()))?;
    }
    file.sync_all()
        .await
        .map_err(|e| format!("failed to sync '{}': {e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

async fn remove_best_effort(path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(path = %path.display(), error = %e, "failed to remove partial download");
        }
    }
}

// ---------------------------------------------------------------------------
// Failure records (shared shapes; the report handoff)
// ---------------------------------------------------------------------------

fn read_failure(data_dir: &Path, version: &Version) -> Result<Option<FailureRecord>> {
    read_json(&failure_path(data_dir, version))
        .into_alien_error()
        .context(ErrorData::SelfUpdateFailed {
            message: format!("failed to read failure record for {version}"),
        })
}

/// Record a failed staging attempt: same artifact increments the count (the
/// backoff input), a different artifact starts fresh at 1.
fn record_attempt_failure(
    data_dir: &Path,
    version: &Version,
    sha256: &str,
    message: &str,
) -> Result<()> {
    let attempts = match read_failure(data_dir, version)? {
        Some(prior) if prior.sha256 == sha256 => prior.attempts + 1,
        _ => 1,
    };
    write_failure(
        data_dir,
        &FailureRecord {
            version: version.clone(),
            sha256: sha256.to_string(),
            phase: OperatorUpdatePhase::Spawn,
            message: message.to_string(),
            attempts,
            last_failed_at: Utc::now(),
        },
    )
}

/// Floor refusals are a persistent condition, not an attempt: write the
/// record once per artifact and leave it untouched afterwards (no
/// attempt-count churn, stable backoff clock).
fn ensure_floor_record(
    data_dir: &Path,
    version: &Version,
    sha256: &str,
    message: &str,
) -> Result<()> {
    if let Some(existing) = read_failure(data_dir, version)? {
        if existing.sha256 == sha256 {
            return Ok(());
        }
    }
    write_failure(
        data_dir,
        &FailureRecord {
            version: version.clone(),
            sha256: sha256.to_string(),
            phase: OperatorUpdatePhase::Spawn,
            message: message.to_string(),
            attempts: 1,
            last_failed_at: Utc::now(),
        },
    )
}

fn write_failure(data_dir: &Path, record: &FailureRecord) -> Result<()> {
    std::fs::create_dir_all(failed_dir(data_dir))
        .into_alien_error()
        .context(ErrorData::SelfUpdateFailed {
            message: "failed to create the failed/ records dir".to_string(),
        })?;
    write_json_atomic(&failure_path(data_dir, &record.version), record)
        .into_alien_error()
        .context(ErrorData::SelfUpdateFailed {
            message: format!("failed to write failure record for {}", record.version),
        })
}

// ---------------------------------------------------------------------------
// Report translation (SyncRequest.operator_update for os-service)
// ---------------------------------------------------------------------------

/// Derive `operator_update` from the on-disk markers: a staged `pending.json`
/// is an in-progress update; otherwise the newest failure record (written by
/// the launcher on rollback, or by us on a staging failure) is reported until
/// convergence or a new target supersedes it.
pub fn marker_update_report(data_dir: &Path) -> Option<OperatorUpdateReport> {
    if let Ok(Some(pending)) = read_json::<PendingMarker>(&pending_path(data_dir)) {
        let attempt = read_json::<FailureRecord>(&failure_path(data_dir, &pending.version))
            .ok()
            .flatten()
            .filter(|record| record.sha256 == pending.sha256)
            .map(|record| record.attempts + 1)
            .unwrap_or(1);
        return Some(OperatorUpdateReport::InProgress {
            target_version: pending.version.as_str(),
            attempt,
        });
    }

    let newest = newest_failure_record(data_dir)?;
    Some(OperatorUpdateReport::Failed {
        target_version: newest.version.as_str(),
        phase: newest.phase,
        message: newest.message,
        attempt: newest.attempts,
    })
}

fn newest_failure_record(data_dir: &Path) -> Option<FailureRecord> {
    let entries = std::fs::read_dir(failed_dir(data_dir)).ok()?;
    let mut newest: Option<FailureRecord> = None;
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        match read_json::<FailureRecord>(&path) {
            Ok(Some(record)) => {
                if newest
                    .as_ref()
                    .is_none_or(|best| record.last_failed_at > best.last_failed_at)
                {
                    newest = Some(record);
                }
            }
            Ok(None) => {}
            Err(e) => warn!(path = %path.display(), error = %e, "unreadable failure record"),
        }
    }
    newest
}

// ---------------------------------------------------------------------------
// Signature verification (feature-gated until the signing workstream lands)
// ---------------------------------------------------------------------------

/// Log the signature posture once, loudly, so a disabled verifier can never
/// be mistaken for an enforced one.
fn log_signature_posture() {
    static LOGGED: Once = Once::new();
    LOGGED.call_once(|| {
        #[cfg(feature = "enforce-signature")]
        info!("self-update artifact signature verification is ENFORCED (ed25519)");
        #[cfg(not(feature = "enforce-signature"))]
        warn!(
            "self-update artifact signature verification is DISABLED — trusting \
             SHA-256 + HTTPS only (enable the `enforce-signature` feature once \
             the signing workstream ships)"
        );
    });
}

#[cfg(feature = "enforce-signature")]
mod signature {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use std::path::Path;

    /// PLACEHOLDER — replaced when the signing workstream lands (release
    /// pipeline signing, key rotation policy). All-zeros is not a valid
    /// ed25519 point, so an enforcement-on build refuses every artifact
    /// until the real key ships — fail closed, never silently open.
    pub const PINNED_PUBKEY: [u8; 32] = [0u8; 32];

    pub async fn verify_file(path: &Path, signature_b64: &str) -> Result<(), String> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| format!("failed to read staged artifact: {e}"))?;
        verify(&bytes, signature_b64, &PINNED_PUBKEY)
    }

    pub fn verify(bytes: &[u8], signature_b64: &str, pubkey: &[u8; 32]) -> Result<(), String> {
        let key = VerifyingKey::from_bytes(pubkey)
            .map_err(|e| format!("pinned public key is invalid: {e}"))?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(signature_b64)
            .map_err(|e| format!("signature is not valid base64: {e}"))?;
        let signature = Signature::from_slice(&raw)
            .map_err(|e| format!("signature has the wrong shape: {e}"))?;
        key.verify(bytes, &signature)
            .map_err(|e| format!("ed25519 verification failed: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn version(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// Serve `bytes` at a local URL, counting hits.
    async fn artifact_server(bytes: Vec<u8>) -> (String, Arc<AtomicU32>) {
        let hits = Arc::new(AtomicU32::new(0));
        let app = Router::new().route(
            "/artifact",
            get({
                let hits = hits.clone();
                move || {
                    let hits = hits.clone();
                    let bytes = bytes.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        bytes
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/artifact"), hits)
    }

    fn binary_target(url: &str, sha256: &str) -> alien_core::sync::OperatorBinaryTarget {
        alien_core::sync::OperatorBinaryTarget {
            url: url.to_string(),
            sha256: sha256.to_string(),
            signature: String::new(),
            min_launcher_version: "0.1.0".to_string(),
        }
    }

    /// Staging succeeds end-to-end. Default build only: with
    /// `enforce-signature` the placeholder pinned key fails closed and no
    /// artifact can stage (see `enforcement_fails_closed_with_placeholder_key`).
    #[cfg(not(feature = "enforce-signature"))]
    #[tokio::test]
    async fn happy_staging_writes_pending_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = b"the-new-operator-binary".to_vec();
        let sha = sha256_hex(&artifact);
        let (url, hits) = artifact_server(artifact.clone()).await;

        let outcome = apply_binary_target(
            dir.path(),
            "1.0.0",
            "9.9.9",
            "1.0.0",
            &binary_target(&url, &sha),
        )
        .await
        .expect("staging should succeed");

        assert_eq!(outcome, BinaryActuation::Staged);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let staged = version_dir(dir.path(), &version("9.9.9")).join(OPERATOR_BINARY);
        assert_eq!(std::fs::read(&staged).unwrap(), artifact);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&staged).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "staged binary must be executable");
        }

        let pending: PendingMarker = read_json(&pending_path(dir.path())).unwrap().unwrap();
        assert_eq!(pending.version, version("9.9.9"));
        assert_eq!(pending.sha256, sha);
        // No partial left behind.
        assert!(!download_dir(dir.path()).join("9.9.9.partial").exists());
    }

    #[tokio::test]
    async fn sha_mismatch_records_spawn_failure_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let (url, _hits) = artifact_server(b"actual-bytes".to_vec()).await;
        let claimed_sha = sha256_hex(b"different-bytes");

        let outcome = apply_binary_target(
            dir.path(),
            "1.0.0",
            "9.9.9",
            "1.0.0",
            &binary_target(&url, &claimed_sha),
        )
        .await
        .expect("mismatch is a recorded failure, not an Err");

        assert_eq!(outcome, BinaryActuation::Failed);
        assert!(
            !version_dir(dir.path(), &version("9.9.9"))
                .join(OPERATOR_BINARY)
                .exists(),
            "nothing staged"
        );
        assert!(read_json::<PendingMarker>(&pending_path(dir.path())).unwrap().is_none());
        assert!(
            !download_dir(dir.path()).join("9.9.9.partial").exists(),
            "partial removed"
        );

        let record: FailureRecord = read_json(&failure_path(dir.path(), &version("9.9.9")))
            .unwrap()
            .expect("failure recorded");
        assert_eq!(record.phase, OperatorUpdatePhase::Spawn);
        assert_eq!(record.attempts, 1);
        assert_eq!(record.sha256, claimed_sha);
        assert!(record.message.contains("digest mismatch"), "{}", record.message);
    }

    #[tokio::test]
    async fn backoff_skips_matching_artifact_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = b"artifact".to_vec();
        let sha = sha256_hex(&artifact);
        let (url, hits) = artifact_server(artifact).await;

        // A fresh failure for the SAME artifact: attempts=3 → 2m window.
        write_failure(
            dir.path(),
            &FailureRecord {
                version: version("9.9.9"),
                sha256: sha.clone(),
                phase: OperatorUpdatePhase::Apply,
                message: "rolled back".to_string(),
                attempts: 3,
                last_failed_at: Utc::now(),
            },
        )
        .unwrap();

        let outcome = apply_binary_target(
            dir.path(),
            "1.0.0",
            "9.9.9",
            "1.0.0",
            &binary_target(&url, &sha),
        )
        .await
        .unwrap();

        assert_eq!(outcome, BinaryActuation::SkippedBackoff);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "no download during backoff");
    }

    #[cfg(not(feature = "enforce-signature"))]
    #[tokio::test]
    async fn new_artifact_ignores_old_failure_record() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = b"fixed-artifact".to_vec();
        let sha = sha256_hex(&artifact);
        let (url, hits) = artifact_server(artifact).await;

        // A failure for the same VERSION but a different sha (old broken build).
        write_failure(
            dir.path(),
            &FailureRecord {
                version: version("9.9.9"),
                sha256: sha256_hex(b"old-broken-artifact"),
                phase: OperatorUpdatePhase::Apply,
                message: "rolled back".to_string(),
                attempts: 5,
                last_failed_at: Utc::now(),
            },
        )
        .unwrap();

        let outcome = apply_binary_target(
            dir.path(),
            "1.0.0",
            "9.9.9",
            "1.0.0",
            &binary_target(&url, &sha),
        )
        .await
        .unwrap();

        assert_eq!(outcome, BinaryActuation::Staged, "new artifact = fresh start");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn floor_refusal_records_once_without_churn() {
        let dir = tempfile::tempdir().unwrap();
        let target = binary_target("http://127.0.0.1:9/unreachable", &"ab".repeat(32));

        let outcome =
            apply_binary_target(dir.path(), "1.0.0", "9.9.9", "5.0.0", &target)
                .await
                .unwrap();
        assert_eq!(outcome, BinaryActuation::RefusedFloor);

        let record: FailureRecord = read_json(&failure_path(dir.path(), &version("9.9.9")))
            .unwrap()
            .expect("floor refusal recorded");
        assert_eq!(record.attempts, 1);
        assert!(record.message.contains("min_supported_version"), "{}", record.message);
        let first_time = record.last_failed_at;

        // Second sync tick: still floored — record untouched (no churn).
        let outcome =
            apply_binary_target(dir.path(), "1.0.0", "9.9.9", "5.0.0", &target)
                .await
                .unwrap();
        assert_eq!(outcome, BinaryActuation::RefusedFloor);
        let record: FailureRecord = read_json(&failure_path(dir.path(), &version("9.9.9")))
            .unwrap()
            .unwrap();
        assert_eq!(record.attempts, 1);
        assert_eq!(record.last_failed_at, first_time, "no rewrite while floored");
    }

    #[tokio::test]
    async fn already_current_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let target = binary_target("http://127.0.0.1:9/unreachable", &"ab".repeat(32));
        let outcome = apply_binary_target(dir.path(), "1.4.0", "1.4.0", "1.0.0", &target)
            .await
            .unwrap();
        assert_eq!(outcome, BinaryActuation::AlreadyCurrent);
        assert!(read_json::<PendingMarker>(&pending_path(dir.path())).unwrap().is_none());
    }

    #[test]
    fn marker_report_translates_failure_then_pending() {
        let dir = tempfile::tempdir().unwrap();
        assert!(marker_update_report(dir.path()).is_none(), "clean store = no report");

        write_failure(
            dir.path(),
            &FailureRecord {
                version: version("1.4.0"),
                sha256: "aa".repeat(32),
                phase: OperatorUpdatePhase::Apply,
                message: "probation failed".to_string(),
                attempts: 2,
                last_failed_at: Utc::now(),
            },
        )
        .unwrap();

        let report = marker_update_report(dir.path()).expect("failure translates");
        assert_eq!(
            report,
            OperatorUpdateReport::Failed {
                target_version: "1.4.0".to_string(),
                phase: OperatorUpdatePhase::Apply,
                message: "probation failed".to_string(),
                attempt: 2,
            }
        );

        // A staged pending for the same artifact wins: InProgress, attempt 3.
        write_json_atomic(
            &pending_path(dir.path()),
            &PendingMarker {
                version: version("1.4.0"),
                sha256: "aa".repeat(32),
                staged_at: Utc::now(),
            },
        )
        .unwrap();
        let report = marker_update_report(dir.path()).unwrap();
        assert_eq!(
            report,
            OperatorUpdateReport::InProgress {
                target_version: "1.4.0".to_string(),
                attempt: 3,
            }
        );
    }

    #[test]
    fn env_gates_pure_logic() {
        // Kubernetes detection wins even with the opt-in present.
        assert!(!enabled_from(true, Some("1")));
        assert!(enabled_from(false, Some("1")));
        assert!(!enabled_from(false, Some("0")));
        assert!(!enabled_from(false, None));

        assert_eq!(
            launcher_version_from(Some("1.2.3".to_string())),
            Some("1.2.3".to_string())
        );
        assert_eq!(launcher_version_from(Some(String::new())), None);
        assert_eq!(launcher_version_from(None), None);
    }

    #[test]
    fn exit_code_plumbing() {
        // Note: process-global — this is the only test touching it.
        assert_eq!(requested_exit_code(), None);
        request_update_handoff_exit();
        assert_eq!(requested_exit_code(), Some(EXIT_CODE_UPDATE_HANDOFF));
    }

    /// With enforcement on and only the placeholder key available, staging
    /// FAILS CLOSED: the artifact is refused, recorded, and never staged.
    #[cfg(feature = "enforce-signature")]
    #[tokio::test]
    async fn enforcement_fails_closed_with_placeholder_key() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = b"the-new-operator-binary".to_vec();
        let sha = sha256_hex(&artifact);
        let (url, _hits) = artifact_server(artifact).await;

        let outcome = apply_binary_target(
            dir.path(),
            "1.0.0",
            "9.9.9",
            "1.0.0",
            &binary_target(&url, &sha),
        )
        .await
        .unwrap();

        assert_eq!(outcome, BinaryActuation::Failed, "placeholder key refuses");
        assert!(read_json::<PendingMarker>(&pending_path(dir.path())).unwrap().is_none());
        let record: FailureRecord = read_json(&failure_path(dir.path(), &version("9.9.9")))
            .unwrap()
            .expect("refusal recorded");
        assert!(record.message.contains("signature"), "{}", record.message);
    }

    /// The enforcing path works end-to-end with a real (deterministic)
    /// keypair, and rejects tampering. Compiled only with the feature.
    #[cfg(feature = "enforce-signature")]
    #[test]
    fn signature_verify_roundtrip_and_tamper() {
        use base64::Engine;
        use ed25519_dalek::Signer;

        let signing = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let verifying = signing.verifying_key().to_bytes();
        let artifact = b"artifact-bytes";
        let sig = signing.sign(artifact);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());

        signature::verify(artifact, &sig_b64, &verifying).expect("valid signature verifies");
        signature::verify(b"tampered-bytes", &sig_b64, &verifying)
            .expect_err("tampered bytes must fail");
        signature::verify(artifact, "not-base64!!!", &verifying)
            .expect_err("garbage signature must fail");
        // The placeholder all-zeros key fails closed.
        signature::verify(artifact, &sig_b64, &signature::PINNED_PUBKEY)
            .expect_err("placeholder key must refuse everything");
    }
}
