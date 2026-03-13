/*!
# CloudWatch Logs Client Integration Tests

These tests perform real AWS CloudWatch Logs operations including creating log groups, log streams,
putting log events, and retrieving log events.

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
- `logs:*` (or specific CloudWatch Logs permissions)
- `logs:CreateLogGroup`
- `logs:DeleteLogGroup`
- `logs:CreateLogStream`
- `logs:DeleteLogStream`
- `logs:PutLogEvents`
- `logs:GetLogEvents`

## Running Tests
```bash
# Run all CloudWatch Logs tests
cargo test --package alien-aws-clients --test aws_cloudwatch_logs_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_cloudwatch_logs_client_tests test_end_to_end_log_operations -- --nocapture
```
*/

use alien_aws_clients::cloudwatch_logs::*;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use aws_credential_types::Credentials;
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct CloudWatchLogsTestContext {
    client: CloudWatchLogsClient,
    created_log_groups: Mutex<HashSet<String>>,
    created_log_streams: Mutex<HashSet<(String, String)>>, // (log_group_name, log_stream_name)
}

impl AsyncTestContext for CloudWatchLogsTestContext {
    async fn setup() -> CloudWatchLogsTestContext {
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
        let client = CloudWatchLogsClient::new(Client::new(), aws_config);

        CloudWatchLogsTestContext {
            client,
            created_log_groups: Mutex::new(HashSet::new()),
            created_log_streams: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting CloudWatch Logs test cleanup...");

        let streams_to_cleanup = {
            let streams = self.created_log_streams.lock().unwrap();
            streams.clone()
        };

        let groups_to_cleanup = {
            let groups = self.created_log_groups.lock().unwrap();
            groups.clone()
        };

        // First clean up log streams
        for (log_group_name, log_stream_name) in streams_to_cleanup {
            self.cleanup_log_stream(&log_group_name, &log_stream_name)
                .await;
        }

        // Then clean up log groups
        for log_group_name in groups_to_cleanup {
            self.cleanup_log_group(&log_group_name).await;
        }

        info!("✅ CloudWatch Logs test cleanup completed");
    }
}

impl CloudWatchLogsTestContext {
    fn track_log_group(&self, log_group_name: &str) {
        let mut groups = self.created_log_groups.lock().unwrap();
        groups.insert(log_group_name.to_string());
        info!("📝 Tracking log group for cleanup: {}", log_group_name);
    }

    fn untrack_log_group(&self, log_group_name: &str) {
        let mut groups = self.created_log_groups.lock().unwrap();
        groups.remove(log_group_name);
        info!(
            "✅ Log group {} successfully cleaned up and untracked",
            log_group_name
        );
    }

    fn track_log_stream(&self, log_group_name: &str, log_stream_name: &str) {
        let mut streams = self.created_log_streams.lock().unwrap();
        streams.insert((log_group_name.to_string(), log_stream_name.to_string()));
        info!(
            "📝 Tracking log stream for cleanup: {} in group {}",
            log_stream_name, log_group_name
        );
    }

    fn untrack_log_stream(&self, log_group_name: &str, log_stream_name: &str) {
        let mut streams = self.created_log_streams.lock().unwrap();
        streams.remove(&(log_group_name.to_string(), log_stream_name.to_string()));
        info!(
            "✅ Log stream {} in group {} successfully cleaned up and untracked",
            log_stream_name, log_group_name
        );
    }

    async fn cleanup_log_stream(&self, log_group_name: &str, log_stream_name: &str) {
        info!(
            "🧹 Cleaning up log stream: {} in group {}",
            log_stream_name, log_group_name
        );

        match self
            .client
            .delete_log_stream(log_group_name, log_stream_name)
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Log stream {} in group {} deleted successfully",
                    log_stream_name, log_group_name
                );
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete log stream {} in group {} during cleanup: {:?}",
                        log_stream_name, log_group_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_log_group(&self, log_group_name: &str) {
        info!("🧹 Cleaning up log group: {}", log_group_name);

        match self.client.delete_log_group(log_group_name).await {
            Ok(_) => {
                info!("✅ Log group {} deleted successfully", log_group_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete log group {} during cleanup: {:?}",
                        log_group_name, e
                    );
                }
            }
        }
    }

    fn get_test_log_group_name(&self) -> String {
        format!("/alien/test/log-group-{}", Uuid::new_v4().simple())
    }

    fn get_test_log_stream_name(&self) -> String {
        format!("test-stream-{}", Uuid::new_v4().simple())
    }

    async fn create_test_log_group(&self, log_group_name: &str) -> Result<(), Error> {
        let request = CreateLogGroupRequest::builder()
            .log_group_name(log_group_name.to_string())
            .log_group_class("STANDARD".to_string())
            .tags({
                let mut tags = std::collections::HashMap::new();
                tags.insert("Environment".to_string(), "Test".to_string());
                tags.insert("Project".to_string(), "Alien".to_string());
                tags
            })
            .build();

        let result = self.client.create_log_group(request).await;
        if result.is_ok() {
            self.track_log_group(log_group_name);
        }
        result
    }

