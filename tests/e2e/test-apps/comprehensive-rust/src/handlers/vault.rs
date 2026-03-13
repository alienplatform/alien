use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use tracing::info;

use crate::{
    models::{AppState, VaultTestResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context, IntoAlienError};

/// Test vault operations with a full E2E flow
#[utoipa::path(
    post,
    path = "/vault-test/{binding_name}",
    tag = "vault",
    params(
        ("binding_name" = String, Path, description = "Name of the vault binding to test")
    ),
    responses(
        (status = 200, description = "Vault test completed", body = VaultTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Vault operation failed", body = AlienError),
    ),
    operation_id = "test_vault",
    summary = "Test vault operations",
    description = "Performs comprehensive E2E testing of vault operations: set secret, get secret, delete secret, and verify deletion"
)]
pub async fn test_vault(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<VaultTestResponse>> {
    info!(%binding_name, "Received vault test request");

    let test_secret_name = format!(
        "test_server_vault_test_{}_{}",
        binding_name,
        Utc::now().timestamp_millis()
    );
    let test_secret_value = "test-secret-value-from-alien-test-server";

    let vault_instance = app_state
        .ctx
        .get_bindings()
        .load_vault(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // 1. Set the secret
    info!(%test_secret_name, "Setting secret");
    vault_instance
        .set_secret(&test_secret_name, test_secret_value)
        .await
        .into_alien_error()
        .context(ErrorData::VaultOperationFailed {
            operation: "set_secret".to_string(),
        })?;

    // 2. Short sleep to ensure secret is fully available (especially for cloud providers)
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 3. Get the secret and verify
    info!(%test_secret_name, "Getting secret and verifying value");
    let retrieved_value = vault_instance
        .get_secret(&test_secret_name)
        .await
        .into_alien_error()
        .context(ErrorData::VaultOperationFailed {
            operation: "get_secret".to_string(),
        })?;

    if retrieved_value != test_secret_value {
        return Err(AlienError::new(ErrorData::VaultOperationFailed {
            operation: "secret_value_verification".to_string(),
        }));
    }

    // 4. Delete the secret
    info!(%test_secret_name, "Deleting secret");
    vault_instance
        .delete_secret(&test_secret_name)
        .await
        .into_alien_error()
        .context(ErrorData::VaultOperationFailed {
            operation: "delete_secret".to_string(),
        })?;

    // 5. Short sleep to ensure deletion is fully propagated (especially for cloud providers)
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 6. Try to get the secret again - should return not found error
    info!(%test_secret_name, "Verifying secret deletion");
    match vault_instance.get_secret(&test_secret_name).await {
        Err(_) => {
            // This is what we expect after deletion - any error indicates the secret is not found
            info!(%test_secret_name, "Secret successfully deleted - get_secret returned error as expected");
        }
        Ok(value) => {
            return Err(AlienError::new(ErrorData::VaultOperationFailed {
                operation: format!(
                    "delete_verification - secret still exists with value: {}",
                    value
                ),
            }));
        }
    }

    info!(%test_secret_name, "Vault test completed successfully");

    Ok(Json(VaultTestResponse {
        binding_name,
        success: true,
    }))
}

/// Get an external secret (pre-populated by test framework)
#[utoipa::path(
    get,
    path = "/external-secret",
    tag = "vault",
    responses(
        (status = 200, description = "External secret retrieved", body = serde_json::Value),
        (status = 500, description = "Failed to retrieve secret", body = AlienError),
    ),
    operation_id = "get_external_secret",
    summary = "Get external secret",
    description = "Tests reading an external secret that was set using platform-native tools (SSM/Secret Manager/Key Vault/K8s Secrets)"
)]
pub async fn get_external_secret(
    State(app_state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    info!("Attempting to read EXTERNAL_TEST_SECRET from test-alien-vault");

    let vault_instance = app_state
        .ctx
        .get_bindings()
        .load_vault("test-alien-vault")
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: "test-alien-vault".to_string(),
        })?;

    match vault_instance.get_secret("EXTERNAL_TEST_SECRET").await {
        Ok(value) => {
            info!("Successfully read external secret");
            Ok(Json(serde_json::json!({
                "exists": true,
                "value": value,
            })))
        }
        Err(_) => {
            info!("External secret not found");
            Ok(Json(serde_json::json!({
                "exists": false,
            })))
        }
    }
}
