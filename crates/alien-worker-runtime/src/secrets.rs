use crate::error::{ErrorData, Result};
use alien_core::DeployerSecretEnv;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Env var with the secrets list a protected install's verifier writes from
/// the signed workload. Same format as `ALIEN_SECRETS`, which unprotected
/// installs keep; a workload has one or the other.
pub const ENV_WORKLOAD_SECRETS: &str = "WORKLOAD_SECRETS";

/// The secrets list the workload was started with, and the env var it came
/// from: `config_env` first, then the process env. Both `ALIEN_SECRETS` and
/// `WORKLOAD_SECRETS` being set is refused, so a list added beside the one the
/// install wrote can't add secrets.
pub fn secrets_list(
    config_env: &HashMap<String, String>,
    process_env: &HashMap<String, String>,
) -> Result<Option<(&'static str, String)>> {
    let mut found = None;
    for name in [alien_core::ENV_ALIEN_SECRETS, ENV_WORKLOAD_SECRETS] {
        let Some(list) = config_env.get(name).or_else(|| process_env.get(name)) else {
            continue;
        };
        if let Some((other, _)) = found {
            return Err(AlienError::new(ErrorData::SecretLoadFailed {
                secret_name: name.to_string(),
                message: format!(
                    "both {other} and {name} are set; a workload has one secrets list"
                ),
            }));
        }
        found = Some((name, list.clone()));
    }
    Ok(found)
}

