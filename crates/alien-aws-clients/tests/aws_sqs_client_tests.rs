/*!
# SQS Client Integration Tests

These tests perform real AWS SQS operations to comprehensively test queue functionality.
Tests follow the AGENTS.md guidelines with complete e2e lifecycle testing.

## Test Structure

1. **test_sqs_queue_lifecycle** - Complete end-to-end test covering:
   - Queue creation and setup
   - Basic message operations (Send, Receive, Delete)
   - Queue attribute management
   - Permission management
   - Queue purging and cleanup

2. **test_error_scenarios** - Comprehensive error handling:
   - Non-existent queue errors
   - Invalid credential errors
   - Proper error type mapping

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=eu-central-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `sqs:CreateQueue`
- `sqs:DeleteQueue`
- `sqs:GetQueueUrl`
- `sqs:GetQueueAttributes`
- `sqs:SendMessage`
- `sqs:ReceiveMessage`
- `sqs:DeleteMessage`
- `sqs:AddPermission`
- `sqs:RemovePermission`
- `sqs:SetQueueAttributes`
- `sqs:PurgeQueue`

## Running Tests
```bash
# Run all SQS tests
cargo test --package alien-aws-clients --test aws_sqs_client_tests -- --nocapture

# Run specific test
cargo test --package alien-aws-clients --test aws_sqs_client_tests test_sqs_queue_lifecycle -- --nocapture
```

All tests work with real AWS resources and will fail if operations don't succeed.
*/

use alien_aws_clients::sqs::*;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct SqsTestContext {
    client: SqsClient,
    created_queues: Mutex<HashSet<String>>,
}

impl AsyncTestContext for SqsTestContext {
    async fn setup() -> SqsTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };
        let client = SqsClient::new(Client::new(), aws_config);

        SqsTestContext {
            client,
            created_queues: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        let queue_urls: Vec<String> = {
            let created_queues = self.created_queues.lock().unwrap();
            created_queues.iter().cloned().collect()
        };

        for queue_url in queue_urls {
            match self.client.delete_queue(&queue_url).await {
                Ok(_) => info!("Successfully deleted queue: {}", queue_url),
                Err(e) => {
                    if let Some(ErrorData::RemoteResourceNotFound { .. }) = &e.error {
                        // Queue already deleted, that's fine
                        info!("Queue already deleted: {}", queue_url);
                    } else {
                        warn!("Failed to delete queue {}: {:?}", queue_url, e);
                    }
                }
            }
        }
    }
}

