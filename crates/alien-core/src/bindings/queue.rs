use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Binding parameters for Queue at runtime or in templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum QueueBinding {
    /// AWS SQS binding
    #[serde(rename_all = "camelCase")]
    Sqs(SqsQueueBinding),
    /// GCP Pub/Sub binding
    #[serde(rename_all = "camelCase")]
    Pubsub(PubSubQueueBinding),
    /// Azure Service Bus binding
    #[serde(rename_all = "camelCase")]
    Servicebus(ServiceBusQueueBinding),
}

impl QueueBinding {
    pub fn sqs(queue_url: impl Into<BindingValue<String>>) -> Self {
        Self::Sqs(SqsQueueBinding {
            queue_url: queue_url.into(),
        })
    }

    pub fn pubsub(
        topic: impl Into<BindingValue<String>>,
        subscription: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Pubsub(PubSubQueueBinding {
            topic: topic.into(),
            subscription: subscription.into(),
        })
    }

    pub fn service_bus(
        namespace: impl Into<BindingValue<String>>,
        queue_name: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Servicebus(ServiceBusQueueBinding {
            namespace: namespace.into(),
            queue_name: queue_name.into(),
        })
    }
}

/// AWS SQS queue parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SqsQueueBinding {
    /// Full SQS queue URL
    pub queue_url: BindingValue<String>,
}

/// GCP Pub/Sub parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PubSubQueueBinding {
    /// Full topic name: projects/{project}/topics/{topic}
    pub topic: BindingValue<String>,
    /// Full subscription name: projects/{project}/subscriptions/{subscription}
    pub subscription: BindingValue<String>,
}

/// Azure Service Bus parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceBusQueueBinding {
    /// Namespace name
    pub namespace: BindingValue<String>,
    /// Queue name
    pub queue_name: BindingValue<String>,
}
