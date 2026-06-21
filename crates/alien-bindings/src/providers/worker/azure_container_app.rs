use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::bindings::ContainerAppWorkerBinding;
use alien_core::{AzureClientConfig, AzureCredentials};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use reqwest::{Client, Method, Url};
use serde::Deserialize;

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";
const MANAGEMENT_ENDPOINT: &str = "https://management.azure.com";
const CONTAINER_APPS_API_VERSION: &str = "2025-01-01";

/// Azure Container Apps worker binding implementation.
pub struct ContainerAppWorker {
    client: Client,
    subscription_id: String,
    management_endpoint: String,
    credential: Arc<dyn TokenCredential>,
    binding: ContainerAppWorkerBinding,
}

impl Debug for ContainerAppWorker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerAppWorker")
            .field("subscription_id", &self.subscription_id)
            .field("management_endpoint", &self.management_endpoint)
            .finish()
    }
}

impl ContainerAppWorker {
    pub fn new(
        client: Client,
        config: AzureClientConfig,
        binding: ContainerAppWorkerBinding,
    ) -> Result<Self> {
        let management_endpoint = config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("management"))
            .cloned()
            .unwrap_or_else(|| MANAGEMENT_ENDPOINT.to_string());

        Ok(Self {
            client,
            subscription_id: config.subscription_id.clone(),
            management_endpoint,
            credential: azure_credential_from_config(&config)?,
            binding,
        })
    }

    /// Get the private URL from the binding, resolving template expressions if needed.
    fn get_private_url(&self) -> Result<String> {
        self.binding
            .private_url
            .clone()
            .into_value("worker", "private_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve private_url from binding".to_string(),
            })
    }

    /// Get the public URL from the binding if available.
    pub async fn get_worker_url(&self) -> Result<Option<String>> {
        self.resolve_worker_url().await
    }

    async fn resolve_worker_url(&self) -> Result<Option<String>> {
        if let Some(url_binding) = &self.binding.public_url {
            let url = url_binding
                .clone()
                .into_value("worker", "public_url")
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "worker".to_string(),
                    reason: "Failed to resolve public_url from binding".to_string(),
                })?;
            return Ok(Some(url));
        }

        let resource_group_name = self
            .binding
            .resource_group_name
            .clone()
            .into_value("worker", "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve resource_group_name from binding".to_string(),
            })?;

        let container_app_name = self
            .binding
            .container_app_name
            .clone()
            .into_value("worker", "container_app_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve container_app_name from binding".to_string(),
            })?;

        let token = self.bearer_token().await?;
        let url = self.build_container_app_url(&resource_group_name, &container_app_name)?;
        let response = self
            .client
            .request(Method::GET, url.clone())
            .bearer_auth(token.token.secret())
            .header("Content-Length", "0")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: "GET".to_string(),
            })?;

        if response.status().as_u16() == 404 || !response.status().is_success() {
            return Ok(None);
        }

        let container_app = response
            .json::<ContainerApp>()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.containerApp".to_string(),
                reason: "Failed to parse Azure Container App response".to_string(),
            })?;

        let ingress = container_app
            .properties
            .and_then(|properties| properties.configuration)
            .and_then(|configuration| configuration.ingress);

        if let Some(ingress) = ingress {
            if ingress.external.unwrap_or(false) {
                return Ok(ingress.fqdn.map(|fqdn| format!("https://{}", fqdn)));
            }
        }

        Ok(None)
    }

    /// Resolve the target URL for invocation.
    async fn resolve_target_url(&self, target_worker: &str) -> Result<String> {
        if !target_worker.is_empty() {
            if target_worker.starts_with("http://") || target_worker.starts_with("https://") {
                Ok(target_worker.to_string())
            } else {
                self.get_private_url()
            }
        } else {
            self.get_private_url()
        }
    }

    fn build_container_app_url(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<Url> {
        let mut url = Url::parse(&format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}",
            self.management_endpoint.trim_end_matches('/'),
            self.subscription_id,
            resource_group_name,
            container_app_name
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.containerApp".to_string(),
            reason: "Invalid Azure Container App URL".to_string(),
        })?;
        url.query_pairs_mut()
            .append_pair("api-version", CONTAINER_APPS_API_VERSION);
        Ok(url)
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&[MANAGEMENT_SCOPE], None)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.containerApp".to_string(),
                reason: "Failed to get Azure management bearer token".to_string(),
            })
    }
}

impl Binding for ContainerAppWorker {}

#[async_trait]
impl Worker for ContainerAppWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        let target_url = self.resolve_target_url(&request.target_worker).await?;

        let url = if request.path.starts_with('/') {
            format!("{}{}", target_url.trim_end_matches('/'), request.path)
        } else {
            format!("{}/{}", target_url.trim_end_matches('/'), request.path)
        };

        let method = match request.method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            _ => {
                return Err(AlienError::new(ErrorData::InvalidInput {
                    operation_context: "Worker invocation".to_string(),
                    details: format!("Unsupported HTTP method: {}", request.method),
                    field_name: Some("method".to_string()),
                }));
            }
        };

        let mut req_builder = self.client.request(method, &url);

        for (key, value) in &request.headers {
            req_builder = req_builder.header(key, value);
        }

        if !request.body.is_empty() {
            req_builder = req_builder.body(request.body.clone());
        }

        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        let response =
            req_builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.clone(),
                    method: request.method.clone(),
                })?;

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_str().unwrap_or("").to_string()))
            .collect::<BTreeMap<String, String>>();
        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.clone(),
                method: "READ_BODY".to_string(),
            })?
            .to_vec();

        Ok(WorkerInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        self.resolve_worker_url().await
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerApp {
    properties: Option<ContainerAppProperties>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerAppProperties {
    configuration: Option<ContainerAppConfiguration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerAppConfiguration {
    ingress: Option<ContainerAppIngress>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerAppIngress {
    external: Option<bool>,
    fqdn: Option<String>,
}

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

fn azure_credential_from_config(config: &AzureClientConfig) -> Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.containerApp".to_string(),
            reason: "Failed to build official Azure service principal credentials".to_string(),
        }),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.containerApp".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "worker.containerApp".to_string(),
                    reason: format!(
                        "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                        identity_endpoint
                    ),
                }));
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.containerApp".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "worker.containerApp".to_string(),
            reason: format!(
                "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                identity_endpoint, client_id
            ),
        })),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}
