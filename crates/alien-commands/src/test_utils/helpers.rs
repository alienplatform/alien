use alien_bindings::presigned::PresignedRequest;
use chrono::{DateTime, Utc};

use crate::types::*;

/// Helper functions for creating test data structures
///
/// These functions provide convenient builders for creating command types
/// with sensible defaults for testing. They help reduce boilerplate
/// in test code and ensure consistent test data.

/// Create a test command with inline params
pub fn test_inline_params(data: &[u8]) -> BodySpec {
    BodySpec::inline(data)
}

/// Create a test command with inline JSON params
pub fn test_json_params(json: &serde_json::Value) -> BodySpec {
    let bytes = serde_json::to_vec(json).expect("Valid JSON");
    BodySpec::inline(&bytes)
}

/// Create a test command with storage params
pub fn test_storage_params(size: u64) -> BodySpec {
    BodySpec::storage(size)
}

/// Create test response handling configuration
pub fn test_response_handling(command_id: &str) -> ResponseHandling {
    ResponseHandling {
        max_inline_bytes: crate::INLINE_MAX_BYTES as u64,
        submit_response_url: format!(
            "https://commands.test.example.com/v1/commands/{}/response",
            command_id
        ),
        storage_upload_request: PresignedRequest::new_http(
            format!(
                "https://storage.test.example.com/commands/{}/response",
                command_id
            ),
            "PUT".to_string(),
            std::collections::HashMap::new(),
            alien_bindings::presigned::PresignedOperation::Put,
            format!("commands/{}/response", command_id),
            Utc::now() + chrono::Duration::hours(1),
        ),
    }
}

/// Create a test command envelope
pub fn test_envelope(command_id: &str, command: &str, params: BodySpec) -> Envelope {
    test_envelope_for_agent("test-agent", command_id, command, params)
}

/// Create a test command envelope for a specific deployment
pub fn test_envelope_for_agent(
    deployment_id: &str,
    command_id: &str,
    command: &str,
    params: BodySpec,
) -> Envelope {
    Envelope::new(
        deployment_id,
        command_id,
        1,
        None,
        command,
        params,
        test_response_handling(command_id),
    )
}

/// Create a simple test envelope with empty params
pub fn test_simple_envelope(command_id: &str, command: &str) -> Envelope {
    test_envelope(command_id, command, BodySpec::inline(b"{}"))
}

/// Create a test envelope with large storage params
pub fn test_large_envelope(command_id: &str, command: &str, size: u64) -> Envelope {
    test_envelope(command_id, command, BodySpec::storage(size))
}

/// Create a test success response
pub fn test_success_response(data: &[u8]) -> CommandResponse {
    CommandResponse::success(data)
}

/// Create a test JSON success response
pub fn test_json_success_response(json: &serde_json::Value) -> CommandResponse {
    CommandResponse::success_json(json).expect("Valid JSON")
}

/// Create a test error response
pub fn test_error_response(code: &str, message: &str) -> CommandResponse {
    CommandResponse::error(code, message)
}

/// Create a test storage success response (for large responses)
pub fn test_storage_success_response(size: u64) -> CommandResponse {
    CommandResponse::success_storage(size)
}

/// Create a test create command request with inline params
pub fn test_inline_create_command(deployment_id: &str, command: &str) -> CreateCommandRequest {
    let (request, _) = test_inline_create_command_with_params(deployment_id, command);
    request
}

/// Create a test create command request with inline params and return the raw params data
pub fn test_inline_create_command_with_params(
    deployment_id: &str,
    command: &str,
) -> (CreateCommandRequest, Vec<u8>) {
    // Create a small but meaningful params body (under 150KB inline limit)
    let test_params = serde_json::json!({
        "testData": "This is test command params for validation",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metadata": {
            "testType": "inline-params",
            "size": "small"
        },
        "items": (0..100).map(|i| format!("item-{}", i)).collect::<Vec<_>>()
    });

    let params_bytes = serde_json::to_vec(&test_params).expect("Valid JSON serialization");

    let request = CreateCommandRequest {
        deployment_id: deployment_id.to_string(),
        command: command.to_string(),
        params: BodySpec::inline(&params_bytes),
        deadline: None,
        idempotency_key: None,
    };

    (request, params_bytes)
}

