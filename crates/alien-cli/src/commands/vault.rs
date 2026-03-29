//! Vault commands for managing secrets.
//!
//! Two entry points:
//! - `alien dev vault` — local dev mode, reads/writes local filesystem
//! - `alien vault` — standalone/platform mode, calls manager vault API

use crate::{
    error::{ErrorData, Result},
    get_current_dir,
};
use alien_bindings::providers::vault::LocalVault;
use alien_bindings::traits::Vault as VaultTrait;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage vault secrets for local dev deployments",
    long_about = "Manage vault secrets for local development deployments.

Vaults store sensitive data like API keys, tokens, and credentials that your
functions need at runtime. Each deployment has isolated vault state.

EXAMPLES:
    # Set a secret in the default deployment
    alien dev vault set customer-secrets GITHUB_TOKEN ghp_xxx
    
    # Get a secret
    alien dev vault get customer-secrets GITHUB_TOKEN
    
    # List all secrets in a vault
    alien dev vault list customer-secrets
    
    # Manage secrets for a specific deployment
    alien dev vault --deployment my-deployment set vault-name KEY value

VAULT STRUCTURE:
    .alien/deployments/{deployment-id}/vault/{vault-name}/secrets.json

See also: https://alien.dev/docs/vaults"
)]
pub struct VaultArgs {
    #[command(subcommand)]
    pub action: VaultAction,

    /// Target deployment name (default: "default")
    #[arg(long, default_value = "default")]
    pub deployment: String,

    /// State directory (default: .alien)
    #[arg(long, default_value = ".alien")]
    pub state_dir: String,
}

#[derive(Subcommand, Debug, Clone)]
pub enum VaultAction {
    /// Set a secret value in a vault
    Set {
        /// Vault name (e.g., "customer-secrets")
        vault_name: String,
        /// Secret name (e.g., "GITHUB_TOKEN")
        secret_name: String,
        /// Secret value
        value: String,
    },
    /// Get a secret value from a vault
    Get {
        /// Vault name
        vault_name: String,
        /// Secret name
        secret_name: String,
    },
    /// List all secrets in a vault
    List {
        /// Vault name
        vault_name: String,
    },
}

/// Execute vault command (dev mode only)
pub async fn vault_task(args: VaultArgs, port: u16) -> Result<()> {
    // Ensure dev server is running (deployments are registered there)
    crate::commands::ensure_server_running(port).await?;

    // Get deployment ID by name
    let deployment_id = get_deployment_id_by_name(&args.deployment, port).await?;

    let current_dir = get_current_dir()?;
    let state_dir = current_dir.join(&args.state_dir);
    let vault_path = state_dir
        .join("deployments")
        .join(&deployment_id)
        .join("vault");

    // Ensure vault directory exists
    std::fs::create_dir_all(&vault_path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: vault_path.display().to_string(),
            reason: "Failed to create vault directory".to_string(),
        })?;

    match args.action {
        VaultAction::Set {
            vault_name,
            secret_name,
            value,
        } => {
            set_secret(&vault_name, &secret_name, &value, &vault_path).await?;
            println!(
                "✅ Secret '{}' set in vault '{}' for deployment '{}'",
                secret_name, vault_name, args.deployment
            );
        }
        VaultAction::Get {
            vault_name,
            secret_name,
        } => {
            let value = get_secret(&vault_name, &secret_name, &vault_path).await?;
            println!("{}", value);
        }
        VaultAction::List { vault_name } => {
            let secrets = list_secrets(&vault_name, &vault_path).await?;
            if secrets.is_empty() {
                info!("No secrets in vault '{}'", vault_name);
            } else {
                for key in secrets {
                    println!("{}", key);
                }
            }
        }
    }

    Ok(())
}

/// Get deployment ID by name using the dev server API
async fn get_deployment_id_by_name(deployment_name: &str, port: u16) -> Result<String> {
    let sdk = alien_platform_api::Client::new(&format!("http://localhost:{}", port));

    let response = sdk
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list deployments from dev server".to_string(),
            url: None,
        })?;

    let deployments = response.into_inner();
    let deployment = deployments
        .items
        .iter()
        .find(|d| d.name.as_str() == deployment_name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "deployment".to_string(),
                message: format!(
                    "Deployment '{}' not found. Create it first with 'alien dev'.",
                    deployment_name
                ),
            })
        })?;

    Ok((*deployment.id).clone())
}

/// Set a secret in a vault
async fn set_secret(
    vault_name: &str,
    secret_name: &str,
    value: &str,
    vault_base_path: &PathBuf,
) -> Result<()> {
    let vault_path = vault_base_path.join(vault_name);
    std::fs::create_dir_all(&vault_path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: vault_path.display().to_string(),
            reason: format!("Failed to create vault directory for '{}'", vault_name),
        })?;

    let vault = LocalVault::new(vault_name.to_string(), vault_path);
    vault
        .set_secret(secret_name, value)
        .await
        .into_alien_error()
        .context(ErrorData::LocalServiceFailed {
            service: "vault".to_string(),
            reason: format!(
                "Failed to set secret '{}' in vault '{}'",
                secret_name, vault_name
            ),
        })?;

    Ok(())
}

