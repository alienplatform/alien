use crate::error::{ErrorData, Result};
use crate::presigned::PresignedRequest;
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Protocol version identifier
pub const COMMANDS_PROTOCOL_VERSION: &str = "arc.v1";

/// Default inline size limit in bytes (150 KB)
/// This is the most conservative platform limit (Azure Service Bus Standard at 256KB)
/// with headroom for base64 encoding (~4/3 inflation) and envelope metadata.
pub const COMMANDS_INLINE_MAX_BYTES: usize = 150_000;

/// Command states in the Commands protocol lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandState {
    /// Client chose storage params but hasn't uploaded yet
    PendingUpload,
    /// Command is fully specified and ready for dispatch
    Pending,
    /// Command has been sent to deployment infrastructure
    Dispatched,
    /// Command completed successfully
    Succeeded,
    /// Command failed
    Failed,
    /// Command expired past deadline
    Expired,
}

impl CommandState {
    /// Check if this is a terminal state (no further transitions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            CommandState::Succeeded | CommandState::Failed | CommandState::Expired
        )
    }

    /// Check if this state can transition to the given target state
    pub fn can_transition_to(&self, target: &CommandState) -> bool {
        // Same state is always allowed (idempotent)
        if self == target {
            return true;
        }

        match (self, target) {
            // From PendingUpload
            (CommandState::PendingUpload, CommandState::Pending) => true,
            (CommandState::PendingUpload, CommandState::Expired) => true,

            // From Pending
            (CommandState::Pending, CommandState::Dispatched) => true,
            (CommandState::Pending, CommandState::Expired) => true,

            // From Dispatched
            (CommandState::Dispatched, CommandState::Pending) => true, // Allow lease release
            (CommandState::Dispatched, CommandState::Succeeded) => true,
            (CommandState::Dispatched, CommandState::Failed) => true,
            (CommandState::Dispatched, CommandState::Expired) => true,

            // Terminal states cannot transition to different states
            _ if self.is_terminal() => false,

            _ => false,
        }
    }
}

impl AsRef<str> for CommandState {
    fn as_ref(&self) -> &str {
        match self {
            CommandState::PendingUpload => "PENDING_UPLOAD",
            CommandState::Pending => "PENDING",
            CommandState::Dispatched => "DISPATCHED",
            CommandState::Succeeded => "SUCCEEDED",
            CommandState::Failed => "FAILED",
            CommandState::Expired => "EXPIRED",
        }
    }
}

/// Body specification supporting inline and storage modes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum BodySpec {
    /// Inline base64-encoded body
    Inline {
        #[serde(rename = "inlineBase64")]
        inline_base64: String,
    },
    /// Storage-backed body
    Storage {
        /// Size of the body in bytes
        size: Option<u64>,
        /// Pre-signed request for retrieving the body (for deployments)
        #[serde(rename = "storageGetRequest", skip_serializing_if = "Option::is_none")]
        storage_get_request: Option<PresignedRequest>,
        /// Indicates storage upload was used for response submission
        #[serde(rename = "storagePutUsed", skip_serializing_if = "Option::is_none")]
        storage_put_used: Option<bool>,
    },
}

impl BodySpec {
    /// Create an inline body from bytes
    pub fn inline(data: &[u8]) -> Self {
        use base64::{engine::general_purpose, Engine as _};
        Self::Inline {
            inline_base64: general_purpose::STANDARD.encode(data),
        }
    }

    /// Create a storage body with just size
    pub fn storage(size: u64) -> Self {
        Self::Storage {
            size: Some(size),
            storage_get_request: None,
            storage_put_used: None,
        }
    }

    /// Create a storage body with presigned request
    pub fn storage_with_request(size: u64, get_request: PresignedRequest) -> Self {
        Self::Storage {
            size: Some(size),
            storage_get_request: Some(get_request),
            storage_put_used: None,
        }
    }

    /// Get the body mode as string
    pub fn mode(&self) -> &str {
        match self {
            BodySpec::Inline { .. } => "inline",
            BodySpec::Storage { .. } => "storage",
        }
    }

