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

#[cfg(feature = "server")]
mod platform_dispatchers {
    use super::*;
    use alien_aws_clients::aws::{
        lambda::{InvocationType, InvokeRequest, LambdaApi, LambdaClient},
        AwsClientConfig,
    };
    use alien_aws_clients::AwsCredentialProvider;
    use alien_azure_clients::azure::{
        service_bus::{
            AzureServiceBusDataPlaneClient, SendMessageParameters, ServiceBusDataPlaneApi,
        },
        token_cache::AzureTokenCache,
        AzureClientConfig,
    };
    use alien_gcp_clients::gcp::{
        pubsub::{PubSubApi, PubSubClient, PublishRequest, PubsubMessage},
        GcpClientConfig,
    };
    use base64::prelude::*;
    use reqwest::Client;
    use std::collections::HashMap;

    /// AWS Lambda command dispatcher using InvokeFunction API
    #[derive(Debug)]
    pub struct LambdaCommandDispatcher {
        lambda_client: LambdaClient,
        function_name: String,
    }

    impl LambdaCommandDispatcher {
        pub async fn new(
            client: Client,
            config: AwsClientConfig,
            function_name: String,
        ) -> Result<Self> {
            let credentials = AwsCredentialProvider::from_config(config)
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to create AWS credential provider".to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: None,
                })?;
            Ok(Self {
                lambda_client: LambdaClient::new(client, credentials),
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
            let invoke_request = InvokeRequest::builder()
                .function_name(function_name.clone())
                .invocation_type(InvocationType::Event) // Async invocation
                .payload(payload)
                .build();

            self.lambda_client.invoke(invoke_request).await.context(
                crate::ErrorData::TransportDispatchFailed {
                    message: format!("Failed to invoke Lambda function {}", function_name),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

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
        pubsub_client: PubSubClient,
        #[allow(dead_code)]
        project_id: String,
        topic_id: String,
    }

    impl PubSubCommandDispatcher {
        pub fn new(client: Client, config: GcpClientConfig, topic_id: String) -> Self {
            let project_id = config.project_id.clone();
            Self {
                pubsub_client: PubSubClient::new(client, config),
                project_id,
                topic_id,
            }
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

            // Base64 encode the JSON payload as required by Pub/Sub
            let data = BASE64_STANDARD.encode(envelope_json.as_bytes());

            let topic_id = self.topic_id.clone();

            // Create the Pub/Sub message with command envelope metadata
            let mut attributes = HashMap::new();
            attributes.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            attributes.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            attributes.insert("cmd-command".to_string(), envelope.command.clone());

            let message = PubsubMessage::builder()
                .data(data)
                .attributes(attributes)
                .build();

            let publish_request = PublishRequest::builder().messages(vec![message]).build();

            self.pubsub_client
                .publish(topic_id.clone(), publish_request)
                .await
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

    /// Azure Service Bus command dispatcher
    #[derive(Debug)]
    pub struct ServiceBusCommandDispatcher {
        servicebus_client: AzureServiceBusDataPlaneClient,
        namespace_name: String,
        queue_name: String,
    }

    impl ServiceBusCommandDispatcher {
        pub fn new(
            client: Client,
            config: AzureClientConfig,
            namespace_name: String,
            queue_name: String,
        ) -> Self {
            Self {
                servicebus_client: AzureServiceBusDataPlaneClient::new(
                    client,
                    AzureTokenCache::new(config),
                ),
                namespace_name,
                queue_name,
            }
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

            let namespace_name = self.namespace_name.clone();
            let queue_name = self.queue_name.clone();

            // Create custom properties for command metadata
            let mut custom_properties = HashMap::new();
            custom_properties.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            custom_properties.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            custom_properties.insert("cmd-command".to_string(), envelope.command.clone());

            let message = SendMessageParameters {
                body: envelope_json,
                broker_properties: None,
                custom_properties,
            };

            self.servicebus_client
                .send_message(namespace_name.clone(), queue_name.clone(), message)
                .await
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: format!(
                        "Failed to send message to Service Bus queue {}/{}",
                        namespace_name, queue_name
                    ),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                })?;

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                namespace = %namespace_name,
                queue = %queue_name,
                "Successfully dispatched command envelope to Service Bus queue"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

#[cfg(feature = "server")]
pub use platform_dispatchers::*;
