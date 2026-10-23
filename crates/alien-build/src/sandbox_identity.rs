//! Read identity data from an OCI archive, without executing the base image.

use std::{fs::File, io::Read};

use alien_core::{
    sandbox_identity::ImageUser,
    sandbox_image::{SandboxImage, SandboxImageCommand, AGENT_PATH, IMAGE_COMMAND_PATH},
};
use alien_error::{AlienError, Context, IntoAlienError};
use dockdash::Image;
use serde::Deserialize;

use crate::error::{ErrorData, Result};

/// Refuse a declaration/image mismatch before publishing a locally built sandbox artifact.
pub(crate) async fn validate_declared_uid(
    sandbox: &alien_core::Sandbox,
    directory: &std::path::Path,
) -> Result<()> {
    let Some(supervisor) = &sandbox.privileged_supervisor else {
        return Ok(());
    };
    let path = directory.join(format!(
        "{}.oci.tar",
        alien_core::BinaryTarget::LinuxArm64.runtime_platform_id()
    ));
    let image =
        Image::from_tarball(path)
            .into_alien_error()
            .context(ErrorData::BuildConfigInvalid {
                message: format!(
                    "Cannot inspect the ARM64 image for sandbox '{}'",
                    sandbox.id
                ),
            })?;
    BaseIdentity::read(&image, &sandbox.id).await?.contract(
        alien_core::sandbox_image::AWS_MICROVM,
        Some(supervisor.command_uid),
    )?;
    Ok(())
}

/// Resolve the boot uid from every built architecture, before the declaration reaches planning.
/// Different users across architectures would make the plan depend on which host runs it.
pub(crate) async fn built_image_user(
    sandbox: &alien_core::Sandbox,
    directory: &std::path::Path,
    targets: &[alien_core::BinaryTarget],
) -> Result<u32> {
    let failed = |message: String| ErrorData::BuildConfigInvalid { message };
    let mut uid = None;
    for target in targets {
        let path = directory.join(format!("{}.oci.tar", target.runtime_platform_id()));
        let image = Image::from_tarball(&path)
            .into_alien_error()
            .context(failed(format!(
                "Cannot inspect sandbox '{}' image {}",
                sandbox.id,
                path.display()
            )))?;
        let base = BaseIdentity::read(&image, &sandbox.id).await?;
        let current = ImageUser {
            user: &base.user,
            passwd: base.passwd.as_deref().unwrap_or_default(),
            group: base.group.as_deref().unwrap_or_default(),
        }
        .image_identity()
        .context(failed(format!(
            "Cannot resolve sandbox '{}' image USER",
            sandbox.id
        )))?
        .uid;
        if uid.is_some_and(|previous| previous != current) {
            return Err(AlienError::new(failed(format!(
                "Sandbox '{}' image USER differs across architectures",
                sandbox.id
            ))));
        }
        uid = Some(current);
    }
    uid.ok_or_else(|| {
        AlienError::new(failed(format!(
            "Sandbox '{}' has no built image targets",
            sandbox.id
        )))
    })
}

/// The image's USER and account databases, read from the same immutable archive.
pub struct BaseIdentity {
    pub user: String,
    pub passwd: Option<String>,
    pub group: Option<String>,
    /// Original startup configuration from the same inspected image. Absent only
    /// for callers constructing a released identity contract without inspection.
    pub image_command: Option<SandboxImageCommand>,
}

impl BaseIdentity {
    /// Read the image configuration and account files before packaging it.
    pub async fn read(image: &Image, resource_name: &str) -> Result<Self> {
        let command = image_command(image).await?;
        Ok(Self {
            user: command.user.clone().ok_or_else(|| {
                AlienError::new(ErrorData::BuildConfigInvalid {
                    message: "The sandbox image has no original USER metadata".into(),
                })
            })?,
            passwd: read_identity_file(image, "/etc/passwd", resource_name).await?,
            group: read_identity_file(image, "/etc/group", resource_name).await?,
            image_command: Some(command),
        })
    }

