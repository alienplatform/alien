//! Release-manifest loading for operator binary self-updates.
//!
//! The release pipeline publishes, per version, a `manifest.json` next to the
//! binaries it uploads:
//!
//! ```json
//! {
//!   "version": "1.4.0",
//!   "minLauncherVersion": "0.1.0",
//!   "artifacts": {
//!     "linux/x86_64":  { "url": "https://…", "sha256": "…", "signature": "" },
//!     "darwin/aarch64": { "url": "https://…", "sha256": "…", "signature": "" }
//!   }
//! }
//! ```
//!
//! When an admin pins a target version for an os-service deployment, the sync
//! handler loads `<releases base>/<version>/manifest.json`, resolves the
//! artifact for the host's reported `(os, arch)`, and emits a per-host
//! `operator_target.binary`. A manifest that fails to load or is missing the
//! host's platform yields NO target — never a partial one.
//!
//! Manifests are immutable once published, so they are cached per URL for the
//! manager's lifetime. The base may be `http(s)://…`, `file://…`, or a plain
//! filesystem path (tests, air-gapped mirrors).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use alien_error::{AlienError, Context, IntoAlienError};
use serde::Deserialize;

use crate::error::{ErrorData, Result};

/// One downloadable binary in a release.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestArtifact {
    /// Download URL for this `(os, arch)`.
    pub url: String,
    /// SHA-256 of exactly that artifact, lowercase hex.
    pub sha256: String,
    /// ed25519 detached signature, base64. Empty until the signing
    /// workstream lands.
    #[serde(default)]
    pub signature: String,
}

/// The per-version release index the pipeline publishes.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseManifest {
    /// The release version this manifest describes.
    pub version: String,
    /// The installed (frozen) launcher must be >= this for the manager to
    /// advertise the target; otherwise "redeploy required".
    pub min_launcher_version: String,
    /// `"<os>/<arch>"` (e.g. `"linux/x86_64"`) → artifact.
    pub artifacts: BTreeMap<String, ManifestArtifact>,
}

/// Load (and cache) the manifest for `version` under `base`. `base` accepts
/// `http(s)://`, `file://`, or a plain filesystem directory path.
pub async fn load(base: &str, version: &str) -> Result<Arc<ReleaseManifest>> {
    let location = format!("{}/{}/manifest.json", base.trim_end_matches('/'), version);

    static CACHE: OnceLock<Mutex<HashMap<String, Arc<ReleaseManifest>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache
        .lock()
        .expect("manifest cache lock should not be poisoned")
        .get(&location)
    {
        return Ok(hit.clone());
    }

    let bytes = fetch(&location).await?;
    let manifest: ReleaseManifest = serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(ErrorData::BadRequest {
            reason: format!("release manifest at '{location}' is not valid manifest JSON"),
        })?;
    let manifest = Arc::new(manifest);

    cache
        .lock()
        .expect("manifest cache lock should not be poisoned")
        .insert(location, manifest.clone());
    Ok(manifest)
}

async fn fetch(location: &str) -> Result<Vec<u8>> {
    if let Some(path) = location.strip_prefix("file://") {
        return read_file(path).await;
    }
    if location.starts_with("http://") || location.starts_with("https://") {
        let response = reqwest::get(location)
            .await
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("failed to fetch release manifest from '{location}'"),
            })?;
        if !response.status().is_success() {
            return Err(AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "release manifest at '{location}' returned HTTP {}",
                    response.status()
                ),
            }));
        }
        return response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("failed to read release manifest body from '{location}'"),
            });
    }
    // Plain filesystem path (tests, air-gapped mirrors).
    read_file(location).await
}

async fn read_file(path: &str) -> Result<Vec<u8>> {
    tokio::fs::read(path)
        .await
        .into_alien_error()
        .context(ErrorData::BadRequest {
            reason: format!("failed to read release manifest file '{path}'"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &std::path::Path, version: &str, body: &str) {
        let version_dir = dir.join(version);
        std::fs::create_dir_all(&version_dir).unwrap();
        std::fs::write(version_dir.join("manifest.json"), body).unwrap();
    }

    const VALID: &str = r#"{
        "version": "1.4.0",
        "minLauncherVersion": "0.2.0",
        "artifacts": {
            "linux/x86_64": { "url": "https://example.com/op-linux", "sha256": "aa11" },
            "macos/aarch64": { "url": "https://example.com/op-mac", "sha256": "bb22", "signature": "sig" }
        }
    }"#;

    #[tokio::test]
    async fn loads_and_caches_from_a_plain_path() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(dir.path(), "1.4.0", VALID);

        let base = dir.path().to_str().unwrap().to_string();
        let manifest = load(&base, "1.4.0").await.expect("manifest should load");
        assert_eq!(manifest.version, "1.4.0");
        assert_eq!(manifest.min_launcher_version, "0.2.0");
        let artifact = &manifest.artifacts["linux/x86_64"];
        assert_eq!(artifact.url, "https://example.com/op-linux");
        assert_eq!(artifact.sha256, "aa11");
        assert_eq!(artifact.signature, "", "signature defaults to empty");
        assert_eq!(manifest.artifacts["macos/aarch64"].signature, "sig");

        // Cached: deleting the file no longer matters (manifests are immutable).
        std::fs::remove_file(dir.path().join("1.4.0/manifest.json")).unwrap();
        let again = load(&base, "1.4.0").await.expect("cache should serve");
        assert_eq!(again.version, "1.4.0");
    }

    #[tokio::test]
    async fn file_url_scheme_works() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(dir.path(), "2.0.0", &VALID.replace("1.4.0", "2.0.0"));
        let base = format!("file://{}", dir.path().to_str().unwrap());
        let manifest = load(&base, "2.0.0").await.expect("file:// should load");
        assert_eq!(manifest.version, "2.0.0");
    }

    #[tokio::test]
    async fn missing_manifest_is_a_loud_error() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let err = load(&base, "9.9.9").await.expect_err("missing must error");
        assert_eq!(err.code, "BAD_REQUEST");
    }

    #[tokio::test]
    async fn invalid_json_is_a_loud_error() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(dir.path(), "3.0.0", "{ not json");
        let base = dir.path().to_str().unwrap().to_string();
        let err = load(&base, "3.0.0").await.expect_err("garbage must error");
        assert_eq!(err.code, "BAD_REQUEST");
        assert!(err.to_string().contains("manifest"), "{err}");
    }
}