    async fn create_test_log_stream(
        &self,
        log_group_name: &str,
        log_stream_name: &str,
    ) -> Result<(), Error> {
        let request = CreateLogStreamRequest::builder()
            .log_group_name(log_group_name.to_string())
            .log_stream_name(log_stream_name.to_string())
            .build();

        let result = self.client.create_log_stream(request).await;
        if result.is_ok() {
            self.track_log_stream(log_group_name, log_stream_name);
        }
        result
    }

    fn get_current_timestamp_millis(&self) -> i64 {
        let now: DateTime<Utc> = Utc::now();
        now.timestamp_millis()
    }
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_create_log_group_success(ctx: &mut CloudWatchLogsTestContext) {
    let log_group_name = ctx.get_test_log_group_name();

    info!("🚀 Testing create log group: {}", log_group_name);

    match ctx.create_test_log_group(&log_group_name).await {
        Ok(_) => {
            info!("✅ Log group created successfully: {}", log_group_name);
        }
        Err(e) => {
            panic!("Log group creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    };

    // Log group will be cleaned up automatically via teardown
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_create_log_stream_success(ctx: &mut CloudWatchLogsTestContext) {
    let log_group_name = ctx.get_test_log_group_name();
    let log_stream_name = ctx.get_test_log_stream_name();

    info!(
        "🔗 Testing create log stream: {} in group {}",
        log_stream_name, log_group_name
    );

    // First create a log group
    match ctx.create_test_log_group(&log_group_name).await {
        Ok(_) => {
            info!("✅ Log group created successfully, now testing log stream creation");

            match ctx
                .create_test_log_stream(&log_group_name, &log_stream_name)
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Successfully created log stream: {} in group {}",
                        log_stream_name, log_group_name
                    );
                }
                Err(e) => {
                    panic!("Log stream creation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Log group creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_put_log_events_success(ctx: &mut CloudWatchLogsTestContext) {
    let log_group_name = ctx.get_test_log_group_name();
    let log_stream_name = ctx.get_test_log_stream_name();

    info!(
        "📝 Testing put log events: {} in group {}",
        log_stream_name, log_group_name
    );

    // Step 1: Create log group
    match ctx.create_test_log_group(&log_group_name).await {
        Ok(_) => {
            info!("✅ Log group created");

            // Step 2: Create log stream
            match ctx
                .create_test_log_stream(&log_group_name, &log_stream_name)
                .await
            {
                Ok(_) => {
                    info!("✅ Log stream created");

                    // Step 3: Put log events
                    let timestamp = ctx.get_current_timestamp_millis();
                    let events = vec![
                        InputLogEvent::builder()
                            .timestamp(timestamp)
                            .message("Test log message 1".to_string())
                            .build(),
                        InputLogEvent::builder()
                            .timestamp(timestamp + 1000)
                            .message("Test log message 2".to_string())
                            .build(),
                    ];

                    let put_request = PutLogEventsRequest::builder()
                        .log_group_name(log_group_name.clone())
                        .log_stream_name(log_stream_name.clone())
                        .log_events(events)
                        .build();

                    match ctx.client.put_log_events(put_request).await {
                        Ok(response) => {
                            info!("✅ Successfully put log events");
                            if let Some(next_token) = &response.next_sequence_token {
                                info!("Next sequence token: {}", next_token);
                            }
                        }
                        Err(e) => {
                            panic!("Put log events failed: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    panic!("Log stream creation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Log group creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_get_log_events_success(ctx: &mut CloudWatchLogsTestContext) {
    let log_group_name = ctx.get_test_log_group_name();
    let log_stream_name = ctx.get_test_log_stream_name();

    info!(
        "📖 Testing get log events: {} in group {}",
        log_stream_name, log_group_name
    );

    // Step 1: Create log group
    match ctx.create_test_log_group(&log_group_name).await {
        Ok(_) => {
            info!("✅ Log group created");

            // Step 2: Create log stream
            match ctx
                .create_test_log_stream(&log_group_name, &log_stream_name)
                .await
            {
                Ok(_) => {
                    info!("✅ Log stream created");

                    // Step 3: Put some log events
                    let timestamp = ctx.get_current_timestamp_millis();
                    let events = vec![InputLogEvent::builder()
                        .timestamp(timestamp)
                        .message("Test log message for retrieval".to_string())
                        .build()];

                    let put_request = PutLogEventsRequest::builder()
                        .log_group_name(log_group_name.clone())
                        .log_stream_name(log_stream_name.clone())
                        .log_events(events)
                        .build();

                    match ctx.client.put_log_events(put_request).await {
                        Ok(_) => {
                            info!("✅ Log events put successfully");

                            // Step 4: Get log events
                            let get_request = GetLogEventsRequest::builder()
                                .log_group_name(log_group_name.clone())
                                .log_stream_name(log_stream_name.clone())
                                .start_from_head(true)
                                .limit(10)
                                .build();

                            match ctx.client.get_log_events(get_request).await {
                                Ok(response) => {
                                    info!("✅ Successfully retrieved log events");
                                    if let Some(events) = &response.events {
                                        info!("Retrieved {} log events", events.len());
                                        for (i, event) in events.iter().enumerate() {
                                            info!(
                                                "Event {}: {} - {}",
                                                i, event.timestamp, event.message
                                            );
                                        }
                                    } else {
                                        info!("No events found in response");
                                    }
                                }
                                Err(e) => {
                                    warn!("Get log events failed: {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            panic!("Put log events failed: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    panic!("Log stream creation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Log group creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_end_to_end_log_operations(ctx: &mut CloudWatchLogsTestContext) {
    let log_group_name = ctx.get_test_log_group_name();
    let log_stream_name = ctx.get_test_log_stream_name();

    info!(
        "🚀 Starting end-to-end CloudWatch Logs test: {} in group {}",
        log_stream_name, log_group_name
    );

    // Step 1: Create log group
    match ctx.create_test_log_group(&log_group_name).await {
        Ok(_) => {
            info!("✅ Log group created: {}", log_group_name);

            // Step 2: Create log stream
            match ctx
                .create_test_log_stream(&log_group_name, &log_stream_name)
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Log stream created: {} in group {}",
                        log_stream_name, log_group_name
                    );

                    // Step 3: Put log events
                    let timestamp = ctx.get_current_timestamp_millis();
                    let test_events = vec![
                        InputLogEvent::builder()
                            .timestamp(timestamp)
                            .message("End-to-end test message 1".to_string())
                            .build(),
                        InputLogEvent::builder()
                            .timestamp(timestamp + 1000)
                            .message("End-to-end test message 2".to_string())
                            .build(),
                        InputLogEvent::builder()
                            .timestamp(timestamp + 2000)
                            .message("End-to-end test message 3".to_string())
                            .build(),
                    ];

                    let put_request = PutLogEventsRequest::builder()
                        .log_group_name(log_group_name.clone())
                        .log_stream_name(log_stream_name.clone())
                        .log_events(test_events.clone())
                        .build();

                    match ctx.client.put_log_events(put_request).await {
                        Ok(response) => {
                            info!("✅ Successfully put {} log events", test_events.len());
                            if let Some(next_token) = &response.next_sequence_token {
                                info!("Next sequence token: {}", next_token);
                            }

                            // Step 4: Get log events
                            let get_request = GetLogEventsRequest::builder()
                                .log_group_name(log_group_name.clone())
                                .log_stream_name(log_stream_name.clone())
                                .start_from_head(true)
                                .limit(10)
                                .build();

                            match ctx.client.get_log_events(get_request).await {
                                Ok(response) => {
                                    info!("✅ Successfully retrieved log events");
                                    if let Some(events) = &response.events {
                                        info!("Retrieved {} log events", events.len());
                                        assert!(
                                            events.len() >= test_events.len(),
                                            "Expected at least {} events, got {}",
                                            test_events.len(),
                                            events.len()
                                        );

                                        // Verify our test messages are present
                                        let messages: Vec<&str> =
                                            events.iter().map(|e| e.message.as_str()).collect();
                                        assert!(
                                            messages.contains(&"End-to-end test message 1"),
                                            "Expected to find test message 1"
                                        );
                                        assert!(
                                            messages.contains(&"End-to-end test message 2"),
                                            "Expected to find test message 2"
                                        );
                                        assert!(
                                            messages.contains(&"End-to-end test message 3"),
                                            "Expected to find test message 3"
                                        );

                                        info!("✅ All test messages verified in retrieved events");
                                    } else {
                                        warn!("No events found in response");
                                    }
                                }
                                Err(e) => {
                                    warn!("Get log events failed: {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            panic!("Put log events failed: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    panic!("Log stream creation failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Log group creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }

    info!("🎉 End-to-end CloudWatch Logs test completed!");
}

#[test_context(CloudWatchLogsTestContext)]
#[tokio::test]
async fn test_serde_structs(ctx: &mut CloudWatchLogsTestContext) {
    // Test serialization and deserialization of key structs
    let create_log_group_request = CreateLogGroupRequest::builder()
        .log_group_name("/test/serde".to_string())
        .log_group_class("STANDARD".to_string())
        .build();

    let json = serde_json::to_string(&create_log_group_request).expect("Should serialize");
    assert!(json.contains("/test/serde"));
    assert!(json.contains("logGroupName")); // Verify camelCase serialization
    assert!(json.contains("logGroupClass"));

    let create_log_stream_request = CreateLogStreamRequest::builder()
        .log_group_name("/test/serde".to_string())
        .log_stream_name("test-stream".to_string())
        .build();

    let stream_json = serde_json::to_string(&create_log_stream_request).expect("Should serialize");
    assert!(stream_json.contains("test-stream"));
    assert!(stream_json.contains("logGroupName"));
    assert!(stream_json.contains("logStreamName"));

    let input_event = InputLogEvent::builder()
        .timestamp(1234567890)
        .message("Test message".to_string())
        .build();

    let event_json = serde_json::to_string(&input_event).expect("Should serialize");
    assert!(event_json.contains("Test message"));
    assert!(event_json.contains("1234567890"));
    assert!(event_json.contains("timestamp"));
    assert!(event_json.contains("message"));
}
