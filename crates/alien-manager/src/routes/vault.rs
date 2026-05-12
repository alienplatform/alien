//! Vault secret management endpoints.
//!
//! Allows setting and getting secrets in a deployment's vault via the manager
//! API. The manager resolves the deployment's credentials and vault
//! configuration, then delegates to the appropriate cloud vault provider.
//!
//! `PUT  /v1/deployments/{id}/vault/{vault_name}/secrets/{key}` — set a secret
//! `GET  /v1/deployments/{id}/vault/{vault_name}/secrets/{key}` — get a secret

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{bindings::VaultBinding, Platform, StackState};
use alien_error::{Context, ContextError, IntoAlienError};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::put,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
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
    caller: &crate::auth::Subject,
    deployment_id: &str,
    vault_name: &str,
) -> Result<Arc<dyn alien_bindings::traits::Vault>> {
    // 1. Look up the deployment.
    let deployment = match state
        .deployment_store
        .get_deployment(caller, deployment_id)
        .await
    {
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

    let platform = deployment.platform;

    // 3. Resolve credentials for the deployment.
    let client_config = state
        .credential_resolver
        .resolve(&deployment)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to resolve credentials for vault operation".to_string(),
        })?;

    // 4. Build a BindingsProvider with the credentials + vault binding.
    let mut bindings = HashMap::new();
    bindings.insert(
        vault_name.to_string(),
        vault_binding_params(stack_state, platform, vault_name)?,
    );

    let provider =
        BindingsProvider::new(client_config, bindings).context(ErrorData::InternalError {
            message: "Failed to create bindings provider for vault operation".to_string(),
        })?;

    // 5. Load the vault.
    let vault = provider
        .load_vault(vault_name)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to load vault".to_string(),
        })?;

    Ok(vault)
}

fn vault_binding_params(
    stack_state: &StackState,
    platform: Platform,
    vault_name: &str,
) -> Result<serde_json::Value> {
    if let Some(binding_params) = stack_state
        .resource(vault_name)
        .and_then(|resource| resource.remote_binding_params.as_ref())
    {
        return Ok(binding_params.clone());
    }

    let vault_prefix = format!("{}-{}", stack_state.resource_prefix, vault_name);
    let vault_binding = match platform {
        Platform::Aws => VaultBinding::parameter_store(&vault_prefix),
        Platform::Gcp => VaultBinding::secret_manager(&vault_prefix),
        Platform::Azure => VaultBinding::key_vault(&vault_prefix),
        other => {
            return Err(ErrorData::bad_request(format!(
                "Vault API not supported for platform: {}",
                other
            )));
        }
    };

    serde_json::to_value(vault_binding)
        .into_alien_error()
        .context(ErrorData::InternalError {
            message: "Failed to serialize vault binding".to_string(),
        })
}

async fn set_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((deployment_id, vault_name, key)): Path<(String, String, String)>,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<serde_json::Value>> {
    let subject = auth::require_auth(&state, &headers).await?;
    let deployment = state
        .deployment_store
        .get_deployment(&subject, &deployment_id)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to load deployment".to_string(),
        })?
        .ok_or_else(|| ErrorData::not_found_deployment(&deployment_id))?;
    if !state.authz.can_update_deployment(&subject, &deployment) {
        return Err(ErrorData::forbidden(
            "Access denied: cannot mutate vault for this deployment",
        ));
    }

    info!(deployment_id = %deployment_id, vault_name = %vault_name, key = %key, "Setting vault secret");

    let vault = load_vault_for_deployment(&state, &subject, &deployment_id, &vault_name).await?;

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
    let deployment = state
        .deployment_store
        .get_deployment(&subject, &deployment_id)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to load deployment".to_string(),
        })?
        .ok_or_else(|| ErrorData::not_found_deployment(&deployment_id))?;
    if !state.authz.can_read_deployment(&subject, &deployment) {
        return Err(ErrorData::forbidden(
            "Access denied: cannot read vault for this deployment",
        ));
    }

    info!(deployment_id = %deployment_id, vault_name = %vault_name, key = %key, "Getting vault secret");

    let vault = load_vault_for_deployment(&state, &subject, &deployment_id, &vault_name).await?;

    let value = vault
        .get_secret(&key)
        .await
        .context(ErrorData::InternalError {
            message: "Failed to get secret".to_string(),
        })?;

    Ok(Json(GetSecretResponse { value }))
}

#[cfg(test)]
mod tests {
    use alien_core::{bindings::VaultBinding, Resource, ResourceStatus, StackResourceState, Vault};

    use super::*;

    #[test]
    fn vault_binding_params_prefers_stack_state_binding() {
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Azure, "alien-e2e-46143711".to_string());
        let binding = VaultBinding::key_vault("alien-e2e-46143711-ali");
        let resource_state = StackResourceState::builder()
            .resource_type(Vault::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(Vault {
                id: "alien-vault".to_string(),
            }))
            .remote_binding_params(serde_json::to_value(&binding).unwrap())
            .dependencies(Vec::new())
            .build();
        stack_state
            .resources
            .insert("alien-vault".to_string(), resource_state);

        let actual = vault_binding_params(&stack_state, Platform::Azure, "alien-vault").unwrap();

        assert_eq!(actual, serde_json::to_value(binding).unwrap());
    }

    #[test]
    fn vault_binding_params_falls_back_to_legacy_synthetic_binding() {
        let stack_state =
            StackState::with_resource_prefix(Platform::Azure, "alien-e2e-46143711".to_string());

        let actual = vault_binding_params(&stack_state, Platform::Azure, "alien-vault").unwrap();

        assert_eq!(
            actual,
            serde_json::to_value(VaultBinding::key_vault("alien-e2e-46143711-alien-vault"))
                .unwrap()
        );
    }
}
