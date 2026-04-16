//! Command Server Implementation
//!
//! The Command server implements the command lifecycle:
//! - CommandRegistry is the SOURCE OF TRUTH for all metadata (state, timestamps, etc.)
//! - KV stores ONLY operational data (params/response blobs, indices, leases)

use std::sync::Arc;
use std::time::Duration;

use alien_bindings::presigned::PresignedRequest;
use alien_bindings::traits::{Kv, PutOptions, Storage};
use alien_core::DeploymentModel;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use chrono::{DateTime, Utc};
use hex;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{debug, info};
use uuid::Uuid;

use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use object_store::path::Path as StoragePath;

use crate::error::{ErrorData, Result};
use crate::types::*;
use crate::INLINE_MAX_BYTES;

/// Max serialized KV value size. Conservative threshold below the hard 24KB
/// boundary (Azure Table Storage) to account for JSON wrapping overhead.
const KV_VALUE_THRESHOLD: usize = 20_000;

pub mod axum_handlers;
pub mod command_registry;
pub mod dispatchers;
pub mod storage;

pub use axum_handlers::{
    create_axum_router, CommandPayloadResponse, HasCommandServer, StorePayloadRequest,
};
pub use command_registry::{
    CommandEnvelopeData, CommandMetadata, CommandRegistry, CommandStatus, InMemoryCommandRegistry,
};
pub use dispatchers::{CommandDispatcher, NullCommandDispatcher};

// =============================================================================
// KV Data Structures (Operational Data Only)
// =============================================================================

/// Params stored in KV (just the blob)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandParamsData {
    pub params: BodySpec,
}

/// Response stored in KV (just the blob)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandResponseData {
    pub response: CommandResponse,
}

/// Lease record with TTL
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LeaseData {
    pub lease_id: String,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub owner: String,
}

/// Deadline index data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeadlineIndexData {
    pub command_id: String,
    pub deadline: DateTime<Utc>,
}

// =============================================================================
// Command Server
// =============================================================================

/// Core command server implementation.
///
/// Uses CommandRegistry as source of truth for metadata.
/// Uses KV for operational data (params, responses, indices, leases).
pub struct CommandServer {
    kv: Arc<dyn Kv>,
    storage: Arc<dyn Storage>,
    command_dispatcher: Arc<dyn CommandDispatcher>,
    command_registry: Arc<dyn CommandRegistry>,
    inline_max_bytes: usize,
    base_url: String,
    response_signing_key: Vec<u8>,
}

impl CommandServer {
    /// Create a new command server instance
    pub fn new(
        kv: Arc<dyn Kv>,
        storage: Arc<dyn Storage>,
        command_dispatcher: Arc<dyn CommandDispatcher>,
        command_registry: Arc<dyn CommandRegistry>,
        base_url: String,
        response_signing_key: Vec<u8>,
    ) -> Self {
        Self {
            kv,
            storage,
            command_dispatcher,
            command_registry,
            inline_max_bytes: INLINE_MAX_BYTES,
            base_url,
            response_signing_key,
        }
    }

    /// Create a new command server with custom inline size limit
    pub fn with_inline_limit(
        kv: Arc<dyn Kv>,
        storage: Arc<dyn Storage>,
        command_dispatcher: Arc<dyn CommandDispatcher>,
        command_registry: Arc<dyn CommandRegistry>,
        base_url: String,
        inline_max_bytes: usize,
        response_signing_key: Vec<u8>,
    ) -> Self {
        Self {
            kv,
            storage,
            command_dispatcher,
            command_registry,
            inline_max_bytes,
            base_url,
            response_signing_key,
        }
    }

    /// Maximum allowed response token lifetime (2 hours).
    /// Tokens are issued with 1-hour expiry; this cap provides headroom for clock skew
    /// while rejecting tokens with absurdly far-future expiration.
    const MAX_RESPONSE_TOKEN_LIFETIME_SECS: i64 = 7200;

    /// Sign a response URL for a specific command.
    ///
    /// Returns `(hmac_hex, expires_epoch)`. The HMAC is computed over
    /// `"arc.v1:{command_id}:{expires}"` using the server's signing key.
    fn sign_response_url(&self, command_id: &str) -> (String, i64) {
        let expires = Utc::now().timestamp() + 3600; // 1 hour
        let message = format!("commands.v1:{}:{}", command_id, expires);

        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(&self.response_signing_key).expect("HMAC accepts any key");
        mac.update(message.as_bytes());
        let result = mac.finalize();
        let token = hex::encode(result.into_bytes());

        (token, expires)
    }

