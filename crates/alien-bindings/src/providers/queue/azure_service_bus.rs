use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

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
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};

const SERVICE_BUS_SCOPE: &str = "https://servicebus.azure.net/.default";

pub struct AzureServiceBusQueue {
    namespace: String,
    queue_name: String,
    endpoint_override: Option<String>,
    client: Client,
    credential: Arc<dyn TokenCredential>,
}

impl Debug for AzureServiceBusQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureServiceBusQueue")
            .field("namespace", &self.namespace)
            .field("queue_name", &self.queue_name)
            .finish()
    }
}

impl AzureServiceBusQueue {
    pub async fn new(
        namespace: String,
        queue_name: String,
        azure_config: AzureClientConfig,
    ) -> Result<Self> {
        let endpoint_override = azure_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("servicebus"))
            .cloned();

        Ok(Self {
            namespace,
            queue_name,
            endpoint_override,
            client: crate::http_client::create_http_client(),
            credential: azure_credential_from_config(&azure_config)?,
        })
    }

    fn build_url(&self, path: &str, query_params: &[(&str, String)]) -> Result<Url> {
        let base_url = self
            .endpoint_override
            .as_deref()
            .map(|override_url| override_url.trim_end_matches('/').to_string())
            .unwrap_or_else(|| format!("https://{}.servicebus.windows.net", self.namespace));
        let mut url = Url::parse(&format!("{base_url}{path}"))
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: format!("Invalid Service Bus URL: {base_url}{path}"),
            })?;

        if !query_params.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in query_params {
                query.append_pair(key, value);
            }
        }

        Ok(url)
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&[SERVICE_BUS_SCOPE], None)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to get Azure Service Bus bearer token".to_string(),
            })
    }
}

impl Binding for AzureServiceBusQueue {}

#[async_trait]
impl Queue for AzureServiceBusQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let body = match message {
            MessagePayload::Json(value) => serde_json::to_string(&value)
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: "Failed to serialize JSON payload".to_string(),
                })?,
            MessagePayload::Text(value) => value,
        };

        if body.len() > MAX_MESSAGE_BYTES {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: format!(
                    "Message size {} bytes exceeds limit of {} bytes",
                    body.len(),
                    MAX_MESSAGE_BYTES
                ),
            }));
        }

        let token = self.bearer_token().await?;
        let url = self.build_url(&format!("/{}/messages", self.queue_name), &[])?;
        let response = self
            .client
            .post(url.clone())
            .bearer_auth(token.token.secret())
            .header(
                "Content-Type",
                "application/atom+xml;type=entry;charset=utf-8",
            )
            .body(body)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to send message".to_string(),
            })?;

        ensure_success(response, "send", url).await
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: format!(
                    "Batch size {} is invalid. Must be between 1 and {}",
                    max_messages, MAX_BATCH_SIZE
                ),
            }));
        }

        let mut messages = Vec::new();
        for _ in 0..std::cmp::min(max_messages, MAX_BATCH_SIZE) {
            let token = self.bearer_token().await?;
            let url = self.build_url(
                &format!("/{}/messages/head", self.queue_name),
                &[("timeout", "30".to_string())],
            )?;
            let response = self
                .client
                .post(url.clone())
                .bearer_auth(token.token.secret())
                .header("Content-Length", "0")
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: "Failed to receive message".to_string(),
                })?;

            if response.status().as_u16() == 204 {
                break;
            }

            if !response.status().is_success() {
                ensure_success(response, "receive", url).await?;
                unreachable!("ensure_success returns on non-success response");
            }

            let broker_properties = broker_properties_from_response(&response, &url)?;
            let body = response.text().await.into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: "Failed to read received message body".to_string(),
                },
            )?;
            let payload = serde_json::from_str::<serde_json::Value>(&body)
                .map(MessagePayload::Json)
                .unwrap_or(MessagePayload::Text(body));
            let message_id = broker_properties.message_id.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: "Received message without message ID".to_string(),
                })
            })?;
            let lock_token = broker_properties.lock_token.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: "Received message without lock token".to_string(),
                })
            })?;

            messages.push(QueueMessage {
                payload,
                receipt_handle: format!("{message_id}\n{lock_token}"),
            });
        }

        Ok(messages)
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let (message_id, lock_token) = receipt_handle.split_once('\n').ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Invalid receipt handle format: expected message_id\\nlock_token"
                    .to_string(),
            })
        })?;
        let token = self.bearer_token().await?;
        let url = self.build_url(
            &format!(
                "/{}/messages/{}/{}",
                self.queue_name, message_id, lock_token
            ),
            &[],
        )?;
        let response = self
            .client
            .delete(url.clone())
            .bearer_auth(token.token.secret())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to complete message".to_string(),
            })?;

        ensure_success(response, "ack", url).await
    }
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
            binding_type: "queue.servicebus".to_string(),
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
            binding_type: "queue.servicebus".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
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
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "queue.servicebus".to_string(),
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BrokerProperties {
    #[serde(rename = "MessageId")]
    message_id: Option<String>,
    #[serde(rename = "LockToken")]
    lock_token: Option<String>,
}

fn broker_properties_from_response(
    response: &reqwest::Response,
    url: &Url,
) -> Result<BrokerProperties> {
    let header = response.headers().get("BrokerProperties").ok_or_else(|| {
        AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "queue.servicebus".to_string(),
            reason: "Received message without BrokerProperties header".to_string(),
        })
    })?;
    let value = header
        .to_str()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.servicebus".to_string(),
            reason: format!(
                "BrokerProperties header contains non-ASCII characters for URL {}",
                url
            ),
        })?;

    serde_json::from_str(value)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.servicebus".to_string(),
            reason: format!("Failed to parse BrokerProperties header: {value}"),
        })
}

async fn ensure_success(response: reqwest::Response, operation: &str, url: Url) -> Result<()> {
    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "failed to read Service Bus error response".to_string());
    Err(AlienError::new(ErrorData::BindingSetupFailed {
        binding_type: "queue.servicebus".to_string(),
        reason: format!(
            "Service Bus {} request to {} failed with status {}: {}",
            operation, url, status, body
        ),
    }))
}
