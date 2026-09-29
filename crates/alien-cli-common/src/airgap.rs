//! Air-gapped bundles: a deployment target and everything it needs, as one
//! file an operator carries into an environment with no network path to the
//! manager.
//!
//! A bundle is an uncompressed tar (image layers are already compressed):
//!
//! ```text
//! manifest.json      what the bundle is, and the sha256 of every other file
//! target.json        the target sync would deliver (release stack + config)
//! chart/<name>.tgz   the Helm chart that installs the Operator
//! oci/               OCI image layout: every image in the target + the Operator
//! ```
//!
//! `alien airgap bundle` writes it; `alien-deploy airgap apply` verifies it,
//! pushes the images to the environment's registry by digest and points the
//! target at them.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use alien_error::{AlienError, AlienErrorData, Context, IntoAlienError};
use oci_client::{
    client::{Client as OciClient, ClientConfig, ClientProtocol},
    manifest::{
        IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
        OCI_IMAGE_MEDIA_TYPE,
    },
    secrets::RegistryAuth,
    Reference,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Errors while writing or applying a bundle.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The bundle file is unreadable, incomplete, or fails its checksums.
    #[error(
        code = "AIRGAP_BUNDLE_INVALID",
        message = "Bundle is not valid: {message}",
        retryable = "false",
        internal = "false"
    )]
    BundleInvalid { message: String },

    /// Copying an image to or from a registry failed.
    #[error(
        code = "AIRGAP_IMAGE_COPY_FAILED",
        message = "Copying image '{image}' failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    ImageCopyFailed { image: String, message: String },

    /// Reading or writing bundle files failed.
    #[error(
        code = "AIRGAP_FILE_FAILED",
        message = "Bundle file operation failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    FileFailed { message: String },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;

/// Current bundle format.
pub const FORMAT_VERSION: u32 = 1;

pub const MANIFEST_FILE: &str = "manifest.json";
/// The manager's signature over `manifest.json`.
pub const SIGNATURE_FILE: &str = "manifest.sig";
pub const TARGET_FILE: &str = "target.json";
pub const OCI_DIR: &str = "oci";
pub const CHART_DIR: &str = "chart";

const MANIFEST_MEDIA_TYPES: [&str; 4] = [
    OCI_IMAGE_INDEX_MEDIA_TYPE,
    IMAGE_MANIFEST_LIST_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE,
    IMAGE_MANIFEST_MEDIA_TYPE,
];

/// `manifest.json`: what the bundle carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub format_version: u32,
    pub deployment_id: String,
    pub deployment_name: String,
    pub release_id: String,
    /// Higher bundles replace lower ones; the Operator ignores older ones.
    pub sequence: u64,
    pub created_at: String,
    /// Chart release name and namespace default.
    pub stack_id: String,
    pub images: Vec<BundleImage>,
    /// Image the Operator runs (one of `images`).
    pub operator_image: String,
    /// Chart archive path inside the bundle.
    pub chart: String,
    /// sha256 of every other file in the bundle, by path.
    pub files: BTreeMap<String, String>,
}

/// One image in the bundle's OCI layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleImage {
    /// Reference as the target (or chart) names it.
    pub source: String,
    /// Repository path without the registry host, e.g. `artifacts/default`.
    pub repository: String,
    /// Tag of `source`, when it has one.
    pub tag: Option<String>,
    /// Digest of the top-level manifest.
    pub digest: String,
    pub media_type: String,
}

/// How to reach a registry.
#[derive(Debug, Clone)]
pub struct RegistryAccess {
    pub auth: RegistryAuth,
    /// Plain HTTP (a private registry without TLS).
    pub insecure: bool,
}

