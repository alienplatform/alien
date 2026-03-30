//! Integration tests for alien-commands
//!
//! These tests focus on the 8 core command scenarios:
//! (PUSH, PULL) × (SMALL PARAMS, LARGE PARAMS) × (SMALL RESPONSE, LARGE RESPONSE)
//!
//! 1. (Push, Small Params, Small Response): inline params auto-dispatched, inline response
//! 2. (Push, Small Params, Large Response): inline params auto-dispatched, storage response
//! 3. (Push, Large Params, Small Response): storage params auto-dispatched after upload, inline response
//! 4. (Push, Large Params, Large Response): storage params auto-dispatched after upload, storage response
//! 5. (Pull, Small Params, Small Response): inline params acquired via lease, inline response
//! 6. (Pull, Small Params, Large Response): inline params acquired via lease, storage response
//! 7. (Pull, Large Params, Small Response): storage params acquired via lease after upload, inline response
//! 8. (Pull, Large Params, Large Response): storage params acquired via lease after upload, storage response
//!
//! Additional component tests verify basic API functionality and runtime integration.

#[cfg(feature = "test-utils")]
mod tests {
    use std::time::Duration;

    use alien_commands::{
        runtime::{decode_params, parse_envelope},
        test_utils::{
            dispatcher::MockDispatcherAssertions, server::TestCommandServerAssertions, *,
        },
        types::*,
    };
    use alien_core::{MessagePayload, QueueMessage};
    use chrono::Utc;

    // ===========================================
    // CORE SCENARIOS: (PUSH, PULL) × (SMALL PARAMS, LARGE PARAMS) × (SMALL RESPONSE, LARGE RESPONSE)
    // ===========================================

    /// Core Scenario 1: Push + Small Params + Small Response
    /// Inline params auto-dispatched, inline response
    #[tokio::test]
    async fn test_core_push_small_params_small_response() {
        let server = TestCommandServer::new().await;

        // 1. Client creates small inline command (auto-dispatched immediately)
        let request = test_inline_create_command("push-agent", "generate-report");
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::Dispatched); // Auto-dispatched
        assert!(response.storage_upload.is_none());

        // 2. Verify envelope was dispatched to mock dispatcher (simulates push to agent)
        let mock_dispatcher = server
            .mock_dispatcher()
            .expect("Should have mock dispatcher");
        mock_dispatcher.assert_has_dispatched().await;
        let dispatched = mock_dispatcher.get_latest().await.unwrap();
        assert_eq!(dispatched.envelope.command_id, response.command_id);
        assert_eq!(dispatched.envelope.command, "generate-report");
        assert!(matches!(
            dispatched.envelope.params,
            BodySpec::Inline { .. }
        ));

        // 3. Simulate agent receiving envelope and processing
        let params = decode_params(&dispatched.envelope).await.unwrap();
        assert!(params.is_object()); // Should have JSON params

        // 4. Agent submits response
        let agent_response = test_success_response(b"report generated");
        server
            .submit_command_response(&response.command_id, agent_response)
            .await
            .unwrap();

