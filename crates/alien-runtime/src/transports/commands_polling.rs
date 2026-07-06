//! Commands Polling Module
//!
//! Polls for commands from a lease endpoint and delivers them via gRPC.
//! Independent of transport - can be enabled alongside any transport.

use std::{sync::Arc, time::Duration};

use alien_commands::{
    runtime::submit_response,
    types::{
        CommandResponse, CommandTarget, CommandTargetType, LeaseInfo, LeaseRequest, LeaseResponse,
    },
};
use alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_worker_protocol::{
    control::{task::Payload, ArcCommand, Task},
    ControlGrpcServer,
};
use reqwest::{Client, Url};
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};

/// Commands polling configuration
pub struct CommandsPolling {
    client: Client,
    url: Url,
    interval: Duration,
    deployment_id: String,
    /// The specific stack resource this runtime polls leases for (its own
    /// resource id). Leases are scoped to this target.
    target_resource_id: String,
    token: String,
    control_server: Arc<ControlGrpcServer>,
}

impl CommandsPolling {
    pub fn new(
        url: Url,
        interval: Duration,
        deployment_id: String,
        target_resource_id: String,
        token: String,
        control_server: Arc<ControlGrpcServer>,
    ) -> Self {
        Self {
            client: Client::new(),
            url,
            interval,
            deployment_id,
            target_resource_id,
            token,
            control_server,
        }
    }