    /// Apply the non-root image USER to a backend's default contract.
    pub fn contract(
        &self,
        mut default: SandboxImage,
        command_uid: Option<u32>,
    ) -> Result<SandboxImage> {
        let identity = ImageUser {
            user: &self.user,
            passwd: self.passwd.as_deref().unwrap_or_default(),
            group: self.group.as_deref().unwrap_or_default(),
        }
        .exec_identity(default.exec_uid, command_uid)
        .context(ErrorData::BuildConfigInvalid {
            message: "Cannot select the sandbox command identity".to_string(),
        })?;
        default.exec_uid = identity.uid;
        default.exec_gid = identity.gid;
        Ok(default)
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProcessConfig {
    #[serde(default)]
    user: String,
    entrypoint: Option<Vec<String>>,
    cmd: Option<Vec<String>>,
    env: Option<Vec<String>>,
    working_dir: Option<String>,
}

/// Preserve the base image command from the same archive used to resolve its identity.
pub async fn image_command(image: &Image) -> Result<SandboxImageCommand> {
    let config = image_config(image)?.config;
    if config
        .entrypoint
        .as_ref()
        .and_then(|argv| argv.first())
        .map(String::as_str)
        == Some(AGENT_PATH)
    {
        // Repackaging must preserve the user's command, never start a second agent as it.
        // Old packages without the original USER can still deploy unchanged, but rebuilding
        // one requires the original base image; guessing would silently change its identity.
        let invalid = || ErrorData::BuildConfigInvalid {
            message: concat!(
                "Cannot repackage this agent image without saved original command and USER ",
                "metadata; rebuild from the original base image"
            )
            .into(),
        };
        let saved = read_identity_file(image, IMAGE_COMMAND_PATH, "sandbox")
            .await?
            .ok_or_else(|| AlienError::new(invalid()))?;
        let command: SandboxImageCommand = serde_json::from_str(&saved)
            .into_alien_error()
            .context(invalid())?;
        if command.user.is_none() {
            return Err(AlienError::new(invalid()));
        }
        return Ok(command);
    }
    let mut env = std::collections::BTreeMap::new();
    for entry in config.env.unwrap_or_default() {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            AlienError::new(ErrorData::BuildConfigInvalid {
                message: "Image environment entry must contain '='".to_string(),
            })
        })?;
        env.insert(key.to_string(), value.to_string());
    }
    Ok(SandboxImageCommand {
        user: Some(config.user),
        command: config
            .entrypoint
            .into_iter()
            .flatten()
            .chain(config.cmd.into_iter().flatten())
            .collect(),
        env,
        working_directory: config
            .working_dir
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/".to_string()),
    })
}

#[derive(Deserialize)]
struct ImageConfig {
    #[serde(default)]
    config: ProcessConfig,
    rootfs: RootFs,
}

#[derive(Deserialize)]
struct RootFs {
    diff_ids: Vec<String>,
}

/// Ordered uncompressed layer identities, used to avoid reapplying an unchanged final layer.
pub(crate) fn image_layers(image: &Image) -> Result<Vec<String>> {
    Ok(image_config(image)?.rootfs.diff_ids)
}

