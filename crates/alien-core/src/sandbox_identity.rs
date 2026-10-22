//! Resolve an OCI image's USER without running code from the image.

use alien_error::{AlienError, Context, IntoAlienError};

use crate::error::{ErrorData, Result};

/// Numeric image identity. Exec identities must additionally exclude uid and gid zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecIdentity {
    /// User id.
    pub uid: u32,
    /// Primary group id; it need not equal the user id.
    pub gid: u32,
}

/// Account data from the same image whose OCI USER is being resolved.
pub struct ImageUser<'a> {
    /// OCI config.User, empty when absent.
    pub user: &'a str,
    /// Image's /etc/passwd, empty when absent.
    pub passwd: &'a str,
    /// Image's /etc/group, empty when absent.
    pub group: &'a str,
}

impl ImageUser<'_> {
    /// Preserve a non-root image USER. Root/absent USER uses the backend default, or an
    /// explicitly declared command uid. A declaration must agree with a non-root image user.
    /// Unknown names and invalid ids fail before an image is built or a session starts.
    pub fn exec_identity(
        &self,
        default_uid: u32,
        command_uid: Option<u32>,
    ) -> Result<ExecIdentity> {
        let identity = self.image_identity()?;
        let invalid = |reason: String| ErrorData::SandboxImageUserInvalid {
            user: self.user.to_string(),
            reason,
        };
        if identity.uid == 0 {
            let uid = command_uid.unwrap_or(default_uid);
            if uid == 0 || uid == u32::MAX {
                return Err(AlienError::new(invalid(
                    "command uid must be non-root and cannot be -1".to_string(),
                )));
            }
            return Ok(ExecIdentity { uid, gid: uid });
        }
        if command_uid.is_some_and(|declared| declared != identity.uid) {
            return Err(AlienError::new(invalid(format!(
                "privilegedSupervisor.commandUid {} disagrees with image USER uid {}",
                command_uid.unwrap_or_default(),
                identity.uid
            ))));
        }
        if identity.gid == 0 {
            return Err(AlienError::new(invalid(
                "the root group is not permitted for sandbox commands; declare USER with a non-root primary group".to_string(),
            )));
        }
        Ok(identity)
    }

    /// Resolve OCI USER for the image boot process, including root and an absent USER.
    /// Numeric users without passwd entries use group zero, as Docker does.
    pub fn image_identity(&self) -> Result<ExecIdentity> {
        let invalid = |reason: String| ErrorData::SandboxImageUserInvalid {
            user: self.user.to_string(),
            reason,
        };
        let parse_id = |value: &str| -> Result<u32> {
            let id = value
                .parse::<u32>()
                .into_alien_error()
                .context(invalid(format!("'{value}' is not a numeric uid/gid")))?;
            if id == u32::MAX {
                return Err(AlienError::new(invalid(
                    "uid/gid -1 is reserved".to_string(),
                )));
            }
            Ok(id)
        };
        let (user, group) = self
            .user
            .split_once(':')
            .map_or((self.user, None), |(u, g)| (u, Some(g)));
        let numeric_uid = if user.is_empty() {
            Some(0)
        } else if user.bytes().all(|c| c.is_ascii_digit()) {
            Some(parse_id(user)?)
        } else {
            None
        };
        let account = self.passwd.lines().find_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            (fields.len() >= 4
                && numeric_uid.map_or_else(
                    || fields[0] == user,
                    |uid| fields[2].parse::<u32>() == Ok(uid),
                ))
            .then_some(fields)
        });
        let uid = match numeric_uid {
            Some(uid) => uid,
            None => parse_id(
                account.as_ref().ok_or_else(|| {
                    AlienError::new(invalid(format!("user '{user}' is absent from /etc/passwd")))
                })?[2],
            )?,
        };
        let gid = match group {
            Some(group) if !group.is_empty() && group.bytes().all(|c| c.is_ascii_digit()) => {
                parse_id(group)?
            }
            Some(group) => {
                let entry = self
                    .group
                    .lines()
                    .find_map(|line| {
                        let fields: Vec<_> = line.split(':').collect();
                        (fields.len() >= 3 && fields[0] == group).then_some(fields)
                    })
                    .ok_or_else(|| {
                        AlienError::new(invalid(format!(
                            "group '{group}' is absent from /etc/group"
                        )))
                    })?;
                parse_id(entry[2])?
            }
            None => match account {
                Some(account) => parse_id(account[3])?,
                // OCI/Docker uses group zero for a numeric user absent from passwd.
                None => 0,
            },
        };
        Ok(ExecIdentity { uid, gid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_root_is_distinct_from_the_non_root_exec_fallback() {
        for user in ["", "0", "0:0", "root", "supervisor:workers"] {
            let image = ImageUser {
                user,
                passwd: "root:x:0:0::/:/bin/sh\nsupervisor:x:0:2000::/:/bin/sh\n",
                group: "workers:x:2000:\n",
            };
            assert_eq!(image.image_identity().unwrap().uid, 0);
            assert_eq!(
                image.exec_identity(60000, None).unwrap(),
                ExecIdentity {
                    uid: 60000,
                    gid: 60000
                }
            );
            assert_eq!(
                image.exec_identity(1000, Some(60001)).unwrap(),
                ExecIdentity {
                    uid: 60001,
                    gid: 60001
                }
            );
        }
    }

    #[test]
    fn image_and_exec_preserve_non_root_users_and_named_primary_groups() {
        for user in ["worker", "worker:workers", "2001:2000", "2001"] {
            let image = ImageUser {
                user,
                passwd: "worker:x:2001:2000::/:/bin/sh\n",
                group: "workers:x:2000:\n",
            };
            let expected = ExecIdentity {
                uid: 2001,
                gid: 2000,
            };
            assert_eq!(image.image_identity().unwrap(), expected);
            assert_eq!(image.exec_identity(60000, None).unwrap(), expected);
            assert!(image.exec_identity(60000, Some(2002)).is_err());
        }
    }

    #[test]
    fn numeric_image_user_can_have_root_group_but_exec_cannot() {
        let image = ImageUser {
            user: "2001",
            passwd: "",
            group: "",
        };
        assert_eq!(
            image.image_identity().unwrap(),
            ExecIdentity { uid: 2001, gid: 0 }
        );
        assert!(image.exec_identity(60000, None).is_err());
    }

    #[test]
    fn invalid_image_users_fail_before_startup() {
        for user in [
            "missing",
            "0:missing",
            "4294967295",
            "1:4294967295",
            "1:2:3",
        ] {
            let image = ImageUser {
                user,
                passwd: "",
                group: "",
            };
            assert_eq!(
                image.image_identity().unwrap_err().code,
                "SANDBOX_IMAGE_USER_INVALID"
            );
        }
    }

    #[test]
    fn released_command_metadata_keeps_its_legacy_identity() {
        let image: crate::sandbox_image::SandboxImageCommand = serde_json::from_str(
            r#"{"command":["/bin/sleep","infinity"],"env":{},"workingDirectory":"/"}"#,
        )
        .unwrap();
        assert_eq!(image.user, None);
        let root: crate::sandbox_image::SandboxImageCommand = serde_json::from_str(
            r#"{"user":"","command":["/bin/sleep","infinity"],"env":{},"workingDirectory":"/"}"#,
        )
        .unwrap();
        assert_eq!(root.user.as_deref(), Some(""));
    }
}
