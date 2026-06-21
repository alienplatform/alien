use std::any::Any;
use std::fmt::Debug;

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;

use crate::{error::Result, types::Envelope};

/// Trait for dispatching command envelopes to agents via platform-specific transport
#[async_trait]
pub trait CommandDispatcher: Send + Sync + Debug {
    /// Dispatch an envelope to the target agent
    async fn dispatch(&self, envelope: &Envelope) -> Result<()>;

    /// Helper method for downcasting to concrete types in tests
    fn as_any(&self) -> &dyn Any;
}

/// No-op command dispatcher that succeeds without doing anything
#[derive(Debug)]
pub struct NullCommandDispatcher;

#[async_trait]
impl CommandDispatcher for NullCommandDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
        tracing::debug!(
            command_id = %envelope.command_id,
            command = %envelope.command,
            "NullCommandDispatcher: no-op dispatch"
        );
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(any(feature = "server", feature = "dispatchers"))]
mod platform_dispatchers {
    use super::*;
    use alien_core::{
        AzureClientConfig, AzureCredentials, GcpClientConfig, GcpCredentials,
        GcpImpersonationConfig,
    };
    use alien_error::AlienError;
    use aws_sdk_lambda::{
        primitives::Blob as LambdaBlob, types::InvocationType, Client as LambdaClient,
    };
    use azure_core::{
        cloud::{CloudConfiguration, CustomConfiguration},
        credentials::{
            AccessToken as AzureAccessToken, Secret, TokenCredential, TokenRequestOptions,
        },
        http::ClientOptions,
        time::{Duration as AzureDuration, OffsetDateTime},
    };
    use azure_identity::{
        ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
        ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
        WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
    };
    use google_cloud_auth::credentials::{
        self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
    };
    use google_cloud_auth::errors::CredentialsError;
    use google_cloud_pubsub::{client::Publisher, model::Message};
    use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
    use reqwest::Client;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use url::Url;

