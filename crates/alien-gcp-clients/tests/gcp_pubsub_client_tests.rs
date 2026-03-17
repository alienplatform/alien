#![cfg(all(test, feature = "gcp"))]

use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::pubsub::{
    ExpirationPolicy, MessageStoragePolicy, PubSubApi, PubSubClient, PublishRequest, PubsubMessage,
    PushConfig, Subscription, SubscriptionPatch, Topic, TopicPatch,
};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct PubSubTestContext {
    client: PubSubClient,
    project_id: String,
    created_topics: Mutex<HashSet<String>>,
    created_subscriptions: Mutex<HashSet<String>>,
}

impl AsyncTestContext for PubSubTestContext {
    async fn setup() -> PubSubTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
        };

        let client = PubSubClient::new(Client::new(), config);

        PubSubTestContext {
            client,
            project_id,
            created_topics: Mutex::new(HashSet::new()),
            created_subscriptions: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Pub/Sub test cleanup...");

        // Clean up subscriptions first (they depend on topics)
        let subscriptions_to_cleanup = {
            let subscriptions = self.created_subscriptions.lock().unwrap();
            subscriptions.clone()
        };

        for subscription_id in subscriptions_to_cleanup {
            self.cleanup_subscription(&subscription_id).await;
        }

        // Then clean up topics
        let topics_to_cleanup = {
            let topics = self.created_topics.lock().unwrap();
            topics.clone()
        };

        for topic_id in topics_to_cleanup {
            self.cleanup_topic(&topic_id).await;
        }

        info!("✅ Pub/Sub test cleanup completed");
    }
}

impl PubSubTestContext {
    fn track_topic(&self, topic_id: &str) {
        let mut topics = self.created_topics.lock().unwrap();
        topics.insert(topic_id.to_string());
        info!("📝 Tracking topic for cleanup: {}", topic_id);
    }

    fn untrack_topic(&self, topic_id: &str) {
        let mut topics = self.created_topics.lock().unwrap();
        topics.remove(topic_id);
        info!(
            "✅ Topic {} successfully cleaned up and untracked",
            topic_id
        );
    }

    fn track_subscription(&self, subscription_id: &str) {
        let mut subscriptions = self.created_subscriptions.lock().unwrap();
        subscriptions.insert(subscription_id.to_string());
        info!("📝 Tracking subscription for cleanup: {}", subscription_id);
    }

    fn untrack_subscription(&self, subscription_id: &str) {
        let mut subscriptions = self.created_subscriptions.lock().unwrap();
        subscriptions.remove(subscription_id);
        info!(
            "✅ Subscription {} successfully cleaned up and untracked",
            subscription_id
        );
    }

