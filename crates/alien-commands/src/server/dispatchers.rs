use std::any::Any;
use std::fmt::Debug;

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;

use crate::{error::Result, types::Envelope};

/// Trait for dispatching ARC envelopes to agents via platform-specific transport
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
    use alien_azure_clients::azure::{
        service_bus::{
            AzureServiceBusDataPlaneClient, SendMessageParameters, ServiceBusDataPlaneApi,
        },
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
    }

    impl LambdaCommandDispatcher {
        pub fn new(client: Client, config: AwsClientConfig) -> Self {
            Self {
                lambda_client: LambdaClient::new(client, config),
            }
        }
    }

    #[async_trait]
    impl CommandDispatcher for LambdaCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the ARC envelope as JSON payload
            let payload = serde_json::to_vec(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize ARC envelope".to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // The function name should be provided via configuration or extracted from context
            // For now, we use the command_id as a placeholder - in practice this would come from
            // agent configuration
            let function_name = envelope.command_id.clone();

            // Use async invocation to send the envelope to the Lambda function
            // The Lambda function should have alien-runtime configured to handle ARC envelopes
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
                "Successfully dispatched ARC envelope to Lambda function"
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
    }

    impl PubSubCommandDispatcher {
        pub fn new(client: Client, config: GcpClientConfig) -> Self {
            let project_id = config.project_id.clone();
            Self {
                pubsub_client: PubSubClient::new(client, config),
                project_id,
            }
        }
    }

    #[async_trait]
    impl CommandDispatcher for PubSubCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the ARC envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize ARC envelope".to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // Base64 encode the JSON payload as required by Pub/Sub
            let data = BASE64_STANDARD.encode(envelope_json.as_bytes());

            // The topic_id should come from agent configuration
            // For now, we use the command_id as a placeholder
            let topic_id = envelope.command_id.clone();

            // Create the Pub/Sub message with ARC envelope metadata
            let mut attributes = HashMap::new();
            attributes.insert("arc-protocol".to_string(), envelope.protocol.clone());
            attributes.insert("arc-command-id".to_string(), envelope.command_id.clone());
            attributes.insert("arc-command".to_string(), envelope.command.clone());

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
                "Successfully dispatched ARC envelope to Pub/Sub topic"
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
    }

    impl ServiceBusCommandDispatcher {
        pub fn new(client: Client, config: AzureClientConfig) -> Self {
            Self {
                servicebus_client: AzureServiceBusDataPlaneClient::new(client, config),
            }
        }
    }

    #[async_trait]
    impl CommandDispatcher for ServiceBusCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the ARC envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to serialize ARC envelope".to_string(),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // Parse namespace and queue from command_id (placeholder)
            // In practice, this would come from agent configuration
            let command_id = &envelope.command_id;
            let (namespace_name, queue_name) = if command_id.contains('/') {
                let parts: Vec<&str> = command_id.splitn(2, '/').collect();
                (parts[0].to_string(), parts[1].to_string())
            } else {
                return Err(AlienError::new(crate::ErrorData::TransportDispatchFailed {
                    message: format!(
                        "Service Bus target must include namespace: expected 'namespace/queue', got '{}'",
                        command_id
                    ),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                }));
            };

            // Create custom properties for ARC metadata
            let mut custom_properties = HashMap::new();
            custom_properties.insert("arc-protocol".to_string(), envelope.protocol.clone());
            custom_properties.insert("arc-command-id".to_string(), envelope.command_id.clone());
            custom_properties.insert("arc-command".to_string(), envelope.command.clone());

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
                "Successfully dispatched ARC envelope to Service Bus queue"
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
