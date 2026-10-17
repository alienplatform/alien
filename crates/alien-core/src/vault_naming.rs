//! How each vault backend names a secret in the cloud's own secret store.
//!
//! The vault providers in `alien-bindings` read and write through these
//! functions, and Alien tells a deployer where to write a vault-native secret
//! with the same functions, so the name a deployer is shown is always the name
//! the workload reads.

/// SSM parameter name: `{vault_prefix}-{secret_name}`.
pub fn parameter_store_parameter_name(vault_prefix: &str, secret_name: &str) -> String {
    format!("{vault_prefix}-{secret_name}")
}

/// Secret Manager secret id: `{vault_prefix}-{secret_name}`.
pub fn secret_manager_secret_id(vault_prefix: &str, secret_name: &str) -> String {
    format!("{vault_prefix}-{secret_name}")
}

/// Key Vault secret name. The Key Vault itself is per vault, so the secret
/// name carries no prefix; Key Vault allows only alphanumerics and hyphens.
pub fn key_vault_secret_name(secret_name: &str) -> String {
    secret_name.replace('_', "-")
}

/// A fresh Azure deployer vault name, limited to 24 characters including its
/// six-character alphanumeric installation suffix. The saved name survives retries.
pub fn azure_deployer_vault_name(resource_prefix: &str, suffix: &str) -> String {
    let prefix: String = format!("{resource_prefix}-deployer-secrets")
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(17)
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    format!("{prefix}-{suffix}")
}

/// Kubernetes Secret name: `{vault_prefix}-{secret_name}`, lowercased, with
/// underscores turned into hyphens and other characters dropped, cut to the
/// 253-character object name limit. The value lives under the `value` key.
pub fn kubernetes_secret_name(vault_prefix: &str, secret_name: &str) -> String {
    let clean = format!("{vault_prefix}-{secret_name}")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
        .replace('_', "-");
    clean.chars().take(253).collect()
}

/// Data key of a Kubernetes Secret that holds a vault value.
pub const KUBERNETES_SECRET_VALUE_KEY: &str = "value";

#[cfg(test)]
mod tests {
    use super::*;

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
