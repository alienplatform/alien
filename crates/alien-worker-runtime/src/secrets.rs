use crate::error::{ErrorData, Result};
use alien_core::DeployerSecretEnv;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Configuration for ALIEN_SECRETS environment variable
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlienSecretsConfig {
    /// Secret keys to load from vault
    keys: Vec<String>,
    /// Hash of all env var values - triggers redeployment when changed
    hash: String,
    /// Environment variables read from vault-native deployer secrets: values
    /// the deployer wrote into their own secret store under Alien-derived
    /// keys. Read fresh on every start, so a rotated value is picked up by
    /// the next restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deployer_secrets: Vec<DeployerSecretEnv>,
}

/// Load secrets from the vault at startup based on ALIEN_SECRETS configuration.
///
/// This function:
/// 1. Parses the JSON to get the secret keys and deployer secrets
/// 2. Loads the vault binding from the BindingsProvider
/// 3. Fetches each secret from the vault
/// 4. Returns them as a HashMap keyed by env var name (caller decides how to
///    expose them to the app)
///
/// A required deployer secret that is not in the vault fails with
/// `DeployerSecretMissing` ("missing: <label>") so the app never starts
/// without it; an optional one is left unset.
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
    let config: AlienSecretsConfig = serde_json::from_str(alien_secrets_json)
        .into_alien_error()
        .context(ErrorData::SecretLoadFailed {
            secret_name: "ALIEN_SECRETS".to_string(),
            message: "Failed to parse ALIEN_SECRETS JSON".to_string(),
        })?;

    if config.keys.is_empty() && config.deployer_secrets.is_empty() {
        debug!("ALIEN_SECRETS contains no keys, skipping secret loading");
        return Ok(HashMap::new());
    }

    info!(
        count = config.keys.len(),
        deployer_secrets = config.deployer_secrets.len(),
        hash = %config.hash,
        "Loading {} secret(s) from vault",
        config.keys.len() + config.deployer_secrets.len()
    );

    // Load the vault binding
    // The vault binding name is conventionally "secrets" (the vault resource added by SecretsVaultMutation)
    let vault = bindings_provider
        .load_vault(alien_core::SECRETS_VAULT_ID)
        .await
        .context(ErrorData::SecretLoadFailed {
            secret_name: "vault".to_string(),
            message: "Failed to load vault binding".to_string(),
        })?;

    let mut loaded_secrets = HashMap::new();
    for secret_key in &config.keys {
        let value = fetch_secret(vault.as_ref(), secret_key, true)
            .await
            .map_err(|e| {
                e.context(ErrorData::SecretLoadFailed {
                    secret_name: secret_key.clone(),
                    message: format!(
                        "Failed to fetch secret '{}' from vault after {} retries",
                        secret_key, MAX_RETRIES
                    ),
                })
            })?;
        loaded_secrets.insert(secret_key.clone(), value);
    }

    for secret in &config.deployer_secrets {
        match fetch_secret(vault.as_ref(), &secret.vault_key, false).await {
            Ok(value) => {
                loaded_secrets.insert(secret.name.clone(), value);
            }
            Err(e) if is_not_found(&e) => {
                if secret.required {
                    return Err(AlienError::new(ErrorData::DeployerSecretMissing {
                        label: secret.label.clone(),
                        secret_name: secret.secret_name.clone(),
                    }));
                }
                info!(
                    env = %secret.name,
                    secret_name = %secret.secret_name,
                    "Optional deployer secret is not set; leaving it unset"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::SecretLoadFailed {
                    secret_name: secret.secret_name.clone(),
                    message: format!(
                        "Failed to fetch deployer secret '{}' from vault after {} retries",
                        secret.label, MAX_RETRIES
                    ),
                }));
            }
        }
    }

    info!(
        count = loaded_secrets.len(),
        "Successfully loaded {} secret(s) from vault",
        loaded_secrets.len()
    );

    Ok(loaded_secrets)
}

// IAM permission propagation on cloud platforms (especially GCP) can take a
// short time after a new service account is granted access. We retry with
// exponential backoff to handle this, but keep total retry time under ~40s to
// stay well within Cloud Run's 240s startup probe timeout.
const MAX_RETRIES: u32 = 5;
const INITIAL_DELAY_SECS: u64 = 2;
const MAX_DELAY_SECS: u64 = 10;