    async fn cleanup_topic(&self, topic_id: &str) {
        match self.client.delete_topic(topic_id.to_string()).await {
            Ok(_) => {
                info!("🗑️ Successfully deleted topic: {}", topic_id);
                self.untrack_topic(topic_id);
            }
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(
                    "🔍 Topic {} not found during cleanup (already deleted)",
                    topic_id
                );
                self.untrack_topic(topic_id);
            }
            Err(e) => {
                warn!("⚠️ Failed to delete topic {}: {}", topic_id, e);
            }
        }
    }

    async fn cleanup_subscription(&self, subscription_id: &str) {
        match self
            .client
            .delete_subscription(subscription_id.to_string())
            .await
        {
            Ok(_) => {
                info!("🗑️ Successfully deleted subscription: {}", subscription_id);
                self.untrack_subscription(subscription_id);
            }
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(
                    "🔍 Subscription {} not found during cleanup (already deleted)",
                    subscription_id
                );
                self.untrack_subscription(subscription_id);
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to delete subscription {}: {}",
                    subscription_id, e
                );
            }
        }
    }

    fn generate_unique_topic_id(&self) -> String {
        format!("alien-test-topic-{}", Uuid::new_v4().simple())
    }

    fn generate_unique_subscription_id(&self) -> String {
        format!("alien-test-subscription-{}", Uuid::new_v4().simple())
    }
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_topic_lifecycle(ctx: &PubSubTestContext) {
    info!("🚀 Starting topic lifecycle test");

    let topic_id = ctx.generate_unique_topic_id();
    info!("📝 Using topic ID: {}", topic_id);

    // Create a topic
    let mut labels = HashMap::new();
    labels.insert("test".to_string(), "true".to_string());
    labels.insert("environment".to_string(), "test".to_string());

    let create_topic = Topic::builder().labels(labels.clone()).build();

    let created_topic = ctx
        .client
        .create_topic(topic_id.clone(), create_topic)
        .await
        .expect("Failed to create topic");

    ctx.track_topic(&topic_id);

    info!("✅ Created topic: {:?}", created_topic.name);

    // Verify the topic was created with expected properties
    assert!(created_topic.name.is_some());
    assert!(created_topic.name.as_ref().unwrap().contains(&topic_id));
    assert_eq!(created_topic.labels, Some(labels.clone()));

    // Get the topic
    let retrieved_topic = ctx
        .client
        .get_topic(topic_id.clone())
        .await
        .expect("Failed to get topic");

    assert_eq!(created_topic.name, retrieved_topic.name);
    assert_eq!(created_topic.labels, retrieved_topic.labels);

    // Update the topic with additional metadata
    labels.insert("updated".to_string(), "true".to_string());
    let message_storage_policy = MessageStoragePolicy::builder()
        .allowed_persistence_regions(vec!["us-central1".to_string(), "us-east1".to_string()])
        .build();

    let update_topic = TopicPatch::builder()
        .labels(labels.clone())
        .message_storage_policy(message_storage_policy)
        .build();

    let updated_topic = ctx
        .client
        .patch_topic(
            topic_id.clone(),
            update_topic,
            Some("labels,messageStoragePolicy".to_string()),
        )
        .await
        .expect("Failed to update topic");

    assert_eq!(updated_topic.labels, Some(labels));
    assert!(updated_topic.message_storage_policy.is_some());

    // List topics to verify it appears
    let topics_response = ctx
        .client
        .list_topics(Some(100), None)
        .await
        .expect("Failed to list topics");

    let found = topics_response.topics.iter().any(|t| {
        t.name
            .as_ref()
            .map(|n| n.contains(&topic_id))
            .unwrap_or(false)
    });
    assert!(found, "Created topic should appear in list");

    // Delete the topic
    ctx.client
        .delete_topic(topic_id.clone())
        .await
        .expect("Failed to delete topic");

    ctx.untrack_topic(&topic_id);

    // Verify the topic is deleted
    let get_result = ctx.client.get_topic(topic_id.clone()).await;
    assert!(matches!(
        get_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    info!("✅ Topic lifecycle test completed successfully");
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_subscription_lifecycle(ctx: &PubSubTestContext) {
    info!("🚀 Starting subscription lifecycle test");

    let topic_id = ctx.generate_unique_topic_id();
    let subscription_id = ctx.generate_unique_subscription_id();
    info!(
        "📝 Using topic ID: {}, subscription ID: {}",
        topic_id, subscription_id
    );

    // First create a topic for the subscription
    let create_topic = Topic::builder().build();

    ctx.client
        .create_topic(topic_id.clone(), create_topic)
        .await
        .expect("Failed to create topic for subscription test");

    ctx.track_topic(&topic_id);

    // Create a push subscription
    let topic_name = format!("projects/{}/topics/{}", ctx.project_id, topic_id);

    let mut labels = HashMap::new();
    labels.insert("test".to_string(), "true".to_string());

    let push_config = PushConfig::builder()
        .push_endpoint("https://example.com/webhook".to_string())
        .build();

    let expiration_policy = ExpirationPolicy::builder()
        .ttl("86400s".to_string()) // 24 hours
        .build();

    let create_subscription = Subscription::builder()
        .topic(topic_name.clone())
        .push_config(push_config)
        .ack_deadline_seconds(600)
        .labels(labels.clone())
        .expiration_policy(expiration_policy)
        .enable_message_ordering(true)
        .build();

    let created_subscription = ctx
        .client
        .create_subscription(subscription_id.clone(), create_subscription)
        .await
        .expect("Failed to create subscription");

    ctx.track_subscription(&subscription_id);

    info!("✅ Created subscription: {:?}", created_subscription.name);

    // Verify the subscription was created with expected properties
    assert!(created_subscription.name.is_some());
    assert!(created_subscription
        .name
        .as_ref()
        .unwrap()
        .contains(&subscription_id));
    assert_eq!(created_subscription.topic, Some(topic_name));
    assert_eq!(created_subscription.ack_deadline_seconds, Some(600));
    assert_eq!(created_subscription.labels, Some(labels.clone()));
    assert_eq!(created_subscription.enable_message_ordering, Some(true));
    assert!(created_subscription.push_config.is_some());

    // Get the subscription
    let retrieved_subscription = ctx
        .client
        .get_subscription(subscription_id.clone())
        .await
        .expect("Failed to get subscription");

    assert_eq!(created_subscription.name, retrieved_subscription.name);
    assert_eq!(created_subscription.topic, retrieved_subscription.topic);

    // Update the subscription
    labels.insert("updated".to_string(), "true".to_string());
    let update_subscription = SubscriptionPatch::builder()
        .ack_deadline_seconds(300)
        .labels(labels.clone())
        .build();

    let updated_subscription = ctx
        .client
        .patch_subscription(
            subscription_id.clone(),
            update_subscription,
            Some("ackDeadlineSeconds,labels".to_string()),
        )
        .await
        .expect("Failed to update subscription");

    assert_eq!(updated_subscription.ack_deadline_seconds, Some(300));
    assert_eq!(updated_subscription.labels, Some(labels));

    // List subscriptions to verify it appears
    let subscriptions_response = ctx
        .client
        .list_subscriptions(Some(100), None)
        .await
        .expect("Failed to list subscriptions");

    let found = subscriptions_response.subscriptions.iter().any(|s| {
        s.name
            .as_ref()
            .map(|n| n.contains(&subscription_id))
            .unwrap_or(false)
    });
    assert!(found, "Created subscription should appear in list");

    // List topic subscriptions
    let topic_subscriptions_response = ctx
        .client
        .list_topic_subscriptions(topic_id.clone(), Some(100), None)
        .await
        .expect("Failed to list topic subscriptions");

    let found_in_topic = topic_subscriptions_response
        .subscriptions
        .iter()
        .any(|s| s.contains(&subscription_id));
    assert!(
        found_in_topic,
        "Created subscription should appear in topic subscriptions list"
    );

    // Delete the subscription
    ctx.client
        .delete_subscription(subscription_id.clone())
        .await
        .expect("Failed to delete subscription");

    ctx.untrack_subscription(&subscription_id);

    // Verify the subscription is deleted
    let get_result = ctx.client.get_subscription(subscription_id.clone()).await;
    assert!(matches!(
        get_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    info!("✅ Subscription lifecycle test completed successfully");
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_publish_messages(ctx: &PubSubTestContext) {
    info!("🚀 Starting message publishing test");

    let topic_id = ctx.generate_unique_topic_id();
    info!("📝 Using topic ID: {}", topic_id);

    // Create a topic for publishing
    let create_topic = Topic::builder().build();

    ctx.client
        .create_topic(topic_id.clone(), create_topic)
        .await
        .expect("Failed to create topic for publishing test");

    ctx.track_topic(&topic_id);

    // Create messages to publish
    let mut attributes1 = HashMap::new();
    attributes1.insert("source".to_string(), "test".to_string());
    attributes1.insert("type".to_string(), "hello".to_string());

    let mut attributes2 = HashMap::new();
    attributes2.insert("source".to_string(), "test".to_string());
    attributes2.insert("type".to_string(), "world".to_string());

    let message1 = PubsubMessage::builder()
        .data(base64_standard.encode("Hello, Pub/Sub!"))
        .attributes(attributes1)
        .ordering_key("test-ordering-key".to_string())
        .build();

    let message2 = PubsubMessage::builder()
        .data(base64_standard.encode("This is a test message"))
        .attributes(attributes2)
        .ordering_key("test-ordering-key".to_string())
        .build();

    let publish_request = PublishRequest::builder()
        .messages(vec![message1, message2])
        .build();

    // Publish the messages
    let publish_response = ctx
        .client
        .publish(topic_id.clone(), publish_request)
        .await
        .expect("Failed to publish messages");

    info!("✅ Published messages: {:?}", publish_response.message_ids);

    // Verify we got message IDs back
    assert_eq!(publish_response.message_ids.len(), 2);
    assert!(!publish_response.message_ids[0].is_empty());
    assert!(!publish_response.message_ids[1].is_empty());

    // Test publishing a single message
    let single_message = PubsubMessage::builder()
        .data(
            base64_standard.encode("{\"test\": \"json\", \"timestamp\": \"2024-01-01T00:00:00Z\"}"),
        )
        .build();

    let single_publish_request = PublishRequest::builder()
        .messages(vec![single_message])
        .build();

    let single_publish_response = ctx
        .client
        .publish(topic_id.clone(), single_publish_request)
        .await
        .expect("Failed to publish single message");

    assert_eq!(single_publish_response.message_ids.len(), 1);
    assert!(!single_publish_response.message_ids[0].is_empty());

    info!("✅ Message publishing test completed successfully");
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_iam_policy_operations(ctx: &PubSubTestContext) {
    info!("🚀 Starting IAM policy operations test");

    let topic_id = ctx.generate_unique_topic_id();
    let subscription_id = ctx.generate_unique_subscription_id();
    info!(
        "📝 Using topic ID: {}, subscription ID: {}",
        topic_id, subscription_id
    );

    // Create a topic
    let create_topic = Topic::builder().build();

    ctx.client
        .create_topic(topic_id.clone(), create_topic)
        .await
        .expect("Failed to create topic for IAM test");

    ctx.track_topic(&topic_id);

    // Create a subscription
    let topic_name = format!("projects/{}/topics/{}", ctx.project_id, topic_id);
    let create_subscription = Subscription::builder().topic(topic_name).build();

    ctx.client
        .create_subscription(subscription_id.clone(), create_subscription)
        .await
        .expect("Failed to create subscription for IAM test");

    ctx.track_subscription(&subscription_id);

    // Test topic IAM operations
    // Get initial IAM policy for topic
    let initial_topic_policy = ctx
        .client
        .get_topic_iam_policy(topic_id.clone())
        .await
        .expect("Failed to get initial topic IAM policy");

    info!("📋 Initial topic IAM policy: {:?}", initial_topic_policy);

    // Use the service account email (org policy blocks allUsers)
    let test_member = {
        let gcp_credentials_json = std::env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").unwrap();
        let sa_value: serde_json::Value = serde_json::from_str(&gcp_credentials_json).unwrap();
        let email = sa_value
            .get("client_email")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("client_email must be in service account JSON");
        format!("serviceAccount:{}", email)
    };

    // Add a binding to the topic policy
    let mut bindings = initial_topic_policy.bindings.clone();
    bindings.push(
        Binding::builder()
            .role("roles/pubsub.viewer".to_string())
            .members(vec![test_member])
            .build(),
    );

    let updated_topic_policy = IamPolicy::builder()
        .maybe_version(initial_topic_policy.version)
        .bindings(bindings)
        .maybe_etag(initial_topic_policy.etag)
        .build();

    let set_topic_policy_result = ctx
        .client
        .set_topic_iam_policy(topic_id.clone(), updated_topic_policy)
        .await
        .expect("Failed to set topic IAM policy");

    info!("✅ Updated topic IAM policy: {:?}", set_topic_policy_result);

    // Test IAM permissions for topic
    let permissions_to_test = vec![
        "pubsub.topics.get".to_string(),
        "pubsub.topics.publish".to_string(),
        "pubsub.topics.delete".to_string(),
    ];

    let topic_permissions_result = ctx
        .client
        .test_topic_iam_permissions(topic_id.clone(), permissions_to_test.clone())
        .await
        .expect("Failed to test topic IAM permissions");

    info!(
        "🔐 Topic permissions result: {:?}",
        topic_permissions_result
    );

    // We should have at least some permissions (likely all since we're using a management service account)
    assert!(!topic_permissions_result.permissions.is_empty());

    // Test subscription IAM operations
    // Get initial IAM policy for subscription
    let initial_subscription_policy = ctx
        .client
        .get_subscription_iam_policy(subscription_id.clone())
        .await
        .expect("Failed to get initial subscription IAM policy");

    info!(
        "📋 Initial subscription IAM policy: {:?}",
        initial_subscription_policy
    );

    // Test IAM permissions for subscription
    let subscription_permissions_to_test = vec![
        "pubsub.subscriptions.get".to_string(),
        "pubsub.subscriptions.consume".to_string(),
        "pubsub.subscriptions.delete".to_string(),
    ];

    let subscription_permissions_result = ctx
        .client
        .test_subscription_iam_permissions(
            subscription_id.clone(),
            subscription_permissions_to_test.clone(),
        )
        .await
        .expect("Failed to test subscription IAM permissions");

    info!(
        "🔐 Subscription permissions result: {:?}",
        subscription_permissions_result
    );

    // We should have at least some permissions
    assert!(!subscription_permissions_result.permissions.is_empty());

    info!("✅ IAM policy operations test completed successfully");
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_error_handling(ctx: &PubSubTestContext) {
    info!("🚀 Starting error handling test");

    let non_existent_topic_id = ctx.generate_unique_topic_id();
    let non_existent_subscription_id = ctx.generate_unique_subscription_id();

    // Test getting non-existent topic
    let get_topic_result = ctx.client.get_topic(non_existent_topic_id.clone()).await;
    assert!(matches!(
        get_topic_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    // Test getting non-existent subscription
    let get_subscription_result = ctx
        .client
        .get_subscription(non_existent_subscription_id.clone())
        .await;
    assert!(matches!(
        get_subscription_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    // Test deleting non-existent topic
    let delete_topic_result = ctx.client.delete_topic(non_existent_topic_id.clone()).await;
    assert!(matches!(
        delete_topic_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    // Test deleting non-existent subscription
    let delete_subscription_result = ctx
        .client
        .delete_subscription(non_existent_subscription_id.clone())
        .await;
    assert!(matches!(
        delete_subscription_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    // Test publishing to non-existent topic
    let message = PubsubMessage::builder()
        .data(base64_standard.encode("test message"))
        .build();

    let publish_request = PublishRequest::builder().messages(vec![message]).build();

    let publish_result = ctx
        .client
        .publish(non_existent_topic_id.clone(), publish_request)
        .await;
    assert!(matches!(
        publish_result,
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        })
    ));

    // Test creating subscription with invalid topic
    let invalid_subscription = Subscription::builder()
        .topic("projects/invalid-project/topics/invalid-topic".to_string())
        .build();

    let create_subscription_result = ctx
        .client
        .create_subscription("test-invalid-sub".to_string(), invalid_subscription)
        .await;

    // This should fail with either resource not found or invalid input
    assert!(create_subscription_result.is_err());

    info!("✅ Error handling test completed successfully");
}

#[test_context(PubSubTestContext)]
#[tokio::test]
async fn test_topic_with_advanced_features(ctx: &PubSubTestContext) {
    info!("🚀 Starting advanced topic features test");

    let topic_id = ctx.generate_unique_topic_id();
    info!("📝 Using topic ID: {}", topic_id);

    // Create a topic with advanced features
    let mut labels = HashMap::new();
    labels.insert("test".to_string(), "advanced".to_string());
    labels.insert("feature".to_string(), "full".to_string());

    let message_storage_policy = MessageStoragePolicy::builder()
        .allowed_persistence_regions(vec!["us-central1".to_string(), "us-east1".to_string()])
        .enforce_in_transit(true)
        .build();

    let create_topic = Topic::builder()
        .labels(labels.clone())
        .message_storage_policy(message_storage_policy)
        .message_retention_duration("86400s".to_string()) // 24 hours
        .satisfies_pzs(false)
        .build();

    let created_topic = ctx
        .client
        .create_topic(topic_id.clone(), create_topic)
        .await
        .expect("Failed to create advanced topic");

    ctx.track_topic(&topic_id);

    info!("✅ Created advanced topic: {:?}", created_topic.name);

    // Verify all the features were set correctly
    assert_eq!(created_topic.labels, Some(labels));
    assert!(created_topic.message_storage_policy.is_some());
    assert_eq!(
        created_topic.message_retention_duration,
        Some("86400s".to_string())
    );

    let storage_policy = created_topic.message_storage_policy.unwrap();
    assert_eq!(
        storage_policy.allowed_persistence_regions,
        vec!["us-central1", "us-east1"]
    );
    assert_eq!(storage_policy.enforce_in_transit, Some(true));

    info!("✅ Advanced topic features test completed successfully");
}
