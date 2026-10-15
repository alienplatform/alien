//! The GCP sandbox image: a base with the sandbox agent, the exec identity and the agent's
//! contract layered on. Agent Platform runs the image as pushed, so all three have to be in it.
//! Nothing runs inside the base, so a base with no shell or no `/etc/passwd` layers the same way.

use crate::error::{ErrorData, Result};
use crate::registry::{registry_failure, RegistryFailure};
use crate::{
    base_image_build_retry_delay, is_retryable_dockdash_image_pull_error,
    BASE_IMAGE_BUILD_MAX_ATTEMPTS,
};
use alien_core::sandbox_image::{
    gcp_agent_platform_env, AGENT_MODE, AGENT_PATH, EXEC_USER, GCP_AGENT_PLATFORM,
    SESSION_ROOT_MODE,
};
use alien_error::{AlienError, Context, IntoAlienError};
use dockdash::{
    Arch, BlobCache, BuildDiagnostics, ClientProtocol, Image, ImageBuilder, Layer, PullPolicy,
    RegistryAuth,
};
use oci_client::errors::{OciDistributionError, OciErrorCode};
use oci_client::Reference;
use std::error::Error as StdError;
use tokio::time::sleep;
use tracing::warn;

/// The sandbox agent layered onto a GCP sandbox, released with the same version as
/// `alien-build`, so a build never layers an agent from a different release than the CLI's.
pub fn sandbox_agent_image() -> String {
    format!(
        "ghcr.io/alienplatform/alien-sandbox-agent:v{}",
        env!("CARGO_PKG_VERSION")
    )
}

/// A layered GCP sandbox image and the agent it was layered from.
#[derive(Debug)]
pub struct GcpSandboxImage {
    pub image: Image,
    /// Manifest digest the agent reference resolved to for linux/amd64.
    pub agent_digest: String,
}