/// Refuse a secret that would become a loader variable such as `LD_PRELOAD`:
/// on a protected install a value in a vault must never decide what code the
/// process loads. Unprotected installs keep loading these names as before.
fn refuse_loader_name(name: &str) -> Result<()> {
    if alien_signing::release::is_loader_env(name) {
        return Err(AlienError::new(ErrorData::SecretLoadFailed {
            secret_name: name.to_string(),
            message: format!(
                "{name} changes what code the process loads, so it is never set from a vault"
            ),
        }));
    }
    Ok(())
}

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
/// * `alien_secrets_json` - The secrets list JSON string (from config.env_vars or std::env)
/// * `list_env` - The env var the list came from; a `WORKLOAD_SECRETS` list
///   (protected installs) may not name loader variables
pub async fn load_secrets_from_vault(
    bindings_provider: &dyn alien_bindings::BindingsProviderApi,
    alien_secrets_json: &str,
    list_env: &str,
) -> Result<std::collections::HashMap<String, String>> {
    // Parse the JSON
    let config: AlienSecretsConfig = serde_json::from_str(alien_secrets_json)
        .into_alien_error()
        .context(ErrorData::SecretLoadFailed {
            secret_name: "secrets list".to_string(),
            message: "Failed to parse the secrets list JSON".to_string(),
        })?;

    if list_env == ENV_WORKLOAD_SECRETS {
        for name in config
            .keys
            .iter()
            .chain(config.deployer_secrets.iter().map(|secret| &secret.name))
        {
            refuse_loader_name(name)?;
        }
    }

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

    let mut loaded_secrets = HashMap::new();
    if !config.keys.is_empty() {
        let vault = bindings_provider
            .load_vault(alien_core::SECRETS_VAULT_ID)
            .await
            .context(ErrorData::SecretLoadFailed {
                secret_name: "vault".to_string(),
                message: "Failed to load vault binding".to_string(),
            })?;

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
    }
    if !config.deployer_secrets.is_empty() {
        let vault = bindings_provider
            .load_vault(alien_core::DEPLOYER_SECRETS_VAULT_ID)
            .await
            .context(ErrorData::SecretLoadFailed {
                secret_name: alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string(),
                message: "Failed to load deployer secrets vault binding".to_string(),
            })?;
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
            alien_core::DEPLOYER_SECRETS_VAULT_ID,
            &state_dir,
        ))
        .unwrap();
        alien_bindings::BindingsProvider::new(
            alien_core::ClientConfig::Local {
                state_directory: state_dir,
            },
            HashMap::from([(alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string(), binding)]),
        )
        .unwrap()
    }

    fn deployer_secrets_json(required: bool) -> String {
        serde_json::json!({
            "keys": [],
            "hash": "h",
            "deployerSecrets": [{
                "name": "DATABASE_PASSWORD",
                "vaultKey": "database-password",
                "secretName": "database-password",
                "label": "Database password",
                "required": required,
            }],
        })
        .to_string()
    }

    #[tokio::test]
    async fn a_vault_value_named_like_a_loader_never_becomes_env_on_a_protected_install() {
        let temp = tempfile::TempDir::new().unwrap();
        let state_dir = temp.path().to_string_lossy().to_string();
        let binding = serde_json::to_value(alien_core::VaultBinding::local(
            alien_core::SECRETS_VAULT_ID,
            &state_dir,
        ))
        .unwrap();
        let provider = alien_bindings::BindingsProvider::new(
            alien_core::ClientConfig::Local {
                state_directory: state_dir,
            },
            HashMap::from([(alien_core::SECRETS_VAULT_ID.to_string(), binding)]),
        )
        .unwrap();
        let vault = alien_bindings::BindingsProviderApi::load_vault(
            &provider,
            alien_core::SECRETS_VAULT_ID,
        )
        .await
        .unwrap();
        vault.set_secret("API_KEY", "k").await.unwrap();
        vault
            .set_secret("LD_PRELOAD", "/tmp/evil.so")
            .await
            .unwrap();
        vault
            .set_secret("NODE_OPTIONS", "--require /tmp/evil.js")
            .await
            .unwrap();

        // Without a loader name the list loads.
        let loaded = load_secrets_from_vault(
            &provider,
            r#"{"keys":["API_KEY"],"hash":"h"}"#,
            ENV_WORKLOAD_SECRETS,
        )
        .await
        .unwrap();
        assert_eq!(loaded["API_KEY"], "k");

        for name in ["LD_PRELOAD", "NODE_OPTIONS"] {
            let list = serde_json::json!({"keys": ["API_KEY", name], "hash": "h"}).to_string();
            // Unprotected installs load the names as they always did.
            let loaded = load_secrets_from_vault(&provider, &list, alien_core::ENV_ALIEN_SECRETS)
                .await
                .unwrap();
            assert!(
                loaded.contains_key(name),
                "{name} loads on an unprotected install"
            );

            let error = load_secrets_from_vault(&provider, &list, ENV_WORKLOAD_SECRETS)
                .await
                .expect_err("a loader name from a vault must be refused");
            assert!(
                error.message.contains("never set from a vault"),
                "{}",
                error.message
            );
        }

        let deployer = serde_json::json!({
            "keys": [],
            "hash": "h",
            "deployerSecrets": [{
                "name": "LD_AUDIT", "vaultKey": "x", "secretName": "x", "label": "x", "required": false,
            }],
        })
        .to_string();
        let error = load_secrets_from_vault(&provider, &deployer, ENV_WORKLOAD_SECRETS)
            .await
            .expect_err("a loader name from the deployer's vault must be refused");
        assert!(error.message.contains("LD_AUDIT"), "{}", error.message);
    }

    #[test]
    fn a_workload_has_one_secrets_list() {
        let list = r#"{"keys":[],"hash":"h"}"#.to_string();
        let none = HashMap::new();
        let alien = HashMap::from([(alien_core::ENV_ALIEN_SECRETS.to_string(), list.clone())]);
        let app = HashMap::from([(ENV_WORKLOAD_SECRETS.to_string(), list.clone())]);

        assert_eq!(secrets_list(&none, &none).unwrap(), None);
        assert_eq!(
            secrets_list(&alien, &none).unwrap(),
            Some((alien_core::ENV_ALIEN_SECRETS, list.clone()))
        );
        assert_eq!(
            secrets_list(&none, &app).unwrap(),
            Some((ENV_WORKLOAD_SECRETS, list.clone()))
        );
        let error = secrets_list(&alien, &app).expect_err("two lists are refused");
        assert!(
            error.message.contains("one secrets list"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_missing_required_deployer_secret_stops_the_start() {
        let temp = tempfile::TempDir::new().unwrap();
        let provider = local_vault_provider(temp.path());

        let error = load_secrets_from_vault(
            &provider,
            &deployer_secrets_json(true),
            alien_core::ENV_ALIEN_SECRETS,
        )
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

        let loaded = load_secrets_from_vault(
            &provider,
            &deployer_secrets_json(false),
            alien_core::ENV_ALIEN_SECRETS,
        )
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
            alien_core::DEPLOYER_SECRETS_VAULT_ID,
        )
        .await
        .unwrap();

        vault
            .set_secret("database-password", "first")
            .await
            .unwrap();
        let loaded = load_secrets_from_vault(
            &provider,
            &deployer_secrets_json(true),
            alien_core::ENV_ALIEN_SECRETS,
        )
        .await
        .unwrap();
        assert_eq!(loaded["DATABASE_PASSWORD"], "first");

        vault
            .set_secret("database-password", "rotated")
            .await
            .unwrap();
        let loaded = load_secrets_from_vault(
            &provider,
            &deployer_secrets_json(true),
            alien_core::ENV_ALIEN_SECRETS,
        )
        .await
        .unwrap();
        assert_eq!(loaded["DATABASE_PASSWORD"], "rotated");
    }
}
