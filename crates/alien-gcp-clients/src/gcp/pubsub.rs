use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Pub/Sub service configuration
#[derive(Debug)]
pub struct PubSubServiceConfig;

impl GcpServiceConfig for PubSubServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://pubsub.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://pubsub.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Pub/Sub"
    }

    fn service_key(&self) -> &'static str {
        "pubsub"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait PubSubApi: Send + Sync + Debug {
    // Topic operations
    async fn create_topic(&self, topic_id: String, topic: Topic) -> Result<Topic>;
    async fn get_topic(&self, topic_id: String) -> Result<Topic>;
    async fn patch_topic(
        &self,
        topic_id: String,
        topic_patch: TopicPatch,
        update_mask: Option<String>,
    ) -> Result<Topic>;
    async fn delete_topic(&self, topic_id: String) -> Result<()>;
    async fn list_topics(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListTopicsResponse>;

    // Topic IAM operations
    async fn set_topic_iam_policy(
        &self,
        topic_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;
    async fn get_topic_iam_policy(&self, topic_id: String) -> Result<IamPolicy>;
    async fn test_topic_iam_permissions(
        &self,
        topic_id: String,
        permissions: Vec<String>,
    ) -> Result<TestIamPermissionsResponse>;

    // Publishing operations
    async fn publish(&self, topic_id: String, request: PublishRequest) -> Result<PublishResponse>;

    // Subscription operations
    async fn create_subscription(
        &self,
        subscription_id: String,
        subscription: Subscription,
    ) -> Result<Subscription>;
    async fn get_subscription(&self, subscription_id: String) -> Result<Subscription>;
    async fn patch_subscription(
        &self,
        subscription_id: String,
        subscription_patch: SubscriptionPatch,
        update_mask: Option<String>,
    ) -> Result<Subscription>;
    async fn delete_subscription(&self, subscription_id: String) -> Result<()>;
    async fn list_subscriptions(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListSubscriptionsResponse>;

    // Subscription IAM operations
    async fn set_subscription_iam_policy(
        &self,
        subscription_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;
    async fn get_subscription_iam_policy(&self, subscription_id: String) -> Result<IamPolicy>;
    async fn test_subscription_iam_permissions(
        &self,
        subscription_id: String,
        permissions: Vec<String>,
    ) -> Result<TestIamPermissionsResponse>;

    // Topic-subscription relationship operations
    async fn list_topic_subscriptions(
        &self,
        topic_id: String,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListTopicSubscriptionsResponse>;

    // Message pulling and acknowledgment operations
    async fn pull(&self, subscription_id: String, request: PullRequest) -> Result<PullResponse>;
    async fn acknowledge(&self, subscription_id: String, request: AcknowledgeRequest)
        -> Result<()>;
    async fn modify_ack_deadline(
        &self,
        subscription_id: String,
        request: ModifyAckDeadlineRequest,
    ) -> Result<()>;
}

/// Pub/Sub client for managing topics, subscriptions, and publishing messages
#[derive(Debug)]
pub struct PubSubClient {
    base: GcpClientBase,
    project_id: String,
}

impl PubSubClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(PubSubServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl PubSubApi for PubSubClient {
    /// Creates a new topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/create
    async fn create_topic(&self, topic_id: String, topic: Topic) -> Result<Topic> {
        let path = format!("projects/{}/topics/{}", self.project_id, topic_id);

        self.base
            .execute_request(Method::PUT, &path, None, Some(topic), &topic_id)
            .await
    }

    /// Gets information about a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/get
    async fn get_topic(&self, topic_id: String) -> Result<Topic> {
        let path = format!("projects/{}/topics/{}", self.project_id, topic_id);

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &topic_id)
            .await
    }

    /// Updates a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/patch
    async fn patch_topic(
        &self,
        topic_id: String,
        topic_patch: TopicPatch,
        update_mask: Option<String>,
    ) -> Result<Topic> {
        let path = format!("projects/{}/topics/{}", self.project_id, topic_id);

        let request = TopicPatchRequest::builder()
            .topic(topic_patch)
            .maybe_update_mask(update_mask)
            .build();

        self.base
            .execute_request(Method::PATCH, &path, None, Some(request), &topic_id)
            .await
    }

    /// Deletes a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/delete
    async fn delete_topic(&self, topic_id: String) -> Result<()> {
        let path = format!("projects/{}/topics/{}", self.project_id, topic_id);

        self.base
            .execute_request_no_response(Method::DELETE, &path, None, Option::<()>::None, &topic_id)
            .await
    }

    /// Lists matching topics.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/list
    async fn list_topics(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListTopicsResponse> {
        let path = format!("projects/{}/topics", self.project_id);

        let mut query_params = Vec::new();
        if let Some(size) = page_size {
            query_params.push(("pageSize", size.to_string()));
        }
        if let Some(token) = page_token {
            query_params.push(("pageToken", token));
        }

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &self.project_id,
            )
            .await
    }

    /// Sets the IAM policy for a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/setIamPolicy
    async fn set_topic_iam_policy(
        &self,
        topic_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/topics/{}:setIamPolicy",
            self.project_id, topic_id
        );

        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &topic_id)
            .await
    }

    /// Gets the IAM policy for a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/getIamPolicy
    async fn get_topic_iam_policy(&self, topic_id: String) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/topics/{}:getIamPolicy",
            self.project_id, topic_id
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &topic_id)
            .await
    }

    /// Tests IAM permissions for a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/testIamPermissions
    async fn test_topic_iam_permissions(
        &self,
        topic_id: String,
        permissions: Vec<String>,
    ) -> Result<TestIamPermissionsResponse> {
        let path = format!(
            "projects/{}/topics/{}:testIamPermissions",
            self.project_id, topic_id
        );

        let request = TestIamPermissionsRequest { permissions };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &topic_id)
            .await
    }

    /// Publishes messages to a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/publish
    async fn publish(&self, topic_id: String, request: PublishRequest) -> Result<PublishResponse> {
        let path = format!("projects/{}/topics/{}:publish", self.project_id, topic_id);

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &topic_id)
            .await
    }

    /// Creates a new subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/create
    async fn create_subscription(
        &self,
        subscription_id: String,
        subscription: Subscription,
    ) -> Result<Subscription> {
        let path = format!(
            "projects/{}/subscriptions/{}",
            self.project_id, subscription_id
        );

        self.base
            .execute_request(
                Method::PUT,
                &path,
                None,
                Some(subscription),
                &subscription_id,
            )
            .await
    }

    /// Gets information about a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/get
    async fn get_subscription(&self, subscription_id: String) -> Result<Subscription> {
        let path = format!(
            "projects/{}/subscriptions/{}",
            self.project_id, subscription_id
        );

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &subscription_id,
            )
            .await
    }

    /// Updates a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/patch
    async fn patch_subscription(
        &self,
        subscription_id: String,
        subscription_patch: SubscriptionPatch,
        update_mask: Option<String>,
    ) -> Result<Subscription> {
        let path = format!(
            "projects/{}/subscriptions/{}",
            self.project_id, subscription_id
        );

        let request = SubscriptionPatchRequest::builder()
            .subscription(subscription_patch)
            .maybe_update_mask(update_mask)
            .build();

        self.base
            .execute_request(Method::PATCH, &path, None, Some(request), &subscription_id)
            .await
    }

    /// Deletes a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/delete
    async fn delete_subscription(&self, subscription_id: String) -> Result<()> {
        let path = format!(
            "projects/{}/subscriptions/{}",
            self.project_id, subscription_id
        );

        self.base
            .execute_request_no_response(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &subscription_id,
            )
            .await
    }

    /// Lists subscriptions in the project.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/list
    async fn list_subscriptions(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListSubscriptionsResponse> {
        let path = format!("projects/{}/subscriptions", self.project_id);

        let mut query_params = Vec::new();
        if let Some(size) = page_size {
            query_params.push(("pageSize", size.to_string()));
        }
        if let Some(token) = page_token {
            query_params.push(("pageToken", token));
        }

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &self.project_id,
            )
            .await
    }

    /// Sets the IAM policy for a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/setIamPolicy
    async fn set_subscription_iam_policy(
        &self,
        subscription_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/subscriptions/{}:setIamPolicy",
            self.project_id, subscription_id
        );

        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &subscription_id)
            .await
    }

    /// Gets the IAM policy for a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/getIamPolicy
    async fn get_subscription_iam_policy(&self, subscription_id: String) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/subscriptions/{}:getIamPolicy",
            self.project_id, subscription_id
        );

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &subscription_id,
            )
            .await
    }

    /// Tests IAM permissions for a subscription.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/testIamPermissions
    async fn test_subscription_iam_permissions(
        &self,
        subscription_id: String,
        permissions: Vec<String>,
    ) -> Result<TestIamPermissionsResponse> {
        let path = format!(
            "projects/{}/subscriptions/{}:testIamPermissions",
            self.project_id, subscription_id
        );

        let request = TestIamPermissionsRequest { permissions };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &subscription_id)
            .await
    }

    /// Lists subscriptions attached to a topic.
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics.subscriptions/list
    async fn list_topic_subscriptions(
        &self,
        topic_id: String,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListTopicSubscriptionsResponse> {
        let path = format!(
            "projects/{}/topics/{}/subscriptions",
            self.project_id, topic_id
        );

        let mut query_params = Vec::new();
        if let Some(size) = page_size {
            query_params.push(("pageSize", size.to_string()));
        }
        if let Some(token) = page_token {
            query_params.push(("pageToken", token));
        }

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &topic_id,
            )
            .await
    }

    /// Pulls messages from a subscription
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/pull
    async fn pull(&self, subscription_id: String, request: PullRequest) -> Result<PullResponse> {
        let path = format!(
            "projects/{}/subscriptions/{}:pull",
            self.project_id, subscription_id
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &subscription_id)
            .await
    }

    /// Acknowledges messages
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/acknowledge
    async fn acknowledge(
        &self,
        subscription_id: String,
        request: AcknowledgeRequest,
    ) -> Result<()> {
        let path = format!(
            "projects/{}/subscriptions/{}:acknowledge",
            self.project_id, subscription_id
        );

        self.base
            .execute_request_no_response(Method::POST, &path, None, Some(request), &subscription_id)
            .await
    }

    /// Modifies acknowledgment deadline for messages
    /// See: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/modifyAckDeadline
    async fn modify_ack_deadline(
        &self,
        subscription_id: String,
        request: ModifyAckDeadlineRequest,
    ) -> Result<()> {
        let path = format!(
            "projects/{}/subscriptions/{}:modifyAckDeadline",
            self.project_id, subscription_id
        );

        self.base
            .execute_request_no_response(Method::POST, &path, None, Some(request), &subscription_id)
            .await
    }
}

