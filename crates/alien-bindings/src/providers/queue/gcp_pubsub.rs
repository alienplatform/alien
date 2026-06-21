use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Duration;

use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::prelude::*;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const PUBSUB_REST_BASE_URL: &str = "https://pubsub.googleapis.com/v1";

pub struct GcpPubSubQueue {
    project_id: String,
    topic: String,
    subscription: String,
    endpoint: String,
    client: Client,
    credentials: Credentials,
}

impl Debug for GcpPubSubQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpPubSubQueue")
            .field("project_id", &self.project_id)
            .field("topic", &self.topic)
            .field("subscription", &self.subscription)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl GcpPubSubQueue {
    pub async fn new(
        topic: String,
        subscription: String,
        gcp_config: GcpClientConfig,
    ) -> Result<Self> {
        let endpoint = gcp_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("pubsub"))
            .cloned()
            .unwrap_or_else(|| PUBSUB_REST_BASE_URL.to_string());

        Ok(Self {
            project_id: gcp_config.project_id.clone(),
            topic,
            subscription,
            endpoint,
            client: crate::http_client::create_http_client(),
            credentials: credentials_from_gcp_config(&gcp_config)?,
        })
    }

    fn topic_name(&self) -> String {
        if self.topic.starts_with("projects/") {
            self.topic.clone()
        } else {
            format!("projects/{}/topics/{}", self.project_id, self.topic)
        }
    }

    fn subscription_name(&self) -> String {
        if self.subscription.starts_with("projects/") {
            self.subscription.clone()
        } else {
            format!(
                "projects/{}/subscriptions/{}",
                self.project_id, self.subscription
            )
        }
    }

    fn build_url(&self, resource: &str, verb: &str) -> Result<Url> {
        Url::parse(&format!(
            "{}/{}:{}",
            self.endpoint.trim_end_matches('/'),
            resource,
            verb
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.pubsub".to_string(),
            reason: format!("Invalid Pub/Sub URL for {resource}:{verb}"),
        })
    }

    async fn authed_request(&self, method: Method, url: Url) -> Result<reqwest::RequestBuilder> {
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to get Google auth headers".to_string(),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Google auth returned NotModified without cached headers".to_string(),
                }));
            }
        };

        Ok(self.client.request(method, url).headers(headers))
    }
}

impl Binding for GcpPubSubQueue {}

#[async_trait]
impl Queue for GcpPubSubQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let data = match message {
            MessagePayload::Json(value) => serde_json::to_vec(&value).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to serialize JSON payload".to_string(),
                },
            )?,
            MessagePayload::Text(value) => value.into_bytes(),
        };

        if data.len() > MAX_MESSAGE_BYTES {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: format!(
                    "Message size {} bytes exceeds limit of {} bytes",
                    data.len(),
                    MAX_MESSAGE_BYTES
                ),
            }));
        }

        let request = PublishRequest {
            messages: vec![PubsubMessage {
                data: Some(BASE64_STANDARD.encode(data)),
                attributes: None,
            }],
        };
        let url = self.build_url(&self.topic_name(), "publish")?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to publish message".to_string(),
            })?;

        ensure_success(response, "publish", url).await
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: format!(
                    "Batch size {} is invalid. Must be between 1 and {}",
                    max_messages, MAX_BATCH_SIZE
                ),
            }));
        }

        let subscription = self.subscription_name();
        let request = PullRequest {
            max_messages: std::cmp::min(max_messages, MAX_BATCH_SIZE) as i32,
        };
        let url = self.build_url(&subscription, "pull")?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to pull messages".to_string(),
            })?;

        if !response.status().is_success() {
            ensure_success(response, "pull", url).await?;
            unreachable!("ensure_success returns on non-success response");
        }

        let response = response
            .json::<PullResponse>()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to parse pull response".to_string(),
            })?;
        let ack_ids = response
            .received_messages
            .iter()
            .map(|message| message.ack_id.clone())
            .collect::<Vec<_>>();

        if !ack_ids.is_empty() {
            let request = ModifyAckDeadlineRequest {
                ack_ids,
                ack_deadline_seconds: 30,
            };
            let url = self.build_url(&subscription, "modifyAckDeadline")?;
            let response = self
                .authed_request(Method::POST, url.clone())
                .await?
                .json(&request)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to modify ack deadline".to_string(),
                })?;
            ensure_success(response, "modifyAckDeadline", url).await?;
        }

        response
            .received_messages
            .into_iter()
            .map(|received_message| {
                let raw_data = received_message.message.data.unwrap_or_default();
                let data = BASE64_STANDARD
                    .decode(&raw_data)
                    .into_alien_error()
                    .context(ErrorData::BindingSetupFailed {
                        binding_type: "queue.pubsub".to_string(),
                        reason: "Failed to decode Pub/Sub message data".to_string(),
                    })?;
                let raw = String::from_utf8(data).into_alien_error().context(
                    ErrorData::BindingSetupFailed {
                        binding_type: "queue.pubsub".to_string(),
                        reason: "Pub/Sub message data is not valid UTF-8".to_string(),
                    },
                )?;
                let payload = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(MessagePayload::Json)
                    .unwrap_or(MessagePayload::Text(raw));

                Ok(QueueMessage {
                    payload,
                    receipt_handle: received_message.ack_id,
                })
            })
            .collect()
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let request = AcknowledgeRequest {
            ack_ids: vec![receipt_handle.to_string()],
        };
        let url = self.build_url(&self.subscription_name(), "acknowledge")?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to acknowledge message".to_string(),
            })?;

        ensure_success(response, "acknowledge", url).await
    }
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.pubsub".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.pubsub".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest {
    messages: Vec<PubsubMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PubsubMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    max_messages: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResponse {
    #[serde(default)]
    received_messages: Vec<ReceivedMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReceivedMessage {
    ack_id: String,
    message: PubsubMessage,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcknowledgeRequest {
    ack_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModifyAckDeadlineRequest {
    ack_ids: Vec<String>,
    ack_deadline_seconds: i32,
}

async fn ensure_success(response: reqwest::Response, operation: &str, url: Url) -> Result<()> {
    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "failed to read Pub/Sub error response".to_string());
    Err(AlienError::new(ErrorData::BindingSetupFailed {
        binding_type: "queue.pubsub".to_string(),
        reason: format!(
            "Pub/Sub {} request to {} failed with status {}: {}",
            operation, url, status, body
        ),
    }))
}