    /// Get the size of the body if known
    pub fn size(&self) -> Option<u64> {
        match self {
            BodySpec::Inline { inline_base64 } => {
                // Calculate approximate decoded size
                let encoded_len = inline_base64.len();
                Some((encoded_len * 3 / 4) as u64)
            }
            BodySpec::Storage { size, .. } => *size,
        }
    }

    /// Decode inline body or return None for storage mode
    pub fn decode_inline(&self) -> Option<Vec<u8>> {
        match self {
            BodySpec::Inline { inline_base64 } => {
                use base64::{engine::general_purpose, Engine as _};
                general_purpose::STANDARD.decode(inline_base64).ok()
            }
            BodySpec::Storage { .. } => None,
        }
    }
}

/// Command response from deployment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum CommandResponse {
    /// Command executed successfully
    Success {
        /// Response data (JSON, can be large)
        response: BodySpec,
    },
    /// Command failed with an error
    Error {
        /// Error code
        code: String,
        /// Error message
        message: String,
        /// Optional additional details
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
}

impl CommandResponse {
    /// Create a success response with inline JSON data
    pub fn success_json(json: &serde_json::Value) -> Result<Self> {
        let body_bytes = serde_json::to_vec(json).into_alien_error().context(
            ErrorData::JsonSerializationFailed {
                reason: "Failed to serialize command response".to_string(),
            },
        )?;
        Ok(Self::Success {
            response: BodySpec::inline(&body_bytes),
        })
    }

    /// Create a success response with inline bytes
    pub fn success(data: &[u8]) -> Self {
        Self::Success {
            response: BodySpec::inline(data),
        }
    }

    /// Create a success response with storage body
    pub fn success_storage(size: u64) -> Self {
        Self::Success {
            response: BodySpec::Storage {
                size: Some(size),
                storage_get_request: None,
                storage_put_used: Some(true),
            },
        }
    }

    /// Create an error response
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Create an error response with details
    pub fn error_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
            details: Some(details.into()),
        }
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        matches!(self, CommandResponse::Success { .. })
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self, CommandResponse::Error { .. })
    }
}

/// Response handling configuration for deployments
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResponseHandling {
    /// Maximum response body size that can be submitted inline
    pub max_inline_bytes: u64,
    /// URL where deployments submit responses
    pub submit_response_url: String,
    /// Pre-signed request for uploading large response bodies
    pub storage_upload_request: PresignedRequest,
}

/// Commands envelope sent to deployments
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// Protocol version identifier
    pub protocol: String,
    /// Target deployment identifier
    pub deployment_id: String,
    /// Unique command identifier
    pub command_id: String,
    /// Attempt number (starts at 1)
    pub attempt: u32,
    /// Command deadline
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    /// Command name (e.g., "generate-report", "sync-data")
    pub command: String,
    /// Command parameters (JSON, can be large)
    pub params: BodySpec,
    /// Response handling configuration
    pub response_handling: ResponseHandling,
}

impl Envelope {
    /// Create a new Commands envelope
    pub fn new(
        deployment_id: impl Into<String>,
        command_id: impl Into<String>,
        attempt: u32,
        deadline: Option<DateTime<Utc>>,
        command: impl Into<String>,
        params: BodySpec,
        response_handling: ResponseHandling,
    ) -> Self {
        Self {
            protocol: COMMANDS_PROTOCOL_VERSION.to_string(),
            deployment_id: deployment_id.into(),
            command_id: command_id.into(),
            attempt,
            deadline,
            command: command.into(),
            params,
            response_handling,
        }
    }

    /// Validate the envelope structure
    pub fn validate(&self) -> Result<()> {
        if self.protocol != COMMANDS_PROTOCOL_VERSION {
            return Err(AlienError::new(ErrorData::InvalidEnvelope {
                message: format!(
                    "Invalid protocol version: expected {}, got {}",
                    COMMANDS_PROTOCOL_VERSION, self.protocol
                ),
                field: Some("protocol".to_string()),
            }));
        }

        if self.command_id.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidEnvelope {
                message: "Command ID cannot be empty".to_string(),
                field: Some("commandId".to_string()),
            }));
        }

        if self.attempt == 0 {
            return Err(AlienError::new(ErrorData::InvalidEnvelope {
                message: "Attempt must be >= 1".to_string(),
                field: Some("attempt".to_string()),
            }));
        }

        if self.command.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidEnvelope {
                message: "Command name cannot be empty".to_string(),
                field: Some("command".to_string()),
            }));
        }

        Ok(())
    }
}