// --- Data Structures ---

/// Topic update request for PATCH operations
#[derive(Debug, Serialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TopicPatchRequest {
    /// The topic to update
    pub topic: TopicPatch,

    /// Field mask specifying which fields to update
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mask: Option<String>,
}

/// Topic fields for PATCH operations (uses camelCase field names as per API)
#[derive(Debug, Serialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TopicPatch {
    /// See Creating and managing labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Policy constraining the set of Google Cloud regions where messages published to the topic may be stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_storage_policy: Option<MessageStoragePolicy>,

    /// The resource name of the Cloud KMS CryptoKey to be used to protect access to messages published on this topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_name: Option<String>,

    /// Settings for validating messages published against a schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_settings: Option<SchemaSettings>,

    /// Indicates the minimum duration to retain a message after it is published to the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_retention_duration: Option<String>,

    /// Settings for ingestion from a data source into this topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingestion_data_source_settings: Option<IngestionDataSourceSettings>,

    /// Optional. Message transforms that are applied to messages published to the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_transforms: Option<Vec<MessageTransform>>,
}

/// Subscription update request for PATCH operations
#[derive(Debug, Serialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPatchRequest {
    /// The subscription to update
    pub subscription: SubscriptionPatch,

    /// Field mask specifying which fields to update
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mask: Option<String>,
}