    /// Verify a response token for a specific command.
    ///
    /// Performs HMAC verification first (constant-time), then checks expiration,
    /// to avoid leaking timing information about token validity windows.
    pub fn verify_response_token(
        &self,
        command_id: &str,
        token: &str,
        expires: i64,
    ) -> bool {
        // Validate token format: SHA-256 HMAC = 32 bytes = 64 hex chars.
        if token.len() != 64 {
            return false;
        }

        let message = format!("commands.v1:{}:{}", command_id, expires);

        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(&self.response_signing_key).expect("HMAC accepts any key");
        mac.update(message.as_bytes());

        // Decode hex and verify HMAC (constant-time comparison).
        let Ok(token_bytes) = hex::decode(token) else {
            return false;
        };
        let hmac_valid = mac.verify_slice(&token_bytes).is_ok();

        // Check expiration and max lifetime AFTER HMAC to avoid timing leaks.
        let now = Utc::now().timestamp();
        let not_expired = now <= expires;
        let within_max_lifetime = expires <= now + Self::MAX_RESPONSE_TOKEN_LIFETIME_SECS;

        hmac_valid && not_expired && within_max_lifetime
    }

    // =========================================================================
    // Public API Methods
    // =========================================================================

    /// Create a new command.
    ///
    /// Flow:
    /// 1. Validate request
    /// 2. Registry creates command metadata (source of truth)
    /// 3. KV stores params blob
    /// 4. KV creates pending index (for Pull) or dispatch (for Push)
    pub async fn create_command(
        &self,
        request: CreateCommandRequest,
    ) -> Result<CreateCommandResponse> {
        // Validate the request
        self.validate_create_command(&request).await?;

        // Check idempotency if key provided
        if let Some(ref idem_key) = request.idempotency_key {
            if let Some(existing_id) = self.check_idempotency(idem_key).await? {
                // Return existing command status
                let status = self
                    .command_registry
                    .get_command_status(&existing_id)
                    .await?;
                if let Some(s) = status {
                    return Ok(CreateCommandResponse {
                        command_id: existing_id,
                        state: s.state,
                        storage_upload: None,
                        inline_allowed_up_to: self.inline_max_bytes as u64,
                        next: "poll".to_string(),
                    });
                }
            }
        }

        // Determine initial state and request size
        let (initial_state, request_size_bytes) = match &request.params {
            BodySpec::Inline { inline_base64 } => {
                let size = inline_base64.len() as u64;
                (CommandState::Pending, Some(size))
            }
            BodySpec::Storage { size, .. } => {
                if size.unwrap_or(0) > self.inline_max_bytes as u64 {
                    (CommandState::PendingUpload, *size)
                } else {
                    (CommandState::Pending, *size)
                }
            }
        };

        // 1. Registry creates command metadata (SOURCE OF TRUTH)
        let metadata = self
            .command_registry
            .create_command(
                &request.deployment_id,
                &request.command,
                initial_state,
                request.deadline,
                request_size_bytes,
            )
            .await?;

        let command_id = metadata.command_id;
        let deployment_model = metadata.deployment_model;

        // 2. Store idempotency mapping in KV
        if let Some(ref idem_key) = request.idempotency_key {
            self.store_idempotency(idem_key, &command_id).await?;
        }

        // 3. Store params in KV
        self.store_params(&command_id, &request.params).await?;

        // 4. Generate storage upload URL if needed
        let storage_upload = if initial_state == CommandState::PendingUpload {
            Some(self.generate_params_upload(&command_id).await?)
        } else {
            None
        };

        // 5. Handle dispatch based on state and deployment model
        let (final_state, next_action) = if initial_state == CommandState::Pending {
            match deployment_model {
                DeploymentModel::Push => {
                    // Push model: dispatch immediately
                    self.dispatch_command_push(&command_id, &request.deployment_id)
                        .await?;
                    (CommandState::Dispatched, "poll")
                }
                DeploymentModel::Pull => {
                    // Pull model: create pending index, deployment will poll
                    self.create_pending_index(&request.deployment_id, &command_id)
                        .await?;
                    debug!(
                        "Command {} ready for pull (deployment will poll)",
                        command_id
                    );
                    (CommandState::Pending, "poll")
                }
            }
        } else {
            // PendingUpload - need upload first
            (initial_state, "upload")
        };

        // 6. Create deadline index if deadline provided
        if let Some(deadline) = request.deadline {
            self.create_deadline_index(&command_id, deadline).await?;
        }

        Ok(CreateCommandResponse {
            command_id,
            state: final_state,
            storage_upload,
            inline_allowed_up_to: self.inline_max_bytes as u64,
            next: next_action.to_string(),
        })
    }