    const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
    const SERVICE_BUS_SCOPE: &str = "https://servicebus.azure.net/.default";

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
        ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>>
               + Send {
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

    #[derive(Debug)]
    struct StaticAzureAccessTokenCredential {
        token: String,
    }

    impl StaticAzureAccessTokenCredential {
        fn new(token: String) -> Self {
            Self { token }
        }
    }

    #[async_trait]
    impl TokenCredential for StaticAzureAccessTokenCredential {
        async fn get_token(
            &self,
            scopes: &[&str],
            _options: Option<TokenRequestOptions<'_>>,
        ) -> azure_core::Result<AzureAccessToken> {
            if scopes.is_empty() {
                return Err(azure_core::Error::with_message(
                    azure_core::error::ErrorKind::Credential,
                    "no scopes specified",
                ));
            }

            Ok(AzureAccessToken::new(
                self.token.clone(),
                OffsetDateTime::now_utc() + AzureDuration::days(365),
            ))
        }
    }

    /// AWS Lambda command dispatcher using InvokeFunction API
    #[derive(Debug)]
    pub struct LambdaCommandDispatcher {
        lambda_client: LambdaClient,
        function_name: String,
    }

    impl LambdaCommandDispatcher {
        pub async fn new(
            _client: Client,
            config: alien_core::AwsClientConfig,
            function_name: String,
        ) -> Result<Self> {
            let lambda_client = alien_bindings::aws_sdk::lambda_client_from_alien_config(&config)
                .await
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to create official Lambda client".to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: None,
                })?;
            Ok(Self {
                lambda_client,
                function_name,
            })
        }
    }

    #[async_trait]
    impl CommandDispatcher for LambdaCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON payload
            let payload = serde_json::to_vec(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize command envelope".to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            let function_name = self.function_name.clone();

            // Use async invocation to send the envelope to the Lambda function
            // The Lambda function should have alien-runtime configured to handle command envelopes
            self.lambda_client
                .invoke()
                .function_name(function_name.clone())
                .invocation_type(InvocationType::Event)
                .payload(LambdaBlob::new(payload))
                .send()
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: format!("Failed to invoke Lambda function {}", function_name),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                })?;

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                function_name = %function_name,
                "Successfully dispatched command envelope to Lambda function"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// GCP Pub/Sub command dispatcher
    #[derive(Debug)]
    pub struct PubSubCommandDispatcher {
        publisher: Publisher,
        #[allow(dead_code)]
        project_id: String,
        topic_id: String,
    }

    impl PubSubCommandDispatcher {
        pub async fn new(
            _client: Client,
            config: GcpClientConfig,
            topic_id: String,
        ) -> Result<Self> {
            let project_id = config.project_id.clone();
            let topic_name = format!("projects/{project_id}/topics/{topic_id}");
            let mut builder = Publisher::builder(topic_name)
                .with_credentials(credentials_from_gcp_config(&config)?);

            if let Some(endpoint) = config
                .service_overrides
                .as_ref()
                .and_then(|overrides| overrides.endpoints.get("pubsub"))
            {
                builder = builder.with_endpoint(pubsub_endpoint_for_official_client(endpoint));
            }

            let publisher = builder.build().await.into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to create official Pub/Sub publisher".to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: Some(topic_id.clone()),
                },
            )?;

            Ok(Self {
                publisher,
                project_id,
                topic_id,
            })
        }
    }

    #[async_trait]
    impl CommandDispatcher for PubSubCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize command envelope".to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            let topic_id = self.topic_id.clone();

            // Create the Pub/Sub message with command envelope metadata
            let mut attributes = HashMap::new();
            attributes.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            attributes.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            attributes.insert("cmd-command".to_string(), envelope.command.clone());

            let message = Message::new()
                .set_data(envelope_json)
                .set_attributes(attributes);

            self.publisher
                .publish(message)
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: format!("Failed to publish to Pub/Sub topic {}", topic_id),
                    transport_type: Some("pubsub".to_string()),
                    target: Some(envelope.command_id.clone()),
                })?;

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                topic_id = %topic_id,
                "Successfully dispatched command envelope to Pub/Sub topic"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
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
                    crate::ErrorData::TransportDispatchFailed {
                        message: "Failed to parse GCP service account key JSON".to_string(),
                        transport_type: Some("pubsub".to_string()),
                        target: None,
                    },
                )?;
                credentials::service_account::Builder::new(key)
                    .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                        [CLOUD_PLATFORM_SCOPE],
                    ))
                    .build()
                    .into_alien_error()
                    .context(crate::ErrorData::TransportDispatchFailed {
                        message: "Failed to build official GCP service account credentials".to_string(),
                        transport_type: Some("pubsub".to_string()),
                        target: None,
                    })
            }
            GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to build official GCP metadata credentials".to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: None,
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
                    .context(crate::ErrorData::TransportDispatchFailed {
                        message: "Failed to build official GCP external account credentials".to_string(),
                        transport_type: Some("pubsub".to_string()),
                        target: None,
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
                    .context(crate::ErrorData::TransportDispatchFailed {
                        message: "Failed to build official GCP authorized user credentials".to_string(),
                        transport_type: Some("pubsub".to_string()),
                        target: None,
                    })
            }
            GcpCredentials::ImpersonatedServiceAccount { source, config } => {
                impersonated_credentials_from_gcp_config(source, config)
            }
            GcpCredentials::ProjectedServiceAccount { .. } => {
                Err(AlienError::new(crate::ErrorData::TransportDispatchFailed {
                    message: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: None,
                }))
            }
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
            .context(crate::ErrorData::TransportDispatchFailed {
                message: "Failed to build official GCP impersonated credentials".to_string(),
                transport_type: Some("pubsub".to_string()),
                target: None,
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
                AlienError::new(crate::ErrorData::TransportDispatchFailed {
                    message: format!("Invalid Google duration '{}': missing 's' suffix", value),
                    transport_type: Some("pubsub".to_string()),
                    target: None,
                })
            })?
            .parse::<u64>()
            .into_alien_error()
            .context(crate::ErrorData::TransportDispatchFailed {
                message: format!("Invalid Google duration '{}'", value),
                transport_type: Some("pubsub".to_string()),
                target: None,
            })?;

        Ok(Duration::from_secs(seconds))
    }

    fn pubsub_endpoint_for_official_client(endpoint: &str) -> String {
        endpoint
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string()
    }

    fn azure_credential_from_config(
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn TokenCredential>> {
        match &config.credentials {
            AzureCredentials::AccessToken { token } => {
                Ok(Arc::new(StaticAzureAccessTokenCredential::new(token.clone())))
            }
            AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } => ClientSecretCredential::new(
                &config.tenant_id,
                client_id.clone(),
                Secret::new(client_secret.clone()),
                Some(ClientSecretCredentialOptions {
                    client_options: azure_client_options(None)?,
                }),
            )
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(crate::ErrorData::TransportDispatchFailed {
                message: "Failed to build official Azure service principal credentials".to_string(),
                transport_type: Some("servicebus".to_string()),
                target: None,
            }),
            AzureCredentials::WorkloadIdentity {
                client_id,
                tenant_id,
                federated_token_file,
                authority_host,
            } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
                credential_options: ClientAssertionCredentialOptions {
                    client_options: azure_client_options(Some(authority_host))?,
                },
                client_id: Some(client_id.clone()),
                tenant_id: Some(tenant_id.clone()),
                token_file_path: Some(PathBuf::from(federated_token_file)),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(crate::ErrorData::TransportDispatchFailed {
                message: "Failed to build official Azure workload identity credentials".to_string(),
                transport_type: Some("servicebus".to_string()),
                target: None,
            }),
            AzureCredentials::VmManagedIdentity {
                client_id,
                identity_endpoint,
            } => {
                if let Some(identity_endpoint) = identity_endpoint {
                    return Err(AlienError::new(crate::ErrorData::TransportDispatchFailed {
                        message: format!(
                            "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                            identity_endpoint
                        ),
                        transport_type: Some("servicebus".to_string()),
                        target: None,
                    }));
                }

                ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                    user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                    client_options: azure_client_options(None)?,
                }))
                .map(|credential| credential as Arc<dyn TokenCredential>)
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to build official Azure VM managed identity credentials"
                        .to_string(),
                    transport_type: Some("servicebus".to_string()),
                    target: None,
                })
            }
            AzureCredentials::ManagedIdentity {
                client_id,
                identity_endpoint,
                ..
            } => Err(AlienError::new(crate::ErrorData::TransportDispatchFailed {
                message: format!(
                    "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                    identity_endpoint, client_id
                ),
                transport_type: Some("servicebus".to_string()),
                target: None,
            })),
        }
    }

    fn azure_client_options(authority_host: Option<&str>) -> Result<ClientOptions> {
        let cloud = authority_host.map(|authority_host| {
            let mut custom = CustomConfiguration::default();
            custom.authority_host = authority_host.to_string();
            Arc::new(CloudConfiguration::Custom(custom))
        });

        Ok(ClientOptions {
            cloud,
            ..Default::default()
        })
    }

    /// Azure Service Bus command dispatcher
    #[derive(Debug)]
    pub struct ServiceBusCommandDispatcher {
        client: Client,
        credential: Arc<dyn TokenCredential>,
        servicebus_endpoint_override: Option<String>,
        namespace_name: String,
        queue_name: String,
    }

    impl ServiceBusCommandDispatcher {
        pub fn new(
            client: Client,
            config: AzureClientConfig,
            namespace_name: String,
            queue_name: String,
        ) -> Result<Self> {
            let credential = azure_credential_from_config(&config)?;
            let servicebus_endpoint_override = config
                .service_overrides
                .as_ref()
                .and_then(|overrides| overrides.endpoints.get("servicebus"))
                .cloned();

            Ok(Self {
                client,
                credential,
                servicebus_endpoint_override,
                namespace_name,
                queue_name,
            })
        }

        fn build_send_url(&self) -> Result<Url> {
            let base_url = self
                .servicebus_endpoint_override
                .as_deref()
                .map(|override_url| override_url.trim_end_matches('/').to_string())
                .unwrap_or_else(|| {
                    format!("https://{}.servicebus.windows.net", self.namespace_name)
                });

            Url::parse(&format!("{}/{}/messages", base_url, self.queue_name))
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: format!(
                        "Invalid Service Bus URL for namespace {}",
                        self.namespace_name
                    ),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(self.queue_name.clone()),
                })
        }
    }

    #[async_trait]
    impl CommandDispatcher for ServiceBusCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize command envelope".to_string(),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // Create custom properties for command metadata
            let mut custom_properties = HashMap::new();
            custom_properties.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            custom_properties.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            custom_properties.insert("cmd-command".to_string(), envelope.command.clone());

            let access_token = self
                .credential
                .get_token(&[SERVICE_BUS_SCOPE], None)
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to get Azure Service Bus bearer token".to_string(),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                })?;

            let url = self.build_send_url()?;

            let mut request = self
                .client
                .post(url.clone())
                .bearer_auth(access_token.token.secret())
                .header(
                    "Content-Type",
                    "application/atom+xml;type=entry;charset=utf-8",
                );

            for (key, value) in &custom_properties {
                request = request.header(key, value);
            }

            let response = request
                .body(envelope_json.clone())
                .send()
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: format!(
                        "Failed to send request to Service Bus queue {}/{}",
                        self.namespace_name, self.queue_name
                    ),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "failed to read Service Bus error response".to_string());
                return Err(AlienError::new(crate::ErrorData::TransportDispatchFailed {
                    message: format!(
                        "Service Bus send to queue {}/{} failed with status {}: {}",
                        self.namespace_name, self.queue_name, status, error_text
                    ),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                }));
            }

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                namespace = %self.namespace_name,
                queue = %self.queue_name,
                "Successfully dispatched command envelope to Service Bus queue"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

#[cfg(any(feature = "server", feature = "dispatchers"))]
pub use platform_dispatchers::*;