/// Subscription fields for PATCH operations (uses camelCase field names as per API)
#[derive(Debug, Serialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPatch {
    /// If push delivery is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_config: Option<PushConfig>,

    /// The approximate amount of time Pub/Sub waits for the subscriber to acknowledge receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ack_deadline_seconds: Option<i32>,

    /// Indicates whether to retain acknowledged messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retain_acked_messages: Option<bool>,

    /// How long to retain unacknowledged messages in the subscription's backlog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_retention_duration: Option<String>,

    /// See Creating and managing labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// If true, messages published with the same orderingKey in PubsubMessage will be delivered to the subscribers in the order in which they are received by the Pub/Sub system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_message_ordering: Option<bool>,

    /// A policy that specifies the conditions for this subscription's expiration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_policy: Option<ExpirationPolicy>,

    /// An expression written in the Pub/Sub filter language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    /// A policy that specifies the conditions for dead lettering messages in this subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_letter_policy: Option<DeadLetterPolicy>,

    /// A policy that specifies how Pub/Sub retries message delivery for this subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,

    /// If delivery to BigQuery is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bigquery_config: Option<BigQueryConfig>,

    /// If delivery to Google Cloud Storage is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_storage_config: Option<CloudStorageConfig>,
}

/// A topic resource.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics#Topic
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    /// The name of the topic. Format: `projects/{project}/topics/{topic}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// See Creating and managing labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Policy constraining the set of Google Cloud regions where messages published to the topic may be stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_storage_policy: Option<MessageStoragePolicy>,

    /// The resource name of the Cloud KMS CryptoKey to be used to protect access to messages published on this topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_name: Option<String>,

    /// Settings for validating messages published against a schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_settings: Option<SchemaSettings>,

    /// Reserved for future use. This field is set only in responses from the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satisfies_pzs: Option<bool>,

    /// Indicates the minimum duration to retain a message after it is published to the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_retention_duration: Option<String>,

    /// Output only. An output-only field that indicates the current state of the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<TopicState>,

    /// Settings for ingestion from a data source into this topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingestion_data_source_settings: Option<IngestionDataSourceSettings>,

    /// Optional. Message transforms that are applied to messages published to the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_transforms: Option<Vec<MessageTransform>>,
}