    /// Mark upload as complete and dispatch command.
    pub async fn upload_complete(
        &self,
        command_id: &str,
        upload_request: UploadCompleteRequest,
    ) -> Result<UploadCompleteResponse> {
        // 1. Get current status from registry (source of truth)
        let status = self
            .command_registry
            .get_command_status(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        // 2. Validate current state
        if status.state != CommandState::PendingUpload {
            return Err(AlienError::new(ErrorData::InvalidStateTransition {
                from: status.state.as_ref().to_string(),
                to: CommandState::Pending.as_ref().to_string(),
            }));
        }

        // 3. Update params in KV with storage reference
        let storage_get_request = self.generate_storage_get_request(command_id).await?;
        let params = BodySpec::Storage {
            size: Some(upload_request.size),
            storage_get_request: Some(storage_get_request),
            storage_put_used: None,
        };
        self.store_params(command_id, &params).await?;

        // 4. Update registry to Pending state
        self.command_registry
            .update_command_state(command_id, CommandState::Pending, None, None, None, None)
            .await?;

        // 5. Get deployment model from registry and handle dispatch
        let metadata = self
            .command_registry
            .get_command_metadata(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        let final_state = match metadata.deployment_model {
            DeploymentModel::Push => {
                self.dispatch_command_push(command_id, &status.deployment_id)
                    .await?;
                CommandState::Dispatched
            }
            DeploymentModel::Pull => {
                self.create_pending_index(&status.deployment_id, command_id)
                    .await?;
                debug!(
                    "Command {} ready for pull after upload (deployment will poll)",
                    command_id
                );
                CommandState::Pending
            }
        };

        Ok(UploadCompleteResponse {
            command_id: command_id.to_string(),
            state: final_state,
        })
    }

    /// Get command status.
    ///
    /// Queries registry for metadata (source of truth), KV for response blob.
    pub async fn get_command_status(&self, command_id: &str) -> Result<CommandStatusResponse> {
        // 1. Get status from registry (SOURCE OF TRUTH)
        let status = self
            .command_registry
            .get_command_status(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        // 2. Check deadline expiry inline
        if let Some(deadline) = status.deadline {
            if Utc::now() > deadline && !status.state.is_terminal() {
                // Expire the command
                self.command_registry
                    .update_command_state(
                        command_id,
                        CommandState::Expired,
                        None,
                        Some(Utc::now()),
                        None,
                        None,
                    )
                    .await?;

                // Clean up pending index
                self.delete_pending_index(&status.deployment_id, command_id)
                    .await?;

                // Return expired status directly (avoid recursion)
                return Ok(CommandStatusResponse {
                    command_id: command_id.to_string(),
                    state: CommandState::Expired,
                    attempt: status.attempt,
                    response: None,
                });
            }
        }

        // 3. Get response blob from KV if terminal state
        let response = if status.state.is_terminal() {
            self.get_response(command_id).await?
        } else {
            None
        };

        Ok(CommandStatusResponse {
            command_id: command_id.to_string(),
            state: status.state,
            attempt: status.attempt,
            response,
        })
    }

    /// Submit response from deployment.
    ///
    /// Stores response blob in KV, updates state in registry.
    pub async fn submit_command_response(
        &self,
        command_id: &str,
        mut response: CommandResponse,
    ) -> Result<()> {
        // 1. Get current status from registry
        let status = self
            .command_registry
            .get_command_status(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        // 2. Handle duplicate responses gracefully
        if status.state.is_terminal() {
            debug!(
                "Ignoring duplicate response for terminal command {}",
                command_id
            );
            return Ok(());
        }

        // 3. Validate state transition
        if status.state != CommandState::Dispatched {
            return Err(AlienError::new(ErrorData::InvalidStateTransition {
                from: status.state.as_ref().to_string(),
                to: CommandState::Succeeded.as_ref().to_string(),
            }));
        }

        // 4. If response was uploaded to storage, generate download URL
        if let CommandResponse::Success {
            response: ref mut body,
        } = response
        {
            if let BodySpec::Storage {
                size,
                storage_get_request,
                storage_put_used,
            } = body
            {
                if storage_get_request.is_none() && storage_put_used.unwrap_or(false) {
                    let get_request = self
                        .generate_response_storage_get_request(command_id)
                        .await?;
                    *body = BodySpec::Storage {
                        size: *size,
                        storage_get_request: Some(get_request),
                        storage_put_used: *storage_put_used,
                    };
                }
            }
        }

        // 5. Store response blob in KV
        self.store_response(command_id, &response).await?;

        // 6. Clean up lease from KV
        self.delete_lease(command_id).await?;

        // 7. Clean up pending index from KV (terminal state)
        self.delete_pending_index(&status.deployment_id, command_id)
            .await?;

        // 8. Update registry state (SOURCE OF TRUTH)
        let (new_state, error) = if response.is_success() {
            (CommandState::Succeeded, None)
        } else if let CommandResponse::Error { code, message, .. } = &response {
            (
                CommandState::Failed,
                Some(serde_json::json!({ "code": code, "message": message })),
            )
        } else {
            (CommandState::Failed, None)
        };

        let response_size = match &response {
            CommandResponse::Success {
                response: BodySpec::Inline { inline_base64 },
            } => Some(inline_base64.len() as u64),
            CommandResponse::Success {
                response: BodySpec::Storage { size, .. },
            } => *size,
            _ => None,
        };

        self.command_registry
            .update_command_state(
                command_id,
                new_state,
                None, // dispatched_at already set
                Some(Utc::now()),
                response_size,
                error,
            )
            .await?;

        info!(
            "Command {} completed with state {:?}",
            command_id, new_state
        );
        Ok(())
    }

    /// Acquire leases for polling deployments.
    ///
    /// Scans KV pending index, queries registry for metadata, creates leases in KV.
    pub async fn acquire_lease(
        &self,
        deployment_id: &str,
        lease_request: &LeaseRequest,
    ) -> Result<LeaseResponse> {
        let mut leases = Vec::new();

        // 1. Scan KV pending index
        let target_prefix = format!("target:{}:pending:", deployment_id);
        let scan_result = self
            .kv
            .scan_prefix(&target_prefix, Some(lease_request.max_leases * 2), None)
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "scan_prefix".to_string(),
                key: target_prefix.clone(),
                message: "Failed to scan for pending commands".to_string(),
            })?;

        for (index_key, _) in scan_result.items {
            if leases.len() >= lease_request.max_leases {
                break;
            }

            let command_id = self.extract_command_id_from_index_key(&index_key)?;

            // 2. Try to acquire lease atomically in KV
            let lease_id = format!("lease_{}", Uuid::new_v4());
            let lease_duration = Duration::from_secs(lease_request.lease_seconds);
            let expires_at =
                Utc::now() + chrono::Duration::seconds(lease_request.lease_seconds as i64);

            let lease_data = LeaseData {
                lease_id: lease_id.clone(),
                acquired_at: Utc::now(),
                expires_at,
                owner: deployment_id.to_string(),
            };

            let lease_key = format!("cmd:{}:lease", command_id);
            let lease_value = serde_json::to_vec(&lease_data).into_alien_error().context(
                ErrorData::SerializationFailed {
                    message: "Failed to serialize lease data".to_string(),
                    data_type: Some("LeaseData".to_string()),
                },
            )?;

            let options = Some(PutOptions {
                ttl: Some(lease_duration),
                if_not_exists: true,
            });

            let success = self
                .kv
                .put(&lease_key, lease_value, options)
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: lease_key.clone(),
                    message: "Failed to create lease".to_string(),
                })?;

            if !success {
                // Lease already exists, skip
                continue;
            }

            // 3. Get metadata from registry
            let metadata = match self
                .command_registry
                .get_command_metadata(&command_id)
                .await?
            {
                Some(m) => m,
                None => {
                    // Command doesn't exist in registry, clean up
                    self.delete_lease(&command_id).await?;
                    let _ = self.kv.delete(&index_key).await;
                    continue;
                }
            };

            // 4. Check if command is in terminal state (stale index)
            if metadata.state.is_terminal() {
                // Clean up stale data
                self.delete_lease(&command_id).await?;
                let _ = self.kv.delete(&index_key).await;
                continue;
            }

            // 5. Check deadline expiry
            if let Some(deadline) = metadata.deadline {
                if Utc::now() > deadline {
                    // Expire the command
                    self.command_registry
                        .update_command_state(
                            &command_id,
                            CommandState::Expired,
                            None,
                            Some(Utc::now()),
                            None,
                            None,
                        )
                        .await?;
                    self.delete_lease(&command_id).await?;
                    let _ = self.kv.delete(&index_key).await;
                    continue;
                }
            }

            // 6. Get params from KV
            let params = match self.get_params(&command_id).await? {
                Some(p) => p,
                None => {
                    // No params, something went wrong
                    self.delete_lease(&command_id).await?;
                    continue;
                }
            };

            // 7. Update registry state to Dispatched
            self.command_registry
                .update_command_state(
                    &command_id,
                    CommandState::Dispatched,
                    Some(Utc::now()),
                    None,
                    None,
                    None,
                )
                .await?;

            // 8. Build envelope
            let envelope = self.build_envelope(&command_id, &metadata, params).await?;

            leases.push(LeaseInfo {
                lease_id,
                lease_expires_at: expires_at,
                command_id: command_id.clone(),
                attempt: metadata.attempt,
                envelope,
            });
        }

        Ok(LeaseResponse { leases })
    }