    /// Create CommandsPolling from environment variables and secrets.
    ///
    /// Reads configuration from:
    /// - `env_vars`: ALIEN_COMMANDS_POLLING_URL, ALIEN_DEPLOYMENT_ID,
    ///   ALIEN_COMMANDS_TARGET_RESOURCE_ID
    /// - `secrets`: ALIEN_COMMANDS_TOKEN
    ///
    /// Returns None if commands polling is not enabled. Errors (fail fast) if
    /// polling is enabled but a required variable — including
    /// `ALIEN_COMMANDS_TARGET_RESOURCE_ID` — is absent.
    pub fn from_env(
        env_vars: &std::collections::HashMap<String, String>,
        secrets: &std::collections::HashMap<String, String>,
        control_server: Arc<ControlGrpcServer>,
    ) -> Result<Option<Self>> {
        if env_vars
            .get("ALIEN_COMMANDS_POLLING_ENABLED")
            .map(|s| s.as_str())
            != Some("true")
        {
            return Ok(None);
        }

        info!("Starting commands polling from environment variables");

        debug!(
            secrets_count = secrets.len(),
            "Loaded secrets available for commands polling"
        );

        // Required environment variables
        let url_str = env_vars.get("ALIEN_COMMANDS_POLLING_URL").ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message:
                    "ALIEN_COMMANDS_POLLING_URL required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                        .to_string(),
                field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
            })
        })?;

        let deployment_id = env_vars.get("ALIEN_DEPLOYMENT_ID").ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: "ALIEN_DEPLOYMENT_ID required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                    .to_string(),
                field: Some("ALIEN_DEPLOYMENT_ID".to_string()),
            })
        })?;

        // ALIEN-219: the target resource id names which stack resource this
        // runtime polls leases for. Required when polling is enabled — fail
        // fast, naming the variable, rather than silently leasing at the wrong
        // scope.
        let target_resource_id = env_vars
            .get(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationInvalid {
                    message: format!(
                        "{ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID} required when \
                         ALIEN_COMMANDS_POLLING_ENABLED=true"
                    ),
                    field: Some(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string()),
                })
            })?;

        // Token can come from secrets (managed mode) or plain env vars (dev/standalone mode)
        let token = secrets
            .get("ALIEN_COMMANDS_TOKEN")
            .or_else(|| env_vars.get("ALIEN_COMMANDS_TOKEN"))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationInvalid {
                    message:
                        "ALIEN_COMMANDS_TOKEN required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                            .to_string(),
                    field: Some("ALIEN_COMMANDS_TOKEN".to_string()),
                })
            })?;

        let url = Url::parse(url_str).map_err(|e| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("Invalid commands polling URL: {}", e),
                field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
            })
        })?;

        Ok(Some(Self::new(
            url,
            Duration::from_secs(5),
            deployment_id.clone(),
            target_resource_id.clone(),
            token.clone(),
            control_server,
        )))
    }

    /// Run the polling loop
    pub async fn run(&self) -> Result<()> {
        info!(
            url = %self.url,
            interval = ?self.interval,
            "Starting commands polling"
        );

        loop {
            match self.poll_once().await {
                Ok(count) => {
                    if count > 0 {
                        debug!(count = count, "Processed commands");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Commands polling error, will retry");
                }
            }

            tokio::time::sleep(self.interval).await;
        }
    }

    /// Poll once for commands
    async fn poll_once(&self) -> Result<usize> {
        let leases = self.acquire_leases().await?;
        let count = leases.len();

        for lease in leases {
            if let Err(e) = self.process_lease(lease).await {
                error!(error = %e, "Failed to process lease");
            }
        }

        Ok(count)
    }

    /// Build the lease request this runtime sends to the manager.
    ///
    /// Extracted as a pure function (no I/O) so the request shape — in
    /// particular that it names this runtime's own target resource, typed as
    /// a Worker (a K8s/Local runtime is always a Worker target — Container/
    /// Daemon poll their own runtimes) — is directly unit-testable without a
    /// mock HTTP server.
    fn build_lease_request(&self) -> LeaseRequest {
        LeaseRequest {
            deployment_id: self.deployment_id.clone(),
            target: CommandTarget::new(self.target_resource_id.clone(), CommandTargetType::Worker),
            max_leases: 10,
            lease_seconds: 60,
        }
    }

    /// Acquire leases from command server
    async fn acquire_leases(&self) -> Result<Vec<LeaseInfo>> {
        let mut lease_endpoint = self.url.clone();
        lease_endpoint
            .path_segments_mut()
            .map_err(|_| {
                AlienError::new(ErrorData::ConfigurationInvalid {
                    message: format!(
                        "Invalid commands polling URL '{}': must be an HTTP/HTTPS URL with a path",
                        self.url
                    ),
                    field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
                })
            })?
            .push("commands")
            .push("leases");

        // ALIEN-219: lease scoped to this runtime's own target resource. The
        // manager scans only this target's pending index (a K8s/Local runtime is
        // always a Worker target — Container/Daemon poll their own runtimes).
        let request = self.build_lease_request();

        let response = self
            .client
            .post(lease_endpoint)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::NetworkRequestFailed {
                url: self.url.to_string(),
                method: Some("POST".to_string()),
                message: "Failed to acquire leases".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::NetworkRequestFailed {
                url: self.url.to_string(),
                method: Some("POST".to_string()),
                message: format!("Lease request failed: {} - {}", status, body),
            }));
        }

        let lease_response: LeaseResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to parse lease response".to_string(),
                })?;

        Ok(lease_response.leases)
    }

    /// Process a single lease - deliver command via gRPC
    async fn process_lease(&self, lease: LeaseInfo) -> Result<()> {
        let command_id = lease.command_id.clone();
        let envelope = lease.envelope;

        info!(
            command_id = %command_id,
            command = %envelope.command,
            "Processing command"
        );

        // Decode params (alien_commands returns AlienError, use .context())
        let params = alien_commands::runtime::decode_params_bytes(&envelope)
            .await
            .context(ErrorData::EventProcessingFailed {
                event_type: "Command".to_string(),
                reason: "Failed to decode params".to_string(),
            })?;

        // Create task for gRPC delivery
        let task = Task {
            task_id: command_id.clone(),
            payload: Some(Payload::ArcCommand(ArcCommand {
                command_id: command_id.clone(),
                command_name: envelope.command.clone(),
                params,
                attempt: envelope.attempt,
                deadline: envelope.deadline.map(|d| prost_types::Timestamp {
                    seconds: d.timestamp(),
                    nanos: d.timestamp_subsec_nanos() as i32,
                }),
                response_url: envelope.response_handling.submit_response_url.clone(),
                storage_upload_url: envelope.response_handling.storage_upload_request.url(),
                max_inline_bytes: envelope.response_handling.max_inline_bytes,
            })),
        };

        // Send task and wait for result
        match self
            .control_server
            .send_task(task, std::time::Duration::from_secs(300))
            .await
        {
            Ok(result) => {
                // Submit response to commands server
                let command_response = if result.success {
                    if result.response_data.is_empty() {
                        CommandResponse::success(b"{}")
                    } else {
                        CommandResponse::success(&result.response_data)
                    }
                } else {
                    CommandResponse::error(
                        result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                        result
                            .error_message
                            .unwrap_or_else(|| "Unknown error".to_string()),
                    )
                };

                // submit_response returns AlienError from alien-arc, use .context()
                submit_response(&envelope, command_response).await.context(
                    ErrorData::NetworkRequestFailed {
                        url: envelope.response_handling.submit_response_url.clone(),
                        method: Some("POST".to_string()),
                        message: "Failed to submit response".to_string(),
                    },
                )?;
            }
            Err(e) => {
                // Submit error response
                let command_response = CommandResponse::error("HANDLER_ERROR", &e);
                let _ = submit_response(&envelope, command_response).await;
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: "Command".to_string(),
                    reason: e,
                }));
            }
        }

        info!(command_id = %command_id, "Command processed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn control_server() -> Arc<ControlGrpcServer> {
        Arc::new(ControlGrpcServer::new())
    }

    /// A fully-populated polling env *except* the target resource id.
    fn base_env() -> HashMap<String, String> {
        HashMap::from([
            (
                "ALIEN_COMMANDS_POLLING_ENABLED".to_string(),
                "true".to_string(),
            ),
            (
                "ALIEN_COMMANDS_POLLING_URL".to_string(),
                "https://commands.example.com".to_string(),
            ),
            ("ALIEN_DEPLOYMENT_ID".to_string(), "dep-123".to_string()),
            ("ALIEN_COMMANDS_TOKEN".to_string(), "tok".to_string()),
        ])
    }

    #[test]
    fn from_env_returns_none_when_disabled() {
        let env = HashMap::new();
        let out = CommandsPolling::from_env(&env, &HashMap::new(), control_server()).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn from_env_requires_target_resource_id() {
        // Everything present except ALIEN_COMMANDS_TARGET_RESOURCE_ID.
        let env = base_env();
        let err = match CommandsPolling::from_env(&env, &HashMap::new(), control_server()) {
            Err(e) => e,
            Ok(_) => panic!("missing target resource id must fail fast"),
        };
        assert_eq!(err.code, "CONFIGURATION_INVALID");
        assert!(
            err.message.contains(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID),
            "error must name the missing variable, got: {}",
            err.message
        );
    }

    #[test]
    fn from_env_populates_target_resource_id() {
        let mut env = base_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            "worker-7".to_string(),
        );
        let polling = CommandsPolling::from_env(&env, &HashMap::new(), control_server())
            .unwrap()
            .expect("polling should be enabled");
        assert_eq!(polling.target_resource_id, "worker-7");
        assert_eq!(polling.deployment_id, "dep-123");
    }

    #[test]
    fn lease_request_carries_worker_target() {
        // Exercise the real request-building path off a `CommandsPolling`
        // constructed via `from_env` (not a hand-built `LeaseRequest`
        // literal) — proves `acquire_leases` would actually send this shape.
        let mut env = base_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            "worker-7".to_string(),
        );
        let polling = CommandsPolling::from_env(&env, &HashMap::new(), control_server())
            .unwrap()
            .expect("polling should be enabled");

        let request = polling.build_lease_request();

        assert_eq!(request.deployment_id, "dep-123");
        assert_eq!(request.target.resource_id, "worker-7");
        assert_eq!(request.target.resource_type, CommandTargetType::Worker);
        assert_eq!(request.max_leases, 10);
        assert_eq!(request.lease_seconds, 60);
    }

    #[test]
    fn build_lease_request_reflects_constructor_target() {
        // Two runtimes built via `new()` with different target resource ids
        // never build a lease request for the other's target — the request
        // shape is derived purely from `self`, not from any shared state.
        let polling_a = CommandsPolling::new(
            Url::parse("https://commands.example.com").unwrap(),
            Duration::from_secs(5),
            "dep-123".to_string(),
            "worker-a".to_string(),
            "tok".to_string(),
            control_server(),
        );
        let polling_b = CommandsPolling::new(
            Url::parse("https://commands.example.com").unwrap(),
            Duration::from_secs(5),
            "dep-123".to_string(),
            "worker-b".to_string(),
            "tok".to_string(),
            control_server(),
        );

        assert_eq!(
            polling_a.build_lease_request().target.resource_id,
            "worker-a"
        );
        assert_eq!(
            polling_b.build_lease_request().target.resource_id,
            "worker-b"
        );
    }
}