/// A subscription resource.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions#Subscription
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// The name of the subscription. Format: `projects/{project}/subscriptions/{subscription}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Required. The name of the topic from which this subscription is receiving messages.
    /// Format: `projects/{project}/topics/{topic}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,

    /// If push delivery is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_config: Option<PushConfig>,

    /// The approximate amount of time Pub/Sub waits for the subscriber to acknowledge receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ack_deadline_seconds: Option<i32>,

    /// Indicates whether to retain acknowledged messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retain_acked_messages: Option<bool>,

    /// How long to retain unacknowledged messages in the subscription's backlog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_retention_duration: Option<String>,

    /// See Creating and managing labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// If true, messages published with the same orderingKey in PubsubMessage will be delivered to the subscribers in the order in which they are received by the Pub/Sub system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_message_ordering: Option<bool>,

    /// A policy that specifies the conditions for this subscription's expiration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_policy: Option<ExpirationPolicy>,

    /// An expression written in the Pub/Sub filter language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    /// A policy that specifies the conditions for dead lettering messages in this subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_letter_policy: Option<DeadLetterPolicy>,

    /// A policy that specifies how Pub/Sub retries message delivery for this subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,

    /// Indicates whether the subscription is detached from its topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,

    /// Output only. Indicates the current state of the subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<SubscriptionState>,

    /// Output only. Information about the associated Analytics Hub subscription.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics_hub_subscription_info: Option<AnalyticsHubSubscriptionInfo>,

    /// If delivery to BigQuery is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bigquery_config: Option<BigQueryConfig>,

    /// If delivery to Google Cloud Storage is used with this subscription, this field is used to configure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_storage_config: Option<CloudStorageConfig>,
}