/// Create a test create command request with storage params
pub fn test_storage_create_command(
    deployment_id: &str,
    command: &str,
    size: usize,
) -> CreateCommandRequest {
    CreateCommandRequest {
        deployment_id: deployment_id.to_string(),
        command: command.to_string(),
        params: BodySpec::storage(size as u64),
        deadline: None,
        idempotency_key: None,
    }
}

/// Create a test create command request
pub fn test_create_command(
    deployment_id: &str,
    command: &str,
    params: BodySpec,
) -> CreateCommandRequest {
    CreateCommandRequest {
        deployment_id: deployment_id.to_string(),
        command: command.to_string(),
        params,
        deadline: None,
        idempotency_key: None,
    }
}

/// Create a test create command request with deadline
pub fn test_create_command_with_deadline(
    deployment_id: &str,
    command: &str,
    params: BodySpec,
    deadline: DateTime<Utc>,
) -> CreateCommandRequest {
    CreateCommandRequest {
        deployment_id: deployment_id.to_string(),
        command: command.to_string(),
        params,
        deadline: Some(deadline),
        idempotency_key: None,
    }
}

/// Create a test upload complete request
pub fn test_upload_complete_request(size: u64) -> UploadCompleteRequest {
    UploadCompleteRequest { size }
}

/// Assert that a body is inline and contains expected data
pub fn assert_inline_body(body: &BodySpec, expected: &[u8]) {
    match body {
        BodySpec::Inline { .. } => {
            let decoded = body.decode_inline().expect("Should be decodable");
            assert_eq!(decoded, expected, "Inline body content mismatch");
        }
        _ => panic!("Expected inline body, but got storage body"),
    }
}

/// Assert that a body is storage mode with expected size
pub fn assert_storage_body(body: &BodySpec, expected_size: Option<u64>) {
    match body {
        BodySpec::Storage { size, .. } => {
            if let Some(expected) = expected_size {
                assert_eq!(*size, Some(expected), "Storage body size mismatch");
            }
        }
        _ => panic!("Expected storage body, but got inline body"),
    }
}

/// Assert that a storage body contains the expected content by downloading it
pub async fn assert_storage_body_content(body: &BodySpec, expected_content: &[u8]) {
    match body {
        BodySpec::Storage {
            storage_get_request,
            size,
            ..
        } => {
            assert!(
                storage_get_request.is_some(),
                "Storage body should have storage_get_request"
            );

            let get_request = storage_get_request.as_ref().unwrap();
            let response = get_request
                .execute(None)
                .await
                .expect("Should be able to download storage response");

            assert_eq!(response.status_code, 200, "Storage download should succeed");

            let actual_content = response.body.expect("Storage response should have body");
            assert_eq!(
                actual_content.as_ref(),
                expected_content,
                "Storage content should match expected"
            );

            if let Some(expected_size) = size {
                assert_eq!(
                    *expected_size,
                    expected_content.len() as u64,
                    "Size should match content length"
                );
            }
        }
        _ => panic!("Expected storage body, but got inline body"),
    }
}

/// Assert that an envelope has the expected command ID
pub fn assert_envelope_command_id(envelope: &Envelope, expected_command_id: &str) {
    assert_eq!(
        envelope.command_id, expected_command_id,
        "Envelope command ID mismatch"
    );
}

/// Assert that an envelope has the expected command name
pub fn assert_envelope_command(envelope: &Envelope, expected_command: &str) {
    assert_eq!(
        envelope.command, expected_command,
        "Envelope command name mismatch"
    );
}

/// Assert that a command response is a success
pub fn assert_success_response(response: &CommandResponse) {
    assert!(
        response.is_success(),
        "Expected success response, got error"
    );
}

