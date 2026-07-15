//! Command Server Implementation
//!
//! The Command server implements the command lifecycle:
//! - CommandRegistry is the SOURCE OF TRUTH for all metadata (state, timestamps, etc.)
//! - KV stores ONLY operational data (params/response blobs, indices, leases)

use std::sync::Arc;
use std::time::Duration;

use alien_bindings::presigned::PresignedRequest;
use alien_bindings::traits::{Kv, PutOptions, Storage};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use chrono::{DateTime, Utc};
use hex;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{debug, info, warn};
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

fn is_definite_dispatch_rejection(error: &crate::error::Error) -> bool {
    matches!(
        error.error.as_ref(),
        Some(ErrorData::TransportDispatchRejected { .. })
    )
}

pub mod axum_handlers;
pub mod command_registry;
pub mod storage;

pub use crate::dispatchers::{CommandDispatcher, NullCommandDispatcher};
pub use axum_handlers::{
    create_axum_router, CommandPayloadResponse, HasCommandServer, StorePayloadRequest,
};
pub use command_registry::{
    delivery_mode_for, select_command_target, validate_command_name, validate_command_target_id,
    CommandEnvelopeData, CommandMetadata, CommandRegistry, CommandStatus, InMemoryCommandRegistry,
    ResolvedCommandTarget,
};

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
    const MAX_RESPONSE_TOKEN_LIFETIME_SECS: i64 = 7200;
    /// Worker execution can run for 1 hour and the operator lease adds 60
    /// seconds of response-submission headroom. Response credentials therefore
    /// need to outlive both; 2 hours stays within the verifier cap.
    const RESPONSE_CREDENTIAL_LIFETIME_SECS: u64 = 7200;

    /// Sign a response URL for a specific command.
    ///
    /// Returns `(hmac_hex, expires_epoch)`. The HMAC is computed over
    /// `"arc.v1:{command_id}:{expires}"` using the server's signing key.
    fn sign_response_url(&self, command_id: &str) -> (String, i64) {
        let expires = Utc::now().timestamp() + Self::RESPONSE_CREDENTIAL_LIFETIME_SECS as i64;
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
    pub fn verify_response_token(&self, command_id: &str, token: &str, expires: i64) -> bool {
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

        // Resolve which command-capable resource this command targets
        // (explicit targetResourceId, or single-target shorthand).
        let resolved_target = self
            .command_registry
            .resolve_target(
                &request.deployment_id,
                request.target_resource_id.as_deref(),
            )
            .await?;

        // Compose the target-scoped idempotency key once, from the resolved
        // target, and reuse it for both the pre-create check and the
        // post-create mapping. Both must derive from the same target, so a
        // single composition is the source of truth (idempotency is scoped per
        // target: the same key addressed to two different targets is two
        // commands).
        let composed_idempotency_key = request.idempotency_key.as_ref().map(|idem_key| {
            Self::compose_idempotency_key(
                &request.deployment_id,
                &resolved_target.target.resource_id,
                &request.command,
                idem_key,
            )
        });

        // Check idempotency if key provided.
        if let Some(ref composed_key) = composed_idempotency_key {
            if let Some(existing_id) = self.check_idempotency(composed_key).await? {
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
                &resolved_target,
                initial_state,
                request.deadline,
                request_size_bytes,
            )
            .await?;

        let command_id = metadata.command_id;
        let delivery_mode = metadata.delivery_mode;

        // 2. Store idempotency mapping in KV, reusing the key composed above
        // from the same resolved target. Losing the conditional put means a
        // concurrent create with the same key raced past the pre-create check
        // together with this one; dedupe by failing the command created above
        // and answering with the winner's, so the caller never observes two
        // live commands for one idempotency key.
        if let Some(ref composed_key) = composed_idempotency_key {
            if let Some(winner_id) = self.store_idempotency(composed_key, &command_id).await? {
                let _ = self
                    .command_registry
                    .complete_command(
                        &command_id,
                        CommandState::Failed,
                        Utc::now(),
                        None,
                        Some(serde_json::json!({
                            "code": "IDEMPOTENT_DUPLICATE",
                            "message": format!(
                                "Superseded by concurrent create '{}' with the same idempotency key",
                                winner_id
                            ),
                        })),
                    )
                    .await?;
                let status = self.command_registry.get_command_status(&winner_id).await?;
                let state = status.map(|s| s.state).unwrap_or(CommandState::Pending);
                return Ok(CreateCommandResponse {
                    command_id: winner_id,
                    state,
                    storage_upload: None,
                    inline_allowed_up_to: self.inline_max_bytes as u64,
                    next: "poll".to_string(),
                });
            }
        }

        // 3. Store params in KV
        self.store_params(&command_id, &request.params).await?;

        // 4. Generate storage upload URL if needed
        let storage_upload = if initial_state == CommandState::PendingUpload {
            Some(self.generate_params_upload(&command_id).await?)
        } else {
            None
        };

        // 5. Handle dispatch based on state and the target's delivery mode
        let (final_state, next_action) = if initial_state == CommandState::Pending {
            match delivery_mode {
                CommandDeliveryMode::Push => {
                    // Push delivery: dispatch immediately
                    let state = self
                        .dispatch_command_push(&command_id, &request.deployment_id)
                        .await?;
                    (state, "poll")
                }
                CommandDeliveryMode::Pull => {
                    // Pull delivery: create a pending index for a receiver or
                    // environment-local operator relay.
                    self.create_pending_index(
                        &request.deployment_id,
                        &metadata.target.resource_id,
                        &command_id,
                    )
                    .await?;
                    debug!("Command {} ready for target-scoped lease", command_id);
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

        // 5. Get delivery mode from registry and handle dispatch
        let metadata = self
            .command_registry
            .get_command_metadata(command_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::CommandNotFound {
                    command_id: command_id.to_string(),
                })
            })?;

        let final_state = match metadata.delivery_mode {
            CommandDeliveryMode::Push => {
                self.dispatch_command_push(command_id, &status.deployment_id)
                    .await?
            }
            CommandDeliveryMode::Pull => {
                self.create_pending_index(
                    &status.deployment_id,
                    &metadata.target.resource_id,
                    command_id,
                )
                .await?;
                debug!(
                    "Command {} ready for pull after upload (target will poll)",
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
                // Expire the command — CONDITIONALLY: a submit can land
                // between the status read above and this write, and an
                // unconditional write would stomp its terminal state (the
                // torn-record class the conditional transition exists for).
                let won = self
                    .command_registry
                    .complete_command(command_id, CommandState::Expired, Utc::now(), None, None)
                    .await?;
                if won {
                    // Clean up pending index
                    self.delete_pending_index(
                        &status.deployment_id,
                        &status.target.resource_id,
                        command_id,
                    )
                    .await?;

                    // Return expired status directly (avoid recursion)
                    return Ok(CommandStatusResponse {
                        command_id: command_id.to_string(),
                        state: CommandState::Expired,
                        attempt: status.attempt,
                        target: status.target,
                        response: None,
                    });
                }
                // Lost to a concurrent submit: fall through and serve the
                // freshly-terminal state below.
                let status = self
                    .command_registry
                    .get_command_status(command_id)
                    .await?
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CommandNotFound {
                            command_id: command_id.to_string(),
                        })
                    })?;
                let response = if status.state.is_terminal() {
                    self.get_response(command_id).await?
                } else {
                    None
                };
                return Ok(CommandStatusResponse {
                    command_id: command_id.to_string(),
                    state: status.state,
                    attempt: status.attempt,
                    target: status.target,
                    response,
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
            target: status.target,
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

        // 6. Update registry state (SOURCE OF TRUTH) BEFORE cleaning up the lease
        // and pending index. Committing the terminal state first guarantees the
        // command can never be stranded as Dispatched-with-no-lease-no-index: if
        // the process dies (or a cleanup step errors) after this point, get/poll
        // sees the terminal state and returns the already-stored response, and any
        // orphaned lease/pending-index entry is reaped by `acquire_lease`'s
        // terminal-state check. The old order (cleanup first, state last) left a
        // crash window in which the response was stored but permanently invisible.
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

        // Conditional terminal transition: exactly one of two racing
        // submitters (a redelivered execution racing the original whose lease
        // expired) wins; a terminal record is never overwritten. The blob was
        // pre-stored above for crash-safety (state never flips terminal with
        // no blob on disk); the winner re-stores its own response below so
        // the recorded state and the served blob agree. Residual: a loser
        // whose pre-store lands after the winner's re-store can still leave
        // its blob — that requires the loser's KV write to outlast the
        // winner's entire transition+re-store, a pathological schedule.
        let won = self
            .command_registry
            .complete_command(command_id, new_state, Utc::now(), response_size, error)
            .await?;
        if !won {
            debug!(
                "Ignoring duplicate response for command {} (lost the terminal transition race)",
                command_id
            );
            return Ok(());
        }
        self.store_response(command_id, &response).await?;

        // 7. Clean up lease from KV (best-effort; the terminal state is already
        // committed above, so a failure here cannot strand the command). Log a
        // warning and continue instead of failing the call — a leftover entry
        // is reaped by `acquire_lease`'s terminal-state check on its next scan.
        if let Err(e) = self.delete_lease(command_id).await {
            warn!(
                command_id,
                error = %e,
                "Failed to clean up lease after terminal response; will be reaped on next lease scan"
            );
        }

        // 8. Clean up pending index from KV (best-effort; terminal state).
        // Same reasoning as the lease cleanup above.
        if let Err(e) = self
            .delete_pending_index(
                &status.deployment_id,
                &status.target.resource_id,
                command_id,
            )
            .await
        {
            warn!(
                command_id,
                error = %e,
                "Failed to clean up pending index after terminal response; will be reaped on next lease scan"
            );
        }

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

        // 1. Scan KV pending index — ONLY the requesting target's prefix.
        // Commands for other targets in the same deployment are invisible here.
        let target_prefix = format!(
            "target:{}:{}:pending:",
            deployment_id, lease_request.target.resource_id
        );
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

            // Reverse index for O(1) release-by-lease-id lookups. Shares the
            // lease's TTL so it self-cleans on expiry; a stale entry is
            // harmless because every reader re-verifies the lease_id against
            // the live `cmd:{id}:lease` record.
            let reverse_key = format!("lease:{}", lease_id);
            self.kv
                .put(
                    &reverse_key,
                    command_id.as_bytes().to_vec(),
                    Some(PutOptions {
                        ttl: Some(lease_duration),
                        if_not_exists: false,
                    }),
                )
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: reverse_key,
                    message: "Failed to create lease reverse index".to_string(),
                })?;

            // 3. Get metadata from registry
            let mut metadata = match self
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

            // 3.1 Expiry-driven redelivery: this command was already
            // dispatched, yet the lease slot was free (the conditional put
            // above succeeded) — the previous lease TTL-expired with no
            // response and no explicit release. Increment the attempt so the
            // redelivered envelope carries `attempt > 1`, the at-least-once
            // redelivery signal both receiver twins document. The explicit
            // release path (`release_lease`) increments the same counter.
            if metadata.state == CommandState::Dispatched {
                self.command_registry.increment_attempt(&command_id).await?;
                metadata.attempt += 1;
            }

            // 3.5 Defense-in-depth: the pending index key said this command
            // belongs to the requesting target — verify the registry agrees.
            // A mismatch means the index is corrupt; fail loudly rather than
            // delivering a command to the wrong resource. The corrupt index
            // key is deliberately retained (only the lease is cleaned up):
            // genuine corruption should never occur, and the key is the
            // evidence an operator needs — do not "fix" this by deleting it.
            if metadata.target != lease_request.target {
                self.delete_lease(&command_id).await?;
                return Err(AlienError::new(ErrorData::Other {
                    message: format!(
                        "Pending index corruption: command '{}' is indexed under target '{}' \
                         but the registry says it belongs to target '{}' — refusing to deliver",
                        command_id, lease_request.target.resource_id, metadata.target.resource_id,
                    ),
                }));
            }

            // 4. Check if command is in terminal state (stale index)
            if metadata.state.is_terminal() {
                // Clean up stale data
                self.delete_lease(&command_id).await?;
                let _ = self.kv.delete(&index_key).await;
                continue;
            }

            // 5. Check deadline expiry — conditionally: a racing submit's
            // terminal state must never be stomped back to Expired.
            if let Some(deadline) = metadata.deadline {
                if Utc::now() > deadline {
                    let _ = self
                        .command_registry
                        .complete_command(
                            &command_id,
                            CommandState::Expired,
                            Utc::now(),
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

            // 7. Mark Dispatched — conditionally. Between the terminal
            // check in step 3.1 and here, the ORIGINAL holder of a
            // TTL-expired lease can still submit; handing out this lease
            // would re-execute a completed command and stomp its state.
            let dispatched = self
                .command_registry
                .mark_dispatched_if_not_terminal(&command_id, Utc::now())
                .await?;
            if !dispatched {
                debug!(
                    command_id = %command_id,
                    "Command turned terminal while leasing; releasing the lease"
                );
                self.delete_lease(&command_id).await?;
                let _ = self.kv.delete(&index_key).await;
                continue;
            }

            // 8. Build envelope. Lease-served envelopes carry manager URLs
            // as root-relative paths: a pull consumer resolves them against
            // its own configured commands endpoint — the one address the
            // platform corrected for that consumer's network (a container's
            // `host.docker.internal`, a BYOC daemon's tunnel URL) — because
            // the manager cannot know an address that is reachable from
            // behind every consumer's boundary. Push envelopes keep absolute
            // URLs: push transports have no configured base, and reaching
            // the manager's public address is inherent to push delivery.
            let mut envelope = self.build_envelope(&command_id, &metadata, params).await?;
            Self::relativize_manager_urls(&mut envelope, &self.base_url);

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

    /// Rewrite manager-origin URLs in a lease-served envelope to
    /// root-relative paths (see the call site in [`Self::acquire_lease`]).
    /// Only URLs on the server's own origin are rewritten — cloud-presigned
    /// storage URLs live on other origins and pass through absolute.
    fn relativize_manager_urls(envelope: &mut Envelope, base_url: &str) {
        let Ok(base) = reqwest::Url::parse(base_url) else {
            // An unparseable base cannot produce a strippable prefix; leave
            // the envelope absolute (the pre-relative behavior).
            return;
        };
        let origin = base.origin().ascii_serialization();
        let strip = |target: &mut String| {
            if let Some(rest) = target.strip_prefix(&origin) {
                if rest.starts_with('/') {
                    *target = rest.to_string();
                }
            }
        };

        strip(&mut envelope.response_handling.submit_response_url);
        if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
            &mut envelope.response_handling.storage_upload_request.backend
        {
            strip(url);
        }
        if let BodySpec::Storage {
            storage_get_request: Some(request),
            ..
        } = &mut envelope.params
        {
            if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
                &mut request.backend
            {
                strip(url);
            }
        }
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

            // 2. Delete lease from KV (and its reverse index)
            self.delete_lease(command_id).await?;
            let _ = self.kv.delete(&format!("lease:{}", lease_id)).await;

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

    /// Resolve a lease_id to `(command_id, owner_deployment_id)` via the
    /// `lease:{lease_id}` reverse index, re-verifying against the live lease
    /// record (the reverse entry can outlive a released lease briefly, and a
    /// command can have been re-leased under a new lease_id since).
    ///
    /// Used by the manager's auth layer to check that the caller may act on
    /// the deployment that holds the lease.
    pub async fn get_lease_owner(&self, lease_id: &str) -> Result<Option<(String, String)>> {
        let reverse_key = format!("lease:{}", lease_id);
        let Some(command_id_bytes) =
            self.kv
                .get(&reverse_key)
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "get".to_string(),
                    key: reverse_key.clone(),
                    message: "Failed to look up lease reverse index".to_string(),
                })?
        else {
            return Ok(None);
        };
        let command_id = String::from_utf8(command_id_bytes).map_err(|_| {
            AlienError::new(ErrorData::Other {
                message: format!("Lease reverse index '{}' is not valid UTF-8", reverse_key),
            })
        })?;

        let lease_key = format!("cmd:{}:lease", command_id);
        let Some(lease_data) =
            self.kv
                .get(&lease_key)
                .await
                .context(ErrorData::KvOperationFailed {
                    operation: "get".to_string(),
                    key: lease_key,
                    message: "Failed to look up lease".to_string(),
                })?
        else {
            return Ok(None);
        };
        let lease: LeaseData = serde_json::from_slice(&lease_data)
            .into_alien_error()
            .context(ErrorData::SerializationFailed {
                message: "Failed to deserialize lease data".to_string(),
                data_type: Some("LeaseData".to_string()),
            })?;
        if lease.lease_id != lease_id {
            return Ok(None);
        }

        Ok(Some((command_id, lease.owner)))
    }

    /// Release a lease by lease_id only (for the API).
    pub async fn release_lease_by_id(&self, lease_id: &str) -> Result<()> {
        match self.get_lease_owner(lease_id).await? {
            Some((command_id, _owner)) => self.release_lease(&command_id, lease_id).await,
            None => Err(AlienError::new(ErrorData::LeaseNotFound {
                lease_id: lease_id.to_string(),
            })),
        }
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

        // The command name occupies one segment of the `:`-delimited
        // idempotency key (`{dep}:{rid}:{command}:{key}`). A ':' in the name
        // would let it bleed into the client-key segment — which routinely
        // contains ':' — so (command="a:b", key="c") and (command="a",
        // key="b:c") would forge the same key and be treated as the same
        // command. Reject ':' here, mirroring the target-id colon guard, so the
        // command segment is always unambiguous.
        validate_command_name(&request.command)?;

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

    /// Compose the target-scoped idempotency key:
    /// `{deploymentId}:{targetResourceId}:{commandName}:{key}`.
    ///
    /// Scoping by target means the same client key addressed to two different
    /// targets creates two distinct commands.
    fn compose_idempotency_key(
        deployment_id: &str,
        target_resource_id: &str,
        command_name: &str,
        idem_key: &str,
    ) -> String {
        // Invariant: at every real call site the target id and command name are
        // both `:`-free (the former enforced at resolution via
        // `validate_command_target_id`, the latter via `validate_command_name`
        // in `validate_create_command`), so each occupies exactly one segment of
        // `{dep}:{rid}:{command}:{key}` and only the trailing client key may
        // carry ':'. Those guards live upstream; this is a pure formatter, so it
        // is not asserted on `command_name` here (the collision tests below
        // deliberately format a ':'-bearing command to demonstrate what the
        // upstream guard prevents).
        debug_assert!(
            !target_resource_id.contains(':'),
            "target_resource_id must be ':'-free before key composition: {target_resource_id}"
        );
        format!(
            "{}:{}:{}:{}",
            deployment_id, target_resource_id, command_name, idem_key
        )
    }

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

    /// Claim the idempotency key for `command_id`.
    ///
    /// Returns `None` when this command won the key. Returns
    /// `Some(winner_id)` when a concurrent create with the same key won the
    /// conditional put first — both requests passed the pre-create
    /// `check_idempotency` before either stored, so the loser must be
    /// detected here, after its command was already created.
    async fn store_idempotency(&self, idem_key: &str, command_id: &str) -> Result<Option<String>> {
        let key = format!("idem:{}", idem_key);
        let ttl = Duration::from_secs(24 * 60 * 60); // 24 hours
        let won = self
            .kv
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
        if won {
            return Ok(None);
        }
        // Lost the conditional put: read back who won. The winner entry can
        // only be absent if it TTL-expired in the microseconds since — treat
        // that as an inconsistency rather than silently duplicating.
        match self.check_idempotency(idem_key).await? {
            Some(winner_id) => Ok(Some(winner_id)),
            None => Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Idempotency key '{}' was concurrently claimed but has no winner entry",
                    key
                ),
            })),
        }
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

    async fn create_pending_index(
        &self,
        deployment_id: &str,
        target_resource_id: &str,
        command_id: &str,
    ) -> Result<()> {
        // Invariant: the target id is `:`-free (enforced at resolution via
        // `validate_command_target_id`), so its prefix `target:{dep}:{rid}:`
        // cannot overlap another target's pending keys.
        debug_assert!(
            !target_resource_id.contains(':'),
            "target_resource_id must be ':'-free in the pending index: {target_resource_id}"
        );
        let timestamp = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let key = format!(
            "target:{}:{}:pending:{}:{}",
            deployment_id, target_resource_id, timestamp, command_id
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

    async fn delete_pending_index(
        &self,
        deployment_id: &str,
        target_resource_id: &str,
        command_id: &str,
    ) -> Result<()> {
        // We need to scan to find the exact key since we don't know the timestamp
        let prefix = format!("target:{}:{}:pending:", deployment_id, target_resource_id);
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

    /// Expire every overdue non-terminal command recorded in the deadline
    /// index. Intended to run periodically from the hosting process.
    ///
    /// Deadlines are otherwise only enforced lazily (status polls and lease
    /// scans), which never reaches a command nobody polls — most notably a
    /// `PendingUpload` whose params upload never completed, which has no
    /// pending-index entry and would otherwise live forever.
    ///
    /// Uses the conditional terminal transition, so racing a concurrent
    /// submit is safe: whoever wins, the record stays consistent. Returns
    /// the number of commands expired.
    pub async fn reap_expired_commands(&self) -> Result<u32> {
        let now = Utc::now();
        let mut expired = 0u32;
        let mut cursor: Option<String> = None;
        // Paginate the whole index (bounded page count as a runaway guard):
        // keys are not numerically ordered — the timestamp segment isn't
        // zero-padded — so due entries can sit behind any number of
        // future-dated ones and a single capped scan would starve them.
        for _ in 0..64 {
            let scan = self
                .kv
                .scan_prefix("deadline:", Some(256), cursor.clone())
                .await
                .into_alien_error()
                .context(ErrorData::KvOperationFailed {
                    operation: "scan_prefix".to_string(),
                    key: "deadline:".to_string(),
                    message: "Failed to scan the deadline index".to_string(),
                })?;
            let next_cursor = scan.next_cursor.clone();
            for (key, value) in scan.items {
                let Ok(data) = serde_json::from_slice::<DeadlineIndexData>(&value) else {
                    warn!(key = %key, "Unparseable deadline index entry; deleting");
                    let _ = self.kv.delete(&key).await;
                    continue;
                };
                if data.deadline > now {
                    // Not due yet. (Keys are not sortable numerically — the
                    // timestamp segment isn't zero-padded — so keep scanning.)
                    continue;
                }

                let status = self
                    .command_registry
                    .get_command_status(&data.command_id)
                    .await?;
                match status {
                    None => {
                        let _ = self.kv.delete(&key).await;
                    }
                    Some(status) if status.state.is_terminal() => {
                        let _ = self.kv.delete(&key).await;
                    }
                    Some(status) => {
                        let won = self
                        .command_registry
                        .complete_command(
                            &data.command_id,
                            CommandState::Expired,
                            now,
                            None,
                            Some(serde_json::json!({
                                "code": "COMMAND_EXPIRED",
                                "message": format!("Deadline {} elapsed", data.deadline.to_rfc3339()),
                            })),
                        )
                        .await?;
                        if won {
                            expired += 1;
                            info!(command_id = %data.command_id, "Expired overdue command");
                            let _ = self.delete_lease(&data.command_id).await;
                            let _ = self
                                .delete_pending_index(
                                    &status.deployment_id,
                                    &status.target.resource_id,
                                    &data.command_id,
                                )
                                .await;
                        }
                        let _ = self.kv.delete(&key).await;
                    }
                }
            }
            match next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(expired)
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

        // The entry must remain VISIBLE well past the deadline: scan_prefix
        // treats logically-expired keys as absent on every provider, so a
        // TTL equal to the deadline would hide the entry at exactly the
        // moment the reaper needs it (the bug that made the reaper inert).
        // The reaper deletes entries as it processes them; the 7-day grace
        // is only self-cleaning for processes that never run a reaper.
        const DEADLINE_INDEX_GRACE: chrono::Duration = chrono::Duration::days(7);
        let ttl = deadline
            .signed_duration_since(Utc::now())
            .checked_add(&DEADLINE_INDEX_GRACE)
            .unwrap_or(DEADLINE_INDEX_GRACE);
        let options = (ttl.num_seconds() > 0).then(|| PutOptions {
            ttl: Some(Duration::from_secs(ttl.num_seconds() as u64)),
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

    async fn dispatch_command_push(
        &self,
        command_id: &str,
        deployment_id: &str,
    ) -> Result<CommandState> {
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

        // Mark Dispatched BEFORE invoking the transport: a fast worker can
        // execute and submit before this function resumes, and submit
        // validates state == Dispatched — the old dispatch-then-mark order
        // could reject that submit (losing the response) or stomp its
        // terminal state back to Dispatched.
        if !self
            .command_registry
            .mark_dispatched_if_not_terminal(command_id, Utc::now())
            .await?
        {
            return Ok(self
                .command_registry
                .get_command_status(command_id)
                .await?
                .map(|status| status.state)
                .unwrap_or(CommandState::Dispatched));
        }

        // A definite pre-delivery rejection (connection refusal, request
        // builder failure, or any HTTP status other than the runtime's exact
        // 202 acceptance) is safe to record as terminal DELIVERY_FAILED.
        // Other transport errors are ambiguous: the target may have accepted
        // the envelope before the response was lost. Keep those Dispatched so
        // a late response remains valid and return the durable ID for polling;
        // reverting/retrying could execute the command twice.
        if let Err(error) = self.command_dispatcher.dispatch(&envelope).await {
            if is_definite_dispatch_rejection(&error) {
                let delivery_failure = CommandResponse::error(
                    "DELIVERY_FAILED",
                    "Worker runtime did not accept command delivery",
                );
                self.submit_command_response(command_id, delivery_failure)
                    .await?;
                warn!(
                    command_id,
                    deployment_id,
                    error = %error,
                    "Push dispatch was definitely rejected; command marked Failed"
                );
                return Ok(CommandState::Failed);
            }

            let error = error.context(ErrorData::TransportDispatchFailed {
                message: "Failed to dispatch command".to_string(),
                transport_type: None,
                target: Some(deployment_id.to_string()),
            });
            warn!(
                command_id,
                deployment_id,
                error = %error,
                "Push dispatch acknowledgement failed; command remains Dispatched for a possible late response"
            );
            return Ok(CommandState::Dispatched);
        }

        info!("Command {} dispatched via push", envelope.command_id);
        Ok(CommandState::Dispatched)
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
        // always mint a fresh GET request. A command may remain Pending longer
        // than the URL minted at upload completion, and leasing must never hand
        // a Worker an already-expired storage credential.
        if let BodySpec::Storage {
            size,
            storage_get_request: _,
            storage_put_used,
        } = &params
        {
            let get_request = self.generate_storage_get_request(command_id).await?;
            params = BodySpec::Storage {
                size: *size,
                storage_get_request: Some(get_request),
                storage_put_used: *storage_put_used,
            };
        }

        Ok(Envelope::new(
            metadata.deployment_id.clone(),
            metadata.target.clone(),
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
        let expires_in = Duration::from_secs(Self::RESPONSE_CREDENTIAL_LIFETIME_SECS);
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

#[cfg(test)]
mod idempotency_key_tests {
    use super::*;
    use crate::server::{validate_command_name, validate_command_target_id};

    #[test]
    fn definite_dispatch_rejection_is_classified_by_typed_error_data() {
        let rejected = AlienError::new(ErrorData::TransportDispatchRejected {
            message: "not accepted".to_string(),
            transport_type: Some("http".to_string()),
            target: Some("command-id".to_string()),
        });
        assert!(is_definite_dispatch_rejection(&rejected));

        let ambiguous = AlienError::new(ErrorData::TransportDispatchFailed {
            message: "acknowledgement lost".to_string(),
            transport_type: Some("http".to_string()),
            target: Some("command-id".to_string()),
        });
        assert!(!is_definite_dispatch_rejection(&ambiguous));
    }

    /// Idempotency keys are `{dep}:{rid}:{command}:{key}`. If a target id could
    /// contain ':', the `rid` segment would be ambiguous: the two triples
    ///   (rid="svc",   command="a:b", key="k")
    ///   (rid="svc:a", command="b",   key="k")
    /// both compose to `dep:svc:a:b:k`. The shared guard forbids ':' in a target
    /// id, so the second can never be a resolved target — closing the collision
    /// at the rid boundary. Only the colon-free composition is exercised here;
    /// the colliding one is proven unreachable via the guard.
    #[test]
    fn target_id_colon_guard_prevents_idempotency_key_collision() {
        let colliding = CommandServer::compose_idempotency_key("dep", "svc", "a:b", "k");
        assert_eq!(colliding, "dep:svc:a:b:k");

        // The alternate triple that would collide needs target id "svc:a".
        assert!(
            validate_command_target_id("svc:a").is_err(),
            "a ':'-bearing target id must be rejected so it cannot forge the rid segment"
        );
        assert!(validate_command_target_id("svc").is_ok());
    }

    /// The command name is the other forgeable segment. Client keys routinely
    /// contain ':', so without a command-name guard these two distinct inputs
    ///   (command="a:b", key="c")
    ///   (command="a",   key="b:c")
    /// both compose to `dep:svc:a:b:c` — a cross-command idempotency collision.
    /// The guard rejects the ':'-bearing command name, so only the second input
    /// is ever composed; the first can never reach key composition.
    #[test]
    fn command_name_colon_guard_prevents_idempotency_key_collision() {
        // Without the guard, both inputs compose to the identical string.
        let forged = CommandServer::compose_idempotency_key("dep", "svc", "a:b", "c");
        let legitimate = CommandServer::compose_idempotency_key("dep", "svc", "a", "b:c");
        assert_eq!(
            forged, legitimate,
            "these inputs are exactly the colliding pair the guard must separate"
        );
        assert_eq!(legitimate, "dep:svc:a:b:c");

        // The guard makes the colliding input unreachable: a ':'-bearing command
        // name is rejected, while the legitimate command name is accepted. With
        // the forger blocked, `a:b`+`c` can never compose the shared key — only
        // the distinct `a`+`b:c` command can.
        let err = validate_command_name("a:b").expect_err("':'-bearing command must be rejected");
        assert_eq!(err.code, "INVALID_COMMAND");
        assert!(validate_command_name("a").is_ok());
    }
}