/// A message published to a topic.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/PubsubMessage
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PubsubMessage {
    /// The message data field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,

    /// Attributes for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, String>>,

    /// ID of this message, assigned by the server when the message is published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    /// The time at which the message was published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_time: Option<String>,

    /// If non-empty, identifies related messages for which you want to ensure processing order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordering_key: Option<String>,
}

/// Request for the Publish method.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/publish#PublishRequest
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PublishRequest {
    /// Required. The messages to publish.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<PubsubMessage>,
}

/// Response for the Publish method.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/publish#PublishResponse
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PublishResponse {
    /// The server-assigned ID of each published message, in the same order as the messages in the request.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
}

/// Response for the ListTopics method.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/list#ListTopicsResponse
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListTopicsResponse {
    /// The resulting topics.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<Topic>,

    /// If not empty, indicates that there may be more topics that match the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Response for the ListSubscriptions method.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.subscriptions/list#ListSubscriptionsResponse
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListSubscriptionsResponse {
    /// The subscriptions that match the request.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<Subscription>,

    /// If not empty, indicates that there may be more subscriptions that match the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Response for the ListTopicSubscriptions method.
/// Based on: https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics.subscriptions/list#ListTopicSubscriptionsResponse
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListTopicSubscriptionsResponse {
    /// The names of subscriptions attached to the topic specified in the request.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<String>,

    /// If not empty, indicates that there may be more subscriptions that match the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Request message for SetIamPolicy method.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetIamPolicyRequest {
    /// The IAM policy to apply.
    pub policy: IamPolicy,
}

/// Request message for TestIamPermissions method.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TestIamPermissionsRequest {
    /// The set of permissions to check for the resource.
    pub permissions: Vec<String>,
}

/// Response message for TestIamPermissions method.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TestIamPermissionsResponse {
    /// A subset of TestPermissionsRequest.permissions that the caller is allowed.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
}

// --- Supporting Data Structures ---

/// A policy constraining the storage of messages published to the topic.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MessageStoragePolicy {
    /// A list of IDs of GCP regions where messages that are published to the topic may be persisted in storage.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_persistence_regions: Vec<String>,

    /// When true, as_if_in_region is used to infer allowed_persistence_regions from a snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_in_transit: Option<bool>,
}

/// Settings for validating messages published against a schema.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSettings {
    /// Required. The name of the schema that messages published should be validated against.
    /// Format: `projects/{project}/schemas/{schema}`.
    pub schema: String,

    /// The encoding of messages validated against schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<SchemaEncoding>,

    /// The minimum (inclusive) revision allowed for validating messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_revision_id: Option<String>,

    /// The maximum (inclusive) revision allowed for validating messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_revision_id: Option<String>,
}

/// Possible encoding types for messages.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchemaEncoding {
    /// Unspecified
    EncodingUnspecified,
    /// JSON encoding
    Json,
    /// Binary encoding
    Binary,
}

/// The state of the topic.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TopicState {
    /// Default value. This value is unused.
    StateUnspecified,
    /// The topic does not have any persistent errors.
    Active,
    /// Ingestion from the data source has encountered a permanent error.
    IngestionResourceError,
}

/// The state of a subscription.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionState {
    /// Default value. This value is unused.
    StateUnspecified,
    /// The subscription can actively receive messages.
    Active,
    /// The subscription cannot receive messages because of an error with the resource to which it pushes messages.
    ResourceError,
}

