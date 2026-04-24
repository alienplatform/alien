//! Vault secret management endpoints.
//!
//! Allows setting and getting secrets in a deployment's vault via the manager
//! API. The manager resolves the deployment's credentials and vault
//! configuration, then delegates to the appropriate cloud vault provider.
//!
//! `PUT  /v1/deployments/{id}/vault/{vault_name}/secrets/{key}` — set a secret
//! `GET  /v1/deployments/{id}/vault/{vault_name}/secrets/{key}` — get a secret

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::put,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use alien_error::{Context, ContextError, IntoAlienError};
use tracing::info;

use super::auth;
use super::AppState;
use crate::error::{ErrorData, Result};

/// Request body for setting a secret.
#[derive(Debug, Deserialize)]
pub struct SetSecretRequest {
    pub value: String,
}

/// Response body for getting a secret.
#[derive(Debug, Serialize)]
pub struct GetSecretResponse {
    pub value: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/v1/deployments/{id}/vault/{vault_name}/secrets/{key}",
        put(set_secret).get(get_secret),
    )
}

/// Build a vault binding and load a vault instance for the given deployment and vault name.
async fn load_vault_for_deployment(
    state: &AppState,
    deployment_id: &str,
    vault_name: &str,
) -> Result<std::sync::Arc<dyn alien_bindings::traits::Vault>> {
    use alien_bindings::BindingsProvider;
    use std::collections::HashMap;

    // 1. Look up the deployment.
    let deployment = match state.deployment_store.get_deployment(deployment_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return Err(ErrorData::not_found_deployment(deployment_id)),
        Err(e) => {
            return Err(e.context(ErrorData::InternalError {
                message: "Failed to get deployment".to_string(),
            }))
        }
    };

    // 2. Get the resource_prefix from stack_state.
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no stack state (not yet provisioned)")
    })?;

    let resource_prefix = &stack_state.resource_prefix;
    let platform = deployment.platform;

    // 3. Construct the vault binding config based on platform.
    let vault_prefix = format!("{}-{}", resource_prefix, vault_name);

    let vault_binding = match platform {
        alien_core::Platform::Aws => {
            alien_core::bindings::VaultBinding::parameter_store(&vault_prefix)
        }
        alien_core::Platform::Gcp => {
            alien_core::bindings::VaultBinding::secret_manager(&vault_prefix)
        }
        alien_core::Platform::Azure => alien_core::bindings::VaultBinding::key_vault(&vault_prefix),
        other => {
            return Err(ErrorData::bad_request(format!(
                "Vault API not supported for platform: {}",
                other
            )));
        }
    };

    let binding_json = serde_json::to_value(&vault_binding)
        .into_alien_error()
        .context(ErrorData::InternalError {
            message: "Failed to serialize vault binding".to_string(),
        })?;

    // 4. Resolve credentials for the deployment.
    let client_config = state
        .credential_resolver
        .resolve(&deployment)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to resolve credentials for vault operation".to_string(),
        })?;

    // 5. Build a BindingsProvider with the credentials + vault binding.
    let mut bindings = HashMap::new();
    bindings.insert(vault_name.to_string(), binding_json);

    let provider = BindingsProvider::new(client_config, bindings).context(
        ErrorData::InternalError {
            message: "Failed to create bindings provider for vault operation".to_string(),
        },
    )?;

    // 6. Load the vault.
    use alien_bindings::BindingsProviderApi;
    let vault = provider.load_vault(vault_name).await.context(
        ErrorData::InternalError {
            message: "Failed to load vault".to_string(),
        },
    )?;

    Ok(vault)
}

async fn set_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((deployment_id, vault_name, key)): Path<(String, String, String)>,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<serde_json::Value>> {
    let subject = auth::require_auth(&state, &headers).await?;
    if !subject.has_full_access() && !subject.can_access_deployment(&deployment_id) {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or matching deployment token",
        ));
    }

    info!(deployment_id = %deployment_id, vault_name = %vault_name, key = %key, "Setting vault secret");

    let vault = load_vault_for_deployment(&state, &deployment_id, &vault_name).await?;

    vault
        .set_secret(&key, &body.value)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to set secret".to_string(),
        })?;

    info!(deployment_id = %deployment_id, vault_name = %vault_name, key = %key, "Vault secret set");
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((deployment_id, vault_name, key)): Path<(String, String, String)>,
) -> Result<Json<GetSecretResponse>> {
    let subject = auth::require_auth(&state, &headers).await?;
    if !subject.has_full_access() && !subject.can_access_deployment(&deployment_id) {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or matching deployment token",
        ));
    }

    info!(deployment_id = %deployment_id, vault_name = %vault_name, key = %key, "Getting vault secret");

    let vault = load_vault_for_deployment(&state, &deployment_id, &vault_name).await?;

    let value = vault
        .get_secret(&key)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to get secret".to_string(),
        })?;

    Ok(Json(GetSecretResponse { value }))
}