        // 5. Client polls for completion
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            assert_inline_body(&body, b"report generated");
        }
    }

    /// Core Scenario 5: Pull + Small Params + Small Response
    /// Inline params acquired via lease, inline response
    #[tokio::test]
    async fn test_core_pull_small_params_small_response() {
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // 1. Client creates command (in Pull mode, stays Pending until lease)
        let request = test_inline_create_command("pull-agent", "sync-data");
        let create_response = server.create_command(request).await.unwrap();
        assert_eq!(create_response.state, CommandState::Pending); // Pending until lease

        // 2. Agent polls for lease (moves to Dispatched)
        let lease = server
            .acquire_single_lease("pull-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.command_id, create_response.command_id);
        assert!(matches!(lease.envelope.params, BodySpec::Inline { .. }));

        // Verify state moved to Dispatched after lease
        let status = server
            .get_command_status(&create_response.command_id)
            .await
            .unwrap();
        assert_eq!(status.state, CommandState::Dispatched);

        // 3. Agent processes envelope (simulated)
        let params = decode_params(&lease.envelope).await.unwrap();
        assert!(params.is_object()); // Should have JSON params

        // 4. Agent submits response
        let agent_response = test_json_success_response(&serde_json::json!({
            "status": "synced",
            "command_id": lease.command_id
        }));
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();

        // 5. Client checks completion
        let final_status = server
            .wait_for_completion(&create_response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let response = final_status.response.unwrap();
        assert!(response.is_success());
        if let CommandResponse::Success { response: body } = response {
            let body_data = body.decode_inline().unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_data).unwrap();
            assert_eq!(json["status"], "synced");
        }
    }

    /// Core Scenario 4: Push + Large Params + Large Response
    /// Storage params auto-dispatched after upload, storage response
    #[tokio::test]
    async fn test_core_push_large_params_large_response() {
        let server = TestCommandServer::new().await; // Use default 150KB limit

        // 1. Client creates large command
        let large_params = vec![b'X'; 160000]; // 160KB > 150KB inline limit
        let request = test_storage_create_command("push-agent", "process-bulk", large_params.len());
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::PendingUpload);
        assert!(response.storage_upload.is_some());

        // 2. Client uploads large params using the presigned URL mechanism
        let storage_upload = response.storage_upload.unwrap();
        storage_upload
            .put_request
            .execute(Some(large_params.clone().into()))
            .await
            .unwrap();

        let upload_complete = test_upload_complete_request(160000);
        server
            .upload_complete(&response.command_id, upload_complete)
            .await
            .unwrap();

        // 3. Command should be auto-dispatched after upload
        let mock_dispatcher = server
            .mock_dispatcher()
            .expect("Should have mock dispatcher");
        assert!(mock_dispatcher.has_dispatched().await);
        let dispatched = mock_dispatcher.get_latest().await.unwrap();
        assert_eq!(dispatched.envelope.command_id, response.command_id);
        assert!(matches!(
            dispatched.envelope.params,
            BodySpec::Storage { .. }
        ));

        // 4. Agent submits large response (> 150KB to force storage)
        let large_response_data = vec![b'R'; 160000]; // 160KB > 150KB inline limit

        // Agent uses the presigned upload request from the envelope
        dispatched
            .envelope
            .response_handling
            .storage_upload_request
            .execute(Some(large_response_data.clone().into()))
            .await
            .unwrap();

        let agent_response = CommandResponse::success_storage(large_response_data.len() as u64);
        server
            .submit_command_response(&response.command_id, agent_response)
            .await
            .unwrap();

        // 5. Client gets final result with large storage response
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            assert_storage_body(&body, Some(160000));
            // Verify we can download and the content matches what the agent uploaded
            assert_storage_body_content(&body, &large_response_data).await;
        }

        // Should have storage objects from both large params and response
        assert!(server.storage_object_count().await > 1);
    }

    /// Core Scenario 8: Pull + Large Params + Large Response
    /// Storage params acquired via lease after upload, storage response
    #[tokio::test]
    async fn test_core_pull_large_params_large_response() {
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // 1. Client creates large command
        let large_params = vec![b'Y'; 160000]; // 160KB > 150KB inline limit
        let request = test_storage_create_command("pull-agent", "bulk-process", large_params.len());
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::PendingUpload);
        assert!(response.storage_upload.is_some());

        // 2. Client uploads large params using the presigned URL mechanism
        let storage_upload = response.storage_upload.unwrap();
        storage_upload
            .put_request
            .execute(Some(large_params.clone().into()))
            .await
            .unwrap();

        let upload_complete = test_upload_complete_request(160000);
        let upload_response = server
            .upload_complete(&response.command_id, upload_complete)
            .await
            .unwrap();
        // In Pull mode, after upload the state is Pending (waiting for lease)
        assert_eq!(upload_response.state, CommandState::Pending);

        // 3. Agent polls for lease (moves to Dispatched)
        let lease = server
            .acquire_single_lease("pull-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.command_id, response.command_id);
        assert!(matches!(lease.envelope.params, BodySpec::Storage { .. }));

        // Verify state moved to Dispatched after lease
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        assert_eq!(status.state, CommandState::Dispatched);

        // 4. Agent simulates processing large params
        let params_bytes = alien_commands::runtime::decode_params_bytes(&lease.envelope)
            .await
            .unwrap();
        assert_eq!(params_bytes.len(), 160000); // Should have reconstructed the large params
        assert_eq!(params_bytes, vec![b'Y'; 160000]); // Should match original data

        // 5. Agent submits large response (upload to storage since > 150KB)
        let large_response_data = vec![b'Z'; 160000]; // 160KB > 150KB inline limit

        // Agent uses the presigned upload request from the envelope
        lease
            .envelope
            .response_handling
            .storage_upload_request
            .execute(Some(large_response_data.clone().into()))
            .await
            .unwrap();

        let agent_response = CommandResponse::success_storage(large_response_data.len() as u64);
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();

        // 6. Verify completion with storage response
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            assert_storage_body(&body, Some(160000));
            // Verify we can download and the content matches what the agent uploaded
            assert_storage_body_content(&body, &large_response_data).await;
        }

        // Should have storage objects from the large payloads
        assert!(server.storage_object_count().await > 0);
    }

    /// Core Scenario 2: Push + Small Params + Large Response
    /// Inline params auto-dispatched, storage response
    #[tokio::test]
    async fn test_core_push_small_params_large_response() {
        let server = TestCommandServer::new().await; // Use default 150KB limit

        // 1. Client creates small inline command (auto-dispatched immediately)
        let request = test_inline_create_command("push-agent", "generate-large-report");
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::Dispatched); // Auto-dispatched
        assert!(response.storage_upload.is_none());

        // 2. Verify envelope was dispatched to mock dispatcher (simulates push to agent)
        let mock_dispatcher = server
            .mock_dispatcher()
            .expect("Should have mock dispatcher");
        mock_dispatcher.assert_has_dispatched().await;
        let dispatched = mock_dispatcher.get_latest().await.unwrap();
        assert_eq!(dispatched.envelope.command_id, response.command_id);
        assert!(matches!(
            dispatched.envelope.params,
            BodySpec::Inline { .. }
        ));

        // 3. Agent submits large response (> 150KB to force storage)
        let large_response_data = vec![b'L'; 160000]; // 160KB > 150KB inline limit

        // Agent uses the presigned upload request from the envelope
        dispatched
            .envelope
            .response_handling
            .storage_upload_request
            .execute(Some(large_response_data.clone().into()))
            .await
            .unwrap();

        let agent_response = CommandResponse::success_storage(large_response_data.len() as u64);
        server
            .submit_command_response(&response.command_id, agent_response)
            .await
            .unwrap();

        // 4. Client gets final result with large storage response
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            assert_storage_body(&body, Some(160000));
            // Verify we can download and the content matches what we uploaded
            assert_storage_body_content(&body, &large_response_data).await;
        }

        // Should have storage object from large response
        assert!(server.storage_object_count().await > 0);
    }

    /// Core Scenario 3: Push + Large Params + Small Response
    /// Storage params auto-dispatched after upload, inline response
    #[tokio::test]
    async fn test_core_push_large_params_small_response() {
        let server = TestCommandServer::new().await; // Use default 150KB limit

        // 1. Client creates large command
        let large_params = vec![b'X'; 160000]; // 160KB > 150KB inline limit
        let request = test_storage_create_command("push-agent", "process-data", large_params.len());
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::PendingUpload);
        assert!(response.storage_upload.is_some());

        // 2. Client uploads large params using the presigned URL mechanism
        let storage_upload = response.storage_upload.unwrap();
        storage_upload
            .put_request
            .execute(Some(large_params.clone().into()))
            .await
            .unwrap();

        let upload_complete = test_upload_complete_request(160000);
        server
            .upload_complete(&response.command_id, upload_complete)
            .await
            .unwrap();

        // 3. Command should be auto-dispatched after upload
        let mock_dispatcher = server
            .mock_dispatcher()
            .expect("Should have mock dispatcher");
        assert!(mock_dispatcher.has_dispatched().await);
        let dispatched = mock_dispatcher.get_latest().await.unwrap();
        assert_eq!(dispatched.envelope.command_id, response.command_id);
        assert!(matches!(
            dispatched.envelope.params,
            BodySpec::Storage { .. }
        ));

        // 4. Agent submits small inline response
        let agent_response = test_success_response(b"ok");
        server
            .submit_command_response(&response.command_id, agent_response)
            .await
            .unwrap();

        // 5. Client gets final result with inline response
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            assert_inline_body(&body, b"ok");
        }

        // Should have storage object from large params but not response
        assert!(server.storage_object_count().await > 0);
    }

    /// Core Scenario 6: Pull + Small Params + Large Response
    /// Inline params acquired via lease, storage response
    #[tokio::test]
    async fn test_core_pull_small_params_large_response() {
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // 1. Client creates command (in Pull mode, stays Pending until lease)
        let request = test_inline_create_command("pull-agent", "generate-large");
        let create_response = server.create_command(request).await.unwrap();
        assert_eq!(create_response.state, CommandState::Pending); // Pending until lease

        // 2. Agent polls for lease (moves to Dispatched)
        let lease = server
            .acquire_single_lease("pull-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.command_id, create_response.command_id);
        assert!(matches!(lease.envelope.params, BodySpec::Inline { .. }));

        // Verify state moved to Dispatched after lease
        let status = server
            .get_command_status(&create_response.command_id)
            .await
            .unwrap();
        assert_eq!(status.state, CommandState::Dispatched);

        // 3. Agent processes envelope (simulated)
        let params = decode_params(&lease.envelope).await.unwrap();
        assert!(params.is_object()); // Should have JSON params

        // 4. Agent submits large response (> 150KB to force storage)
        let large_response_data = vec![b'M'; 160000]; // 160KB > 150KB inline limit

        // Agent uses the presigned upload request from the envelope
        lease
            .envelope
            .response_handling
            .storage_upload_request
            .execute(Some(large_response_data.clone().into()))
            .await
            .unwrap();

        let agent_response = CommandResponse::success_storage(large_response_data.len() as u64);
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();

        // 5. Client checks completion with large storage response
        let final_status = server
            .wait_for_completion(&create_response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let response = final_status.response.unwrap();
        assert!(response.is_success());
        if let CommandResponse::Success { response: body } = response {
            assert_storage_body(&body, Some(160000));
            // Verify we can download and the content matches what the agent uploaded
            assert_storage_body_content(&body, &large_response_data).await;
        }

        // Should have storage object from large response
        assert!(server.storage_object_count().await > 0);
    }

    /// Core Scenario 7: Pull + Large Params + Small Response
    /// Storage params acquired via lease after upload, inline response
    #[tokio::test]
    async fn test_core_pull_large_params_small_response() {
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // 1. Client creates large command
        let large_params = vec![b'Y'; 160000]; // 160KB > 150KB inline limit
        let request = test_storage_create_command("pull-agent", "process-bulk", large_params.len());
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::PendingUpload);
        assert!(response.storage_upload.is_some());

        // 2. Client uploads large params using the presigned URL mechanism
        let storage_upload = response.storage_upload.unwrap();
        storage_upload
            .put_request
            .execute(Some(large_params.clone().into()))
            .await
            .unwrap();

        let upload_complete = test_upload_complete_request(160000);
        let upload_response = server
            .upload_complete(&response.command_id, upload_complete)
            .await
            .unwrap();
        // In Pull mode, after upload the state is Pending (waiting for lease)
        assert_eq!(upload_response.state, CommandState::Pending);

        // 3. Agent polls for lease (moves to Dispatched)
        let lease = server
            .acquire_single_lease("pull-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.command_id, response.command_id);
        assert!(matches!(lease.envelope.params, BodySpec::Storage { .. }));

        // Verify state moved to Dispatched after lease
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        assert_eq!(status.state, CommandState::Dispatched);

        // 4. Agent simulates processing large params
        let params_bytes = alien_commands::runtime::decode_params_bytes(&lease.envelope)
            .await
            .unwrap();
        assert_eq!(params_bytes.len(), 160000); // Should have reconstructed the large params
        assert_eq!(params_bytes, vec![b'Y'; 160000]); // Should match original data

        // 5. Agent submits small inline response
        let agent_response = test_json_success_response(&serde_json::json!({
            "status": "processed",
            "command_id": lease.command_id
        }));
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();

        // 6. Verify completion with inline response
        let final_status = server
            .wait_for_completion(&response.command_id, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(final_status.state, CommandState::Succeeded);

        let final_response = final_status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            let body_data = body.decode_inline().unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_data).unwrap();
            assert_eq!(json["status"], "processed");
        }

        // Should have storage object from large params but not response
        assert!(server.storage_object_count().await > 0);
    }

    // ===============================================
    // ESSENTIAL COMPONENT TESTS
    // ===============================================

    /// Test basic API operations
    #[tokio::test]
    async fn test_basic_api_operations() {
        let server = TestCommandServer::new().await;

        // Test create command with inline payload
        let request = test_inline_create_command("api-agent", "test-command");
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::Dispatched);
        assert!(response.command_id.starts_with("cmd_"));
        assert!(response.storage_upload.is_none());

        // Test status check
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        assert_eq!(status.command_id, response.command_id);
        assert_eq!(status.state, CommandState::Dispatched);
        assert_eq!(status.attempt, 1);

        // Test create large command requiring storage
        let large_request = test_storage_create_command("storage-agent", "upload-command", 200_000);
        let response = server.create_command(large_request).await.unwrap();
        assert_eq!(response.state, CommandState::PendingUpload);
        assert!(response.storage_upload.is_some());

        // Test upload completion
        let upload_complete = test_upload_complete_request(200_000);
        let complete_response = server
            .upload_complete(&response.command_id, upload_complete)
            .await
            .unwrap();
        assert_eq!(complete_response.state, CommandState::Dispatched);
    }

    /// Test lease operations
    #[tokio::test]
    async fn test_lease_operations() {
        // Lease operations require Pull mode (Push mode dispatches immediately)
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // Create command (stays Pending in Pull mode)
        let request = test_inline_create_command("lease-agent", "lease-command");
        let response = server.create_command(request).await.unwrap();
        assert_eq!(response.state, CommandState::Pending);

        // Acquire lease
        let lease = server
            .acquire_single_lease("lease-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.command_id, response.command_id);
        assert_eq!(lease.attempt, 1);
        assert!(lease.lease_expires_at > Utc::now());

        // Verify envelope details
        assert_envelope_command_id(&lease.envelope, &response.command_id);
        assert_envelope_command(&lease.envelope, "lease-command");

        // Test no available leases
        let empty_response = server
            .acquire_lease("nonexistent-agent", LeaseRequest::default())
            .await
            .unwrap();
        assert_eq!(empty_response.leases.len(), 0);

        // Release lease
        server
            .release_lease(&lease.command_id, &lease.lease_id)
            .await
            .unwrap();
        server
            .assert_command_state(&response.command_id, CommandState::Pending)
            .await;
    }

    /// Test response submission and idempotency
    #[tokio::test]
    async fn test_response_operations() {
        // Lease operations require Pull mode (Push mode dispatches immediately)
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // Create command and acquire lease
        let request = test_inline_create_command("response-agent", "response-command");
        let response = server.create_command(request).await.unwrap();
        let lease = server
            .acquire_single_lease("response-agent")
            .await
            .unwrap()
            .unwrap();

        // Submit response
        let agent_response = test_json_success_response(&serde_json::json!({
            "result": "success",
            "data": [1, 2, 3]
        }));
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();
        server.assert_command_succeeded(&response.command_id).await;

        // Verify response data
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        let final_response = status.response.unwrap();
        assert!(final_response.is_success());
        if let CommandResponse::Success { response: body } = final_response {
            let decoded = body.decode_inline().unwrap();
            let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
            assert_eq!(json["result"], "success");
        }

        // Test duplicate response submission (should be idempotent)
        let duplicate_response = test_success_response(b"second response");
        let result = server
            .submit_command_response(&lease.command_id, duplicate_response)
            .await;
        assert!(result.is_ok()); // Should not error, just ignore

        // Original response should still be there
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        let final_response = status.response.unwrap();
        if let CommandResponse::Success { response: body } = final_response {
            let decoded = body.decode_inline().unwrap();
            let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
            assert_eq!(json["result"], "success"); // Not changed
        }
    }

    /// Test runtime envelope parsing
    #[tokio::test]
    async fn test_runtime_integration() {
        // Test envelope parsing from queue message
        let envelope = test_simple_envelope("cmd_runtime_test", "runtime-command");
        let envelope_json = serde_json::to_value(&envelope).unwrap();
        let queue_message = QueueMessage {
            id: "msg_123".to_string(),
            payload: MessagePayload::Json(envelope_json),
            receipt_handle: "handle_123".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };

        let parsed = parse_envelope(&queue_message).unwrap();
        assert!(parsed.is_some());
        let parsed_envelope = parsed.unwrap();
        assert_eq!(parsed_envelope.command_id, "cmd_runtime_test");
        assert_eq!(parsed_envelope.command, "runtime-command");

        // Test non-command message
        let non_arc_message = QueueMessage {
            id: "msg_456".to_string(),
            payload: MessagePayload::Json(serde_json::json!({"regular": "message"})),
            receipt_handle: "handle_456".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };
        let parsed = parse_envelope(&non_arc_message).unwrap();
        assert!(parsed.is_none());

        // Test params decoding
        let params_json = serde_json::json!({"key": "value", "number": 42});
        let params_bytes = serde_json::to_vec(&params_json).unwrap();
        let test_envelope = test_envelope(
            "cmd_params",
            "params-command",
            BodySpec::inline(&params_bytes),
        );
        let decoded_params = decode_params(&test_envelope).await.unwrap();
        assert_eq!(decoded_params["key"], "value");
        assert_eq!(decoded_params["number"], 42);
    }

    /// Test error handling and edge cases
    #[tokio::test]
    async fn test_error_handling() {
        let server = TestCommandServer::new().await;

        // Test invalid command (empty command name)
        let invalid_request = CreateCommandRequest {
            deployment_id: "error-agent".to_string(),
            command: "".to_string(), // Invalid: empty command name
            params: BodySpec::inline(b"{}"),
            deadline: None,
            idempotency_key: None,
        };
        let result = server.create_command(invalid_request).await;
        assert!(result.is_err());

        // Test invalid command (empty deployment_id)
        let invalid_request = CreateCommandRequest {
            deployment_id: "".to_string(), // Invalid: empty deployment_id
            command: "test".to_string(),
            params: BodySpec::inline(b"{}"),
            deadline: None,
            idempotency_key: None,
        };
        let result = server.create_command(invalid_request).await;
        assert!(result.is_err());

        // Test operations on non-existent commands
        let upload_complete = test_upload_complete_request(1000);
        assert!(server
            .upload_complete("nonexistent", upload_complete)
            .await
            .is_err());
        assert!(server.get_command_status("nonexistent").await.is_err());
        assert!(server
            .submit_command_response("nonexistent", test_success_response(b"test"))
            .await
            .is_err());

        // Test command expiration (past deadline)
        let past_deadline = Utc::now() - chrono::Duration::minutes(1);
        let expired_request = test_create_command_with_deadline(
            "expired-agent",
            "expired-command",
            BodySpec::inline(b"{}"),
            past_deadline,
        );
        assert!(server.create_command(expired_request).await.is_err());
    }

    /// Test error response handling
    #[tokio::test]
    async fn test_error_response_handling() {
        // Lease operations require Pull mode (Push mode dispatches immediately)
        let server = TestCommandServer::builder().with_pull_mode().build().await;

        // Create command
        let request = test_inline_create_command("error-agent", "error-command");
        let response = server.create_command(request).await.unwrap();

        // Acquire lease
        let lease = server
            .acquire_single_lease("error-agent")
            .await
            .unwrap()
            .unwrap();

        // Submit error response
        let agent_response = test_error_response("PROCESSING_FAILED", "Something went wrong");
        server
            .submit_command_response(&lease.command_id, agent_response)
            .await
            .unwrap();

        // Verify command failed
        let status = server
            .get_command_status(&response.command_id)
            .await
            .unwrap();
        assert_eq!(status.state, CommandState::Failed);

        let final_response = status.response.unwrap();
        assert!(final_response.is_error());
        if let CommandResponse::Error { code, message, .. } = final_response {
            assert_eq!(code, "PROCESSING_FAILED");
            assert_eq!(message, "Something went wrong");
        }
    }
}