/// Settings for an ingestion data source on a topic.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IngestionDataSourceSettings {
    /// Optional. Platform Logs settings. If unset, no Platform Logs will be generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_logs_settings: Option<PlatformLogsSettings>,
}

/// Settings for Platform Logs produced by Pub/Sub.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PlatformLogsSettings {
    /// Optional. The minimum severity level of Platform Logs that will be written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<PlatformLogsSeverity>,
}

/// Severity levels of Platform Logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlatformLogsSeverity {
    /// Default value. Logs level is unspecified. Logs will be disabled.
    SeverityUnspecified,
    /// Logs will be disabled.
    Disabled,
    /// Debug logs and higher-severity logs will be written.
    Debug,
    /// Info logs and higher-severity logs will be written.
    Info,
    /// Warning logs and higher-severity logs will be written.
    Warning,
    /// Only error logs will be written.
    Error,
}

/// A transformation of a message.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MessageTransform {
    /// Optional. CEL expression to filter messages that are subject to transformation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transformation_template: Option<String>,
}

/// Configuration for a push delivery endpoint.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PushConfig {
    /// A URL locating the endpoint to which messages should be pushed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_endpoint: Option<String>,

    /// Endpoint configuration attributes that can be used to control different aspects of the message delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, String>>,

    /// If specified, Pub/Sub will generate and attach an OIDC JWT token as an Authorization header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_token: Option<OidcToken>,

    /// If specified, Pub/Sub will generate and attach a Pub/Sub-generated JWT as an Authorization header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_wrapper: Option<PubsubWrapper>,

    /// When set, the payload to the push endpoint is not wrapped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_wrapper: Option<NoWrapper>,
}

/// Contains information needed for generating an OpenID Connect token.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OidcToken {
    /// Service account email to be used for generating the OIDC token.
    pub service_account_email: String,

    /// Audience to be used when generating OIDC token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
}

/// Contains information needed for generating a Pub/Sub-generated JWT.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PubsubWrapper {
    /// When true, writes the Pub/Sub message metadata to x-goog-pubsub-* headers of the HTTP request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_metadata: Option<bool>,
}

/// Sets the data sent as the message body.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NoWrapper {
    /// When true, writes the Pub/Sub message metadata to x-goog-pubsub-* headers of the HTTP request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_metadata: Option<bool>,
}

/// A policy that specifies the conditions for this subscription's expiration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ExpirationPolicy {
    /// Specifies the "time-to-live" duration for an associated resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// A policy that specifies the conditions for dead lettering messages in this subscription.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeadLetterPolicy {
    /// The name of the topic to which dead letter messages should be published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_letter_topic: Option<String>,

    /// The maximum number of delivery attempts for any message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_delivery_attempts: Option<i32>,
}

/// A policy that specifies how Pub/Sub retries message delivery for this subscription.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    /// The minimum delay between consecutive deliveries of a given message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_backoff: Option<String>,

    /// The maximum delay between consecutive deliveries of a given message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_backoff: Option<String>,
}

/// Information about the associated Analytics Hub subscription.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsHubSubscriptionInfo {
    /// The name of the associated Analytics Hub listing resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<String>,

    /// The name of the associated Analytics Hub subscription resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,
}

/// Configuration for a BigQuery subscription.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BigQueryConfig {
    /// The name of the table to which to write data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,

    /// When true, use the topic's schema as the columns to write to in BigQuery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_topic_schema: Option<bool>,

    /// When true, write the subscription name, message_id, publish_time, attributes, and ordering_key to additional BigQuery columns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_metadata: Option<bool>,

    /// When true, and the table's schema is incompatible with the message data, throw an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_unknown_fields: Option<bool>,

    /// Output only. An output-only field that indicates whether or not the subscription can receive messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<BigQueryConfigState>,

    /// When true, use the BigQuery table's schema as the columns to write to in BigQuery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_table_schema: Option<bool>,

    /// Output only. An output-only field that indicates the last time that the BigQuery table was updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Possible states for a BigQuery subscription.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BigQueryConfigState {
    /// Default value. This value is unused.
    StateUnspecified,
    /// The subscription can actively send messages to BigQuery.
    Active,
    /// Cannot write to the BigQuery table because of permission denied errors.
    PermissionDenied,
    /// Cannot write to the BigQuery table because it does not exist.
    NotFound,
    /// Cannot write to the BigQuery table due to a schema mismatch.
    SchemaMismatch,
    /// Cannot write to the destination because enforce_in_transit is set to true and the destination locations are not in the allowed regions.
    InTransitLocationRestriction,
}

