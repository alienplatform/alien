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

/// Refuse the Azure data action `requested` unless `azure.allow` lists it.
///
/// Azure's deny settings don't reach the data plane, so a protected install's management roles
/// name the data actions they may hold.
pub fn require_azure_data_action(permission_set_id: &str, requested: &str) -> Result<()> {
    static LIST: OnceLock<AllowList> = OnceLock::new();
    let list = LIST
        .get_or_init(|| AllowList::parse(include_str!("../../protected-management/azure.allow")));
    if list.covers(requested) {
        return Ok(());
    }
    Err(alien_error::AlienError::new(
        ErrorData::ProtectedManagementActionRefused {
            permission_set_id: permission_set_id.to_string(),
            action: requested.to_string(),
            allow_list: "crates/alien-permissions/protected-management/azure.allow".to_string(),
        },
    ))
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

struct AllowList {
    patterns: Vec<String>,
}

impl AllowList {
    /// Parse `data <pattern>  # <reason>` lines. The file is compiled in and checked by a test.
    fn parse(text: &str) -> Self {
        let patterns = text
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
                assert_eq!(kind, "data", "allow-list line has unknown kind: {line}");
                pattern.trim().to_ascii_lowercase()
            })
            .collect();
        Self { patterns }
    }

    fn covers(&self, requested: &str) -> bool {
        let requested = requested.to_ascii_lowercase();
        self.patterns
            .iter()
            .any(|pattern| pattern_covers(pattern.as_bytes(), requested.as_bytes()))
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
        let list = AllowList::parse(include_str!("../../protected-management/azure.allow"));
        assert!(!list.patterns.is_empty());
    }
}
