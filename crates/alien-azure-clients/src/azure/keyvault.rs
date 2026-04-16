use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::certificates::{CertificateBundle, CertificateImportParameters};
use crate::azure::models::keyvault::{Vault, VaultCreateOrUpdateParameters};
use crate::azure::models::secrets::{SecretBundle, SecretSetParameters, SecretUpdateParameters};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

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
                message: format!(
                    "Azure CreateOrUpdateVault: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
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
                message: format!("Azure GetVault: JSON parse error. Body: {}", response_body),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
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
                message: format!(
                    "Azure UpdateVault: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
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
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

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

        let resp = self
            .client
            .put(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure SetSecret: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure SetSecret failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: Some(body),
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!("Azure SetSecret: JSON parse error. Body: {}", response_body),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(secret)
    }

    /// Get a secret from the key vault
    async fn get_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
        secret_version: Option<String>,
    ) -> Result<SecretBundle> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

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

        let resp = self
            .client
            .get(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure GetSecret: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Handle 404 errors as RemoteResourceNotFound
            if status == 404 {
                return Err(AlienError::new(ErrorData::RemoteResourceNotFound {
                    resource_type: "Azure Key Vault Secret".to_string(),
                    resource_name: secret_name,
                }));
            }

            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetSecret failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!("Azure GetSecret: JSON parse error. Body: {}", response_body),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

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
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

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

        let resp = self
            .client
            .patch(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure UpdateSecret: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure UpdateSecret failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: Some(body),
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure UpdateSecret: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(secret)
    }

    /// Delete a secret from the key vault
    async fn delete_secret(
        &self,
        vault_base_url: String,
        secret_name: String,
    ) -> Result<SecretBundle> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

        let url = self.build_secrets_url(
            &vault_base_url,
            &format!("/secrets/{}", secret_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let resp = self
            .client
            .delete(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure DeleteSecret: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure DeleteSecret failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure DeleteSecret: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

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
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

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

        let resp = self
            .client
            .post(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure ImportCertificate: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ImportCertificate failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: Some(body),
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ImportCertificate: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(cert)
    }

    /// Delete a certificate from Key Vault
    async fn delete_certificate(
        &self,
        vault_base_url: String,
        certificate_name: String,
    ) -> Result<CertificateBundle> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://vault.azure.net/.default")
            .await?;

        let url = self.build_certificates_url(
            &vault_base_url,
            &format!("/certificates/{}", certificate_name),
            Some(vec![("api-version", "7.4".into())]),
        )?;

        let resp = self
            .client
            .delete(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure DeleteCertificate: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Handle 404 errors as RemoteResourceNotFound
            if status == 404 {
                return Err(AlienError::new(ErrorData::RemoteResourceNotFound {
                    resource_type: "Azure Key Vault Certificate".to_string(),
                    resource_name: certificate_name,
                }));
            }

            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Azure DeleteCertificate failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
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
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure DeleteCertificate: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(cert)
    }
}