/// Configuration for a Cloud Storage subscription.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CloudStorageConfig {
    /// Required. User-provided name for the Cloud Storage bucket.
    pub bucket: String,

    /// User-provided prefix for Cloud Storage filename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename_prefix: Option<String>,

    /// User-provided suffix for Cloud Storage filename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename_suffix: Option<String>,

    /// User-provided format string specifying how to represent datetimes in Cloud Storage filenames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename_datetime_format: Option<String>,

    /// The maximum duration that can elapse before a new Cloud Storage file is created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_duration: Option<String>,

    /// The maximum bytes that can be written to a Cloud Storage file before a new file is created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,

    /// The maximum number of messages that can be written to a Cloud Storage file before a new file is created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<i64>,

    /// Output only. An output-only field that indicates whether or not the subscription can receive messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<CloudStorageConfigState>,

    /// Configuration for writing message data in text format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_config: Option<CloudStorageTextConfig>,

    /// Configuration for writing message data in Avro format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avro_config: Option<CloudStorageAvroConfig>,
}

/// Possible states for a Cloud Storage subscription.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CloudStorageConfigState {
    /// Default value. This value is unused.
    StateUnspecified,
    /// The subscription can actively send messages to Cloud Storage.
    Active,
    /// Cannot write to the Cloud Storage bucket because of permission denied errors.
    PermissionDenied,
    /// Cannot write to the Cloud Storage bucket because it does not exist.
    NotFound,
    /// Cannot write to the destination because enforce_in_transit is set to true and the destination locations are not in the allowed regions.
    InTransitLocationRestriction,
}

/// Configuration for writing message data in text format.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CloudStorageTextConfig {
    // This type has no fields - message data is written as raw text.
}

/// Configuration for writing message data in Avro format.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CloudStorageAvroConfig {
    /// When true, write the subscription name, message_id, publish_time, attributes, and ordering_key as additional fields in the output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_metadata: Option<bool>,

    /// When true, the output Cloud Storage file will be serialized using the topic schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_topic_schema: Option<bool>,
}

// --- Message pulling and acknowledgment structures ---

/// Request for pulling messages from a subscription
#[derive(Debug, Serialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// The maximum number of messages to return for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<i32>,

    /// If this field set to true, the system will respond immediately even if it there are no messages available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_immediately: Option<bool>,

    /// The ack deadline for returned messages. If this parameter is 0, the messages are made available for another pull request immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_excess_messages: Option<bool>,
}

/// Response from pulling messages
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResponse {
    /// The received messages
    #[serde(default)]
    pub received_messages: Vec<ReceivedMessage>,
}

/// A message and its ack_id for pulling
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedMessage {
    /// The acknowledgment ID for this message
    pub ack_id: String,

    /// The message
    pub message: PubsubMessage,

    /// The approximate number of times that Cloud Pub/Sub has attempted to deliver the message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_attempt: Option<i32>,
}

/// Request for acknowledging messages
#[derive(Debug, Serialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AcknowledgeRequest {
    /// The acknowledgment IDs for the messages being acknowledged
    pub ack_ids: Vec<String>,
}

/// Request for modifying ack deadline
#[derive(Debug, Serialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ModifyAckDeadlineRequest {
    /// The acknowledgment IDs for the messages whose deadline is being modified
    pub ack_ids: Vec<String>,

    /// The new ack deadline in seconds
    pub ack_deadline_seconds: i32,
}