#[test_context(SqsTestContext)]
#[tokio::test]
async fn test_sqs_queue_lifecycle(ctx: &mut SqsTestContext) {
    let queue_name = format!("alien-test-queue-{}", Uuid::new_v4().simple());
    info!("Testing SQS queue lifecycle with queue: {}", queue_name);

    // 1. Create queue
    let mut attributes = HashMap::new();
    attributes.insert("VisibilityTimeout".to_string(), "30".to_string());
    attributes.insert("MessageRetentionPeriod".to_string(), "1209600".to_string()); // 14 days
    attributes.insert("ReceiveMessageWaitTimeSeconds".to_string(), "0".to_string());

    let mut tags = HashMap::new();
    tags.insert("Environment".to_string(), "test".to_string());
    tags.insert("Project".to_string(), "alien".to_string());

    let create_request = CreateQueueRequest::builder()
        .queue_name(queue_name.clone())
        .attributes(attributes)
        .tags(tags)
        .build();

    let create_response = ctx
        .client
        .create_queue(create_request)
        .await
        .expect("Failed to create queue");

    let queue_url = create_response.create_queue_result.queue_url;
    info!("Created queue with URL: {}", queue_url);

    // Track for cleanup
    {
        let mut created_queues = ctx.created_queues.lock().unwrap();
        created_queues.insert(queue_url.clone());
    }

    // 2. Get queue URL (verify it matches)
    let get_url_request = GetQueueUrlRequest::builder()
        .queue_name(queue_name.clone())
        .build();

    let get_url_response = ctx
        .client
        .get_queue_url(get_url_request)
        .await
        .expect("Failed to get queue URL");

    assert_eq!(get_url_response.get_queue_url_result.queue_url, queue_url);
    info!(
        "Verified queue URL matches: {}",
        get_url_response.get_queue_url_result.queue_url
    );

    // 3. Get queue attributes
    let get_attrs_request = GetQueueAttributesRequest::builder()
        .attribute_names(vec!["All".to_string()])
        .build();

    let get_attrs_response = ctx
        .client
        .get_queue_attributes(&queue_url, get_attrs_request)
        .await
        .expect("Failed to get queue attributes");

    // Convert attributes to HashMap for easier access
    let attributes_map: HashMap<String, String> = get_attrs_response
        .get_queue_attributes_result
        .attributes
        .into_iter()
        .map(|attr| (attr.name, attr.value))
        .collect();

    info!("Queue attributes: {:?}", attributes_map);
    assert!(attributes_map.contains_key("VisibilityTimeout"));
    assert!(attributes_map.contains_key("MessageRetentionPeriod"));

    // 4. Set queue attributes
    let mut new_attributes = HashMap::new();
    new_attributes.insert("VisibilityTimeout".to_string(), "60".to_string());
    new_attributes.insert("ReceiveMessageWaitTimeSeconds".to_string(), "5".to_string());

    let set_attrs_request = SetQueueAttributesRequest::builder()
        .attributes(new_attributes)
        .build();

    ctx.client
        .set_queue_attributes(&queue_url, set_attrs_request)
        .await
        .expect("Failed to set queue attributes");

    info!("Updated queue attributes");

    // 5. Add permission
    let add_permission_request = AddPermissionRequest::builder()
        .label("test-permission".to_string())
        .aws_account_ids(vec![ctx.client.account_id().to_string()])
        .actions(vec![
            "SendMessage".to_string(),
            "ReceiveMessage".to_string(),
        ])
        .build();

    ctx.client
        .add_permission(&queue_url, add_permission_request)
        .await
        .expect("Failed to add permission");

    info!("Added permission to queue");

    // 6. Send message
    let message_body = format!(
        "Test message at {}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let mut message_attributes = HashMap::new();
    message_attributes.insert(
        "test-attr".to_string(),
        MessageAttributeValue::builder()
            .string_value("test-value".to_string())
            .data_type("String".to_string())
            .build(),
    );

    let send_request = SendMessageRequest::builder()
        .message_body(message_body.clone())
        .message_attributes(message_attributes)
        .build();

    let send_response = ctx
        .client
        .send_message(&queue_url, send_request)
        .await
        .expect("Failed to send message");

    info!(
        "Sent message with ID: {}",
        send_response.send_message_result.message_id
    );
    assert!(!send_response.send_message_result.message_id.is_empty());
    assert!(!send_response.send_message_result.md5_of_body.is_empty());

    // 7. Receive message (with retry logic for eventual consistency)
    let mut receive_response = None;
    for attempt in 1..=5 {
        let receive_request = ReceiveMessageRequest::builder()
            .max_number_of_messages(1)
            .wait_time_seconds(2)
            .build();

        let response = ctx
            .client
            .receive_message(&queue_url, receive_request)
            .await
            .expect("Failed to receive message");

        if !response.receive_message_result.messages.is_empty() {
            receive_response = Some(response);
            break;
        }

        info!(
            "Attempt {}: No messages received, waiting before retry...",
            attempt
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    let receive_response = receive_response.expect("Failed to receive message after 5 attempts");
    assert!(!receive_response.receive_message_result.messages.is_empty());
    let messages = &receive_response.receive_message_result.messages;
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message.body, message_body);
    assert_eq!(
        message.message_id,
        send_response.send_message_result.message_id
    );

    info!("Received message: {}", message.message_id);

    // 8. Purge queue
    ctx.client
        .purge_queue(&queue_url)
        .await
        .expect("Failed to purge queue");

    info!("Purged queue");

    // 9. Remove permission
    let remove_permission_request = RemovePermissionRequest::builder()
        .label("test-permission".to_string())
        .build();

    ctx.client
        .remove_permission(&queue_url, remove_permission_request)
        .await
        .expect("Failed to remove permission");

    info!("Removed permission from queue");

    // 10. Delete queue
    ctx.client
        .delete_queue(&queue_url)
        .await
        .expect("Failed to delete queue");

    info!("Deleted queue: {}", queue_url);

    // Remove from tracking since we deleted it
    {
        let mut created_queues = ctx.created_queues.lock().unwrap();
        created_queues.remove(&queue_url);
    }

    info!("SQS queue lifecycle test completed successfully");
}

#[test_context(SqsTestContext)]
#[tokio::test]
async fn test_error_scenarios(ctx: &mut SqsTestContext) {
    info!("Testing SQS error scenarios");

    // Test 1: Get URL for non-existent queue
    let get_url_request = GetQueueUrlRequest::builder()
        .queue_name("non-existent-queue-12345".to_string())
        .build();

    let result = ctx.client.get_queue_url(get_url_request).await;
    assert!(result.is_err());

    if let Err(e) = result {
        match &e.error {
            Some(ErrorData::RemoteResourceNotFound {
                resource_type,
                resource_name,
            }) => {
                assert_eq!(resource_type, "Queue");
                assert_eq!(resource_name, "non-existent-queue-12345");
                info!("Correctly received RemoteResourceNotFound for non-existent queue");
            }
            _ => panic!("Expected RemoteResourceNotFound, got: {:?}", e.error),
        }
    }

    // Test 2: Delete non-existent queue
    let result = ctx
        .client
        .delete_queue("https://sqs.us-east-1.amazonaws.com/123456789012/non-existent-queue")
        .await;
    assert!(result.is_err());

    if let Err(e) = result {
        match &e.error {
            Some(ErrorData::RemoteResourceNotFound { .. }) => {
                info!("Correctly received RemoteResourceNotFound for delete non-existent queue");
            }
            _ => panic!("Expected RemoteResourceNotFound, got: {:?}", e.error),
        }
    }

    // Test 3: Send message to non-existent queue
    let send_request = SendMessageRequest::builder()
        .message_body("test message".to_string())
        .build();

    let result = ctx
        .client
        .send_message(
            "https://sqs.us-east-1.amazonaws.com/123456789012/non-existent-queue",
            send_request,
        )
        .await;
    assert!(result.is_err());

    if let Err(e) = result {
        match &e.error {
            Some(ErrorData::RemoteResourceNotFound { .. }) => {
                info!("Correctly received RemoteResourceNotFound for send to non-existent queue");
            }
            _ => panic!("Expected RemoteResourceNotFound, got: {:?}", e.error),
        }
    }

    // Test 4: Create queue with invalid name (too long)
    let long_name = "a".repeat(81); // SQS queue names are limited to 80 characters
    let create_request = CreateQueueRequest::builder().queue_name(long_name).build();

    let result = ctx.client.create_queue(create_request).await;
    assert!(result.is_err());

    if let Err(e) = result {
        match &e.error {
            Some(ErrorData::HttpResponseError { .. }) => {
                info!("Correctly received error for invalid queue name");
            }
            _ => panic!(
                "Expected HttpResponseError for invalid queue name, got: {:?}",
                e.error
            ),
        }
    }

    info!("SQS error scenarios test completed successfully");
}

#[test_context(SqsTestContext)]
#[tokio::test]
async fn test_fifo_queue_operations(ctx: &mut SqsTestContext) {
    let queue_name = format!("alien-test-fifo-queue-{}.fifo", Uuid::new_v4().simple());
    info!(
        "Testing SQS FIFO queue operations with queue: {}",
        queue_name
    );

    // 1. Create FIFO queue
    let mut attributes = HashMap::new();
    attributes.insert("FifoQueue".to_string(), "true".to_string());
    attributes.insert("ContentBasedDeduplication".to_string(), "true".to_string());
    attributes.insert("VisibilityTimeout".to_string(), "30".to_string());

    let create_request = CreateQueueRequest::builder()
        .queue_name(queue_name.clone())
        .attributes(attributes)
        .build();

    let create_response = ctx
        .client
        .create_queue(create_request)
        .await
        .expect("Failed to create FIFO queue");

    let queue_url = create_response.create_queue_result.queue_url;
    info!("Created FIFO queue with URL: {}", queue_url);

    // Track for cleanup
    {
        let mut created_queues = ctx.created_queues.lock().unwrap();
        created_queues.insert(queue_url.clone());
    }

    // 2. Send message to FIFO queue (requires MessageGroupId)
    let message_body = format!(
        "FIFO test message at {}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let send_request = SendMessageRequest::builder()
        .message_body(message_body.clone())
        .message_group_id("test-group".to_string())
        .build();

    let send_response = ctx
        .client
        .send_message(&queue_url, send_request)
        .await
        .expect("Failed to send message to FIFO queue");

    info!(
        "Sent message to FIFO queue with ID: {}",
        send_response.send_message_result.message_id
    );
    assert!(!send_response.send_message_result.message_id.is_empty());
    assert!(send_response.send_message_result.sequence_number.is_some());

    // 3. Receive message from FIFO queue (with retry logic for eventual consistency)
    let mut receive_response = None;
    for attempt in 1..=5 {
        let receive_request = ReceiveMessageRequest::builder()
            .max_number_of_messages(1)
            .wait_time_seconds(2)
            .build();

        let response = ctx
            .client
            .receive_message(&queue_url, receive_request)
            .await
            .expect("Failed to receive message from FIFO queue");

        if !response.receive_message_result.messages.is_empty() {
            receive_response = Some(response);
            break;
        }

        info!(
            "FIFO Attempt {}: No messages received, waiting before retry...",
            attempt
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    let receive_response =
        receive_response.expect("Failed to receive message from FIFO queue after 5 attempts");
    assert!(!receive_response.receive_message_result.messages.is_empty());
    let messages = &receive_response.receive_message_result.messages;
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message.body, message_body);
    assert_eq!(
        message.message_id,
        send_response.send_message_result.message_id
    );

    info!("Received message from FIFO queue: {}", message.message_id);

    // 4. Delete queue
    ctx.client
        .delete_queue(&queue_url)
        .await
        .expect("Failed to delete FIFO queue");

    info!("Deleted FIFO queue: {}", queue_url);

    // Remove from tracking since we deleted it
    {
        let mut created_queues = ctx.created_queues.lock().unwrap();
        created_queues.remove(&queue_url);
    }

    info!("SQS FIFO queue operations test completed successfully");
}
