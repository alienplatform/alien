//! The GCP sandbox image: a base with the sandbox agent, the exec identity and the agent's
//! contract layered on. Agent Platform runs the image as pushed, so all three have to be in it.
//! Nothing runs inside the base, so a base with no shell or no `/etc/passwd` layers the same way.

use crate::error::{ErrorData, Result};
use crate::{
    base_image_build_retry_delay, is_retryable_dockdash_image_pull_error,
    BASE_IMAGE_BUILD_MAX_ATTEMPTS,
};
use alien_core::sandbox_image::{
    gcp_agent_platform_env, AGENT_MODE, AGENT_PATH, EXEC_USER, GCP_AGENT_PLATFORM,
    SESSION_ROOT_MODE,
};
use alien_error::{AlienError, Context, IntoAlienError};
use dockdash::{Arch, ClientProtocol, Image, ImageBuilder, Layer, PullPolicy, RegistryAuth};
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

/// Layers the agent from `agent_image`, the exec identity and the GCP Agent Platform contract
/// onto `base_image`, a registry reference pulled with `base_auth`. `base` is a local copy of the
/// same image, read for its `/etc/passwd` and `/etc/group`. `resource_name` is the sandbox the
/// image is for, named in any error.
pub async fn build_gcp_sandbox_image(
    resource_name: &str,
    base: &Image,
    base_image: &str,
    agent_image: &str,
    protocol: ClientProtocol,
    base_auth: RegistryAuth,
) -> Result<Image> {
    let failed = |reason: String| ErrorData::ImageBuildFailed {
        resource_name: resource_name.to_string(),
        reason,
        build_output: None,
    };
    let passwd = read_identity_file(base, "/etc/passwd", resource_name).await?;
    let group = read_identity_file(base, "/etc/group", resource_name).await?;

    // The agent image is public, so no registry credential is sent to its registry.
    let pulling_agent = format!("Pulling the agent image {agent_image}");
    let agent = build_with_retry(resource_name, agent_image, &pulling_agent, || async {
        Ok(Image::builder()
            .from(agent_image)
            .platform("linux", &Arch::Amd64)
            .pull_policy(PullPolicy::Always)
            .protocol(protocol.clone())
            .auth(RegistryAuth::Anonymous))
    })
    .await?;
    let agent_binary = agent
        .read_file(AGENT_PATH)
        .await
        .into_alien_error()
        .context(failed(format!(
            "Failed to read {AGENT_PATH} from the agent image {agent_image}"
        )))?
        .ok_or_else(|| {
            AlienError::new(failed(format!(
                "The agent image {agent_image} has no file at {AGENT_PATH}"
            )))
        })?;
    // Each pulled image holds a full archive on disk; only the agent binary was needed.
    drop(agent);

    let contract = GCP_AGENT_PLATFORM;
    let user = contract.user().ok_or_else(|| {
        AlienError::new(failed(
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
    build_with_retry(resource_name, base_image, &assembling, || async {
        // A layer is consumed by the image builder, so each attempt builds its own.
        let layer = Layer::builder()
            .and_then(|layer| layer.data_with_owner("/etc/passwd", passwd.as_bytes(), 0o644, 0, 0))
            .and_then(|layer| layer.data_with_owner("/etc/group", group.as_bytes(), 0o644, 0, 0))
            .and_then(|layer| layer.data_with_owner(AGENT_PATH, &agent_binary, AGENT_MODE, 0, 0))
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
            .layer(layer)
            .user(&user)
            .expose_port(&contract.exposed_port())
            .entrypoint(vec![AGENT_PATH.to_string()]);
        for (name, value) in gcp_agent_platform_env() {
            builder = builder.env(name, &value);
        }
        Ok(builder)
    })
    .await
}

/// Builds the image `prepare` describes, retrying a transient pull of `reference`. `step` names
/// what failed when the build does.
async fn build_with_retry<F, Fut>(
    resource_name: &str,
    reference: &str,
    step: &str,
    prepare: F,
) -> Result<Image>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<ImageBuilder>>,
{
    for attempt in 1..=BASE_IMAGE_BUILD_MAX_ATTEMPTS {
        match prepare().await?.build().await {
            Ok((image, _)) => return Ok(image),
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
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        resource_name: resource_name.to_string(),
                        reason: step.to_string(),
                        build_output: None,
                    });
            }
        }
    }

    unreachable!("image build retry loop always returns")
}

/// `None` for a base with no such file, which is how a scratch or distroless base looks. A link
/// or directory in its place is an error: following it could read a file the runtime never uses.
async fn read_identity_file(
    base: &Image,
    path: &str,
    resource_name: &str,
) -> Result<Option<String>> {
    let failed = |reason: String| ErrorData::ImageBuildFailed {
        resource_name: resource_name.to_string(),
        reason,
        build_output: None,
    };
    let bytes = base
        .read_file(path)
        .await
        .into_alien_error()
        .context(failed(format!("Failed to read {path} from the base image")))?;
    bytes
        .map(|bytes| {
            String::from_utf8(bytes)
                .into_alien_error()
                .context(failed(format!("{path} in the base image is not UTF-8")))
        })
        .transpose()
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
    use oci_client::Reference;
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

    /// Layers `base` onto a stand-in agent in a local registry, pushes the result, and returns its
    /// reference and the protocol that reads it.
    async fn layer_and_push(host: &str, base_files: &[(&str, &[u8])]) -> (String, ClientProtocol) {
        let agent_image = format!("{host}/alien/agent:test");
        push_image(&agent_image, &[(AGENT_PATH, b"#!/bin/sh\nexit 0\n")]).await;
        let base_image = format!("{host}/proj/sandbox:base");
        let base = push_image(&base_image, base_files).await;

        let protocol = ClientProtocol::HttpsExcept(vec![host.to_string()]);
        let layered = build_gcp_sandbox_image(
            "box",
            &base,
            &base_image,
            &agent_image,
            protocol.clone(),
            RegistryAuth::Anonymous,
        )
        .await
        .expect("the agent layers onto the base");
        let target = format!("{base_image}-agent");
        layered
            .push(
                &target,
                &PushOptions {
                    protocol: ClientProtocol::Http,
                    ..Default::default()
                },
            )
            .await
            .expect("the layered image pushes");
        (target, protocol)
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
        let (target, protocol) = layer_and_push(
            &host,
            &[
                (
                    "/etc/passwd",
                    b"root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1::/usr/sbin:/usr/sbin/nologin\n\
                      node:x:1000:1000::/home/node:/bin/bash\n",
                ),
                ("/etc/group", b"root:x:0:\nnode:x:1000:\nstaff:x:50:\n"),
            ],
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
        let (target, protocol) =
            layer_and_push(&host, &[("/marker", b"alien-e2e-source-build\n")]).await;

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
}