/// Assert that a command response is an error with expected code
pub fn assert_error_response(response: &CommandResponse, expected_code: &str) {
    match response {
        CommandResponse::Error { code, .. } => {
            assert_eq!(code, expected_code, "Error code mismatch");
        }
        _ => panic!("Expected error response, got success"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_spec_handling() {
        // Test inline body creation and validation
        let inline_params = test_inline_params(b"test data");
        assert_eq!(inline_params.mode(), "inline");
        assert_inline_body(&inline_params, b"test data");

        // Test storage body creation and validation
        let storage_params = test_storage_params(1024);
        assert_eq!(storage_params.mode(), "storage");
        assert_storage_body(&storage_params, Some(1024));

        // Test JSON params
        let json_value = serde_json::json!({"test": "value", "number": 42});
        let json_params = test_json_params(&json_value);
        assert_eq!(json_params.mode(), "inline");

        let decoded = json_params.decode_inline().expect("Should decode");
        let parsed: serde_json::Value =
            serde_json::from_slice(&decoded).expect("Should parse JSON");
        assert_eq!(parsed["test"], "value");
        assert_eq!(parsed["number"], 42);
    }

    #[test]
    fn test_response_creation() {
        // Test successful response
        let success = test_success_response(b"result");
        assert!(success.is_success());

        // Test JSON success response
        let json_value = serde_json::json!({"result": "ok", "count": 5});
        let json_success = test_json_success_response(&json_value);
        assert!(json_success.is_success());

        // Test error response
        let error = test_error_response("TEST_ERROR", "Something failed");
        assert!(error.is_error());

        if let CommandResponse::Error { code, message, .. } = error {
            assert_eq!(code, "TEST_ERROR");
            assert_eq!(message, "Something failed");
        }

        // Test storage success response
        let storage_success = test_storage_success_response(50000);
        assert!(storage_success.is_success());
    }

    #[test]
    fn test_envelope_creation() {
        // Test complete envelope creation
        let params = test_json_params(&serde_json::json!({"key": "value"}));
        let envelope = test_envelope("cmd_123", "test-command", params);

        assert_envelope_command_id(&envelope, "cmd_123");
        assert_envelope_command(&envelope, "test-command");

        // Verify response handling configuration
        assert!(envelope
            .response_handling
            .submit_response_url
            .contains("cmd_123"));
        assert_eq!(
            envelope.response_handling.max_inline_bytes,
            crate::INLINE_MAX_BYTES as u64
        );

        // Test simple envelope helper
        let simple_envelope = test_simple_envelope("cmd_simple", "simple-command");
        assert_envelope_command_id(&simple_envelope, "cmd_simple");
        assert_envelope_command(&simple_envelope, "simple-command");

        // Test large envelope helper
        let large_envelope = test_large_envelope("cmd_large", "large-command", 100_000);
        assert_storage_body(&large_envelope.params, Some(100_000));
    }

    #[test]
    fn test_create_command_variations() {
        // Test inline create command
        let create_cmd = test_inline_create_command("agent-1", "process-data");
        assert_eq!(create_cmd.deployment_id, "agent-1");
        assert_eq!(create_cmd.command, "process-data");
        assert!(create_cmd.deadline.is_none());
        assert!(create_cmd.idempotency_key.is_none());
        assert!(matches!(create_cmd.params, BodySpec::Inline { .. }));

        // Test create command with storage params
        let storage_create = test_storage_create_command("agent-2", "upload-file", 2048);
        assert_eq!(storage_create.deployment_id, "agent-2");
        assert_storage_body(&storage_create.params, Some(2048));

        // Test create command with deadline
        let deadline = Utc::now() + chrono::Duration::hours(1);
        let deadline_create = test_create_command_with_deadline(
            "agent-3",
            "time-sensitive",
            BodySpec::inline(b"{}"),
            deadline,
        );
        assert_eq!(deadline_create.deadline, Some(deadline));
    }
}
