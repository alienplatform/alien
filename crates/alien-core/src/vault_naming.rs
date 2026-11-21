//! How each vault backend names a secret in the cloud's own secret store.
//!
//! The vault providers in `alien-bindings` read and write through these
//! functions, and Alien tells a deployer where to write a vault-native secret
//! with the same functions, so the name a deployer is shown is always the name
//! the workload reads.

use sha2::{Digest, Sha256};

/// SSM parameter name: `{vault_prefix}-{secret_name}`.
pub fn parameter_store_parameter_name(vault_prefix: &str, secret_name: &str) -> String {
    format!("{vault_prefix}-{secret_name}")
}

/// Secret Manager secret id: `{vault_prefix}-{secret_name}`.
pub fn secret_manager_secret_id(vault_prefix: &str, secret_name: &str) -> String {
    format!("{vault_prefix}-{secret_name}")
}

/// Label or tag key that records the vault prefix a value was created under.
pub const VAULT_PREFIX_MARKER_KEY: &str = "alien-vault-prefix";

/// Label or tag key that records the id of the vault a value was created in.
pub const VAULT_ID_MARKER_KEY: &str = "alien-vault";

fn sha256_hex(raw: &str) -> String {
    Sha256::digest(raw.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The value stored under a vault marker key.
///
/// Secret Manager label values must be 1 to 63 characters of `[a-z0-9_-]`. A value that meets that
/// rule is stored as is. Any other value is stored as `h-` followed by the first 40 hex digits of
/// its SHA-256, so the same input always gives the same marker and the marker still fits. Parameter
/// Store tags use the same encoding, so one rule reads both. The erase helper in the generated
/// Terraform module computes the same function.
pub fn vault_marker_value(raw: &str) -> String {
    let fits = !raw.is_empty()
        && raw.len() <= 63
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if fits {
        return raw.to_string();
    }
    format!("h-{}", &sha256_hex(raw)[..40])
}

/// Key Vault secret name. The Key Vault itself is per vault, so the secret
/// name carries no prefix; Key Vault allows only alphanumerics and hyphens.
pub fn key_vault_secret_name(secret_name: &str) -> String {
    secret_name.replace('_', "-")
}

/// A fresh Azure deployer vault name: up to eight characters of the resource prefix, the fixed
/// word `deployer` and a six-character alphanumeric installation suffix, within Key Vault's 24
/// characters. The word keeps the name apart from the stack's own vaults (`{prefix}-{id}`), whatever
/// the prefix length. The saved name survives retries.
pub fn azure_deployer_vault_name(resource_prefix: &str, suffix: &str) -> String {
    let prefix: String = resource_prefix
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(8)
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    format!("{prefix}-deployer-{suffix}")
}

/// Whether `name` is the name [`azure_deployer_vault_name`] gives an installation's deployer
/// vault for `resource_prefix`, with any suffix.
pub fn is_azure_deployer_vault_name(resource_prefix: &str, name: &str) -> bool {
    let stem = azure_deployer_vault_name(resource_prefix, "");
    name.strip_prefix(&stem).is_some_and(|suffix| {
        suffix.len() == 6
            && suffix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

/// Longest Kubernetes object name.
pub const KUBERNETES_SECRET_NAME_MAX_CHARS: usize = 253;

/// The part of a Kubernetes Secret name that follows the vault prefix: `-{secret_name}`,
/// lowercased, with underscores turned into hyphens and other characters dropped. Not cut to
/// the length limit: the limit applies to the whole name, prefix included.
pub fn kubernetes_secret_name_tail(secret_name: &str) -> String {
    format!("-{secret_name}")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
        .replace('_', "-")
}

/// Kubernetes Secret name: `{vault_prefix}-{secret_name}`, lowercased, with
/// underscores turned into hyphens and other characters dropped, cut to the
/// 253-character object name limit (the whole name, prefix included). The value
/// lives under the `value` key.
pub fn kubernetes_secret_name(vault_prefix: &str, secret_name: &str) -> String {
    let clean = format!("{vault_prefix}{}", kubernetes_secret_name_tail(secret_name));
    let clean: String = clean
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
        .replace('_', "-");
    clean
        .chars()
        .take(KUBERNETES_SECRET_NAME_MAX_CHARS)
        .collect()
}

/// Data key of a Kubernetes Secret that holds a vault value.
pub const KUBERNETES_SECRET_VALUE_KEY: &str = "value";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_values_that_fit_are_kept() {
        assert_eq!(vault_marker_value("acme-1a2b-secrets"), "acme-1a2b-secrets");
        assert_eq!(vault_marker_value("a_b-9"), "a_b-9");
        let longest = "a".repeat(63);
        assert_eq!(vault_marker_value(&longest), longest);
    }

    #[test]
    fn marker_values_that_do_not_fit_are_hashed_to_a_valid_label() {
        let too_long = "a".repeat(64);
        for raw in [too_long.as_str(), "Upper", "dot.name", "", "ünï"] {
            let value = vault_marker_value(raw);
            assert_eq!(value, vault_marker_value(raw), "stable");
            assert_eq!(value.len(), 42);
            assert!(value.starts_with("h-"));
            assert!(value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'));
        }
        assert_ne!(vault_marker_value("Upper"), vault_marker_value("upper2"));
        assert_eq!(
            vault_marker_value("Upper"),
            "h-".to_string() + &sha256_hex("Upper")[..40]
        );
    }

    #[test]
    fn azure_deployer_vault_names_fit_and_vary_between_installations() {
        for prefix in ["a", "a-very-long-stack-resource-prefix"] {
            let first = azure_deployer_vault_name(prefix, "a1b2c3");
            assert!((3..=24).contains(&first.len()));
            assert!(first
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
            assert_ne!(first, azure_deployer_vault_name(prefix, "d4e5f6"));
        }
    }

    #[test]
    fn only_this_installations_deployer_vault_name_matches() {
        let name = azure_deployer_vault_name("acme-prod", "a1b2c3");
        assert!(is_azure_deployer_vault_name("acme-prod", &name));
        assert!(!is_azure_deployer_vault_name("acme-stage", &name));
        assert!(!is_azure_deployer_vault_name(
            "acme-prod",
            "acme-pro-deployer-a1b2c3d4"
        ));
        assert!(!is_azure_deployer_vault_name(
            "acme-prod",
            "acme-prod-signing-a1b2c3"
        ));
        assert!(!is_azure_deployer_vault_name(
            "acme-prod",
            "acme-prod-deployer-secrets-A1B2C3"
        ));
    }

    #[test]
    fn a_stack_vault_is_never_taken_for_the_deployer_vault() {
        for prefix in ["acme", "acme-prod-eastus", "acme-prod-eastus1"] {
            let orders = format!("{prefix}-orders");
            assert!(!is_azure_deployer_vault_name(prefix, &orders), "{orders}");
            let own = azure_deployer_vault_name(prefix, "a1b2c3");
            assert!(is_azure_deployer_vault_name(prefix, &own), "{own}");
            assert!(own.len() <= 24, "{own}");
        }
    }

    #[test]
    fn kubernetes_names_are_dns_safe() {
        assert_eq!(
            kubernetes_secret_name("Acme-Monitoring-secrets", "API_KEY"),
            "acme-monitoring-secrets-api-key"
        );
        assert_eq!(kubernetes_secret_name("p", &"x".repeat(400)).len(), 253);
    }

    #[test]
    fn key_vault_names_use_hyphens() {
        assert_eq!(key_vault_secret_name("API_KEY"), "API-KEY");
    }
}
