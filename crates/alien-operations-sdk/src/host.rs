//! What an operation needs from the machine it runs on, for installs on Linux
//! machines and Docker hosts. An installer grants exactly this and nothing else.
//!
//! ```json
//! "hostPermissions": {
//!   "groups": ["systemd-journal"],
//!   "sudoCommands": ["/usr/bin/systemctl restart {unit}.service"],
//!   "dockerSocket": false
//! }
//! ```
//!
//! Only three things can be declared: membership of a group from a fixed list,
//! exact `sudo` command templates, and use of the Docker socket. The only
//! template accepted is `/usr/bin/systemctl restart {unit}.service`, where the
//! installer fills `{unit}` with an operator-approved unit name. Everything else
//! (wildcards, shell characters, other programs, other groups) is rejected.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::{ErrorData, Result, RiskTier};

/// Groups an operation may ask the installer to add the service user to.
pub const ALLOWED_GROUPS: [&str; 2] = ["systemd-journal", "docker"];

/// Versioned requirements declared in an operation's `hostPermissions`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPermissions {
    /// Groups the service user must belong to: `systemd-journal`, `docker`. Membership of
    /// the `docker` group is equivalent to root on that host, so a read-only operation
    /// cannot declare it.
    #[serde(default)]
    pub groups: Vec<String>,
    /// Exact `sudo` command templates, e.g. `/usr/bin/systemctl restart {unit}.service`.
    #[serde(default)]
    pub sudo_commands: Vec<String>,
    /// The operation uses the Docker socket. Access to the socket is equivalent to root on
    /// that host, so a read-only operation cannot declare it.
    #[serde(default)]
    pub docker_socket: bool,
}

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ManifestInvalid {
        reason: format!("invalid host permissions: {}", reason.into()),
    })
}

/// The one command an operation may ask to run through `sudo`: restart a service
/// unit. Nothing else is accepted, so a manifest cannot ask the installer for a
/// shell, a wildcard or another program.
pub const ALLOWED_SUDO_COMMANDS: [&str; 1] = ["/usr/bin/systemctl restart {unit}.service"];

fn validate_sudo_template(template: &str) -> Result<()> {
    if ALLOWED_SUDO_COMMANDS.contains(&template) {
        Ok(())
    } else {
        Err(invalid(format!(
            "sudo command '{template}' is not allowed; the only command is: {}",
            ALLOWED_SUDO_COMMANDS.join(", ")
        )))
    }
}

impl HostPermissions {
    /// Validate before an installer acts on the declaration. `tier` is the
    /// operation's effective risk tier: a read-only operation never needs sudo.
    pub fn validate(&self, tier: RiskTier) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for group in &self.groups {
            if !ALLOWED_GROUPS.contains(&group.as_str()) {
                return Err(invalid(format!(
                    "group '{group}' is not allowed; use one of: {}",
                    ALLOWED_GROUPS.join(", ")
                )));
            }
            if !seen.insert(group) {
                return Err(invalid(format!("group '{group}' is listed twice")));
            }
        }
        if tier == RiskTier::ReadOnly
            && (self.docker_socket || self.groups.iter().any(|g| g == "docker"))
        {
            return Err(invalid(
                "a read-only operation cannot use the docker group or the Docker socket, which give root on the host; use the mutating tier or higher",
            ));
        }
        if !self.sudo_commands.is_empty() && tier == RiskTier::ReadOnly {
            return Err(invalid(
                "a read-only operation cannot declare sudo commands",
            ));
        }
        let mut commands = std::collections::BTreeSet::new();
        for command in &self.sudo_commands {
            validate_sudo_template(command)?;
            if !commands.insert(command) {
                return Err(invalid(format!("sudo command '{command}' is listed twice")));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(groups: &[&str], sudo: &[&str], docker: bool) -> HostPermissions {
        HostPermissions {
            groups: groups.iter().map(|s| s.to_string()).collect(),
            sudo_commands: sudo.iter().map(|s| s.to_string()).collect(),
            docker_socket: docker,
        }
    }

    #[test]
    fn the_declarations_the_built_in_plugins_use_are_accepted() {
        with(&["systemd-journal"], &[], false)
            .validate(RiskTier::ReadOnly)
            .unwrap();
        with(&["docker"], &[], true)
            .validate(RiskTier::Mutating)
            .unwrap();
        with(&[], &["/usr/bin/systemctl restart {unit}.service"], false)
            .validate(RiskTier::Mutating)
            .unwrap();
    }

    #[test]
    fn a_read_only_operation_cannot_declare_the_docker_group_or_socket() {
        for permissions in [
            with(&["docker"], &[], false),
            with(&[], &[], true),
            with(&["systemd-journal", "docker"], &[], true),
        ] {
            let error = permissions
                .validate(RiskTier::ReadOnly)
                .unwrap_err()
                .to_string();
            assert!(error.contains("read-only"), "{error}");
            permissions.validate(RiskTier::Mutating).unwrap();
        }
    }

    #[test]
    fn wildcards_other_groups_and_anything_off_the_list_are_rejected() {
        for sudo in [
            "/usr/bin/systemctl restart *",
            "/usr/bin/systemctl restart {unit}.service *",
            "/usr/bin/systemctl restart nginx; reboot",
            "/usr/bin/systemctl restart $(id)",
            "/usr/bin/systemctl restart ../x",
            "/usr/bin/systemctl restart {unit} {unit}",
            "/usr/bin/systemctl restart {unit}.*",
            "/usr/bin/systemctl",
            "/bin/systemctl restart {unit}.service",
            "/usr/bin/../bin/sh -c x",
            "systemctl restart {unit}.service",
            "/usr/bin/sh -c {unit}",
            "/usr/bin/systemctl  restart {unit}.service",
            "/usr/bin/systemctl restart [a-z]",
            "/usr/bin/systemctl restart UPPER",
        ] {
            let result = with(&[], &[sudo], false).validate(RiskTier::Mutating);
            assert!(result.is_err(), "{sudo}");
        }
        for group in ["root", "wheel", "sudo", "adm", "*", ""] {
            assert!(
                with(&[group], &[], false)
                    .validate(RiskTier::Mutating)
                    .is_err(),
                "{group}"
            );
        }
        assert!(with(&["docker", "docker"], &[], false)
            .validate(RiskTier::Mutating)
            .is_err());
        assert!(
            with(&[], &["/usr/bin/systemctl restart {unit}.service"], false)
                .validate(RiskTier::ReadOnly)
                .is_err()
        );
    }
}

#[cfg(test)]
mod manifest_tests {
    use crate::CanonicalPluginManifest;

    fn manifest(host_permissions: &str) -> Vec<u8> {
        format!(
            r#"{{"name":"p","version":"1","tier":"read-only","binaries":{{"amd64":"p-linux-amd64"}},
            "operations":[{{"name":"restart","tier":"mutating","requiredPermissions":[],
            "hostPermissions":{host_permissions}}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn the_manifest_validation_rejects_a_wildcard_and_accepts_an_exact_template() {
        let good = manifest(r#"{"sudoCommands":["/usr/bin/systemctl restart {unit}.service"]}"#);
        let bad = manifest(r#"{"sudoCommands":["/usr/bin/systemctl restart *"]}"#);
        assert!(CanonicalPluginManifest::parse_and_validate(&good).is_ok());
        assert!(CanonicalPluginManifest::parse_and_validate(&bad).is_err());
    }
}
