use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use tracing::{info, warn};

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
    description = "Performs E2E testing of vault set/get operations and schedules best-effort cleanup"
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
        .bindings()
        .vault(&binding_name)
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

    let cleanup_vault = vault_instance.clone();
    let cleanup_secret_name = test_secret_name.clone();
    tokio::spawn(async move {
        info!(test_secret_name = %cleanup_secret_name, "Deleting vault test secret");
        if let Err(error) = cleanup_vault.delete_secret(&cleanup_secret_name).await {
            warn!(
                test_secret_name = %cleanup_secret_name,
                error = ?error,
                "Failed to delete vault test secret during best-effort cleanup"
            );
        }
    });

    info!(%test_secret_name, "Vault test completed successfully");

    Ok(Json(VaultTestResponse {
        binding_name,
        success: true,
    }))
}

/// Get a managed test secret from Alien's internal `secrets` vault.
#[utoipa::path(
    get,
    path = "/managed-secret",
    tag = "vault",
    responses(
        (status = 200, description = "Managed secret retrieved", body = serde_json::Value),
        (status = 500, description = "Failed to retrieve secret", body = AlienError),
    ),
    operation_id = "get_managed_secret",
    summary = "Get managed secret",
    description = "Tests reading a secret seeded into Alien's internal secrets vault by the test harness"
)]
pub async fn get_managed_secret(
    State(app_state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    info!("Attempting to read MANAGED_TEST_SECRET from secrets vault");

    let vault_instance =
        app_state
            .ctx
            .bindings()
            .vault("secrets")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "secrets".to_string(),
            })?;

    match vault_instance.get_secret("MANAGED_TEST_SECRET").await {
        Ok(value) => {
            info!("Successfully read managed secret");
            Ok(Json(serde_json::json!({
                "exists": true,
                "value": value,
            })))
        }
        Err(_) => {
            info!("Managed secret not found");
            Ok(Json(serde_json::json!({
                "exists": false,
            })))
        }
    }
}
