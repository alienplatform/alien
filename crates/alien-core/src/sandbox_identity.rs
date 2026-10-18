//! Resolve an OCI image's USER without running code from the image.

use alien_error::{AlienError, Context, IntoAlienError};

use crate::error::{ErrorData, Result};

/// Numeric identity used for sandbox commands, never uid zero.
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
                && (fields[0] == user
                    || numeric_uid.is_some_and(|uid| fields[2].parse::<u32>() == Ok(uid))))
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
        if uid == 0 {
            let uid = command_uid.unwrap_or(default_uid);
            if uid == 0 || uid == u32::MAX {
                return Err(AlienError::new(invalid(
                    "command uid must be non-root and cannot be -1".to_string(),
                )));
            }
            return Ok(ExecIdentity { uid, gid: uid });
        }
        if command_uid.is_some_and(|declared| declared != uid) {
            return Err(AlienError::new(invalid(format!(
                "privilegedSupervisor.commandUid {} disagrees with image USER uid {uid}",
                command_uid.unwrap_or_default()
            ))));
        }
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