// Client-facing API types

/// Request to create a new command
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateCommandRequest {
    /// Target deployment identifier
    pub deployment_id: String,
    /// Command name (e.g., "generate-report", "sync-data")
    pub command: String,
    /// Command parameters (JSON, can be large)
    pub params: BodySpec,
    /// Optional deadline for command completion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    /// Optional idempotency key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Storage upload information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorageUpload {
    /// Pre-signed request for uploading command params
    pub put_request: PresignedRequest,
    /// Expiration time for upload URL
    pub expires_at: DateTime<Utc>,
}

/// Response to command creation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateCommandResponse {
    /// Unique command identifier
    pub command_id: String,
    /// Current command state
    pub state: CommandState,
    /// Storage upload info (only for storage mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_upload: Option<StorageUpload>,
    /// Maximum inline body size allowed
    pub inline_allowed_up_to: u64,
    /// Next action for client: "upload" | "poll"
    pub next: String,
}

/// Request to mark upload as complete
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UploadCompleteRequest {
    /// Size of uploaded data
    pub size: u64,
}

/// Response to upload completion
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UploadCompleteResponse {
    /// Command identifier
    pub command_id: String,
    /// Updated command state
    pub state: CommandState,
}

/// Response to status queries
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CommandStatusResponse {
    /// Command identifier
    pub command_id: String,
    /// Current command state
    pub state: CommandState,
    /// Current attempt number
    pub attempt: u32,
    /// Response data (only for succeeded/failed state)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<CommandResponse>,
}

/// Request to submit a command response (from deployment)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SubmitResponseRequest {
    /// The command response
    #[serde(flatten)]
    pub response: CommandResponse,
}

// Leasing system types

/// Request for acquiring leases
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LeaseRequest {
    /// Deployment identifier
    pub deployment_id: String,
    /// Maximum number of leases to acquire
    #[serde(default = "default_max_leases")]
    pub max_leases: usize,
    /// Lease duration in seconds
    #[serde(default = "default_lease_seconds")]
    pub lease_seconds: u64,
}

fn default_max_leases() -> usize {
    1
}

fn default_lease_seconds() -> u64 {
    60
}

impl Default for LeaseRequest {
    fn default() -> Self {
        Self {
            deployment_id: "default-deployment".to_string(),
            max_leases: default_max_leases(),
            lease_seconds: default_lease_seconds(),
        }
    }
}

/// Lease information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LeaseInfo {
    /// Unique lease identifier
    pub lease_id: String,
    /// When lease expires
    pub lease_expires_at: DateTime<Utc>,
    /// Command identifier
    pub command_id: String,
    /// Attempt number
    pub attempt: u32,
    /// Commands envelope to process
    pub envelope: Envelope,
}

/// Response to lease acquisition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LeaseResponse {
    /// Acquired leases (empty array if none available)
    pub leases: Vec<LeaseInfo>,
}

/// Request to release a lease
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRequest {
    /// Lease identifier to release
    pub lease_id: String,
}

#[cfg(test)]
mod tests {
    use crate::PresignedOperation;

    use super::*;

    #[test]
    fn test_command_state_transitions() {
        use CommandState::*;

        // Valid transitions
        assert!(PendingUpload.can_transition_to(&Pending));
        assert!(PendingUpload.can_transition_to(&Expired));
        assert!(Pending.can_transition_to(&Dispatched));
        assert!(Pending.can_transition_to(&Expired));
        assert!(Dispatched.can_transition_to(&Pending)); // Lease release
        assert!(Dispatched.can_transition_to(&Succeeded));
        assert!(Dispatched.can_transition_to(&Failed));
        assert!(Dispatched.can_transition_to(&Expired));

        // Invalid transitions
        assert!(!PendingUpload.can_transition_to(&Dispatched));
        assert!(!Pending.can_transition_to(&PendingUpload));
        assert!(!Succeeded.can_transition_to(&Failed));

        // Idempotent transitions
        assert!(Pending.can_transition_to(&Pending));
        assert!(Succeeded.can_transition_to(&Succeeded));
    }

