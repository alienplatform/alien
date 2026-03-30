use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use serde::Deserialize;
use std::collections::HashMap;

// Re-export types from alien-core
pub use alien_core::{
    AzureClientConfig, AzureCredentials, AzureImpersonationConfig,
    AzureServiceOverrides as ServiceOverrides,
};

pub mod authorization;
pub mod blob_containers;
pub mod common;
pub mod compute;
pub mod container_apps;
pub mod containerregistry;
pub mod disks;
pub mod keyvault;
pub mod load_balancers;
pub mod long_running_operation;
pub mod managed_identity;
pub mod models;
pub mod network;
pub mod resources;
pub mod service_bus;
pub mod storage_accounts;
pub mod tables;
pub mod token_cache;

/// Get a bearer token using Azure AD Workload Identity (federated identity)
async fn get_workload_identity_token(
    client_id: &str,
    tenant_id: &str,
    federated_token_file: &str,
    authority_host: &str,
    scope: &str,
) -> Result<String> {
    use reqwest::Client;
    use std::collections::HashMap;

    // Read the federated token from the file
    let federated_token = std::fs::read_to_string(federated_token_file)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!(
                "Failed to read federated token file: {}",
                federated_token_file
            ),
            errors: None,
        })?
        .trim()
        .to_string();

    let client = Client::new();
    let token_url = format!(
        "{}{}/oauth2/v2.0/token",
        authority_host.trim_end_matches('/'),
        tenant_id
    );

    let mut form_data = HashMap::new();
    form_data.insert("grant_type", "client_credentials");
    form_data.insert("client_id", client_id);
    form_data.insert(
        "client_assertion_type",
        "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
    );
    form_data.insert("client_assertion", &federated_token);
    form_data.insert("scope", scope);

    let response = client
        .post(&token_url)
        .form(&form_data)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to request Azure access token using workload identity".to_string(),
        })?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::AuthenticationError {
            message: format!(
                "Failed to get workload identity access token: {}",
                error_text
            ),
        }));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_response: TokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to parse Azure workload identity token response".to_string(),
            })?;

    Ok(token_response.access_token)
}

/// Get an access token for impersonating the specified client ID
async fn get_impersonated_token(
    config: &AzureClientConfig,
    impersonation_config: &AzureImpersonationConfig,
) -> Result<String> {
    use reqwest::Client;
    use std::collections::HashMap;

    // Note: This is a simplified implementation. In a production environment,
    // you would need to implement proper Azure AD token exchange or managed identity flows.
    // For now, this demonstrates the concept by using the current credentials to get a token
    // with the specified scope and client context.

    match &config.credentials {
        AzureCredentials::AccessToken { .. } => {
            // If we already have an access token, we can't directly impersonate
            // In practice, you'd need to use Azure AD's on-behalf-of flow
            Err(AlienError::new(ErrorData::InvalidInput {
                message: "Cannot impersonate using an existing access token. Use service principal credentials instead.".to_string(),
                field_name: Some("credentials".to_string()),
            }))
        }
        AzureCredentials::WorkloadIdentity {
            client_id: _,
            tenant_id: _,
            federated_token_file,
            authority_host: _,
        } => {
            let oidc_token = std::fs::read_to_string(federated_token_file)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!(
                        "Failed to read federated token file for impersonation: {}",
                        federated_token_file
                    ),
                    errors: None,
                })?
                .trim()
                .to_string();

            let tenant_id = impersonation_config
                .tenant_id
                .as_ref()
                .unwrap_or(&config.tenant_id);

            let token_url = format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                tenant_id
            );

            let client = Client::new();
            let mut form_data = HashMap::new();
            form_data.insert("grant_type", "client_credentials".to_string());
            form_data.insert("client_id", impersonation_config.client_id.clone());
            form_data.insert(
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_string(),
            );
            form_data.insert("client_assertion", oidc_token);
            form_data.insert("scope", impersonation_config.scope.clone());

            let response = client
                .post(&token_url)
                .form(&form_data)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to exchange OIDC token for impersonation".to_string(),
                })?;

            if !response.status().is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AlienError::new(ErrorData::AuthenticationError {
                    message: format!(
                        "OIDC token exchange for impersonation failed: {}",
                        error_text
                    ),
                }));
            }

            #[derive(Deserialize)]
            struct TokenResponse {
                access_token: String,
            }

            let token_response: TokenResponse =
                response
                    .json()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Failed to parse OIDC impersonation token response".to_string(),
                    })?;

            Ok(token_response.access_token)
        }
        AzureCredentials::ManagedIdentity { .. } => {
            // Managed identity already represents the target identity
            Err(AlienError::new(ErrorData::InvalidInput {
                message: "Cannot impersonate using managed identity. The managed identity itself is the impersonated identity.".to_string(),
                field_name: Some("credentials".to_string()),
            }))
        }
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => {
            // Use service principal to get token for the target identity
            let client = Client::new();
            let tenant_id = impersonation_config
                .tenant_id
                .as_ref()
                .unwrap_or(&config.tenant_id);

            let mut form_data = HashMap::new();
            form_data.insert("grant_type", "client_credentials");
            form_data.insert("client_id", client_id);
            form_data.insert("client_secret", client_secret);
            form_data.insert("scope", &impersonation_config.scope);

            // In a real implementation, you might use different grant types
            // like "urn:ietf:params:oauth:grant-type:jwt-bearer" for impersonation

            let token_url = format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                tenant_id
            );

            let response = client
                .post(&token_url)
                .form(&form_data)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to request Azure access token for impersonation".to_string(),
                })?;

            if !response.status().is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AlienError::new(ErrorData::AuthenticationError {
                    message: format!("Failed to get impersonated access token: {}", error_text),
                }));
            }

            #[derive(Deserialize)]
            struct TokenResponse {
                access_token: String,
            }

            let token_response: TokenResponse =
                response
                    .json()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Failed to parse Azure token response".to_string(),
                    })?;

            Ok(token_response.access_token)
        }
    }
}