fn client(access: &RegistryAccess) -> OciClient {
    OciClient::new(ClientConfig {
        protocol: if access.insecure {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
        ..Default::default()
    })
}

fn copy_failed(image: &str, message: impl Into<String>) -> ErrorData {
    ErrorData::ImageCopyFailed {
        image: image.to_string(),
        message: message.into(),
    }
}

fn file_failed(message: impl Into<String>) -> ErrorData {
    ErrorData::FileFailed {
        message: message.into(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn blob_path(layout: &Path, digest: &str) -> Result<PathBuf> {
    let hex = digest.strip_prefix("sha256:").ok_or_else(|| {
        AlienError::new(ErrorData::BundleInvalid {
            message: format!("unsupported digest '{digest}'"),
        })
    })?;
    Ok(layout.join("blobs").join("sha256").join(hex))
}

fn media_type_of(manifest: &[u8]) -> String {
    let value: serde_json::Value = serde_json::from_slice(manifest).unwrap_or_default();
    match value.get("mediaType").and_then(|m| m.as_str()) {
        Some(media_type) => media_type.to_string(),
        None if value.get("manifests").is_some() => OCI_IMAGE_INDEX_MEDIA_TYPE.to_string(),
        None => OCI_IMAGE_MEDIA_TYPE.to_string(),
    }
}

/// Digests a manifest refers to: child manifests (for an index) or config
/// and layers (for an image).
fn referenced(manifest: &[u8]) -> (Vec<String>, Vec<String>) {
    let value: serde_json::Value = serde_json::from_slice(manifest).unwrap_or_default();
    let digests = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("digest")?.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let children = digests("manifests");
    let mut blobs = digests("layers");
    if let Some(config) = value
        .get("config")
        .and_then(|c| c.get("digest"))
        .and_then(|d| d.as_str())
    {
        blobs.push(config.to_string());
    }
    (children, blobs)
}

/// Copy `images` from their registries into an OCI image layout at `layout`.
pub async fn export_images(
    layout: &Path,
    images: &[String],
    access: &RegistryAccess,
) -> Result<Vec<BundleImage>> {
    tokio::fs::create_dir_all(layout.join("blobs").join("sha256"))
        .await
        .into_alien_error()
        .context(file_failed("creating the OCI layout"))?;
    let client = client(access);
    let mut exported = Vec::new();
    let mut index_entries = Vec::new();

    for image in images {
        let reference: Reference = image
            .parse()
            .into_alien_error()
            .context(copy_failed(image, "not a valid image reference"))?;
        let (manifest, digest) = client
            .pull_manifest_raw(&reference, &access.auth, &MANIFEST_MEDIA_TYPES)
            .await
            .into_alien_error()
            .context(copy_failed(image, "pulling the manifest"))?;
        let media_type = media_type_of(&manifest);
        write_manifest_tree(
            &client,
            &reference,
            &access.auth,
            layout,
            &manifest,
            &digest,
        )
        .await?;

        index_entries.push(serde_json::json!({
            "mediaType": media_type,
            "digest": digest,
            "size": manifest.len(),
            "annotations": { "org.opencontainers.image.ref.name": image },
        }));
        exported.push(BundleImage {
            source: image.clone(),
            repository: reference.repository().to_string(),
            tag: reference.tag().map(str::to_string),
            digest,
            media_type,
        });
    }

    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
        "manifests": index_entries,
    });
    write_json(&layout.join("index.json"), &index).await?;
    write_json(
        &layout.join("oci-layout"),
        &serde_json::json!({ "imageLayoutVersion": "1.0.0" }),
    )
    .await?;
    Ok(exported)
}

/// Store a manifest, and everything it references, as blobs.
async fn write_manifest_tree(
    client: &OciClient,
    reference: &Reference,
    auth: &RegistryAuth,
    layout: &Path,
    manifest: &[u8],
    digest: &str,
) -> Result<()> {
    let image = reference.whole();
    if format!("sha256:{}", sha256_hex(manifest)) != digest {
        return Err(AlienError::new(copy_failed(
            &image,
            "manifest does not match its digest",
        )));
    }
    tokio::fs::write(blob_path(layout, digest)?, manifest)
        .await
        .into_alien_error()
        .context(file_failed("writing a manifest"))?;

    let (children, blobs) = referenced(manifest);
    for child in children {
        let child_ref = Reference::with_digest(
            reference.registry().to_string(),
            reference.repository().to_string(),
            child.clone(),
        );
        let (child_manifest, child_digest) = client
            .pull_manifest_raw(&child_ref, auth, &MANIFEST_MEDIA_TYPES)
            .await
            .into_alien_error()
            .context(copy_failed(&image, format!("pulling manifest {child}")))?;
        Box::pin(write_manifest_tree(
            client,
            &child_ref,
            auth,
            layout,
            &child_manifest,
            &child_digest,
        ))
        .await?;
    }
    for blob in blobs {
        let path = blob_path(layout, &blob)?;
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            continue;
        }
        let mut file = tokio::fs::File::create(&path)
            .await
            .into_alien_error()
            .context(file_failed("creating a blob file"))?;
        client
            .pull_blob(reference, blob.as_str(), &mut file)
            .await
            .into_alien_error()
            .context(copy_failed(&image, format!("pulling blob {blob}")))?;
        let bytes = tokio::fs::read(&path)
            .await
            .into_alien_error()
            .context(file_failed("reading a blob back"))?;
        if format!("sha256:{}", sha256_hex(&bytes)) != blob {
            return Err(AlienError::new(copy_failed(
                &image,
                format!("blob {blob} does not match its digest"),
            )));
        }
    }
    Ok(())
}

/// Push every image in the OCI layout at `layout` to `registry_prefix`
/// (`host[:port]/optional/path`), by digest. Returns the new reference for
/// each source reference: `<registry_prefix>/<repository>@<digest>`.
pub async fn import_images(
    layout: &Path,
    images: &[BundleImage],
    registry_prefix: &str,
    access: &RegistryAccess,
) -> Result<HashMap<String, String>> {
    let client = client(access);
    let (host, path_prefix) = match registry_prefix.trim_end_matches('/').split_once('/') {
        Some((host, path)) => (host.to_string(), format!("{path}/")),
        None => (
            registry_prefix.trim_end_matches('/').to_string(),
            String::new(),
        ),
    };
    let mut mapping = HashMap::new();
    for image in images {
        let repository = format!("{path_prefix}{}", image.repository);
        let target = Reference::with_tag(
            host.clone(),
            repository.clone(),
            image.tag.clone().unwrap_or_else(|| "latest".to_string()),
        );
        client
            .store_auth_if_needed(target.resolve_registry(), &access.auth)
            .await;
        push_manifest_tree(&client, &target, layout, &image.digest, true).await?;
        mapping.insert(
            image.source.clone(),
            format!("{host}/{repository}@{}", image.digest),
        );
    }
    Ok(mapping)
}

async fn push_manifest_tree(
    client: &OciClient,
    target: &Reference,
    layout: &Path,
    digest: &str,
    tag_it: bool,
) -> Result<()> {
    let image = target.whole();
    let manifest = tokio::fs::read(blob_path(layout, digest)?)
        .await
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: format!("missing manifest {digest}"),
        })?;
    let (children, blobs) = referenced(&manifest);
    let by_digest = Reference::with_digest(
        target.registry().to_string(),
        target.repository().to_string(),
        digest.to_string(),
    );
    for child in children {
        Box::pin(push_manifest_tree(
            client, &by_digest, layout, &child, false,
        ))
        .await?;
    }
    for blob in blobs {
        let bytes = tokio::fs::read(blob_path(layout, &blob)?)
            .await
            .into_alien_error()
            .context(ErrorData::BundleInvalid {
                message: format!("missing blob {blob}"),
            })?;
        client
            .push_blob(&by_digest, &bytes, &blob)
            .await
            .into_alien_error()
            .context(copy_failed(&image, format!("pushing blob {blob}")))?;
    }
    let content_type = media_type_of(&manifest)
        .parse()
        .into_alien_error()
        .context(copy_failed(&image, "invalid manifest media type"))?;
    let destination = if tag_it { target } else { &by_digest };
    client
        .push_manifest_raw(destination, manifest, content_type)
        .await
        .into_alien_error()
        .context(copy_failed(&image, "pushing the manifest"))?;
    Ok(())
}

