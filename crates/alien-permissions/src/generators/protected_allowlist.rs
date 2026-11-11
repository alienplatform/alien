//! What a protected install accepts in its management identity's grants.
//!
//! A deny list leaks: every new action, wildcard or provider role that nobody listed is allowed.
//! A protected install lists what it accepts instead, in one reviewed file per cloud under
//! `protected-management/`, and refuses the rest when the grant is rendered.
//!
//! A requested pattern is accepted when an entry covers it: the entry matches the pattern's text
//! with the pattern's own `*` and `?` taken as literal characters that an entry's `*` can absorb
//! (and an entry's `?` can absorb a `?`). That is a sound test that every string the pattern matches is matched by the entry, so
//! `s3:Get*` covers `s3:GetObject` and `s3:Get*Tagging` and does not cover `s3:*` or `*`.

use std::sync::OnceLock;

use alien_core::PermissionSet;

use crate::{
    error::{ErrorData, Result},
    registry::get_permission_set,
};

/// The kind of a line in an allow-list file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An AWS IAM action or an Azure control-plane action pattern.
    Action,
    /// An Azure data-plane action pattern.
    DataAction,
    /// A GCP permission pattern.
    Permission,
    /// A predefined role, matched by exact name.
    Role,
}

/// The cloud an allow-list belongs to, and the file that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowListCloud {
    /// `protected-management/aws.allow`
    Aws,
    /// `protected-management/azure.allow`
    Azure,
    /// `protected-management/gcp.allow`
    Gcp,
}

impl AllowListCloud {
    /// The path of the allow-list file, as an error names it.
    pub fn file(self) -> &'static str {
        match self {
            Self::Aws => "crates/alien-permissions/protected-management/aws.allow",
            Self::Azure => "crates/alien-permissions/protected-management/azure.allow",
            Self::Gcp => "crates/alien-permissions/protected-management/gcp.allow",
        }
    }

    fn list(self) -> &'static AllowList {
        static AWS: OnceLock<AllowList> = OnceLock::new();
        static AZURE: OnceLock<AllowList> = OnceLock::new();
        static GCP: OnceLock<AllowList> = OnceLock::new();
        match self {
            Self::Aws => AWS.get_or_init(|| {
                AllowList::parse(include_str!("../../protected-management/aws.allow"))
            }),
            Self::Azure => AZURE.get_or_init(|| {
                AllowList::parse(include_str!("../../protected-management/azure.allow"))
            }),
            Self::Gcp => GCP.get_or_init(|| {
                AllowList::parse(include_str!("../../protected-management/gcp.allow"))
            }),
        }
    }

    /// Refuse `requested` unless the cloud's allow-list covers it.
    pub fn require(self, permission_set_id: &str, kind: Kind, requested: &str) -> Result<()> {
        if self.list().covers(kind, requested) {
            return Ok(());
        }
        Err(alien_error::AlienError::new(
            ErrorData::ProtectedManagementActionRefused {
                permission_set_id: permission_set_id.to_string(),
                action: requested.to_string(),
                allow_list: self.file().to_string(),
            },
        ))
    }
}

/// Whether `permission_set` is one a protected install's management identity holds by design,
/// without its grants being checked against the cloud's allow-list: the built-in set of its id,
/// unchanged, and named in `protected-management/held-sets.allow`. Every other set is checked.
pub fn is_management_held(permission_set: &PermissionSet) -> bool {
    static HELD: OnceLock<HeldSets> = OnceLock::new();
    (get_permission_set(&permission_set.id) == Some(permission_set)
        && HELD
            .get_or_init(|| {
                HeldSets::parse(include_str!("../../protected-management/held-sets.allow"))
            })
            .holds(&permission_set.id))
        || permission_set == management_stack_secrets_data()
}

/// Whether `permission_set` is the built-in set of its id, unchanged. Anything else came from the
/// stack, so its grants and IAM conditions are not ones this repository reviewed.
pub fn is_built_in_unchanged(permission_set: &PermissionSet) -> bool {
    get_permission_set(&permission_set.id) == Some(permission_set)
}

/// The set that lets automatic AWS setup's management role read and sync the stack's vault
/// parameters. Setup adds it as an inline set, so it is held by its exact contents, not its id.
/// The deployer-secrets and signing Denies still apply on top of it.
pub fn management_stack_secrets_data() -> &'static PermissionSet {
    static SET: OnceLock<PermissionSet> = OnceLock::new();
    SET.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../protected-management/stack-secrets-data.json"
        ))
        .expect("protected-management/stack-secrets-data.json is a permission set")
    })
}

struct HeldSets {
    held: Vec<String>,
    unheld: Vec<String>,
}