/// Extract the caller's object ID (oid) from an Azure JWT access token.
/// Azure access tokens are JWTs — we decode the payload to read the `oid` claim.
pub fn extract_oid_from_token(token: &str) -> Result<String> {
    use base64::Engine;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Azure access token is not a valid JWT (expected 3 parts)".to_string(),
            field_name: None,
        }));
    }

    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| {
            AlienError::new(ErrorData::InvalidInput {
                message: format!("Failed to base64-decode Azure JWT payload: {}", e),
                field_name: None,
            })
        })?;

    #[derive(Deserialize)]
    struct JwtClaims {
        oid: Option<String>,
    }

    let claims: JwtClaims = serde_json::from_slice(&payload_bytes).map_err(|e| {
        AlienError::new(ErrorData::InvalidInput {
            message: format!("Failed to parse Azure JWT payload: {}", e),
            field_name: None,
        })
    })?;

    claims.oid.ok_or_else(|| {
        AlienError::new(ErrorData::InvalidInput {
            message: "Azure JWT does not contain 'oid' claim".to_string(),
            field_name: None,
        })
    })
}

/// Trait for Azure platform configuration operations
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait AzureClientConfigExt {
    /// Create a new `AzureClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>)
        -> Result<AzureClientConfig>;

    /// Create a new `AzureClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<AzureClientConfig>;

    /// Impersonate an Azure managed identity and return a new platform config with impersonated credentials
    async fn impersonate(&self, config: AzureImpersonationConfig) -> Result<AzureClientConfig>;

    /// Gets a bearer token for Azure API authentication with default Resource Manager scope
    async fn get_bearer_token(&self) -> Result<String>;

    /// Gets a bearer token for Azure API authentication with a specific scope
    async fn get_bearer_token_with_scope(&self, scope: &str) -> Result<String>;

    /// Gets the Azure resource management endpoint URL
    fn management_endpoint(&self) -> &str;

    /// Gets the Azure Storage blob endpoint for a given storage account
    fn storage_blob_endpoint(&self, storage_account_name: &str) -> String;

    /// Gets the Azure Storage queue endpoint for a given storage account
    fn storage_queue_endpoint(&self, storage_account_name: &str) -> String;

    /// Gets the Azure Storage table endpoint for a given storage account
    fn storage_table_endpoint(&self, storage_account_name: &str) -> String;

    /// Get the endpoint for a specific service, with override support (returns Option)
    fn get_service_endpoint(&self, service_name: &str) -> Option<&str>;

    /// Create a config with service endpoint overrides for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn with_service_overrides(self, overrides: ServiceOverrides) -> Self;

    /// Create a mock AzureClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self;
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AzureClientConfigExt for AzureClientConfig {
    /// Create a new `AzureClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<Self> {
        let credentials = if let Some(token) = environment_variables.get("AZURE_ACCESS_TOKEN") {
            AzureCredentials::AccessToken {
                token: token.clone(),
            }
        } else if let (Some(client_id), Some(federated_token_file)) = (
            environment_variables.get("AZURE_CLIENT_ID"),
            environment_variables.get("AZURE_FEDERATED_TOKEN_FILE"),
        ) {
            // Azure AD Workload Identity
            let tenant_id = environment_variables
                .get("AZURE_TENANT_ID")
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message:
                            "Missing AZURE_TENANT_ID environment variable for workload identity"
                                .to_string(),
                        errors: None,
                    })
                })?;
            let authority_host = environment_variables
                .get("AZURE_AUTHORITY_HOST")
                .cloned()
                .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string());

            AzureCredentials::WorkloadIdentity {
                client_id: client_id.clone(),
                tenant_id: tenant_id.clone(),
                federated_token_file: federated_token_file.clone(),
                authority_host,
            }
        } else if let (Some(client_id), Some(client_secret)) = (
            environment_variables.get("AZURE_CLIENT_ID"),
            environment_variables.get("AZURE_CLIENT_SECRET"),
        ) {
            AzureCredentials::ServicePrincipal {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
            }
        } else if let (Some(client_id), Some(identity_endpoint), Some(identity_header)) = (
            environment_variables.get("AZURE_CLIENT_ID"),
            environment_variables.get("IDENTITY_ENDPOINT"),
            environment_variables.get("IDENTITY_HEADER"),
        ) {
            // Azure Container Apps / App Service managed identity
            AzureCredentials::ManagedIdentity {
                client_id: client_id.clone(),
                identity_endpoint: identity_endpoint.clone(),
                identity_header: identity_header.clone(),
            }
        } else {
            return Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Missing Azure credentials environment variables. Provide one of: AZURE_ACCESS_TOKEN, AZURE_CLIENT_ID+AZURE_CLIENT_SECRET, AZURE_CLIENT_ID+AZURE_FEDERATED_TOKEN_FILE, or AZURE_CLIENT_ID+IDENTITY_ENDPOINT+IDENTITY_HEADER".to_string(),
                errors: None,
            }));
        };

        Ok(Self {
            subscription_id: environment_variables
                .get("AZURE_SUBSCRIPTION_ID")
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "Missing AZURE_SUBSCRIPTION_ID environment variable".to_string(),
                        errors: None,
                    })
                })?
                .clone(),
            tenant_id: environment_variables
                .get("AZURE_TENANT_ID")
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "Missing AZURE_TENANT_ID environment variable".to_string(),
                        errors: None,
                    })
                })?
                .clone(),
            region: environment_variables.get("AZURE_REGION").cloned(),
            credentials,
            service_overrides: if let Some(endpoints_json) =
                environment_variables.get("AZURE_SERVICE_OVERRIDES_ENDPOINTS")
            {
                let endpoints: HashMap<String, String> = serde_json::from_str(endpoints_json)
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: "Failed to parse AZURE_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
                        errors: None,
                    })?;
                Some(ServiceOverrides { endpoints })
            } else {
                None
            },
        })
    }

    /// Create a new `AzureClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<Self> {
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_env(&env_vars).await
    }

    /// Impersonate an Azure managed identity and return a new platform config with impersonated credentials
    ///
    /// Note: This implementation assumes the current service principal has permission to obtain tokens
    /// for the target managed identity. In practice, you would need appropriate RBAC permissions.
    async fn impersonate(&self, config: AzureImpersonationConfig) -> Result<AzureClientConfig> {
        // For Azure impersonation, we need to get an access token for the target identity
        // This typically involves using the current credentials to request a token on behalf of the target identity

        let token = get_impersonated_token(self, &config).await?;

        // Use target overrides when provided (cross-subscription impersonation).
        Ok(AzureClientConfig {
            subscription_id: config
                .target_subscription_id
                .unwrap_or_else(|| self.subscription_id.clone()),
            tenant_id: config.tenant_id.unwrap_or_else(|| self.tenant_id.clone()),
            region: config
                .target_region
                .or_else(|| self.region.clone()),
            credentials: AzureCredentials::AccessToken { token },
            service_overrides: self.service_overrides.clone(),
        })
    }

    /// Gets a bearer token for Azure API authentication with default Resource Manager scope
    async fn get_bearer_token(&self) -> Result<String> {
        self.get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
    }

    /// Gets a bearer token for Azure API authentication with a specific scope
    async fn get_bearer_token_with_scope(&self, scope: &str) -> Result<String> {
        match &self.credentials {
            AzureCredentials::AccessToken { token } => Ok(token.clone()),
            AzureCredentials::WorkloadIdentity {
                client_id,
                tenant_id,
                federated_token_file,
                authority_host,
            } => {
                get_workload_identity_token(
                    client_id,
                    tenant_id,
                    federated_token_file,
                    authority_host,
                    scope,
                )
                .await
            }
            AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } => {
                #[derive(Deserialize)]
                struct TokenResponse {
                    access_token: String,
                }

                let token_url = format!(
                    "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                    self.tenant_id
                );

                // Create the form data for the token request
                let form_data = [
                    ("grant_type", "client_credentials"),
                    ("client_id", client_id),
                    ("client_secret", client_secret),
                    ("scope", scope),
                ];

                let client = reqwest::Client::new();
                let response = client
                    .post(&token_url)
                    .form(&form_data)
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::AuthenticationError {
                        message: format!(
                            "Failed to get Azure service principal token for scope '{}'",
                            scope
                        ),
                    })?;

                let token_response: TokenResponse =
                    response.json().await.into_alien_error().context(
                        ErrorData::AuthenticationError {
                            message: format!(
                            "Failed to parse Azure service principal token response for scope '{}'",
                            scope
                        ),
                        },
                    )?;

                Ok(token_response.access_token)
            }
            AzureCredentials::ManagedIdentity {
                client_id,
                identity_endpoint,
                identity_header,
            } => {
                #[derive(Deserialize)]
                struct TokenResponse {
                    access_token: String,
                }

                // Managed identity uses "resource" (not "scope"), strip ".default" suffix
                let resource = scope.trim_end_matches("/.default");

                let client = reqwest::Client::new();
                let response = client
                    .get(identity_endpoint)
                    .query(&[
                        ("resource", resource),
                        ("api-version", "2019-08-01"),
                        ("client_id", client_id),
                    ])
                    .header("X-IDENTITY-HEADER", identity_header)
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::AuthenticationError {
                        message: format!(
                            "Failed to get Azure managed identity token for resource '{}'",
                            resource
                        ),
                    })?;

                let token_response: TokenResponse = response
                    .json()
                    .await
                    .into_alien_error()
                    .context(ErrorData::AuthenticationError {
                    message: format!(
                        "Failed to parse Azure managed identity token response for resource '{}'",
                        resource
                    ),
                })?;

                Ok(token_response.access_token)
            }
        }
    }

    /// Gets the Azure resource management endpoint URL
    fn management_endpoint(&self) -> &str {
        if let Some(override_url) = self.get_service_endpoint("management") {
            override_url
        } else {
            "https://management.azure.com"
        }
    }

    /// Gets the Azure Storage blob endpoint for a given storage account
    fn storage_blob_endpoint(&self, storage_account_name: &str) -> String {
        if let Some(override_url) = self.get_service_endpoint("storage") {
            format!("{}/blob", override_url.trim_end_matches('/'))
        } else {
            format!("https://{}.blob.core.windows.net", storage_account_name)
        }
    }

    /// Gets the Azure Storage queue endpoint for a given storage account
    fn storage_queue_endpoint(&self, storage_account_name: &str) -> String {
        if let Some(override_url) = self.get_service_endpoint("storage") {
            format!("{}/queue", override_url.trim_end_matches('/'))
        } else {
            format!("https://{}.queue.core.windows.net", storage_account_name)
        }
    }

    /// Gets the Azure Storage table endpoint for a given storage account
    fn storage_table_endpoint(&self, storage_account_name: &str) -> String {
        if let Some(override_url) = self.get_service_endpoint("storage") {
            format!("{}/table", override_url.trim_end_matches('/'))
        } else {
            format!("https://{}.table.core.windows.net", storage_account_name)
        }
    }

    /// Get the endpoint for a specific service, with override support
    fn get_service_endpoint(&self, service_name: &str) -> Option<&str> {
        self.service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get(service_name))
            .map(|s| s.as_str())
    }

    /// Create a config with service endpoint overrides for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn with_service_overrides(mut self, overrides: ServiceOverrides) -> Self {
        self.service_overrides = Some(overrides);
        self
    }

    /// Create a mock Azure platform config with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self {
        Self {
            subscription_id: "12345678-1234-1234-1234-123456789012".to_string(),
            tenant_id: "87654321-4321-4321-4321-210987654321".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "mock_access_token_for_testing".to_string(),
            },
            service_overrides: None,
        }
    }
}