/// Replace every string in `value` that equals a key of `mapping`.
pub fn rewrite_references(value: &mut serde_json::Value, mapping: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(replacement) = mapping.get(text.as_str()) {
                *text = replacement.clone();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rewrite_references(item, mapping);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                rewrite_references(item, mapping);
            }
        }
        _ => {}
    }
}

/// Every image reference (`"image": "..."`) in a target or stack JSON.
pub fn image_references(value: &serde_json::Value) -> Vec<String> {
    let mut images = Vec::new();
    collect_images(value, &mut images);
    images.sort();
    images.dedup();
    images
}

fn collect_images(value: &serde_json::Value, images: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                match (key.as_str(), item) {
                    ("image", serde_json::Value::String(image)) if image.contains('/') => {
                        images.push(image.clone())
                    }
                    _ => collect_images(item, images),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_images(item, images);
            }
        }
        _ => {}
    }
}

/// sha256 of each file under `dir` (except the manifest), keyed by relative path.
pub async fn checksums(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut sums = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .into_alien_error()
            .context(file_failed("listing bundle files"))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .into_alien_error()
            .context(file_failed("listing bundle files"))?
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(dir)
                .expect("walked paths are under the bundle root")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == MANIFEST_FILE || relative == SIGNATURE_FILE {
                continue;
            }
            let bytes = tokio::fs::read(&path)
                .await
                .into_alien_error()
                .context(file_failed(format!("reading {relative}")))?;
            sums.insert(relative, sha256_hex(&bytes));
        }
    }
    Ok(sums)
}