/// Layers the agent from `agent_image`, the exec identity and the GCP Agent Platform contract
/// onto `base_image`, a registry reference pulled with `base_auth`. `base` is a local copy of the
/// same image, read for its `/etc/passwd` and `/etc/group`. It is dropped before the agent pull,
/// so a pulled base's archive is off the disk before more is pulled; a base opened with
/// [`Image::from_tarball`] leaves its tarball to the caller. `resource_name` is the sandbox the
/// image is for, named in a build failure. `cache` holds the pulled blobs and the agent layer.
///
/// The agent pull and the assembly, which fetches the base again, are each tried up to
/// `BASE_IMAGE_BUILD_MAX_ATTEMPTS` times when the registry throttles, answers with a server error
/// or drops the connection. A pull still failing that way, or failing on a connection error that
/// is not retried here, is [`ErrorData::ImagePullTemporarilyUnavailable`]. A missing agent tag is
/// not retried here; it is returned as [`ErrorData::SandboxAgentImageMissing`]. Any other registry
/// refusal is [`ErrorData::ImageLookupRejected`], and an image that cannot hold the agent, or has a
/// link or directory where a file it needs should be, is [`ErrorData::ImageBuildRejected`];
/// neither is retryable.
pub async fn build_gcp_sandbox_image(
    resource_name: &str,
    base: Image,
    base_image: &str,
    agent_image: &str,
    protocol: ClientProtocol,
    base_auth: RegistryAuth,
    cache: BlobCache,
) -> Result<GcpSandboxImage> {
    let failed = |reason: String| ErrorData::ImageBuildFailed {
        resource_name: resource_name.to_string(),
        reason,
        build_output: None,
    };
    let rejected = |reason: String| ErrorData::ImageBuildRejected {
        resource_name: resource_name.to_string(),
        reason,
    };
    let passwd = read_identity_file(&base, "/etc/passwd", resource_name).await?;
    let group = read_identity_file(&base, "/etc/group", resource_name).await?;
    // Only the identity files were needed, and a pulled base is a full archive on disk.
    drop(base);

    // The agent image is public, so no registry credential is sent to its registry.
    let pulling_agent = format!("Pulling the agent image {agent_image}");
    let (agent, agent_build) = build_with_retry(
        resource_name,
        agent_image,
        Pulled::Agent,
        &pulling_agent,
        || async {
            Ok(Image::builder()
                .from(agent_image)
                .platform("linux", &Arch::Amd64)
                .pull_policy(PullPolicy::Always)
                .protocol(protocol.clone())
                .auth(RegistryAuth::Anonymous)
                .blob_cache(cache.clone()))
        },
    )
    .await?;
    let agent_binary = match agent.read_file(AGENT_PATH).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let context = unreadable_file(
                &error,
                resource_name,
                AGENT_PATH,
                &format!("the agent image {agent_image}"),
            );
            return Err(error).into_alien_error().context(context);
        }
    }
    .ok_or_else(|| {
        AlienError::new(rejected(format!(
            "The agent image {agent_image} has no file at {AGENT_PATH}"
        )))
    })?;
    // Each pulled image holds a full archive on disk; only the agent binary was needed.
    drop(agent);

    let contract = GCP_AGENT_PLATFORM;
    let user = contract.user().ok_or_else(|| {
        AlienError::new(rejected(
            "The GCP image contract declares no user".to_string(),
        ))
    })?;
    let passwd = merge_identity(
        passwd.as_deref(),
        "root:x:0:0:root:/root:/sbin/nologin",
        &contract.passwd_entry(),
        contract.exec_uid,
    );
    let group = merge_identity(
        group.as_deref(),
        "root:x:0:",
        &contract.group_entry(),
        contract.exec_gid(),
    );

    let assembling = format!("Assembling the image from {base_image} and the agent layer");
    let (image, _) = build_with_retry(
        resource_name,
        base_image,
        Pulled::Base,
        &assembling,
        || async {
            // A layer is consumed by the image builder, so each attempt builds its own.
            let layer = Layer::builder()
                .map(|layer| layer.blob_cache(cache.clone()))
                .and_then(|layer| {
                    layer.data_with_owner("/etc/passwd", passwd.as_bytes(), 0o644, 0, 0)
                })
                .and_then(|layer| {
                    layer.data_with_owner("/etc/group", group.as_bytes(), 0o644, 0, 0)
                })
                .and_then(|layer| {
                    layer.data_with_owner(AGENT_PATH, &agent_binary, AGENT_MODE, 0, 0)
                })
                .and_then(|layer| {
                    layer.empty_directory(
                        contract.session_root,
                        SESSION_ROOT_MODE,
                        contract.exec_uid.into(),
                        contract.exec_gid().into(),
                    )
                })
                .into_alien_error()
                .context(failed("Failed to assemble the agent layer".to_string()))?
                .build()
                .await
                .into_alien_error()
                .context(failed("Failed to build the agent layer".to_string()))?;
            let mut builder = Image::builder()
                .from(base_image)
                .platform("linux", &Arch::Amd64)
                .protocol(protocol.clone())
                .auth(base_auth.clone())
                .blob_cache(cache.clone())
                .layer(layer)
                .user(&user)
                .expose_port(&contract.exposed_port())
                .entrypoint(vec![AGENT_PATH.to_string()]);
            for (name, value) in gcp_agent_platform_env() {
                builder = builder.env(name, &value);
            }
            Ok(builder)
        },
    )
    .await?;
    Ok(GcpSandboxImage {
        image,
        agent_digest: agent_build.resolved_manifest_digest,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pulled {
    /// The developer's image: a missing tag is theirs to fix.
    Base,
    /// Alien's agent, tagged with this release: a missing tag is
    /// [`ErrorData::SandboxAgentImageMissing`].
    Agent,
}

impl Pulled {
    /// What the image is pulled for, as a still-failing pull names it.
    fn purpose(self) -> &'static str {
        match self {
            Pulled::Base => "the base the sandbox agent is layered onto",
            Pulled::Agent => "the sandbox agent layered onto the image",
        }
    }
}

/// Builds the image `prepare` describes, retrying a transient pull of `reference`. `step` names
/// what failed when the build does. A pull still failing transiently after the last retry stays
/// retryable, so a caller can try again later rather than treat the build as broken.
async fn build_with_retry<F, Fut>(
    resource_name: &str,
    reference: &str,
    pulled: Pulled,
    step: &str,
    prepare: F,
) -> Result<(Image, BuildDiagnostics)>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<ImageBuilder>>,
{
    for attempt in 1..=BASE_IMAGE_BUILD_MAX_ATTEMPTS {
        match prepare().await?.build().await {
            Ok(built) => return Ok(built),
            Err(error)
                if attempt < BASE_IMAGE_BUILD_MAX_ATTEMPTS
                    && is_retryable_dockdash_image_pull_error(&error) =>
            {
                let delay = base_image_build_retry_delay(attempt);
                warn!(
                    image = reference,
                    attempt,
                    max_attempts = BASE_IMAGE_BUILD_MAX_ATTEMPTS,
                    delay_secs = delay.as_secs(),
                    error = %error,
                    "Transient image pull failure, retrying"
                );
                sleep(delay).await;
            }
            Err(error) => {
                let context = if is_retryable_dockdash_image_pull_error(&error) {
                    ErrorData::ImagePullTemporarilyUnavailable {
                        image: reference.to_string(),
                        reason: pulled.purpose().to_string(),
                    }
                } else if pulled == Pulled::Agent && is_missing_manifest(&error) {
                    ErrorData::SandboxAgentImageMissing {
                        image: reference.to_string(),
                        version: release_tag(reference),
                    }
                } else if let Some(answer) = oci_cause(&error) {
                    match registry_failure(answer) {
                        RegistryFailure::Transient(_) => {
                            ErrorData::ImagePullTemporarilyUnavailable {
                                image: reference.to_string(),
                                reason: pulled.purpose().to_string(),
                            }
                        }
                        RegistryFailure::Rejected(reason) => ErrorData::ImageLookupRejected {
                            image: reference.to_string(),
                            reason,
                        },
                    }
                } else {
                    ErrorData::ImageBuildFailed {
                        resource_name: resource_name.to_string(),
                        reason: step.to_string(),
                        build_output: None,
                    }
                };
                return Err(error).into_alien_error().context(context);
            }
        }
    }

    unreachable!("image build retry loop always returns")
}

/// The registry answered that the tag or repository does not exist. A platform missing from an
/// index is `ImageManifestNotFoundError`, which is not this. Narrower than the manifest lookup in
/// `registry.rs`: this error comes from a whole pull, where `BLOB_UNKNOWN` can be a missing layer.
fn is_missing_manifest(error: &dockdash::Error) -> bool {
    match oci_cause(error) {
        Some(OciDistributionError::RegistryError { envelope, .. }) => {
            envelope.errors.iter().any(|entry| {
                matches!(
                    entry.code,
                    OciErrorCode::ManifestUnknown | OciErrorCode::NameUnknown
                )
            })
        }
        Some(OciDistributionError::ServerError { code, .. }) => *code == 404,
        _ => false,
    }
}

/// The registry's answer `error` carries, or `None` for a failure on this side of the pull.
fn oci_cause(error: &dockdash::Error) -> Option<&OciDistributionError> {
    let mut cause: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(current) = cause {
        if let Some(oci_error) = current.downcast_ref::<OciDistributionError>() {
            return Some(oci_error);
        }
        cause = current.source();
    }
    None
}

/// The release `reference` is tagged with, as the hint names it: `v1.2.3` reads as `1.2.3`, and a
/// reference with neither a tag nor a digest reads as `latest`, the tag a registry resolves it to.
/// A digest-only reference names itself.
fn release_tag(reference: &str) -> String {
    Reference::try_from(reference)
        .ok()
        .and_then(|parsed| {
            let tag = parsed.tag()?;
            Some(tag.strip_prefix('v').unwrap_or(tag).to_string())
        })
        .unwrap_or_else(|| reference.to_string())
}

/// `None` for a base with no such file, which is how a scratch or distroless base looks. A link
/// or directory in its place is an error: following it could read a file the runtime never uses.
async fn read_identity_file(
    base: &Image,
    path: &str,
    resource_name: &str,
) -> Result<Option<String>> {
    let bytes = match base.read_file(path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let context = unreadable_file(&error, resource_name, path, "the base image");
            return Err(error).into_alien_error().context(context);
        }
    };
    bytes
        .map(|bytes| {
            String::from_utf8(bytes)
                .into_alien_error()
                .context(ErrorData::ImageBuildRejected {
                    resource_name: resource_name.to_string(),
                    reason: format!("{path} in the base image is not UTF-8"),
                })
        })
        .transpose()
}