    #[test]
    fn test_body_spec() {
        let data = b"Hello, World!";
        let body = BodySpec::inline(data);

        assert_eq!(body.mode(), "inline");
        assert_eq!(body.decode_inline().unwrap(), data);

        let storage_body = BodySpec::storage(1024);
        assert_eq!(storage_body.mode(), "storage");
        assert_eq!(storage_body.size(), Some(1024));
        assert!(storage_body.decode_inline().is_none());
    }

    #[test]
    fn test_command_response() {
        // Test success response
        let success = CommandResponse::success(b"result data");
        assert!(success.is_success());
        assert!(!success.is_error());

        // Test error response
        let error = CommandResponse::error("INVALID_INPUT", "Missing required field");
        assert!(error.is_error());
        assert!(!error.is_success());

        // Test JSON success response
        let json = serde_json::json!({"result": "ok"});
        let json_success = CommandResponse::success_json(&json).unwrap();
        assert!(json_success.is_success());
    }

    #[test]
    fn test_envelope_validation() {
        let params = BodySpec::inline(b"{}");
        let response_handling = ResponseHandling {
            max_inline_bytes: 150000,
            submit_response_url: "https://arc.example.com/response".to_string(),
            storage_upload_request: PresignedRequest::new_http(
                "https://storage.example.com/upload".to_string(),
                "PUT".to_string(),
                std::collections::HashMap::new(),
                PresignedOperation::Put,
                "test-path".to_string(),
                Utc::now() + chrono::Duration::hours(1),
            ),
        };

        let envelope = Envelope::new(
            "deployment_123",
            "cmd_123",
            1,
            None,
            "generate-report",
            params,
            response_handling,
        );

        assert!(envelope.validate().is_ok());

        // Test invalid protocol
        let mut invalid_envelope = envelope.clone();
        invalid_envelope.protocol = "invalid".to_string();
        assert!(invalid_envelope.validate().is_err());

        // Test empty command ID
        let mut invalid_envelope = envelope.clone();
        invalid_envelope.command_id = "".to_string();
        assert!(invalid_envelope.validate().is_err());

        // Test zero attempt
        let mut invalid_envelope = envelope.clone();
        invalid_envelope.attempt = 0;
        assert!(invalid_envelope.validate().is_err());

        // Test empty command name
        let mut invalid_envelope = envelope.clone();
        invalid_envelope.command = "".to_string();
        assert!(invalid_envelope.validate().is_err());
    }

    #[test]
    fn test_serialization() {
        // Test CommandResponse serialization
        let success = CommandResponse::success(b"test");
        let json = serde_json::to_string(&success).unwrap();
        assert!(json.contains("\"status\":\"success\""));

        let error = CommandResponse::error("ERR", "msg");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"status\":\"error\""));

        // Test Envelope serialization
        let params = BodySpec::inline(b"{}");
        let response_handling = ResponseHandling {
            max_inline_bytes: 150000,
            submit_response_url: "https://arc.example.com/response".to_string(),
            storage_upload_request: PresignedRequest::new_http(
                "https://storage.example.com/upload".to_string(),
                "PUT".to_string(),
                std::collections::HashMap::new(),
                PresignedOperation::Put,
                "test-path".to_string(),
                Utc::now() + chrono::Duration::hours(1),
            ),
        };

        let envelope = Envelope::new(
            "deployment_123",
            "cmd_123",
            1,
            None,
            "test-command",
            params,
            response_handling,
        );

        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"deploymentId\":\"deployment_123\""));
        assert!(json.contains("\"commandId\":\"cmd_123\""));
        assert!(json.contains("\"command\":\"test-command\""));
        assert!(json.contains("\"protocol\":\"arc.v1\""));
    }
}