/// Get a secret from a vault
async fn get_secret(
    vault_name: &str,
    secret_name: &str,
    vault_base_path: &PathBuf,
) -> Result<String> {
    let vault_path = vault_base_path.join(vault_name);

    if !vault_path.exists() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "vault".to_string(),
            message: format!("Vault '{}' does not exist", vault_name),
        }));
    }

    let vault = LocalVault::new(vault_name.to_string(), vault_path);
    let value: String = vault
        .get_secret(secret_name)
        .await
        .into_alien_error()
        .context(ErrorData::LocalServiceFailed {
            service: "vault".to_string(),
            reason: format!(
                "Failed to get secret '{}' from vault '{}'",
                secret_name, vault_name
            ),
        })?;

    Ok(value)
}

/// List all secrets in a vault
async fn list_secrets(vault_name: &str, vault_base_path: &PathBuf) -> Result<Vec<String>> {
    let vault_path = vault_base_path.join(vault_name);
    let secrets_file = vault_path.join("secrets.json");

    if !secrets_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&secrets_file)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: secrets_file.display().to_string(),
            reason: "Failed to read secrets file".to_string(),
        })?;

    let secrets: HashMap<String, String> = serde_json::from_str(&content)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse secrets file at {}", secrets_file.display()),
        })?;

    let mut keys: Vec<String> = secrets.keys().cloned().collect();
    keys.sort();
    Ok(keys)
}

// ---------------------------------------------------------------------------
// Remote vault command (standalone / platform mode)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage vault secrets for a deployment",
    long_about = "Manage vault secrets for a deployment via the manager API.

Vaults store sensitive data like API keys, tokens, and credentials that your
functions need at runtime. Each deployment has isolated vault state backed by
the cloud provider's secret management service (AWS SSM, GCP Secret Manager,
Azure Key Vault).

EXAMPLES:
    # Set a secret
    alien vault set --deployment my-deployment customer-secrets GITHUB_TOKEN ghp_xxx

    # Get a secret
    alien vault get --deployment my-deployment customer-secrets GITHUB_TOKEN

See also: https://alien.dev/docs/vaults"
)]
pub struct VaultRemoteArgs {
    #[command(subcommand)]
    pub action: VaultAction,

    /// Target deployment ID or name
    #[arg(long)]
    pub deployment: String,
}

/// Execute vault command via the manager API (standalone/platform mode).
pub async fn vault_remote_task(
    args: VaultRemoteArgs,
    ctx: crate::execution_context::ExecutionMode,
) -> Result<()> {
    let manager_url = ctx.manager_url();
    let http = ctx.auth_http().await?.client;

    // Resolve deployment ID: if the user passed a name, look it up.
    let deployment_id = resolve_deployment_id(&args.deployment, &http, &manager_url).await?;

    match args.action {
        VaultAction::Set {
            vault_name,
            secret_name,
            value,
        } => {
            let url = format!(
                "{}/v1/deployments/{}/vault/{}/secrets/{}",
                manager_url, deployment_id, vault_name, secret_name,
            );
            let resp = http
                .put(&url)
                .json(&serde_json::json!({ "value": value }))
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to set vault secret".to_string(),
                    url: Some(url.clone()),
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!("Failed to set secret ({status}): {body}"),
                    url: Some(url),
                }));
            }

            println!(
                "Secret '{}' set in vault '{}' for deployment '{}'",
                secret_name, vault_name, args.deployment,
            );
        }
        VaultAction::Get {
            vault_name,
            secret_name,
        } => {
            let url = format!(
                "{}/v1/deployments/{}/vault/{}/secrets/{}",
                manager_url, deployment_id, vault_name, secret_name,
            );
            let resp = http.get(&url).send().await.into_alien_error().context(
                ErrorData::ApiRequestFailed {
                    message: "Failed to get vault secret".to_string(),
                    url: Some(url.clone()),
                },
            )?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!("Failed to get secret ({status}): {body}"),
                    url: Some(url),
                }));
            }

            let body: serde_json::Value =
                resp.json()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ApiRequestFailed {
                        message: "Failed to parse vault secret response".to_string(),
                        url: Some(url),
                    })?;

            if let Some(value) = body.get("value").and_then(|v| v.as_str()) {
                println!("{}", value);
            } else {
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: "Secret response missing 'value' field".to_string(),
                    url: None,
                }));
            }
        }
        VaultAction::List { vault_name: _ } => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "action".to_string(),
                message: "List is not supported via the manager API. Use 'get' to retrieve individual secrets.".to_string(),
            }));
        }
    }

    Ok(())
}

/// Resolve a deployment ID from a name or ID string.
///
/// If the input looks like a UUID, use it directly. Otherwise, list deployments
/// from the manager and find a match by name.
async fn resolve_deployment_id(
    name_or_id: &str,
    http: &reqwest::Client,
    manager_url: &str,
) -> Result<String> {
    // If it looks like a UUID, use it directly.
    if uuid::Uuid::try_parse(name_or_id).is_ok() {
        return Ok(name_or_id.to_string());
    }

    // Otherwise, list deployments and find by name.
    let url = format!("{}/v1/deployments", manager_url);
    let resp =
        http.get(&url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to list deployments".to_string(),
                url: Some(url.clone()),
            })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Failed to list deployments ({status}): {body}"),
            url: Some(url),
        }));
    }

    let body: serde_json::Value =
        resp.json()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to parse deployments response".to_string(),
                url: Some(url),
            })?;

    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: "Deployments response missing 'items' array".to_string(),
                url: None,
            })
        })?;

    for item in items {
        if item.get("name").and_then(|v| v.as_str()) == Some(name_or_id) {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                return Ok(id.to_string());
            }
        }
    }

    Err(AlienError::new(ErrorData::ValidationError {
        field: "deployment".to_string(),
        message: format!("Deployment '{}' not found", name_or_id),
    }))
}