fn is_not_found(error: &AlienError<alien_bindings::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
    )
}

/// Fetches one secret with retries. A secret the vault reports as not found
/// is returned at once unless `retry_not_found`: a deployer secret that was
/// never written will not appear by waiting.
async fn fetch_secret(
    vault: &dyn alien_bindings::Vault,
    secret_key: &str,
    retry_not_found: bool,
) -> std::result::Result<String, AlienError<alien_bindings::ErrorData>> {
    debug!(secret_name = %secret_key, "Fetching secret from vault");

    let mut attempt = 0;
    loop {
        match vault.get_secret(secret_key).await {
            Ok(value) => {
                if attempt > 0 {
                    info!(
                        secret_name = %secret_key,
                        attempt = attempt,
                        "Secret loaded from vault after retry"
                    );
                }
                debug!(secret_name = %secret_key, "Secret loaded from vault");
                return Ok(value);
            }
            Err(e) if attempt >= MAX_RETRIES || (!retry_not_found && is_not_found(&e)) => {
                return Err(e);
            }
            Err(e) => {
                let delay = std::cmp::min(INITIAL_DELAY_SECS * 2u64.pow(attempt), MAX_DELAY_SECS);
                warn!(
                    secret_name = %secret_key,
                    attempt = attempt + 1,
                    max_retries = MAX_RETRIES,
                    delay_secs = delay,
                    error = %e,
                    "Failed to fetch secret, retrying (IAM propagation may be pending)"
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                attempt += 1;
            }
        }
    }
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

    fn local_vault_provider(state_dir: &std::path::Path) -> alien_bindings::BindingsProvider {
        let state_dir = state_dir.to_string_lossy().to_string();
        let binding = serde_json::to_value(alien_core::VaultBinding::local(
            alien_core::SECRETS_VAULT_ID,
            &state_dir,
        ))
        .unwrap();
        alien_bindings::BindingsProvider::new(
            alien_core::ClientConfig::Local {
                state_directory: state_dir,
            },
            HashMap::from([(alien_core::SECRETS_VAULT_ID.to_string(), binding)]),
        )
        .unwrap()
    }

    fn deployer_secrets_json(required: bool) -> String {
        serde_json::json!({
            "keys": [],
            "hash": "h",
            "deployerSecrets": [{
                "name": "DATABASE_PASSWORD",
                "vaultKey": "input-database-password",
                "secretName": "input-database-password",
                "label": "Database password",
                "required": required,
            }],
        })
        .to_string()
    }

    #[tokio::test]
    async fn a_missing_required_deployer_secret_stops_the_start() {
        let temp = tempfile::TempDir::new().unwrap();
        let provider = local_vault_provider(temp.path());

        let error = load_secrets_from_vault(&provider, &deployer_secrets_json(true))
            .await
            .expect_err("start must fail without the secret");

        assert_eq!(error.code, "DEPLOYER_SECRET_MISSING");
        assert!(
            error.message.starts_with("missing: Database password"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_missing_optional_deployer_secret_stays_unset() {
        let temp = tempfile::TempDir::new().unwrap();
        let provider = local_vault_provider(temp.path());

        let loaded = load_secrets_from_vault(&provider, &deployer_secrets_json(false))
            .await
            .unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn each_start_reads_the_deployer_secret_as_written_now() {
        let temp = tempfile::TempDir::new().unwrap();
        let provider = local_vault_provider(temp.path());
        let vault = alien_bindings::BindingsProviderApi::load_vault(
            &provider,
            alien_core::SECRETS_VAULT_ID,
        )
        .await
        .unwrap();

        vault
            .set_secret("input-database-password", "first")
            .await
            .unwrap();
        let loaded = load_secrets_from_vault(&provider, &deployer_secrets_json(true))
            .await
            .unwrap();
        assert_eq!(loaded["DATABASE_PASSWORD"], "first");

        vault
            .set_secret("input-database-password", "rotated")
            .await
            .unwrap();
        let loaded = load_secrets_from_vault(&provider, &deployer_secrets_json(true))
            .await
            .unwrap();
        assert_eq!(loaded["DATABASE_PASSWORD"], "rotated");
    }
}
