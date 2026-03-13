//! Commands Polling Module
//!
//! Polls for commands from a lease endpoint and delivers them via gRPC.
//! Independent of transport - can be enabled alongside any transport.

use std::{sync::Arc, time::Duration};

use alien_bindings::grpc::control_service::{
    alien_bindings::control::{task::Payload, ArcCommand, Task},
    ControlGrpcServer,
};
use alien_commands::{
    runtime::submit_response,
    types::{CommandResponse, LeaseInfo, LeaseRequest, LeaseResponse},
};
use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::{Client, Url};
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};

/// Commands polling configuration
pub struct CommandsPolling {
    client: Client,
    url: Url,
    interval: Duration,
    deployment_id: String,
    token: String,
    control_server: Arc<ControlGrpcServer>,
}

impl CommandsPolling {
    pub fn new(
        url: Url,
        interval: Duration,
        deployment_id: String,
        token: String,
        control_server: Arc<ControlGrpcServer>,
    ) -> Self {
        Self {
            client: Client::new(),
            url,
            interval,
            deployment_id,
            token,
            control_server,
        }
    }

    /// Create CommandsPolling from environment variables and secrets.
    ///
    /// Reads configuration from:
    /// - `env_vars`: ALIEN_COMMANDS_POLLING_URL, ALIEN_DEPLOYMENT_ID
    /// - `secrets`: ALIEN_COMMANDS_TOKEN
    ///
    /// Returns None if commands polling is not enabled.
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
            secrets_keys = ?secrets.keys().collect::<Vec<_>>(),
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

        // Token is loaded from vault (as a secret-type env var)
        let token = secrets.get("ALIEN_COMMANDS_TOKEN")
            .ok_or_else(|| AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "ALIEN_COMMANDS_TOKEN required when ALIEN_COMMANDS_POLLING_ENABLED=true. Available secrets: {:?}",
                    secrets.keys().collect::<Vec<_>>()
                ),
                field: Some("ALIEN_COMMANDS_TOKEN".to_string()),
            }))?;

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

        let request = LeaseRequest {
            deployment_id: self.deployment_id.clone(),
            max_leases: 10,
            lease_seconds: 60,
        };

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
                event_type: "ArcCommand".to_string(),
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
                    event_type: "ArcCommand".to_string(),
                    reason: e,
                }));
            }
        }

        info!(command_id = %command_id, "Command processed");
        Ok(())
    }
}