impl HeldSets {
    fn parse(text: &str) -> Self {
        let (mut held, mut unheld) = (Vec::new(), Vec::new());
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (entry, _reason) = line
                .split_once(" #")
                .unwrap_or_else(|| panic!("held-sets line has no reason: {line}"));
            let (kind, id) = entry
                .trim()
                .split_once(' ')
                .unwrap_or_else(|| panic!("held-sets line has no id: {line}"));
            let id = id.trim().to_ascii_lowercase();
            match kind {
                "held" => held.push(id),
                "unheld" => unheld.push(id),
                other => panic!("held-sets line has unknown kind {other}: {line}"),
            }
        }
        Self { held, unheld }
    }

    fn holds(&self, id: &str) -> bool {
        let id = id.to_ascii_lowercase();
        let matches = |pattern: &String| pattern_covers(pattern.as_bytes(), id.as_bytes());
        self.held.iter().any(matches) && !self.unheld.iter().any(matches)
    }
}

struct Entry {
    kind: Kind,
    pattern: String,
}

struct AllowList {
    entries: Vec<Entry>,
}

impl AllowList {
    /// Parse `<kind> <pattern>  # <reason>` lines. The files are compiled in and checked by a
    /// test, so a malformed line is a bug in the repository, not an input.
    fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let (entry, _reason) = line
                    .split_once(" #")
                    .unwrap_or_else(|| panic!("allow-list line has no reason: {line}"));
                let (kind, pattern) = entry
                    .trim()
                    .split_once(' ')
                    .unwrap_or_else(|| panic!("allow-list line has no pattern: {line}"));
                let kind = match kind {
                    "action" => Kind::Action,
                    "data" => Kind::DataAction,
                    "permission" => Kind::Permission,
                    "role" => Kind::Role,
                    other => panic!("allow-list line has unknown kind {other}: {line}"),
                };
                Entry {
                    kind,
                    pattern: pattern.trim().to_ascii_lowercase(),
                }
            })
            .collect();
        Self { entries }
    }

    fn covers(&self, kind: Kind, requested: &str) -> bool {
        let requested = requested.to_ascii_lowercase();
        self.entries.iter().any(|entry| {
            entry.kind == kind
                && match kind {
                    Kind::Role => entry.pattern == requested,
                    _ => pattern_covers(entry.pattern.as_bytes(), requested.as_bytes()),
                }
        })
    }
}

/// Whether `allowed` matches `requested` with `requested`'s `*` and `?` as literal characters.
/// An `allowed` `*` absorbs any run, including those characters. An `allowed` `?` absorbs one
/// ordinary character, or a `?` of the request, but never a `*`, which stands for any run.
fn pattern_covers(allowed: &[u8], requested: &[u8]) -> bool {
    match (allowed.first(), requested.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            pattern_covers(&allowed[1..], requested)
                || (!requested.is_empty() && pattern_covers(allowed, &requested[1..]))
        }
        (Some(b'?'), Some(c)) if !matches!(c, b'*' | b'?') => {
            pattern_covers(&allowed[1..], &requested[1..])
        }
        (Some(expected), Some(actual)) if expected == actual => {
            pattern_covers(&allowed[1..], &requested[1..])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_covers_the_patterns_it_matches_in_full() {
        assert!(pattern_covers(b"s3:get*", b"s3:getobject"));
        assert!(pattern_covers(b"s3:get*", b"s3:get*tagging"));
        assert!(pattern_covers(b"*:get*", b"iam:get*"));
        assert!(pattern_covers(b"*/read", b"microsoft.compute/*/read"));
        assert!(pattern_covers(b"s3:getobject", b"s3:getobject"));
    }

    #[test]
    fn an_entry_does_not_cover_a_wider_pattern() {
        assert!(!pattern_covers(b"s3:get*", b"s3:*"));
        assert!(!pattern_covers(b"s3:get*", b"*"));
        assert!(!pattern_covers(b"*:get*", b"*"));
        assert!(!pattern_covers(b"*:get*", b"*:*"));
        assert!(!pattern_covers(b"s3:getobject", b"s3:get*"));
        assert!(!pattern_covers(b"s3:get?", b"s3:get*"));
        assert!(!pattern_covers(b"s3:get?", b"s3:get??"));
        assert!(!pattern_covers(b"s3:getobject", b"s3:get?bject"));
        assert!(!pattern_covers(b"*/read", b"microsoft.compute/*"));
        assert!(!pattern_covers(b"*/read", b"*/read*"));
    }

    #[test]
    fn every_line_of_every_file_names_a_reason_and_a_known_kind() {
        for cloud in [
            AllowListCloud::Aws,
            AllowListCloud::Azure,
            AllowListCloud::Gcp,
        ] {
            assert!(!cloud.list().entries.is_empty(), "{}", cloud.file());
        }
    }
}
