use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::certificates::{CertificateBundle, CertificateImportParameters};
use crate::azure::models::keyvault::{Vault, VaultCreateOrUpdateParameters};
use crate::azure::models::secrets::{SecretBundle, SecretSetParameters, SecretUpdateParameters};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};

#[cfg(feature = "test-utils")]
use mockall::automock;

const KEY_VAULT_SCOPE: &str = "https://vault.azure.net/.default";

async fn invalidate_rejected_key_vault_token(token_cache: &AzureTokenCache, status: StatusCode) {
    if status == StatusCode::UNAUTHORIZED {
        token_cache
            .invalidate_bearer_token_with_scope(KEY_VAULT_SCOPE)
            .await;
    }
}

async fn send_key_vault_request(
    token_cache: &AzureTokenCache,
    request: RequestBuilder,
    operation: &str,
) -> Result<Response> {
    let retry_request = request.try_clone();
    let bearer_token = token_cache
        .get_bearer_token_with_scope(KEY_VAULT_SCOPE)
        .await?;
    let response = request
        .bearer_auth(bearer_token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Azure {operation}: failed to execute request"),
        })?;

    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok(response);
    }

    drop(response);
    token_cache
        .invalidate_bearer_token_with_scope(KEY_VAULT_SCOPE)
        .await;
    let retry_request = retry_request.ok_or_else(|| {
        AlienError::new(ErrorData::InvalidInput {
            message: format!("Azure {operation}: request body cannot be retried after HTTP 401"),
            field_name: None,
        })
    })?;
    let bearer_token = token_cache
        .get_bearer_token_with_scope(KEY_VAULT_SCOPE)
        .await?;
    let response = retry_request
        .bearer_auth(bearer_token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!(
                "Azure {operation}: failed to execute request after refreshing authentication"
            ),
        })?;
    invalidate_rejected_key_vault_token(token_cache, response.status()).await;
    Ok(response)
}

fn key_vault_response_error(
    status: StatusCode,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
    url: &url::Url,
) -> alien_client_core::Error {
    let http_error = AlienError::new(ErrorData::HttpResponseError {
        message: format!(
            "Azure {operation} failed for {resource_type} '{resource_name}': HTTP {status}"
        ),
        url: url.to_string(),
        http_status: status.as_u16(),
        http_request_text: None,
        http_response_text: None,
    });

    let service_error = match status {
        StatusCode::BAD_REQUEST => ErrorData::InvalidInput {
            message: format!("Bad request for {resource_type} '{resource_name}'"),
            field_name: None,
        },
        StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => {
            ErrorData::RemoteResourceConflict {
                message: "Azure reported a conflicting resource state".to_string(),
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            }
        }
        StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        },
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        },
        StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
            message: format!("Azure rate limit exceeded for {resource_type} '{resource_name}'"),
        },
        StatusCode::BAD_GATEWAY
        | StatusCode::SERVICE_UNAVAILABLE
        | StatusCode::INTERNAL_SERVER_ERROR => ErrorData::RemoteServiceUnavailable {
            message: format!("Azure service unavailable for {resource_type} '{resource_name}'"),
        },
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => ErrorData::Timeout {
            message: format!("Azure request timed out for {resource_type} '{resource_name}'"),
        },
        status if status.as_u16() == 499 => ErrorData::Timeout {
            message: format!("Azure request closed for {resource_type} '{resource_name}'"),
        },
        _ => ErrorData::GenericError {
            message: format!(
                "Azure {operation} failed for {resource_type} '{resource_name}' with HTTP {status}"
            ),
        },
    };

    http_error.context(service_error)
}

fn key_vault_parse_error(operation: &str, url: &url::Url) -> ErrorData {
    ErrorData::HttpResponseError {
        message: format!("Azure {operation}: failed to parse response JSON"),
        url: url.to_string(),
        http_status: StatusCode::OK.as_u16(),
        http_request_text: None,
        http_response_text: None,
    }
}

