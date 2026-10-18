//! Read identity data from an OCI archive, without executing the base image.

use std::{fs::File, io::Read};

use alien_core::{
    sandbox_identity::ImageUser,
    sandbox_image::{SandboxImage, SandboxImageCommand},
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

/// The image's USER and account databases, read from the same immutable archive.
pub struct BaseIdentity {
    pub user: String,
    pub passwd: Option<String>,
    pub group: Option<String>,
}

impl BaseIdentity {
    /// Read the image configuration and account files before packaging it.
    pub async fn read(image: &Image, resource_name: &str) -> Result<Self> {
        let config = image_config(image)?;
        Ok(Self {
            user: config.user,
            passwd: read_identity_file(image, "/etc/passwd", resource_name).await?,
            group: read_identity_file(image, "/etc/group", resource_name).await?,
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
pub fn image_command(image: &Image) -> Result<SandboxImageCommand> {
    let config = image_config(image)?;
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
}

fn image_config(image: &Image) -> Result<ProcessConfig> {
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
            return Ok(config.config);
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