/// Why `path` could not be read from `image`. A link or a directory where the file should be is the
/// image's to fix, so a retry would read the same thing; any other failure is on this side.
fn unreadable_file(
    error: &dockdash::Error,
    resource_name: &str,
    path: &str,
    image: &str,
) -> ErrorData {
    match error {
        dockdash::Error::InvalidPath { .. } => ErrorData::ImageBuildRejected {
            resource_name: resource_name.to_string(),
            reason: format!("{path} in {image} is not a regular file"),
        },
        _ => ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!("Failed to read {path} from {image}"),
            build_output: None,
        },
    }
}

/// Keeps the base's entries except the exec identity's name or id, keeps or adds `root`, and
/// appends the exec identity. A base that ships its own user at the exec id (`node`, `ubuntu`)
/// loses that entry, since two names for one id make `id` and every exec ambiguous.
fn merge_identity(base: Option<&str>, root_line: &str, exec_line: &str, exec_id: u32) -> String {
    let exec_id = exec_id.to_string();
    let kept: Vec<&str> = base
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            fields.first() != Some(&EXEC_USER) && fields.get(2) != Some(&exec_id.as_str())
        })
        .collect();
    let mut merged = String::new();
    if !kept.iter().any(|line| line.starts_with("root:")) {
        merged.push_str(root_line);
        merged.push('\n');
    }
    for line in kept {
        merged.push_str(line);
        merged.push('\n');
    }
    merged.push_str(exec_line);
    merged.push('\n');
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockdash::test_utils::setup_local_registry;
    use dockdash::PushOptions;
    use oci_client::client::{Client, ClientConfig};
    use std::io::Read;

    /// An amd64 image holding `files`, pushed to `reference`; also returned, as the local copy a
    /// release reads the base's identity files from.
    async fn push_image(reference: &str, files: &[(&str, &[u8])]) -> Image {
        let mut layer = Layer::builder().expect("layer");
        for (path, content) in files {
            layer = layer
                .data_with_owner(path, content, 0o644, 0, 0)
                .expect("file");
        }
        let (image, _) = Image::builder()
            .platform("linux", &Arch::Amd64)
            .layer(layer.build().await.expect("layer builds"))
            .build()
            .await
            .expect("image builds");
        image
            .push(
                reference,
                &PushOptions {
                    protocol: ClientProtocol::Http,
                    ..Default::default()
                },
            )
            .await
            .expect("image pushes");
        image
    }

    /// What [`layer_and_push`] pushed and how to read it back.
    struct Layered {
        target: String,
        protocol: ClientProtocol,
        agent_image: String,
        agent_digest: String,
    }

    /// Layers `base` onto a stand-in agent in a local registry with `cache`, and pushes the result.
    async fn layer_and_push(host: &str, base_files: &[(&str, &[u8])], cache: BlobCache) -> Layered {
        let agent_image = format!("{host}/alien/agent:test");
        push_image(&agent_image, &[(AGENT_PATH, b"#!/bin/sh\nexit 0\n")]).await;
        let base_image = format!("{host}/proj/sandbox:base");
        let base = push_image(&base_image, base_files).await;

        let protocol = ClientProtocol::HttpsExcept(vec![host.to_string()]);
        let layered = build_gcp_sandbox_image(
            "box",
            base,
            &base_image,
            &agent_image,
            protocol.clone(),
            RegistryAuth::Anonymous,
            cache,
        )
        .await
        .expect("the agent layers onto the base");
        let target = format!("{base_image}-agent");
        layered
            .image
            .push(
                &target,
                &PushOptions {
                    protocol: ClientProtocol::Http,
                    ..Default::default()
                },
            )
            .await
            .expect("the layered image pushes");
        Layered {
            target,
            protocol,
            agent_image,
            agent_digest: layered.agent_digest,
        }
    }

    fn default_cache() -> BlobCache {
        BlobCache::new().expect("blob cache")
    }

    fn client() -> Client {
        Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        })
    }

    /// The image config Agent Platform starts the container from.
    async fn pushed_config(target: &str) -> serde_json::Value {
        let reference: Reference = target.parse().expect("reference");
        let (_, _, config) = client()
            .pull_manifest_and_config(&reference, &oci_client::secrets::RegistryAuth::Anonymous)
            .await
            .expect("the pushed image reads back");
        serde_json::from_str(&config).expect("config is JSON")
    }

    async fn pushed_file(target: &str, protocol: ClientProtocol, path: &str) -> Option<String> {
        let (image, _) = Image::builder()
            .from(target)
            .platform("linux", &Arch::Amd64)
            .protocol(protocol)
            .auth(RegistryAuth::Anonymous)
            .build()
            .await
            .expect("the pushed image pulls");
        image
            .read_file(path)
            .await
            .expect("the file reads")
            .map(|bytes| String::from_utf8(bytes).expect("UTF-8"))
    }

    /// `(path, uid, gid, mode)` of every entry in the image's top layer: the owner and mode the
    /// runtime applies, which a file read cannot show.
    async fn top_layer_entries(target: &str) -> Vec<(String, u64, u64, u32)> {
        let reference: Reference = target.parse().expect("reference");
        let (manifest, _) = client()
            .pull_image_manifest(&reference, &oci_client::secrets::RegistryAuth::Anonymous)
            .await
            .expect("manifest");
        let top = manifest.layers.last().expect("a layer");
        let mut blob = Vec::new();
        client()
            .pull_blob(&reference, top, &mut blob)
            .await
            .expect("layer blob");
        let tar: Box<dyn Read + '_> = if top.media_type.contains("zstd") {
            Box::new(zstd::Decoder::new(blob.as_slice()).expect("zstd"))
        } else {
            Box::new(flate2::read::GzDecoder::new(blob.as_slice()))
        };
        let entries = tar::Archive::new(tar)
            .entries()
            .expect("tar entries")
            .map(|entry| {
                let entry = entry.expect("entry");
                let header = entry.header();
                (
                    format!("/{}", entry.path().expect("path").display())
                        .trim_end_matches('/')
                        .to_string(),
                    header.uid().expect("uid"),
                    header.gid().expect("gid"),
                    header.mode().expect("mode") & 0o7777,
                )
            })
            .collect();
        entries
    }

    fn at_exec_id(file: &str) -> Vec<&str> {
        file.lines()
            .filter(|line| line.split(':').nth(2) == Some("1000"))
            .collect()
    }

    /// `node` ships at uid 1000, the exec uid, so the base takes the replacement path: one identity
    /// at that id, named per the contract, and every other base user kept.
    #[tokio::test]
    async fn the_layered_image_carries_the_agent_identity_and_contract() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let Layered {
            target, protocol, ..
        } = layer_and_push(
            &host,
            &[
                (
                    "/etc/passwd",
                    b"root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1::/usr/sbin:/usr/sbin/nologin\n\
                      node:x:1000:1000::/home/node:/bin/bash\n",
                ),
                ("/etc/group", b"root:x:0:\nnode:x:1000:\nstaff:x:50:\n"),
            ],
            default_cache(),
        )
        .await;

        let config = pushed_config(&target).await;
        assert_eq!(config["architecture"], "amd64");
        assert_eq!(config["os"], "linux");
        let runtime = &config["config"];
        assert_eq!(runtime["User"], "1000:1000");
        assert_eq!(
            runtime["ExposedPorts"],
            serde_json::json!({ "8080/tcp": {} })
        );
        assert_eq!(runtime["Entrypoint"], serde_json::json!([AGENT_PATH]));
        let env: Vec<&str> = runtime["Env"]
            .as_array()
            .expect("Env")
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        for (name, value) in gcp_agent_platform_env() {
            assert!(
                env.contains(&format!("{name}={value}").as_str()),
                "{name}: {env:?}"
            );
        }

        let passwd = pushed_file(&target, protocol.clone(), "/etc/passwd")
            .await
            .expect("passwd");
        assert_eq!(
            at_exec_id(&passwd),
            vec![GCP_AGENT_PLATFORM.passwd_entry().as_str()]
        );
        assert!(
            passwd.starts_with("root:x:0:0:root:/root:/bin/bash\n"),
            "{passwd}"
        );
        assert!(
            passwd.contains("\ndaemon:x:1:1:"),
            "other base users are kept: {passwd}"
        );
        let group = pushed_file(&target, protocol.clone(), "/etc/group")
            .await
            .expect("group");
        assert_eq!(
            at_exec_id(&group),
            vec![GCP_AGENT_PLATFORM.group_entry().as_str()]
        );
        assert_eq!(
            pushed_file(&target, protocol, AGENT_PATH).await.as_deref(),
            Some("#!/bin/sh\nexit 0\n")
        );

        let entries = top_layer_entries(&target).await;
        let entry = |path: &str| {
            entries
                .iter()
                .find(|(entry, ..)| entry == path)
                .unwrap_or_else(|| panic!("{path} is in the layer: {entries:?}"))
                .clone()
        };
        assert_eq!(
            entry(AGENT_PATH),
            (AGENT_PATH.to_string(), 0, 0, AGENT_MODE)
        );
        assert_eq!(
            entry(GCP_AGENT_PLATFORM.session_root),
            (
                GCP_AGENT_PLATFORM.session_root.to_string(),
                1000,
                1000,
                SESSION_ROOT_MODE
            )
        );
    }

    /// A scratch or distroless base has no `/etc/passwd` to merge into. Nothing runs inside the
    /// base, so it layers all the same, and the base's own files stay.
    #[tokio::test]
    async fn a_base_without_identity_files_still_layers() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let Layered {
            target, protocol, ..
        } = layer_and_push(
            &host,
            &[("/marker", b"alien-e2e-source-build\n")],
            default_cache(),
        )
        .await;

        assert_eq!(pushed_config(&target).await["config"]["User"], "1000:1000");
        assert_eq!(
            pushed_file(&target, protocol.clone(), "/etc/passwd").await,
            Some(format!(
                "root:x:0:0:root:/root:/sbin/nologin\n{}\n",
                GCP_AGENT_PLATFORM.passwd_entry()
            ))
        );
        assert_eq!(
            pushed_file(&target, protocol, "/marker").await.as_deref(),
            Some("alien-e2e-source-build\n")
        );
    }

    /// The local copy of a base whose identity files are read before any pull.
    async fn local_base() -> Image {
        let layer = Layer::builder()
            .and_then(|layer| layer.data_with_owner("/marker", b"base\n", 0o644, 0, 0))
            .expect("layer");
        Image::builder()
            .platform("linux", &Arch::Amd64)
            .layer(layer.build().await.expect("layer builds"))
            .build()
            .await
            .expect("image builds")
            .0
    }

    /// The layering error for `agent_image` and `base_image`, with nothing else to fail on.
    async fn layering_error(agent_image: &str, base_image: &str) -> AlienError<ErrorData> {
        build_gcp_sandbox_image(
            "box",
            local_base().await,
            base_image,
            agent_image,
            ClientProtocol::Http,
            RegistryAuth::Anonymous,
            default_cache(),
        )
        .await
        .expect_err("the image cannot be built")
    }

    /// The layer digests `reference`'s manifest lists.
    async fn layer_digests(reference: &str) -> Vec<String> {
        let reference: Reference = reference.parse().expect("reference");
        let (manifest, _) = client()
            .pull_image_manifest(&reference, &oci_client::secrets::RegistryAuth::Anonymous)
            .await
            .expect("manifest");
        manifest
            .layers
            .into_iter()
            .map(|layer| layer.digest)
            .collect()
    }

    /// Every blob the layering pulls or builds goes to the caller's cache: the agent's layers, the
    /// base's layers and the agent layer built on top. The pushed image is base layers plus that
    /// top layer, so its manifest names both.
    #[tokio::test]
    async fn every_pulled_and_built_blob_lands_in_the_given_cache() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let cache_dir = tempfile::tempdir().expect("temp dir");
        let cache = BlobCache::with_path(cache_dir.path().to_path_buf()).expect("cache");
        let layered = layer_and_push(&host, &[("/marker", b"base\n")], cache.clone()).await;

        let agent_layers = layer_digests(&layered.agent_image).await;
        let target_layers = layer_digests(&layered.target).await;
        assert_eq!(target_layers.len(), 2, "base layer plus the agent layer");
        for digest in agent_layers.iter().chain(&target_layers) {
            assert!(
                cache.get_blob(digest).await.expect("cache reads").is_some(),
                "{digest} is in the given cache"
            );
        }
    }

    /// The digest returned is the agent manifest the layering used, so a caller can record it
    /// without pulling the agent again.
    #[tokio::test]
    async fn the_agent_digest_is_the_pulled_agent_manifest() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let layered = layer_and_push(&host, &[("/marker", b"base\n")], default_cache()).await;

        let reference: Reference = layered.agent_image.parse().expect("reference");
        let (_, registry_digest) = client()
            .pull_image_manifest(&reference, &oci_client::secrets::RegistryAuth::Anonymous)
            .await
            .expect("agent manifest");
        assert_eq!(layered.agent_digest, registry_digest);
    }

    /// A registry that never answers says nothing about the image, so once the retries run out
    /// the failure is still one a caller can retry later.
    #[tokio::test]
    async fn a_pull_still_failing_transiently_after_its_retries_stays_retryable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        drop(listener);

        let error = layering_error(
            &format!("{address}/alien/agent:test"),
            "registry.example.com/proj/sandbox:base",
        )
        .await;
        assert_eq!(error.code, "IMAGE_PULL_TEMPORARILY_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
    }

    /// Docker Hub and GHCR answer a rate limit with 429 and a `TOOMANYREQUESTS` error envelope,
    /// which says nothing about the image: it is retried, and still retryable when the retries
    /// run out.
    #[tokio::test]
    async fn a_rate_limited_pull_is_retried_and_stays_retryable() {
        let server = httpmock::MockServer::start_async().await;
        let manifests = server
            .mock_async(|when, then| {
                when.path_contains("/manifests/");
                then.status(429)
                    .header("content-type", "application/json")
                    .body(r#"{"errors":[{"code":"TOOMANYREQUESTS","message":"rate limited"}]}"#);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200);
            })
            .await;

        let error = layering_error(
            &format!("{}/alien/agent:test", server.address()),
            "registry.example.com/proj/sandbox:base",
        )
        .await;
        assert_eq!(error.code, "IMAGE_PULL_TEMPORARILY_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
        assert_eq!(
            reason(&error),
            Pulled::Agent.purpose(),
            "the reason says what the pull was for, not the image again"
        );
        assert_eq!(
            manifests.hits_async().await,
            crate::BASE_IMAGE_BUILD_MAX_ATTEMPTS,
            "every attempt is made"
        );
    }

    /// A registry that throttles the base fetch during assembly gets every attempt, and the
    /// failure stays retryable and names the base's purpose as its reason.
    #[tokio::test]
    async fn a_throttled_assembly_is_retried_and_stays_retryable() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let agent_image = format!("{host}/alien/agent:test");
        push_image(&agent_image, &[(AGENT_PATH, b"#!/bin/sh\nexit 0\n")]).await;
        let server = httpmock::MockServer::start_async().await;
        let manifests = server
            .mock_async(|when, then| {
                when.path_contains("/manifests/");
                then.status(429)
                    .header("content-type", "application/json")
                    .body(r#"{"errors":[{"code":"TOOMANYREQUESTS","message":"rate limited"}]}"#);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200);
            })
            .await;

        let error = layering_error(
            &agent_image,
            &format!("{}/proj/sandbox:base", server.address()),
        )
        .await;
        assert_eq!(error.code, "IMAGE_PULL_TEMPORARILY_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
        assert_eq!(reason(&error), Pulled::Base.purpose());
        assert_eq!(
            manifests.hits_async().await,
            crate::BASE_IMAGE_BUILD_MAX_ATTEMPTS,
            "every attempt is made"
        );
    }

    /// The `reason` an error carries.
    fn reason(error: &AlienError<ErrorData>) -> &str {
        error
            .context
            .as_ref()
            .and_then(|context| context.get("reason"))
            .and_then(|reason| reason.as_str())
            .expect("the error carries a reason")
    }

    /// A registry that answers every manifest request as a real one does for a missing tag.
    async fn registry_without_the_tag() -> httpmock::MockServer {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path_contains("/manifests/");
                then.status(404)
                    .header("content-type", "application/json")
                    .body(
                        r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"manifest unknown"}]}"#,
                    );
            })
            .await;
        server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200);
            })
            .await;
        server
    }

    /// The agent tag is published by a release workflow a deploy can race, so a missing one is
    /// its own retryable error, whose hint names the release of the tag the caller asked for.
    #[tokio::test]
    async fn a_missing_agent_tag_names_the_release_it_waits_for() {
        let registry = registry_without_the_tag().await;

        let error = layering_error(
            &format!("{}/alien/agent:v0.0.0-unreleased", registry.address()),
            "registry.example.com/proj/sandbox:base",
        )
        .await;
        assert_eq!(error.code, "SANDBOX_AGENT_IMAGE_MISSING", "{error}");
        assert!(error.retryable, "{error}");
        let hint = error.hint.as_deref().expect("the error carries a hint");
        assert!(
            hint.contains("If Alien 0.0.0-unreleased was released"),
            "{hint}"
        );
    }

    /// The base is the developer's image; a tag the registry does not have will not appear on a
    /// retry.
    #[tokio::test]
    async fn a_missing_base_tag_is_a_rejected_lookup() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let agent_image = format!("{host}/alien/agent:test");
        push_image(&agent_image, &[(AGENT_PATH, b"#!/bin/sh\nexit 0\n")]).await;
        let base_registry = registry_without_the_tag().await;

        let error = layering_error(
            &agent_image,
            &format!("{}/proj/sandbox:missing", base_registry.address()),
        )
        .await;
        assert_eq!(error.code, "IMAGE_LOOKUP_REJECTED", "{error}");
        assert!(!error.retryable, "{error}");
        assert_eq!(reason(&error), "Registry answered ManifestUnknown");
    }

    /// An agent image without the agent binary is a broken release; pulling it again returns the
    /// same image.
    #[tokio::test]
    async fn an_agent_image_without_the_agent_is_rejected() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let agent_image = format!("{host}/alien/agent:empty");
        push_image(&agent_image, &[("/README", b"no agent here\n")]).await;

        let error = layering_error(&agent_image, "registry.example.com/proj/sandbox:base").await;
        assert_eq!(error.code, "IMAGE_BUILD_REJECTED", "{error}");
        assert!(!error.retryable, "{error}");
    }

    /// A base whose `/etc/passwd` is not text cannot have the exec identity merged into it, on
    /// any attempt.
    #[tokio::test]
    async fn a_base_with_a_binary_passwd_is_rejected() {
        let layer = Layer::builder()
            .and_then(|layer| layer.data_with_owner("/etc/passwd", &[0xff, 0xfe], 0o644, 0, 0))
            .expect("layer");
        let (base, _) = Image::builder()
            .platform("linux", &Arch::Amd64)
            .layer(layer.build().await.expect("layer builds"))
            .build()
            .await
            .expect("image builds");

        let error = build_gcp_sandbox_image(
            "box",
            base,
            "registry.example.com/proj/sandbox:base",
            "registry.example.com/alien/agent:test",
            ClientProtocol::Http,
            RegistryAuth::Anonymous,
            default_cache(),
        )
        .await
        .expect_err("a binary passwd cannot be merged");
        assert_eq!(error.code, "IMAGE_BUILD_REJECTED", "{error}");
        assert!(!error.retryable, "{error}");
    }

    /// An amd64 image whose only file is a symlink at `path`.
    async fn image_with_symlink_at(path: &str) -> Image {
        let dir = tempfile::tempdir().expect("temp dir");
        let (parent, name) = path.rsplit_once('/').expect("an absolute path");
        let on_disk = dir.path().join(parent.trim_start_matches('/'));
        std::fs::create_dir_all(&on_disk).expect("parent directory");
        std::os::unix::fs::symlink("/elsewhere", on_disk.join(name)).expect("symlink");
        let layer = Layer::builder()
            .and_then(|layer| layer.directory(&on_disk, parent))
            .expect("layer");
        Image::builder()
            .platform("linux", &Arch::Amd64)
            .layer(layer.build().await.expect("layer builds"))
            .build()
            .await
            .expect("image builds")
            .0
    }

    /// A link is not followed, so a base whose `/etc/passwd` is one reads the same on every
    /// attempt.
    #[tokio::test]
    async fn a_base_with_a_symlinked_passwd_is_rejected() {
        let error = build_gcp_sandbox_image(
            "box",
            image_with_symlink_at("/etc/passwd").await,
            "registry.example.com/proj/sandbox:base",
            "registry.example.com/alien/agent:test",
            ClientProtocol::Http,
            RegistryAuth::Anonymous,
            default_cache(),
        )
        .await
        .expect_err("a symlinked passwd cannot be merged");
        assert_eq!(error.code, "IMAGE_BUILD_REJECTED", "{error}");
        assert!(!error.retryable, "{error}");
        assert_eq!(
            reason(&error),
            "/etc/passwd in the base image is not a regular file"
        );
    }

    /// An agent image whose agent is a link is a broken release; pulling it again returns the
    /// same image.
    #[tokio::test]
    async fn an_agent_image_with_a_symlinked_agent_is_rejected() {
        let (_registry, host) = setup_local_registry().await.expect("local registry");
        let agent_image = format!("{host}/alien/agent:symlink");
        image_with_symlink_at(AGENT_PATH)
            .await
            .push(
                &agent_image,
                &PushOptions {
                    protocol: ClientProtocol::Http,
                    ..Default::default()
                },
            )
            .await
            .expect("image pushes");

        let error = layering_error(&agent_image, "registry.example.com/proj/sandbox:base").await;
        assert_eq!(error.code, "IMAGE_BUILD_REJECTED", "{error}");
        assert!(!error.retryable, "{error}");
        assert_eq!(
            reason(&error),
            format!("{AGENT_PATH} in the agent image {agent_image} is not a regular file")
        );
    }

    /// A request the registry client gives up on without retrying, such as a redirect loop, says
    /// nothing about the image, so it stays retryable rather than reading as a refusal.
    #[tokio::test]
    async fn a_connection_failure_that_is_not_retried_here_stays_retryable() {
        let server = httpmock::MockServer::start_async().await;
        let manifests = server
            .mock_async(|when, then| {
                when.path_contains("/manifests/");
                then.status(302)
                    .header("location", "/v2/alien/agent/manifests/test");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200);
            })
            .await;

        let error = layering_error(
            &format!("{}/alien/agent:test", server.address()),
            "registry.example.com/proj/sandbox:base",
        )
        .await;
        assert_eq!(error.code, "IMAGE_PULL_TEMPORARILY_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
        assert_eq!(reason(&error), Pulled::Agent.purpose());
        assert!(
            manifests.hits_async().await > crate::BASE_IMAGE_BUILD_MAX_ATTEMPTS,
            "the redirects were followed"
        );
    }

    #[test]
    fn only_a_missing_tag_counts_as_a_missing_manifest() {
        use oci_client::errors::{OciEnvelope, OciError};
        let pull_failed = |source: OciDistributionError| dockdash::Error::ImagePull {
            image_ref: "ghcr.io/acme/sandbox-agent:v1".to_string(),
            message: "Failed to pull".to_string(),
            source: Some(Box::new(source)),
        };
        let registry = |code| {
            pull_failed(OciDistributionError::RegistryError {
                envelope: OciEnvelope {
                    errors: vec![OciError {
                        code,
                        message: String::new(),
                        detail: serde_json::Value::Null,
                    }],
                },
                url: "https://ghcr.io/v2/".to_string(),
            })
        };
        assert!(is_missing_manifest(&registry(
            OciErrorCode::ManifestUnknown
        )));
        assert!(is_missing_manifest(&registry(OciErrorCode::NameUnknown)));
        assert!(is_missing_manifest(&pull_failed(
            OciDistributionError::ServerError {
                code: 404,
                url: "https://ghcr.io/v2/".to_string(),
                message: String::new(),
            }
        )));
        assert!(!is_missing_manifest(&registry(OciErrorCode::Denied)));
        assert!(!is_missing_manifest(&registry(OciErrorCode::BlobUnknown)));
        assert!(!is_missing_manifest(&pull_failed(
            OciDistributionError::ImageManifestNotFoundError("no linux/amd64 variant".to_string())
        )));
    }

    /// A base that owns its archive, as a pulled one does, is deleted once its identity files are
    /// read rather than held through the agent pull and the assembly. A base opened from the
    /// caller's tarball is not deleted, so only the owned case has anything to observe.
    #[tokio::test]
    async fn the_base_archive_is_gone_before_the_agent_is_pulled() {
        use std::io::Write;

        let base = local_base().await;
        let archive = base.path().to_path_buf();
        assert!(archive.exists(), "the local base is an archive on disk");

        // A registry that records whether the archive exists when the agent pull first connects,
        // then answers every request with a missing manifest.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let (first_connect, archive_at_pull) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let body = r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"","detail":null}]}"#;
            for stream in listener.incoming() {
                let _ = first_connect.send(archive.exists());
                let Ok(mut stream) = stream else { continue };
                let _ = stream.read(&mut [0u8; 8192]);
                let _ = write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });

        build_gcp_sandbox_image(
            "box",
            base,
            "registry.example.com/proj/sandbox:base",
            &format!("{address}/alien/agent:test"),
            ClientProtocol::Http,
            RegistryAuth::Anonymous,
            default_cache(),
        )
        .await
        .expect_err("the agent tag is missing");
        assert_eq!(
            archive_at_pull.try_recv(),
            Ok(false),
            "the base archive was deleted before the agent pull"
        );
    }
}
