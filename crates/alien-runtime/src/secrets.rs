use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Configuration for ALIEN_SECRETS environment variable
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AlienSecretsConfig {
    /// Secret keys to load from vault
    keys: Vec<String>,
    /// Hash of all env var values - triggers redeployment when changed
    hash: String,
}

/// Load secrets from the vault at startup based on ALIEN_SECRETS configuration.
///
/// This function:
/// 1. Parses the JSON to get the list of secret keys
/// 2. Loads the vault binding from the BindingsProvider
/// 3. Fetches each secret from the vault
/// 4. Returns them as a HashMap (caller decides how to expose to app)
///
/// This should be called BEFORE starting the application subprocess.
/// For cloud platforms (separate process), returned secrets can be set via std::env::set_var.
/// For local platform (embedded), returned secrets should be passed via Command::env to avoid races.
///
/// # Arguments
/// * `bindings_provider` - The bindings provider to load vault from
/// * `alien_secrets_json` - The ALIEN_SECRETS JSON string (from config.env_vars or std::env)
pub async fn load_secrets_from_vault(
    bindings_provider: &dyn alien_bindings::BindingsProviderApi,
    alien_secrets_json: &str,
) -> Result<std::collections::HashMap<String, String>> {
    // Parse the JSON
    let config: AlienSecretsConfig = serde_json::from_str(&alien_secrets_json)
        .into_alien_error()
        .context(ErrorData::SecretLoadFailed {
            secret_name: "ALIEN_SECRETS".to_string(),
            message: "Failed to parse ALIEN_SECRETS JSON".to_string(),
        })?;

    if config.keys.is_empty() {
        debug!("ALIEN_SECRETS contains no keys, skipping secret loading");
        return Ok(HashMap::new());
    }

    info!(
        count = config.keys.len(),
        hash = %config.hash,
        "Loading {} secret(s) from vault",
        config.keys.len()
    );

    // Load the vault binding
    // The vault binding name is conventionally "secrets" (the vault resource added by SecretsVaultMutation)
    let vault =
        bindings_provider
            .load_vault("secrets")
            .await
            .context(ErrorData::SecretLoadFailed {
                secret_name: "vault".to_string(),
                message: "Failed to load vault binding".to_string(),
            })?;

    // Fetch each secret from vault with retries.
    // IAM permission propagation on cloud platforms (especially GCP) can take
    // a short time after a new service account is granted access.
    // We retry with exponential backoff to handle this, but keep total retry
    // time under ~40s to stay well within Cloud Run's 240s startup probe timeout.
    const MAX_RETRIES: u32 = 5;
    const INITIAL_DELAY_SECS: u64 = 2;
    const MAX_DELAY_SECS: u64 = 10;

    let mut loaded_secrets = HashMap::new();
    for secret_key in &config.keys {
        debug!(secret_name = %secret_key, "Fetching secret from vault");

        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            match vault.get_secret(secret_key).await {
                Ok(value) => {
                    if attempt > 0 {
                        info!(
                            secret_name = %secret_key,
                            attempt = attempt,
                            "Secret loaded from vault after retry"
                        );
                    }
                    loaded_secrets.insert(secret_key.clone(), value);
                    last_error = None;
                    break;
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let delay = std::cmp::min(
                            INITIAL_DELAY_SECS * 2u64.pow(attempt),
                            MAX_DELAY_SECS,
                        );
                        warn!(
                            secret_name = %secret_key,
                            attempt = attempt + 1,
                            max_retries = MAX_RETRIES,
                            delay_secs = delay,
                            error = %e,
                            "Failed to fetch secret, retrying (IAM propagation may be pending)"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    }
                    last_error = Some(e);
                }
            }
        }

        if let Some(e) = last_error {
            return Err(AlienError::from(e)).context(ErrorData::SecretLoadFailed {
                secret_name: secret_key.clone(),
                message: format!("Failed to fetch secret '{}' from vault after {} retries", secret_key, MAX_RETRIES),
            });
        }

        debug!(
            secret_name = %secret_key,
            "Secret loaded from vault"
        );
    }

    info!(
        count = loaded_secrets.len(),
        "Successfully loaded {} secret(s) from vault",
        loaded_secrets.len()
    );

    Ok(loaded_secrets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_alien_secrets_config() {
        let json = r#"{"keys":["API_KEY","DATABASE_PASSWORD"],"hash":"a3f2c1d5"}"#;
        let config: AlienSecretsConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.keys.len(), 2);
        assert_eq!(config.keys[0], "API_KEY");
        assert_eq!(config.keys[1], "DATABASE_PASSWORD");
        assert_eq!(config.hash, "a3f2c1d5");
    }

    #[test]
    fn test_parse_alien_secrets_empty_keys() {
        let json = r#"{"keys":[],"hash":"empty"}"#;
        let config: AlienSecretsConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.keys.len(), 0);
        assert_eq!(config.hash, "empty");
    }
}