/// Check a bundle directory: its manifest must be signed by `trusted_key`
/// (the manager's bundle signing key), and every file must match the
/// manifest's checksums.
pub async fn verify(dir: &Path, trusted_key: &str) -> Result<BundleManifest> {
    let manifest_bytes = tokio::fs::read(dir.join(MANIFEST_FILE))
        .await
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: "no manifest.json".to_string(),
        })?;
    let signature = tokio::fs::read_to_string(dir.join(SIGNATURE_FILE))
        .await
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: format!("the bundle is not signed (no {SIGNATURE_FILE})"),
        })?;
    alien_core::bundle_signature::verify(trusted_key, &manifest_bytes, &signature).context(
        ErrorData::BundleInvalid {
            message: "the signature does not match the trusted key".to_string(),
        },
    )?;
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: "manifest.json is not valid".to_string(),
        })?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(AlienError::new(ErrorData::BundleInvalid {
            message: format!(
                "format {} is not supported (this tool reads format {FORMAT_VERSION})",
                manifest.format_version
            ),
        }));
    }
    let actual = checksums(dir).await?;
    if actual != manifest.files {
        let changed: Vec<_> = manifest
            .files
            .iter()
            .filter(|(path, sum)| actual.get(*path) != Some(sum))
            .map(|(path, _)| path.as_str())
            .chain(
                actual
                    .keys()
                    .filter(|path| !manifest.files.contains_key(*path))
                    .map(String::as_str),
            )
            .take(5)
            .collect();
        return Err(AlienError::new(ErrorData::BundleInvalid {
            message: format!("files changed or missing: {}", changed.join(", ")),
        }));
    }
    Ok(manifest)
}

/// Pack `dir` into an uncompressed tar at `file`.
pub fn pack(dir: &Path, file: &Path) -> Result<()> {
    let out = std::fs::File::create(file)
        .into_alien_error()
        .context(file_failed(format!("creating {}", file.display())))?;
    let mut builder = tar::Builder::new(out);
    builder
        .append_dir_all(".", dir)
        .into_alien_error()
        .context(file_failed("writing the bundle"))?;
    builder
        .finish()
        .into_alien_error()
        .context(file_failed("finishing the bundle"))
}

/// Unpack a bundle tar into `dir`.
pub fn unpack(file: &Path, dir: &Path) -> Result<()> {
    let input = std::fs::File::open(file)
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: format!("cannot open {}", file.display()),
        })?;
    tar::Archive::new(input)
        .unpack(dir)
        .into_alien_error()
        .context(ErrorData::BundleInvalid {
            message: "not a readable tar archive".to_string(),
        })
}

async fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .into_alien_error()
        .context(file_failed("encoding JSON"))?;
    tokio::fs::write(path, bytes)
        .await
        .into_alien_error()
        .context(file_failed(format!("writing {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bundle_signature::BundleSigningKey;

    #[test]
    fn finds_and_rewrites_image_references() {
        let mut target = serde_json::json!({
            "releaseInfo": {"stack": {"resources": {
                "api": {"config": {"code": {"type": "image", "image": "manager.example.com:5050/artifacts/default:api-1"}}},
                "worker": {"config": {"code": {"type": "image", "image": "manager.example.com:5050/artifacts/default:w-1"}}},
                "db": {"config": {"image": "not-a-registry-image"}}
            }}}
        });
        let images = image_references(&target);
        assert_eq!(
            images,
            vec![
                "manager.example.com:5050/artifacts/default:api-1".to_string(),
                "manager.example.com:5050/artifacts/default:w-1".to_string(),
            ]
        );

        let mapping = HashMap::from([(
            images[0].clone(),
            "registry.internal/acme/artifacts/default@sha256:abc".to_string(),
        )]);
        rewrite_references(&mut target, &mapping);
        assert_eq!(
            target["releaseInfo"]["stack"]["resources"]["api"]["config"]["code"]["image"],
            "registry.internal/acme/artifacts/default@sha256:abc"
        );
        assert_eq!(
            target["releaseInfo"]["stack"]["resources"]["worker"]["config"]["code"]["image"],
            "manager.example.com:5050/artifacts/default:w-1"
        );
    }

    #[tokio::test]
    async fn verify_rejects_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(TARGET_FILE), b"{}")
            .await
            .unwrap();
        let manifest = BundleManifest {
            format_version: FORMAT_VERSION,
            deployment_id: "dep_1".to_string(),
            deployment_name: "customer-1".to_string(),
            release_id: "rel_1".to_string(),
            sequence: 1,
            created_at: "now".to_string(),
            stack_id: "app".to_string(),
            images: vec![],
            operator_image: "op".to_string(),
            chart: "chart/app.tgz".to_string(),
            files: checksums(dir.path()).await.unwrap(),
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        tokio::fs::write(dir.path().join(MANIFEST_FILE), &manifest_bytes)
            .await
            .unwrap();
        let key = BundleSigningKey::from_seed([5; 32]);
        let trusted = key.public_key();

        let error = verify(dir.path(), &trusted)
            .await
            .expect_err("an unsigned bundle fails");
        assert!(error.message.contains("not signed"), "{}", error.message);

        tokio::fs::write(dir.path().join(SIGNATURE_FILE), key.sign(&manifest_bytes))
            .await
            .unwrap();
        verify(dir.path(), &trusted)
            .await
            .expect("untouched bundle verifies");

        let other = BundleSigningKey::from_seed([6; 32]).public_key();
        let error = verify(dir.path(), &other)
            .await
            .expect_err("a bundle signed by another key fails");
        assert_eq!(error.code, "AIRGAP_BUNDLE_INVALID");

        tokio::fs::write(dir.path().join(TARGET_FILE), b"{\"tampered\":true}")
            .await
            .unwrap();
        let error = verify(dir.path(), &trusted)
            .await
            .expect_err("tampered bundle fails");
        assert_eq!(error.code, "AIRGAP_BUNDLE_INVALID");
        assert!(error.message.contains(TARGET_FILE));
    }

    #[tokio::test]
    async fn verify_rejects_a_manifest_rewritten_to_match_changed_files() {
        // Someone who changes a file and recomputes the checksums in
        // manifest.json still can't produce a valid signature.
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(TARGET_FILE), b"{}")
            .await
            .unwrap();
        let mut manifest = BundleManifest {
            format_version: FORMAT_VERSION,
            deployment_id: "dep_1".to_string(),
            deployment_name: "customer-1".to_string(),
            release_id: "rel_1".to_string(),
            sequence: 1,
            created_at: "now".to_string(),
            stack_id: "app".to_string(),
            images: vec![],
            operator_image: "op".to_string(),
            chart: "chart/app.tgz".to_string(),
            files: checksums(dir.path()).await.unwrap(),
        };
        let key = BundleSigningKey::from_seed([5; 32]);
        let signature = key.sign(&serde_json::to_vec(&manifest).unwrap());

        tokio::fs::write(dir.path().join(TARGET_FILE), b"{\"tampered\":true}")
            .await
            .unwrap();
        manifest.files = checksums(dir.path()).await.unwrap();
        tokio::fs::write(
            dir.path().join(MANIFEST_FILE),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .await
        .unwrap();
        tokio::fs::write(dir.path().join(SIGNATURE_FILE), signature)
            .await
            .unwrap();

        let error = verify(dir.path(), &key.public_key())
            .await
            .expect_err("a rewritten manifest fails");
        assert!(error.message.contains("signature"), "{}", error.message);
    }
}