    /// Release a lease manually.
    ///
    /// Increments attempt count in registry, returns command to Pending state.
    pub async fn release_lease(&self, command_id: &str, lease_id: &str) -> Result<()> {
        let lease_key = format!("cmd:{}:lease", command_id);

        // 1. Verify lease ownership
        if let Ok(Some(lease_data)) = self.kv.get(&lease_key).await {
            let lease: LeaseData = serde_json::from_slice(&lease_data)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to deserialize lease data".to_string(),
                    data_type: Some("LeaseData".to_string()),
                })?;

            if lease.lease_id != lease_id {
                return Err(AlienError::new(ErrorData::LeaseNotFound {
                    lease_id: lease_id.to_string(),
                }));
            }

            // 2. Delete lease from KV
            self.delete_lease(command_id).await?;

            // 3. Increment attempt in registry
            self.command_registry.increment_attempt(command_id).await?;

            // 4. Update registry state back to Pending
            self.command_registry
                .update_command_state(command_id, CommandState::Pending, None, None, None, None)
                .await?;

            // Note: Pending index is NOT removed on lease, so command is still there
            debug!("Lease {} released for command {}", lease_id, command_id);
        }

        Ok(())
    }

    /// Get the deployment_id that owns a command.
    ///
    /// Used by the manager's auth layer to check whether the caller has access
    /// to a specific command without fetching the full status.
    pub async fn get_command_deployment_id(&self, command_id: &str) -> Result<Option<String>> {
        let status = self.command_registry.get_command_status(command_id).await?;
        Ok(status.map(|s| s.deployment_id))
    }

    /// Release a lease by lease_id only (for the API).
    pub async fn release_lease_by_id(&self, lease_id: &str) -> Result<()> {
        // Scan for leases to find the one with this lease_id
        let lease_prefix = "cmd:";
        let scan_result = self
            .kv
            .scan_prefix(lease_prefix, None, None)
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "scan_prefix".to_string(),
                key: lease_prefix.to_string(),
                message: "Failed to scan for lease keys".to_string(),
            })?;

        for (key, value) in scan_result.items {
            if key.ends_with(":lease") {
                if let Ok(lease) = serde_json::from_slice::<LeaseData>(&value) {
                    if lease.lease_id == lease_id {
                        let command_id = key
                            .strip_prefix("cmd:")
                            .and_then(|s| s.strip_suffix(":lease"))
                            .ok_or_else(|| {
                                AlienError::new(ErrorData::Other {
                                    message: format!("Invalid lease key format: {}", key),
                                })
                            })?;

                        return self.release_lease(command_id, lease_id).await;
                    }
                }
            }
        }

        Err(AlienError::new(ErrorData::LeaseNotFound {
            lease_id: lease_id.to_string(),
        }))
    }

    // =========================================================================
    // Internal Helper Methods
    // =========================================================================

    async fn validate_create_command(&self, request: &CreateCommandRequest) -> Result<()> {
        if request.command.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidCommand {
                message: "Command name cannot be empty".to_string(),
            }));
        }

        if request.deployment_id.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidCommand {
                message: "Deployment ID cannot be empty".to_string(),
            }));
        }

        if let Some(deadline) = request.deadline {
            if deadline <= Utc::now() {
                return Err(AlienError::new(ErrorData::InvalidCommand {
                    message: "Deadline must be in the future".to_string(),
                }));
            }
        }

        Ok(())
    }

    // --- Idempotency ---

    async fn check_idempotency(&self, idem_key: &str) -> Result<Option<String>> {
        let key = format!("idem:{}", idem_key);
        if let Some(data) = self
            .kv
            .get(&key)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: key.clone(),
                message: "Failed to check idempotency".to_string(),
            })?
        {
            let command_id = String::from_utf8(data).into_alien_error().context(
                ErrorData::SerializationFailed {
                    message: "Invalid idempotency data".to_string(),
                    data_type: Some("String".to_string()),
                },
            )?;
            return Ok(Some(command_id));
        }
        Ok(None)
    }

    async fn store_idempotency(&self, idem_key: &str, command_id: &str) -> Result<()> {
        let key = format!("idem:{}", idem_key);
        let ttl = Duration::from_secs(24 * 60 * 60); // 24 hours
        self.kv
            .put(
                &key,
                command_id.as_bytes().to_vec(),
                Some(PutOptions {
                    ttl: Some(ttl),
                    if_not_exists: true,
                }),
            )
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.clone(),
                message: "Failed to store idempotency".to_string(),
            })?;
        Ok(())
    }

    // --- Params ---

    pub async fn store_params(&self, command_id: &str, params: &BodySpec) -> Result<()> {
        let key = format!("cmd:{}:params", command_id);

        // Try serializing as-is first
        let data = CommandParamsData {
            params: params.clone(),
        };
        let value = serde_json::to_vec(&data).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "Failed to serialize params".to_string(),
                data_type: Some("CommandParamsData".to_string()),
            },
        )?;

        // If it fits in KV, store directly (fast path)
        if value.len() <= KV_VALUE_THRESHOLD {
            self.kv
                .put(&key, value, None)
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.clone(),
                    message: "Failed to store params".to_string(),
                })?;
            return Ok(());
        }

        // Auto-promote: inline data exceeds KV limit, store raw bytes in blob
        if let BodySpec::Inline { inline_base64 } = params {
            let raw_bytes = general_purpose::STANDARD
                .decode(inline_base64)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to decode inline base64 params for auto-promotion".to_string(),
                    data_type: Some("base64".to_string()),
                })?;

            let raw_len = raw_bytes.len() as u64;
            let blob_path = StoragePath::from(format!("arc/commands/{}/params", command_id));

            self.storage
                .put(&blob_path, Bytes::from(raw_bytes).into())
                .await
                .into_alien_error()
                .context(ErrorData::StorageOperationFailed {
                    message: "Failed to auto-promote params to blob storage".to_string(),
                    operation: Some("put".to_string()),
                    path: Some(blob_path.to_string()),
                })?;

            debug!(
                "Auto-promoted params for command {} to blob ({} bytes raw)",
                command_id, raw_len
            );

            // Store tiny reference in KV instead
            let promoted = CommandParamsData {
                params: BodySpec::Storage {
                    size: Some(raw_len),
                    storage_get_request: None,
                    storage_put_used: Some(true),
                },
            };
            let promoted_value = serde_json::to_vec(&promoted).into_alien_error().context(
                ErrorData::SerializationFailed {
                    message: "Failed to serialize promoted params reference".to_string(),
                    data_type: Some("CommandParamsData".to_string()),
                },
            )?;
            self.kv.put(&key, promoted_value, None).await.context(
                ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.clone(),
                    message: "Failed to store promoted params reference".to_string(),
                },
            )?;
            return Ok(());
        }

        // Storage references are always tiny, store as-is
        self.kv
            .put(&key, value, None)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.clone(),
                message: "Failed to store params".to_string(),
            })?;
        Ok(())
    }

    pub async fn get_params(&self, command_id: &str) -> Result<Option<BodySpec>> {
        let key = format!("cmd:{}:params", command_id);
        if let Some(value) = self
            .kv
            .get(&key)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: key.clone(),
                message: "Failed to get params".to_string(),
            })?
        {
            let data: CommandParamsData = serde_json::from_slice(&value)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to deserialize params".to_string(),
                    data_type: Some("CommandParamsData".to_string()),
                })?;
            return Ok(Some(data.params));
        }
        Ok(None)
    }

    // --- Response ---

    pub async fn store_response(&self, command_id: &str, response: &CommandResponse) -> Result<()> {
        let key = format!("cmd:{}:response", command_id);
        let data = CommandResponseData {
            response: response.clone(),
        };
        let value = serde_json::to_vec(&data).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "Failed to serialize response".to_string(),
                data_type: Some("CommandResponseData".to_string()),
            },
        )?;

        // If it fits in KV, store directly (fast path)
        if value.len() <= KV_VALUE_THRESHOLD {
            self.kv
                .put(&key, value, None)
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.clone(),
                    message: "Failed to store response".to_string(),
                })?;
            return Ok(());
        }

        // Auto-promote: inline response exceeds KV limit
        if let CommandResponse::Success {
            response: BodySpec::Inline { inline_base64 },
        } = response
        {
            let raw_bytes = general_purpose::STANDARD
                .decode(inline_base64)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to decode inline base64 response for auto-promotion"
                        .to_string(),
                    data_type: Some("base64".to_string()),
                })?;

            let raw_len = raw_bytes.len() as u64;
            let blob_path = StoragePath::from(format!("arc/commands/{}/response", command_id));

            self.storage
                .put(&blob_path, Bytes::from(raw_bytes).into())
                .await
                .into_alien_error()
                .context(ErrorData::StorageOperationFailed {
                    message: "Failed to auto-promote response to blob storage".to_string(),
                    operation: Some("put".to_string()),
                    path: Some(blob_path.to_string()),
                })?;

            // Generate presigned GET URL for the caller
            let get_request = self
                .generate_response_storage_get_request(command_id)
                .await?;

            debug!(
                "Auto-promoted response for command {} to blob ({} bytes raw)",
                command_id, raw_len
            );

            // Store tiny reference in KV
            let promoted = CommandResponseData {
                response: CommandResponse::Success {
                    response: BodySpec::Storage {
                        size: Some(raw_len),
                        storage_get_request: Some(get_request),
                        storage_put_used: Some(true),
                    },
                },
            };
            let promoted_value = serde_json::to_vec(&promoted).into_alien_error().context(
                ErrorData::SerializationFailed {
                    message: "Failed to serialize promoted response reference".to_string(),
                    data_type: Some("CommandResponseData".to_string()),
                },
            )?;
            self.kv.put(&key, promoted_value, None).await.context(
                ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.clone(),
                    message: "Failed to store promoted response reference".to_string(),
                },
            )?;
            return Ok(());
        }

        // Error responses or storage references are always small, store as-is
        self.kv
            .put(&key, value, None)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.clone(),
                message: "Failed to store response".to_string(),
            })?;
        Ok(())
    }

    pub async fn get_response(&self, command_id: &str) -> Result<Option<CommandResponse>> {
        let key = format!("cmd:{}:response", command_id);
        if let Some(value) = self
            .kv
            .get(&key)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: key.clone(),
                message: "Failed to get response".to_string(),
            })?
        {
            let data: CommandResponseData = serde_json::from_slice(&value)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to deserialize response".to_string(),
                    data_type: Some("CommandResponseData".to_string()),
                })?;
            return Ok(Some(data.response));
        }
        Ok(None)
    }

    // --- Pending Index ---

    async fn create_pending_index(&self, deployment_id: &str, command_id: &str) -> Result<()> {
        let timestamp = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let key = format!(
            "target:{}:pending:{}:{}",
            deployment_id, timestamp, command_id
        );

        // Store empty value - just for ordering
        self.kv
            .put(&key, vec![], None)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.clone(),
                message: "Failed to create pending index".to_string(),
            })?;
        Ok(())
    }

    async fn delete_pending_index(&self, deployment_id: &str, command_id: &str) -> Result<()> {
        // We need to scan to find the exact key since we don't know the timestamp
        let prefix = format!("target:{}:pending:", deployment_id);
        let scan_result = self
            .kv
            .scan_prefix(&prefix, Some(100), None)
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "scan_prefix".to_string(),
                key: prefix.clone(),
                message: "Failed to scan pending index".to_string(),
            })?;

        for (key, _) in scan_result.items {
            if key.ends_with(&format!(":{}", command_id)) {
                let _ = self.kv.delete(&key).await;
                break;
            }
        }
        Ok(())
    }

    // --- Lease ---

    async fn delete_lease(&self, command_id: &str) -> Result<()> {
        let key = format!("cmd:{}:lease", command_id);
        let _ = self.kv.delete(&key).await;
        Ok(())
    }

    // --- Deadline Index ---

    async fn create_deadline_index(&self, command_id: &str, deadline: DateTime<Utc>) -> Result<()> {
        let key = format!(
            "deadline:{}:{}",
            deadline.timestamp_nanos_opt().unwrap_or(0),
            command_id
        );

        let data = DeadlineIndexData {
            command_id: command_id.to_string(),
            deadline,
        };
        let value = serde_json::to_vec(&data).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "Failed to serialize deadline index".to_string(),
                data_type: Some("DeadlineIndexData".to_string()),
            },
        )?;

        let ttl = deadline.signed_duration_since(Utc::now());
        let ttl_duration = if ttl.num_seconds() > 0 {
            Some(Duration::from_secs(ttl.num_seconds() as u64))
        } else {
            None
        };

        let options = ttl_duration.map(|ttl| PutOptions {
            ttl: Some(ttl),
            if_not_exists: false,
        });

        self.kv
            .put(&key, value, options)
            .await
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.clone(),
                message: "Failed to create deadline index".to_string(),
            })?;
        Ok(())
    }

    // --- Dispatch ---

    async fn dispatch_command_push(&self, command_id: &str, deployment_id: &str) -> Result<()> {
        // Get metadata from registry
        let metadata = self
            .command_registry
            .get_command_metadata(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        // Get params from KV
        let params = self.get_params(command_id).await?.ok_or_else(|| {
            AlienError::new(ErrorData::CommandNotFound {
                command_id: command_id.to_string(),
            })
        })?;

        // Build envelope
        let envelope = self.build_envelope(command_id, &metadata, params).await?;

        // Dispatch via transport
        self.command_dispatcher
            .dispatch(&envelope)
            .await
            .map_err(|e| {
                e.context(ErrorData::TransportDispatchFailed {
                    message: "Failed to dispatch command".to_string(),
                    transport_type: None,
                    target: Some(deployment_id.to_string()),
                })
            })?;

        // Update registry state
        self.command_registry
            .update_command_state(
                command_id,
                CommandState::Dispatched,
                Some(Utc::now()),
                None,
                None,
                None,
            )
            .await?;

        info!("Command {} dispatched via push", command_id);
        Ok(())
    }

    async fn build_envelope(
        &self,
        command_id: &str,
        metadata: &CommandEnvelopeData,
        mut params: BodySpec,
    ) -> Result<Envelope> {
        let response_handling = self.create_response_handling(command_id).await?;

        // Re-inline: if params are in blob but fit in transport limit, read and embed inline.
        // This avoids unnecessary storage downloads for medium-sized params (18KB–150KB).
        if let BodySpec::Storage { size, .. } = &params {
            let raw_size = size.unwrap_or(0) as usize;
            if raw_size > 0 && raw_size <= self.inline_max_bytes {
                let blob_path = StoragePath::from(format!("arc/commands/{}/params", command_id));
                match self.storage.get(&blob_path).await {
                    Ok(get_result) => match get_result.bytes().await {
                        Ok(raw_bytes) => {
                            params = BodySpec::inline(&raw_bytes);
                            debug!(
                                "Re-inlined params for command {} ({} bytes) into envelope",
                                command_id, raw_size
                            );
                        }
                        Err(e) => {
                            debug!(
                                    "Failed to read blob bytes for re-inline (command {}), falling back to presigned URL: {}",
                                    command_id, e
                                );
                        }
                    },
                    Err(e) => {
                        debug!(
                            "Failed to read blob for re-inline (command {}), falling back to presigned URL: {}",
                            command_id, e
                        );
                    }
                }
            }
        }

        // If params are still Storage (either too large or re-inline failed),
        // ensure they have a presigned GET request
        if let BodySpec::Storage {
            size,
            storage_get_request,
            storage_put_used,
        } = &params
        {
            if storage_get_request.is_none() {
                let get_request = self.generate_storage_get_request(command_id).await?;
                params = BodySpec::Storage {
                    size: *size,
                    storage_get_request: Some(get_request),
                    storage_put_used: *storage_put_used,
                };
            }
        }

        Ok(Envelope::new(
            metadata.deployment_id.clone(),
            command_id.to_string(),
            metadata.attempt,
            metadata.deadline,
            metadata.command.clone(),
            params,
            response_handling,
        ))
    }

    async fn create_response_handling(&self, command_id: &str) -> Result<ResponseHandling> {
        let upload_path = StoragePath::from(format!("arc/commands/{}/response", command_id));
        let expires_in = Duration::from_secs(3600);
        let presigned = self
            .storage
            .presigned_put(&upload_path, expires_in)
            .await
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create response upload URL".to_string(),
                operation: Some("presigned_put".to_string()),
                path: Some(upload_path.to_string()),
            })?;

        let (response_token, expires) = self.sign_response_url(command_id);

        Ok(ResponseHandling {
            max_inline_bytes: self.inline_max_bytes as u64,
            submit_response_url: format!(
                "{}/commands/{}/response?response_token={}&expires={}",
                self.base_url.trim_end_matches('/'),
                command_id,
                response_token,
                expires,
            ),
            storage_upload_request: presigned,
        })
    }

    async fn generate_params_upload(&self, command_id: &str) -> Result<StorageUpload> {
        let upload_path = StoragePath::from(format!("arc/commands/{}/params", command_id));
        let expires_in = Duration::from_secs(3600);
        let presigned = self
            .storage
            .presigned_put(&upload_path, expires_in)
            .await
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create presigned URL".to_string(),
                operation: Some("presigned_put".to_string()),
                path: Some(upload_path.to_string()),
            })?;

        Ok(StorageUpload {
            put_request: presigned.clone(),
            expires_at: presigned.expiration,
        })
    }

    async fn generate_storage_get_request(&self, command_id: &str) -> Result<PresignedRequest> {
        let path = StoragePath::from(format!("arc/commands/{}/params", command_id));
        let expires_in = Duration::from_secs(3600);
        self.storage.presigned_get(&path, expires_in).await.context(
            ErrorData::StorageOperationFailed {
                message: "Failed to create storage get request".to_string(),
                operation: Some("presigned_get".to_string()),
                path: Some(path.to_string()),
            },
        )
    }

    async fn generate_response_storage_get_request(
        &self,
        command_id: &str,
    ) -> Result<PresignedRequest> {
        let path = StoragePath::from(format!("arc/commands/{}/response", command_id));
        let expires_in = Duration::from_secs(3600);
        self.storage.presigned_get(&path, expires_in).await.context(
            ErrorData::StorageOperationFailed {
                message: "Failed to create response storage get request".to_string(),
                operation: Some("presigned_get".to_string()),
                path: Some(path.to_string()),
            },
        )
    }

    fn extract_command_id_from_index_key(&self, index_key: &str) -> Result<String> {
        index_key
            .split(':')
            .last()
            .ok_or_else(|| {
                AlienError::new(ErrorData::Other {
                    message: format!("Invalid index key format: {}", index_key),
                })
            })
            .map(|s| s.to_string())
    }
}
