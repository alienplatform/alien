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
//! exact `sudo` command templates, and use of the Docker socket. A template
//! holds literal words and at most one `{unit}` placeholder, which the installer
//! fills with an operator-approved unit name. Wildcards, shell characters, paths
//! outside `/usr/bin/` and every other group are rejected.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::{ErrorData, Result, RiskTier};

/// Groups an operation may ask the installer to add the service user to.
pub const ALLOWED_GROUPS: [&str; 2] = ["systemd-journal", "docker"];

const SUDO_COMMAND_DIRECTORY: &str = "/usr/bin/";
const UNIT_PLACEHOLDER: &str = "{unit}";

/// Versioned requirements declared in an operation's `hostPermissions`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPermissions {
    /// Groups the service user must belong to: `systemd-journal`, `docker`.
    #[serde(default)]
    pub groups: Vec<String>,
    /// Exact `sudo` command templates, e.g. `/usr/bin/systemctl restart {unit}.service`.
    #[serde(default)]
    pub sudo_commands: Vec<String>,
    /// The operation uses the Docker socket.
    #[serde(default)]
    pub docker_socket: bool,
}

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ManifestInvalid {
        reason: format!("invalid host permissions: {}", reason.into()),
    })
}

fn literal_word(word: &str) -> bool {
    !word.is_empty()
        && word.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

/// A template is `/usr/bin/<program> <word>...` where a word is literal
/// (`[a-z0-9._-]+`) or `{unit}` optionally followed by a literal suffix.
fn validate_sudo_template(template: &str) -> Result<()> {
    let mut words = template.split(' ');
    let program = words.next().unwrap_or_default();
    let Some(name) = program.strip_prefix(SUDO_COMMAND_DIRECTORY) else {
        return Err(invalid(format!(
            "sudo command '{template}' must start with {SUDO_COMMAND_DIRECTORY}"
        )));
    };
    if !literal_word(name) {
        return Err(invalid(format!(
            "sudo command '{template}' names a program that is not a plain file name"
        )));
    }
    let mut placeholders = 0;
    let mut count = 0;
    for word in words {
        count += 1;
        match word.strip_prefix(UNIT_PLACEHOLDER) {
            Some(suffix) => {
                placeholders += 1;
                if !suffix.is_empty() && !literal_word(suffix) {
                    return Err(invalid(format!(
                        "sudo command '{template}' has an invalid word '{word}'"
                    )));
                }
            }
            None if literal_word(word) => {}
            None => {
                return Err(invalid(format!(
                    "sudo command '{template}' has the word '{word}'; only plain words and {UNIT_PLACEHOLDER} are allowed (no wildcards or shell characters)"
                )));
            }
        }
    }
    if count == 0 {
        return Err(invalid(format!(
            "sudo command '{template}' needs an argument"
        )));
    }
    if placeholders > 1 {
        return Err(invalid(format!(
            "sudo command '{template}' has more than one {UNIT_PLACEHOLDER}"
        )));
    }
    Ok(())
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
            // `/usr/bin/sh -c {unit}` is a plain template; the list of programs is the installer's.
            if sudo == "/usr/bin/sh -c {unit}" {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err(), "{sudo}");
            }
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