// -----------------------------------------------------------------------------
// Key Vault API traits
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait KeyVaultManagementApi: Send + Sync + std::fmt::Debug {
    /// Create or update a key vault
    async fn create_or_update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault>;

    /// Delete a key vault
    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()>;

    /// Get a key vault
    async fn get_vault(&self, resource_group_name: String, vault_name: String) -> Result<Vault>;

    /// Update a key vault
    async fn update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault>;
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait KeyVaultSecretsApi: Send + Sync + std::fmt::Debug {
    /// Set a secret in the key vault
    async fn set_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        parameters: SecretSetParameters,
    ) -> Result<SecretBundle>;

    /// Get a secret from the key vault
    async fn get_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        secret_version: Option<String>,
    ) -> Result<SecretBundle>;

    /// Update a secret in the key vault
    async fn update_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        secret_version: String,
        parameters: SecretUpdateParameters,
    ) -> Result<SecretBundle>;

    /// Delete a secret from the key vault
    async fn delete_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
    ) -> Result<SecretBundle>;
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait KeyVaultCertificatesApi: Send + Sync + std::fmt::Debug {
    /// Import a certificate into Key Vault
    async fn import_certificate(
        &self,
        vault_base_url: String,
        certificate_name: String,
        parameters: CertificateImportParameters,
    ) -> Result<CertificateBundle>;

    /// Delete a certificate from Key Vault
    async fn delete_certificate(
        &self,
        vault_base_url: String,
        certificate_name: String,
    ) -> Result<CertificateBundle>;
}

// -----------------------------------------------------------------------------
// Key Vault Management client
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureKeyVaultManagementClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureKeyVaultManagementClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl KeyVaultManagementApi for AzureKeyVaultManagementClient {
    /// Create or update a key vault
    async fn create_or_update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                self.token_cache.config().subscription_id,
                resource_group_name,
                vault_name
            ),
            Some(vec![("api-version", "2022-07-01".into())]),
        );

        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize VaultCreateOrUpdateParameters for vault '{}'",
                    vault_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateVault", &vault_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure CreateOrUpdateVault: failed to read response body"),
                })?;

        let vault: Vault = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: "Azure CreateOrUpdateVault: JSON parse error".to_string(),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        Ok(vault)
    }

    /// Delete a key vault
    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                self.token_cache.config().subscription_id,
                resource_group_name,
                vault_name
            ),
            Some(vec![("api-version", "2022-07-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteVault", &vault_name)
            .await?;

        Ok(())
    }

    /// Get a key vault
    async fn get_vault(&self, resource_group_name: String, vault_name: String) -> Result<Vault> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                self.token_cache.config().subscription_id,
                resource_group_name,
                vault_name
            ),
            Some(vec![("api-version", "2022-07-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetVault", &vault_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure GetVault: failed to read response body"),
                })?;

        let vault: Vault = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: "Azure GetVault: JSON parse error".to_string(),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        Ok(vault)
    }

    /// Update a key vault
    async fn update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                self.token_cache.config().subscription_id,
                resource_group_name,
                vault_name
            ),
            Some(vec![("api-version", "2022-07-01".into())]),
        );

        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize VaultCreateOrUpdateParameters for vault update '{}'",
                    vault_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "UpdateVault", &vault_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure UpdateVault: failed to read response body"),
                })?;

        let vault: Vault = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: "Azure UpdateVault: JSON parse error".to_string(),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        Ok(vault)
    }
}

// -----------------------------------------------------------------------------
// Key Vault Secrets client
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureKeyVaultSecretsClient {
    pub client: Client,
    pub token_cache: AzureTokenCache,
}

impl AzureKeyVaultSecretsClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        Self {
            client,
            token_cache,
        }
    }

    /// Build the full URL for Key Vault secrets operations
    fn build_secrets_url(
        &self,
        vault_base_url: &str,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
    ) -> Result<url::Url> {
        let base_url = if let Some(override_url) = self.token_cache.get_service_endpoint("keyvault")
        {
            override_url.trim_end_matches('/').to_string()
        } else {
            let trimmed = vault_base_url.trim_end_matches('/');
            // Add https:// protocol if not present
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                trimmed.to_string()
            } else {
                format!("https://{}", trimmed)
            }
        };

        let mut url = url::Url::parse(&format!("{}{}", base_url, path))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!("Invalid Key Vault URL: {}{}", base_url, path),
                errors: None,
            })?;

        if let Some(params) = query_params {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, &v);
            }
        }

        Ok(url)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl KeyVaultSecretsApi for AzureKeyVaultSecretsClient {
    /// Set a secret in the key vault
    async fn set_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        parameters: SecretSetParameters,
    ) -> Result<SecretBundle> {
        let url = self.build_secrets_url(
            &vault_base_url,
            &format!("/secrets/{}", secret_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize SecretSetParameters for secret '{}'",
                    secret_name
                ),
            })?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client
                .put(url.to_string())
                .header("Content-Type", "application/json")
                .body(body),
            "SetSecret",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "SetSecret",
                "Azure Key Vault Secret",
                &secret_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure SetSecret: failed to read response body"),
                })?;

        let secret: SecretBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("SetSecret", &url))?;

        Ok(secret)
    }

    /// Get a secret from the key vault
    async fn get_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        secret_version: Option<String>,
    ) -> Result<SecretBundle> {
        let path = if let Some(version) = secret_version {
            format!("/secrets/{}/{}", secret_name, version)
        } else {
            format!("/secrets/{}", secret_name)
        };

        let url = self.build_secrets_url(
            &vault_base_url,
            &path,
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client.get(url.to_string()),
            "GetSecret",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "GetSecret",
                "Azure Key Vault Secret",
                &secret_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure GetSecret: failed to read response body"),
                })?;

        let secret: SecretBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("GetSecret", &url))?;

        Ok(secret)
    }

    /// Update a secret in the key vault
    async fn update_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        secret_version: String,
        parameters: SecretUpdateParameters,
    ) -> Result<SecretBundle> {
        let url = self.build_secrets_url(
            &vault_base_url,
            &format!("/secrets/{}/{}", secret_name, secret_version),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize SecretUpdateParameters for secret '{}'",
                    secret_name
                ),
            })?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client
                .patch(url.to_string())
                .header("Content-Type", "application/json")
                .body(body),
            "UpdateSecret",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "UpdateSecret",
                "Azure Key Vault Secret",
                &secret_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure UpdateSecret: failed to read response body"),
                })?;

        let secret: SecretBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("UpdateSecret", &url))?;

        Ok(secret)
    }

    /// Delete a secret from the key vault
    async fn delete_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
    ) -> Result<SecretBundle> {
        let url = self.build_secrets_url(
            &vault_base_url,
            &format!("/secrets/{}", secret_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client.delete(url.to_string()),
            "DeleteSecret",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "DeleteSecret",
                "Azure Key Vault Secret",
                &secret_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure DeleteSecret: failed to read response body"),
                })?;

        let secret: SecretBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("DeleteSecret", &url))?;

        Ok(secret)
    }
}

// -----------------------------------------------------------------------------
// Key Vault Certificates client
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureKeyVaultCertificatesClient {
    pub client: Client,
    pub token_cache: AzureTokenCache,
}

impl AzureKeyVaultCertificatesClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        Self {
            client,
            token_cache,
        }
    }

    /// Build the full URL for Key Vault certificates operations
    fn build_certificates_url(
        &self,
        vault_base_url: &str,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
    ) -> Result<url::Url> {
        let base_url = if let Some(override_url) = self.token_cache.get_service_endpoint("keyvault")
        {
            override_url.trim_end_matches('/').to_string()
        } else {
            let trimmed = vault_base_url.trim_end_matches('/');
            // Add https:// protocol if not present
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                trimmed.to_string()
            } else {
                format!("https://{}", trimmed)
            }
        };

        let mut url = url::Url::parse(&format!("{}{}", base_url, path))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!("Invalid Key Vault URL: {}{}", base_url, path),
                errors: None,
            })?;

        if let Some(params) = query_params {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, &v);
            }
        }

        Ok(url)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl KeyVaultCertificatesApi for AzureKeyVaultCertificatesClient {
    /// Import a certificate into Key Vault
    async fn import_certificate(
        &self,
        vault_base_url: String,
        certificate_name: String,
        parameters: CertificateImportParameters,
    ) -> Result<CertificateBundle> {
        let url = self.build_certificates_url(
            &vault_base_url,
            &format!("/certificates/{}/import", certificate_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CertificateImportParameters for certificate '{}'",
                    certificate_name
                ),
            })?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client
                .post(url.to_string())
                .header("Content-Type", "application/json")
                .body(body),
            "ImportCertificate",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "ImportCertificate",
                "Azure Key Vault Certificate",
                &certificate_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure ImportCertificate: failed to read response body"),
                })?;

        let cert: CertificateBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("ImportCertificate", &url))?;

        Ok(cert)
    }

    /// Delete a certificate from Key Vault
    async fn delete_certificate(
        &self,
        vault_base_url: String,
        certificate_name: String,
    ) -> Result<CertificateBundle> {
        let url = self.build_certificates_url(
            &vault_base_url,
            &format!("/certificates/{}", certificate_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let resp = send_key_vault_request(
            &self.token_cache,
            self.client.delete(url.to_string()),
            "DeleteCertificate",
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(key_vault_response_error(
                status,
                "DeleteCertificate",
                "Azure Key Vault Certificate",
                &certificate_name,
                &url,
            ));
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure DeleteCertificate: failed to read response body"),
                })?;

        let cert: CertificateBundle = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(key_vault_parse_error("DeleteCertificate", &url))?;

        Ok(cert)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    use httpmock::{Method::DELETE, Method::PATCH, Method::POST, Method::PUT, MockServer};
    use serde_json::json;
    use tempfile::NamedTempFile;

    use super::*;
    use crate::azure::{
        AzureClientConfig, AzureClientConfigExt, AzureCredentials, ServiceOverrides,
    };

    fn assert_error_omits_sentinels(error: &alien_client_core::Error, sentinels: &[&str]) {
        let serialized = serde_json::to_string(error).expect("the client error should serialize");
        for sentinel in sentinels {
            assert!(
                !serialized.contains(sentinel),
                "sensitive body leaked into serialized error: {serialized}"
            );
        }
    }

    fn sequential_token_server(tokens: &[&str]) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("token server listener should bind");
        let address = listener
            .local_addr()
            .expect("token server listener should have a local address");
        let tokens = tokens
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>();
        let handle = thread::spawn(move || {
            for token in tokens {
                let (mut stream, _) = listener
                    .accept()
                    .expect("token server should accept a request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let bytes_read = stream
                        .read(&mut buffer)
                        .expect("token server should read the request");
                    assert!(
                        bytes_read > 0,
                        "token request closed before headers arrived"
                    );
                    request.extend_from_slice(&buffer[..bytes_read]);

                    let Some(header_end) = request
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|index| index + 4)
                    else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }

                let body = json!({ "access_token": token }).to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("token server should write the response");
            }
        });
        (format!("http://{address}"), handle)
    }

    async fn delete_secret_error(status: u16, azure_error_code: &str) -> alien_client_core::Error {
        let server = MockServer::start_async().await;
        let response = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/secrets/ALIEN-COMMANDS-TOKEN")
                    .query_param("api-version", "7.4");
                then.status(status).json_body(json!({
                    "error": {
                        "code": azure_error_code,
                        "message": "response used to verify delete error classification"
                    }
                }));
            })
            .await;

        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("keyvault".to_string(), server.base_url())]),
        });
        let client = AzureKeyVaultSecretsClient::new(Client::new(), AzureTokenCache::new(config));

        let error = client
            .delete_secret(
                "ignored-by-service-override".to_string(),
                "ALIEN-COMMANDS-TOKEN".to_string(),
            )
            .await
            .expect_err("the mock response should fail the client request");

        response.assert_async().await;
        error
    }

    #[tokio::test]
    async fn delete_secret_maps_azure_secret_not_found_to_remote_resource_not_found() {
        let error = delete_secret_error(404, "SecretNotFound").await;

        match error.error {
            Some(ErrorData::RemoteResourceNotFound {
                resource_type,
                resource_name,
            }) => {
                assert_eq!(resource_type, "Azure Key Vault Secret");
                assert_eq!(resource_name, "ALIEN-COMMANDS-TOKEN");
            }
            other => panic!("expected RemoteResourceNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_secret_preserves_auth_and_server_failures() {
        let forbidden = delete_secret_error(403, "Forbidden").await;
        assert_eq!(forbidden.code, "REMOTE_ACCESS_DENIED");

        let server_error = delete_secret_error(500, "InternalServerError").await;
        assert_eq!(server_error.code, "REMOTE_SERVICE_UNAVAILABLE");
    }

    #[tokio::test]
    async fn delete_secret_refreshes_rejected_workload_identity_token_once() {
        let server = MockServer::start_async().await;
        let (authority_host, token_server) =
            sequential_token_server(&["rejected-token", "fresh-token"]);
        let unauthorized = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/secrets/ALIEN-COMMANDS-TOKEN")
                    .query_param("api-version", "7.4")
                    .header("authorization", "Bearer rejected-token");
                then.status(401).json_body(json!({
                    "error": {
                        "code": "Unauthorized",
                        "message": "[AggregatedAuthenticationFailure] Error validating token: 'S2S17001'."
                    }
                }));
            })
            .await;
        let authorized = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/secrets/ALIEN-COMMANDS-TOKEN")
                    .query_param("api-version", "7.4")
                    .header("authorization", "Bearer fresh-token");
                then.status(200).json_body(json!({
                    "id": "https://vault.example/secrets/ALIEN-COMMANDS-TOKEN/version"
                }));
            })
            .await;

        let mut federated_token_file =
            NamedTempFile::new().expect("federated token file should be created");
        federated_token_file
            .write_all(b"test-federated-token")
            .expect("federated token should be written");

        let config = AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::WorkloadIdentity {
                client_id: "management-uami-client".to_string(),
                tenant_id: "tenant".to_string(),
                federated_token_file: federated_token_file.path().to_string_lossy().into_owned(),
                authority_host,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("keyvault".to_string(), server.base_url())]),
            }),
        };
        let client = AzureKeyVaultSecretsClient::new(Client::new(), AzureTokenCache::new(config));

        let deleted = client
            .delete_secret(
                "ignored-by-service-override".to_string(),
                "ALIEN-COMMANDS-TOKEN".to_string(),
            )
            .await
            .expect("the fresh token should succeed");
        assert_eq!(
            deleted.id.as_deref(),
            Some("https://vault.example/secrets/ALIEN-COMMANDS-TOKEN/version")
        );

        token_server
            .join()
            .expect("token server should serve both token exchanges");
        assert_eq!(unauthorized.hits_async().await, 1);
        assert_eq!(authorized.hits_async().await, 1);
    }

    #[tokio::test]
    async fn secret_errors_never_retain_request_or_response_bodies() {
        const SET_REQUEST_SENTINEL: &str = "SET-SECRET-REQUEST-DO-NOT-SERIALIZE";
        const SET_RESPONSE_SENTINEL: &str = "SET-SECRET-RESPONSE-DO-NOT-SERIALIZE";
        const UPDATE_REQUEST_SENTINEL: &str = "UPDATE-SECRET-REQUEST-DO-NOT-SERIALIZE";
        const UPDATE_RESPONSE_SENTINEL: &str = "UPDATE-SECRET-RESPONSE-DO-NOT-SERIALIZE";

        let server = MockServer::start_async().await;
        let failed_set = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/secrets/SET-SECRET")
                    .query_param("api-version", "7.4")
                    .body_contains(SET_REQUEST_SENTINEL);
                then.status(500).json_body(json!({
                    "error": {
                        "code": "InternalServerError",
                        "message": SET_RESPONSE_SENTINEL
                    }
                }));
            })
            .await;
        let malformed_update = server
            .mock_async(|when, then| {
                when.method(PATCH)
                    .path("/secrets/UPDATE-SECRET/v1")
                    .query_param("api-version", "7.4")
                    .body_contains(UPDATE_REQUEST_SENTINEL);
                then.status(200)
                    .body(format!("{{\"value\":\"{UPDATE_RESPONSE_SENTINEL}\""));
            })
            .await;
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("keyvault".to_string(), server.base_url())]),
        });
        let client = AzureKeyVaultSecretsClient::new(Client::new(), AzureTokenCache::new(config));

        let set_error = client
            .set_secret(
                "ignored-by-service-override".to_string(),
                "SET-SECRET".to_string(),
                SecretSetParameters {
                    attributes: None,
                    content_type: None,
                    tags: HashMap::new(),
                    value: SET_REQUEST_SENTINEL.to_string(),
                },
            )
            .await
            .expect_err("the synthetic set failure should be returned");
        let update_error = client
            .update_secret(
                "ignored-by-service-override".to_string(),
                "UPDATE-SECRET".to_string(),
                "v1".to_string(),
                SecretUpdateParameters {
                    attributes: None,
                    content_type: None,
                    tags: HashMap::from([(
                        "sensitive-test-tag".to_string(),
                        UPDATE_REQUEST_SENTINEL.to_string(),
                    )]),
                },
            )
            .await
            .expect_err("the malformed update response should be returned");

        failed_set.assert_async().await;
        malformed_update.assert_async().await;
        assert_error_omits_sentinels(&set_error, &[SET_REQUEST_SENTINEL, SET_RESPONSE_SENTINEL]);
        assert_error_omits_sentinels(
            &update_error,
            &[UPDATE_REQUEST_SENTINEL, UPDATE_RESPONSE_SENTINEL],
        );
    }

    #[tokio::test]
    async fn import_certificate_refreshes_rejected_workload_identity_token_once() {
        let server = MockServer::start_async().await;
        let (authority_host, token_server) =
            sequential_token_server(&["rejected-token", "fresh-token"]);
        let unauthorized = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/certificates/TEST-CERT/import")
                    .query_param("api-version", "7.4")
                    .header("authorization", "Bearer rejected-token");
                then.status(401);
            })
            .await;
        let authorized = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/certificates/TEST-CERT/import")
                    .query_param("api-version", "7.4")
                    .header("authorization", "Bearer fresh-token");
                then.status(200).json_body(json!({
                    "id": "https://vault.example/certificates/TEST-CERT/version"
                }));
            })
            .await;

        let mut federated_token_file =
            NamedTempFile::new().expect("federated token file should be created");
        federated_token_file
            .write_all(b"test-federated-token")
            .expect("federated token should be written");

        let config = AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::WorkloadIdentity {
                client_id: "management-uami-client".to_string(),
                tenant_id: "tenant".to_string(),
                federated_token_file: federated_token_file.path().to_string_lossy().into_owned(),
                authority_host,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("keyvault".to_string(), server.base_url())]),
            }),
        };
        let client =
            AzureKeyVaultCertificatesClient::new(Client::new(), AzureTokenCache::new(config));

        let imported = client
            .import_certificate(
                "ignored-by-service-override".to_string(),
                "TEST-CERT".to_string(),
                CertificateImportParameters {
                    attributes: None,
                    policy: None,
                    preserve_cert_order: None,
                    pwd: None,
                    tags: HashMap::new(),
                    value: "test-certificate".to_string(),
                },
            )
            .await
            .expect("the fresh token should succeed");
        assert_eq!(
            imported.id.as_deref(),
            Some("https://vault.example/certificates/TEST-CERT/version")
        );

        token_server
            .join()
            .expect("token server should serve both token exchanges");
        assert_eq!(unauthorized.hits_async().await, 1);
        assert_eq!(authorized.hits_async().await, 1);
    }

    #[tokio::test]
    async fn certificate_errors_never_retain_request_or_response_bodies() {
        const IMPORT_REQUEST_SENTINEL: &str = "CERTIFICATE-REQUEST-DO-NOT-SERIALIZE";
        const IMPORT_RESPONSE_SENTINEL: &str = "CERTIFICATE-RESPONSE-DO-NOT-SERIALIZE";
        const DELETE_RESPONSE_SENTINEL: &str = "DELETE-CERTIFICATE-RESPONSE-DO-NOT-SERIALIZE";

        let server = MockServer::start_async().await;
        let failed_import = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/certificates/IMPORT-CERT/import")
                    .query_param("api-version", "7.4")
                    .body_contains(IMPORT_REQUEST_SENTINEL);
                then.status(500).json_body(json!({
                    "error": {
                        "code": "InternalServerError",
                        "message": IMPORT_RESPONSE_SENTINEL
                    }
                }));
            })
            .await;
        let malformed_delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/certificates/DELETE-CERT")
                    .query_param("api-version", "7.4");
                then.status(200)
                    .body(format!("{{\"cer\":\"{DELETE_RESPONSE_SENTINEL}\""));
            })
            .await;
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("keyvault".to_string(), server.base_url())]),
        });
        let client =
            AzureKeyVaultCertificatesClient::new(Client::new(), AzureTokenCache::new(config));

        let import_error = client
            .import_certificate(
                "ignored-by-service-override".to_string(),
                "IMPORT-CERT".to_string(),
                CertificateImportParameters {
                    attributes: None,
                    policy: None,
                    preserve_cert_order: None,
                    pwd: None,
                    tags: HashMap::new(),
                    value: IMPORT_REQUEST_SENTINEL.to_string(),
                },
            )
            .await
            .expect_err("the synthetic import failure should be returned");
        let delete_error = client
            .delete_certificate(
                "ignored-by-service-override".to_string(),
                "DELETE-CERT".to_string(),
            )
            .await
            .expect_err("the malformed delete response should be returned");

        failed_import.assert_async().await;
        malformed_delete.assert_async().await;
        assert_error_omits_sentinels(
            &import_error,
            &[IMPORT_REQUEST_SENTINEL, IMPORT_RESPONSE_SENTINEL],
        );
        assert_error_omits_sentinels(&delete_error, &[DELETE_RESPONSE_SENTINEL]);
    }
}