fn image_config(image: &Image) -> Result<ImageConfig> {
    let failed = || ErrorData::BuildConfigInvalid {
        message: "Cannot read the sandbox base image's OCI configuration".to_string(),
    };
    let path = format!("blobs/{}", image.config_digest().replace(':', "/"));
    let file = File::open(image.path())
        .into_alien_error()
        .context(failed())?;
    let mut archive = tar::Archive::new(file);
    for entry in archive
        .entries_with_seek()
        .into_alien_error()
        .context(failed())?
    {
        let mut entry = entry.into_alien_error().context(failed())?;
        if entry.path().into_alien_error().context(failed())?.as_ref()
            == std::path::Path::new(&path)
        {
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(1 << 20)
                .read_to_end(&mut bytes)
                .into_alien_error()
                .context(failed())?;
            let config: ImageConfig = serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(failed())?;
            return Ok(config);
        }
    }
    Err(AlienError::new(failed()))
}
/// `None` for a base with no such file, which is how a scratch or distroless base looks. A link
/// or directory in its place is an error: following it could read a file the runtime never uses.
pub(crate) async fn read_identity_file(
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
pub(crate) fn unreadable_file(
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::sandbox_image::{AWS_MICROVM, GCP_AGENT_PLATFORM};
    use dockdash::{Arch, Layer};

    async fn base(user: &str) -> Image {
        let layer = Layer::builder()
            .unwrap()
            .data_with_owner(
                "/etc/passwd",
                b"root:x:0:0::/root:/bin/sh\n1001:x:9001:9001::/:/bin/sh\napp:x:1001:2001::/home/app:/bin/sh\n",
                0o644,
                0,
                0,
            )
            .unwrap()
            .data_with_owner(
                "/etc/group",
                b"root:x:0:\napp:x:2001:\ndata:x:3001:\n",
                0o644,
                0,
                0,
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        Image::builder()
            .platform("linux", &Arch::Amd64)
            .user(user)
            .layer(layer)
            .build()
            .await
            .unwrap()
            .0
    }

    async fn packaged(saved: Option<&str>) -> Image {
        let mut layer = Layer::builder()
            .unwrap()
            .data_with_owner(
                "/etc/passwd",
                b"root:x:0:0::/root:/bin/sh\napp:x:2001:2001::/home/app:/bin/sh\n",
                0o644,
                0,
                0,
            )
            .unwrap()
            .data_with_owner("/etc/group", b"root:x:0:\napp:x:2001:\n", 0o644, 0, 0)
            .unwrap();
        if let Some(saved) = saved {
            layer = layer
                .data_with_owner(IMAGE_COMMAND_PATH, saved.as_bytes(), 0o600, 0, 0)
                .unwrap();
        }
        Image::builder()
            .platform("linux", &Arch::Amd64)
            .user("0:0")
            .entrypoint(vec![AGENT_PATH.to_string()])
            .layer(layer.build().await.unwrap())
            .build()
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn repackaging_preserves_original_command_and_user_instead_of_the_agent() {
        for (user, uid) in [("", 0), ("root", 0), ("app", 2001)] {
            let saved = serde_json::json!({
                "user": user, "command": ["/app/supervisor", "--serve"],
                "env": {"ORIGINAL": "kept"}, "workingDirectory": "/app"
            });
            let image = packaged(Some(&saved.to_string())).await;
            let command = image_command(&image).await.unwrap();
            assert_eq!(serde_json::to_value(command).unwrap(), saved);
            let identity = BaseIdentity::read(&image, "repackaged").await.unwrap();
            assert_eq!(identity.user, user);
            assert_eq!(
                ImageUser {
                    user: &identity.user,
                    passwd: identity.passwd.as_deref().unwrap(),
                    group: identity.group.as_deref().unwrap(),
                }
                .image_identity()
                .unwrap()
                .uid,
                uid
            );
        }
        for saved in [
            None,
            Some("invalid"),
            Some(r#"{"command":[],"env":{},"workingDirectory":"/"}"#),
        ] {
            let image = packaged(saved).await;
            let error = image_command(&image).await.unwrap_err();
            assert_eq!(error.code, "PACKAGE_CONFIG_INVALID");
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn built_archives_resolve_boot_user_and_refuse_architecture_mismatch() {
        let sandbox = alien_core::Sandbox::new("boot".into())
            .code(alien_core::SandboxCode::Image {
                image: "unused".into(),
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let directory = tempfile::tempdir().unwrap();
        let targets = [
            alien_core::BinaryTarget::LinuxX64,
            alien_core::BinaryTarget::LinuxArm64,
        ];
        for (user, expected) in [("", 0), ("root", 0), ("app", 1001), ("2000:2000", 2000)] {
            let image = base(user).await;
            for target in &targets {
                std::fs::copy(
                    image.path(),
                    directory
                        .path()
                        .join(format!("{}.oci.tar", target.runtime_platform_id())),
                )
                .unwrap();
            }
            assert_eq!(
                built_image_user(&sandbox, directory.path(), &targets)
                    .await
                    .unwrap(),
                expected
            );
        }
        let root = base("0").await;
        std::fs::copy(
            root.path(),
            directory
                .path()
                .join(format!("{}.oci.tar", targets[0].runtime_platform_id())),
        )
        .unwrap();
        let error = built_image_user(&sandbox, directory.path(), &targets)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("differs across architectures"));
        std::fs::remove_file(
            directory
                .path()
                .join(format!("{}.oci.tar", targets[1].runtime_platform_id())),
        )
        .unwrap();
        assert!(built_image_user(&sandbox, directory.path(), &targets)
            .await
            .is_err());
        assert!(built_image_user(&sandbox, directory.path(), &[])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn resolves_the_user_and_primary_group_from_an_oci_image() {
        for (user, expected) in [
            ("app", (1001, 2001)),
            ("1001", (1001, 2001)),
            ("app:data", (1001, 3001)),
            ("1001:3001", (1001, 3001)),
        ] {
            let image = base(user).await;
            let identity = BaseIdentity::read(&image, "test").await.unwrap();
            for default in [AWS_MICROVM, GCP_AGENT_PLATFORM] {
                let contract = identity.contract(default, None).unwrap();
                assert_eq!((contract.exec_uid, contract.exec_gid), expected, "{user}");
            }
        }
    }

    #[tokio::test]
    async fn root_and_absent_user_keep_the_backend_defaults() {
        for user in ["", "root", "0", "0:0"] {
            let image = base(user).await;
            let identity = BaseIdentity::read(&image, "test").await.unwrap();
            for default in [AWS_MICROVM, GCP_AGENT_PLATFORM] {
                assert_eq!(identity.contract(default, None).unwrap(), default);
            }
        }
    }

    #[tokio::test]
    async fn a_declared_uid_must_agree_with_the_image_user() {
        let image = base("app").await;
        let identity = BaseIdentity::read(&image, "test").await.unwrap();
        let error = identity.contract(AWS_MICROVM, Some(60000)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("disagrees with image USER uid 1001"),
            "{error}"
        );
        let agreed = identity.contract(AWS_MICROVM, Some(1001)).unwrap();
        assert_eq!((agreed.exec_uid, agreed.exec_gid), (1001, 2001));
    }

    #[tokio::test]
    async fn unresolved_and_privileged_identities_are_rejected_before_build() {
        for user in [
            "missing",
            "app:missing",
            "app:0",
            "4294967295",
            "12345",
            "app:-1",
        ] {
            let image = base(user).await;
            let identity = BaseIdentity::read(&image, "test").await.unwrap();
            identity.contract(AWS_MICROVM, None).expect_err(user);
        }
    }
}
