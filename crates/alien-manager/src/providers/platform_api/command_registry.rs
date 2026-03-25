use alien_platform_api::{
    types::{
        CommandState as SdkCommandState, CreateCommandResponseDeploymentModel,
        UpdateCommandRequestState,
    },
    Client, SdkResultExt,
};
use alien_commands::{
    error::{ErrorData as ArcErrorData, Result},
    server::{CommandEnvelopeData, CommandMetadata, CommandRegistry, CommandStatus},
};
use alien_core::{CommandState, DeploymentModel};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::error::ErrorData as PlatformErrorData;

/// Platform API implementation of CommandRegistry.
pub struct PlatformCommandRegistry {
    client: Client,
}

impl PlatformCommandRegistry {
    pub fn new(api_base_url: &str, api_key: &str) -> super::error::Result<Self> {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

        let auth_value = format!("Bearer {}", api_key);
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .into_alien_error()
                .context(PlatformErrorData::ConfigurationError {
                    message: "Invalid API key format for authorization header".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-manager"));

        let reqwest_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(PlatformErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?;

        let client = Client::new_with_client(api_base_url, reqwest_client);

        Ok(Self { client })
    }
}

fn sdk_state_to_core(state: SdkCommandState) -> CommandState {
    match state {
        SdkCommandState::PendingUpload => CommandState::PendingUpload,
        SdkCommandState::Pending => CommandState::Pending,
        SdkCommandState::Dispatched => CommandState::Dispatched,
        SdkCommandState::Succeeded => CommandState::Succeeded,
        SdkCommandState::Failed => CommandState::Failed,
        SdkCommandState::Expired => CommandState::Expired,
    }
}

fn sdk_deployment_model_to_core(
    model: alien_platform_api::types::CommandDeploymentModel,
) -> DeploymentModel {
    match model {
        alien_platform_api::types::CommandDeploymentModel::Push => DeploymentModel::Push,
        alien_platform_api::types::CommandDeploymentModel::Pull => DeploymentModel::Pull,
    }
}

fn core_state_to_sdk(state: CommandState) -> UpdateCommandRequestState {
    match state {
        CommandState::PendingUpload => UpdateCommandRequestState::PendingUpload,
        CommandState::Pending => UpdateCommandRequestState::Pending,
        CommandState::Dispatched => UpdateCommandRequestState::Dispatched,
        CommandState::Succeeded => UpdateCommandRequestState::Succeeded,
        CommandState::Failed => UpdateCommandRequestState::Failed,
        CommandState::Expired => UpdateCommandRequestState::Expired,
    }
}

#[async_trait]
impl CommandRegistry for PlatformCommandRegistry {
    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> Result<CommandMetadata> {
        use alien_platform_api::types::{CreateCommandRequest, CreateCommandRequestInitialState};

        let initial_state_api = match initial_state {
            CommandState::PendingUpload => CreateCommandRequestInitialState::PendingUpload,
            CommandState::Pending => CreateCommandRequestInitialState::Pending,
            _ => {
                return Err(AlienError::new(ArcErrorData::InvalidCommand {
                    message: format!(
                        "Invalid initial state: {}. Only PENDING_UPLOAD or PENDING allowed.",
                        initial_state.as_ref()
                    ),
                }))
            }
        };

        let response = self
            .client
            .create_command()
            .body(CreateCommandRequest {
                deployment_id: deployment_id.parse().into_alien_error().context(
                    ArcErrorData::InvalidCommand {
                        message: format!("Invalid deployment_id format: {}", deployment_id),
                    },
                )?,
                name: command_name.parse().into_alien_error().context(
                    ArcErrorData::InvalidCommand {
                        message: format!("Invalid command name format: {}", command_name),
                    },
                )?,
                initial_state: Some(initial_state_api),
                deadline,
                request_size_bytes: request_size_bytes.map(|n| n as f64),
            })
            .send()
            .await
            .into_sdk_error()
            .context(ArcErrorData::HttpOperationFailed {
                message: "Failed to create command in Platform API".to_string(),
                method: Some("POST".to_string()),
                url: Some("/commands".to_string()),
            })?;

        let data = response.into_inner();

        let deployment_model = match data.deployment_model {
            CreateCommandResponseDeploymentModel::Push => DeploymentModel::Push,
            CreateCommandResponseDeploymentModel::Pull => DeploymentModel::Pull,
        };

        Ok(CommandMetadata {
            command_id: data.id.to_string(),
            deployment_model,
            project_id: data.project_id,
        })
    }

    async fn get_command_metadata(&self, command_id: &str) -> Result<Option<CommandEnvelopeData>> {
        let response = self
            .client
            .get_command()
            .id(command_id)
            .send()
            .await
            .into_sdk_error();

        let data = match response {
            Ok(r) => r.into_inner(),
            Err(e) => {
                if e.code.contains("NOT_FOUND") || e.http_status_code == Some(404) {
                    return Ok(None);
                }
                return Err(e.context(ArcErrorData::HttpOperationFailed {
                    message: format!("Failed to get command {} from Platform API", command_id),
                    method: Some("GET".to_string()),
                    url: Some(format!("/commands/{}", command_id)),
                }));
            }
        };

        let state = sdk_state_to_core(data.state);
        let deployment_model = sdk_deployment_model_to_core(data.deployment_model);

        Ok(Some(CommandEnvelopeData {
            command_id: data.id.to_string(),
            deployment_id: data.deployment_id.to_string(),
            command: data.name,
            attempt: data.attempt.map(|n| n as u32).unwrap_or(1),
            deadline: data.deadline,
            state,
            deployment_model,
        }))
    }

    async fn get_command_status(&self, command_id: &str) -> Result<Option<CommandStatus>> {
        let response = self
            .client
            .get_command()
            .id(command_id)
            .send()
            .await
            .into_sdk_error();

        let data = match response {
            Ok(r) => r.into_inner(),
            Err(e) => {
                if e.code.contains("NOT_FOUND") || e.http_status_code == Some(404) {
                    return Ok(None);
                }
                return Err(e.context(ArcErrorData::HttpOperationFailed {
                    message: format!("Failed to get command {} from Platform API", command_id),
                    method: Some("GET".to_string()),
                    url: Some(format!("/commands/{}", command_id)),
                }));
            }
        };

        let state = sdk_state_to_core(data.state);
        let error = data.error.map(serde_json::Value::Object);

        Ok(Some(CommandStatus {
            command_id: data.id.to_string(),
            deployment_id: data.deployment_id.to_string(),
            command: data.name,
            state,
            attempt: data.attempt.map(|n| n as u32).unwrap_or(1),
            deadline: data.deadline,
            created_at: data.created_at,
            dispatched_at: data.dispatched_at,
            completed_at: data.completed_at,
            error,
            request_size_bytes: data.request_size_bytes.map(|n| n as u64),
            response_size_bytes: data.response_size_bytes.map(|n| n as u64),
        }))
    }

    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<()> {
        use alien_platform_api::types::UpdateCommandRequest;

        let state_api = core_state_to_sdk(state);

        let error_map = error.and_then(|v| {
            if let serde_json::Value::Object(map) = v {
                Some(map)
            } else {
                None
            }
        });

        self.client
            .update_command()
            .id(command_id)
            .body(UpdateCommandRequest {
                state: Some(state_api),
                attempt: None,
                dispatched_at,
                completed_at,
                response_size_bytes: response_size_bytes.map(|n| n as f64),
                error: error_map,
            })
            .send()
            .await
            .into_sdk_error()
            .context(ArcErrorData::HttpOperationFailed {
                message: format!("Failed to update command {} in Platform API", command_id),
                method: Some("PATCH".to_string()),
                url: Some(format!("/commands/{}", command_id)),
            })?;

        Ok(())
    }

    async fn increment_attempt(&self, command_id: &str) -> Result<u32> {
        use alien_platform_api::types::UpdateCommandRequest;

        let current = self.get_command_status(command_id).await?.ok_or_else(|| {
            AlienError::new(ArcErrorData::CommandNotFound {
                command_id: command_id.to_string(),
            })
        })?;

        let new_attempt = current.attempt + 1;

        self.client
            .update_command()
            .id(command_id)
            .body(UpdateCommandRequest {
                state: None,
                attempt: Some(new_attempt as f64),
                dispatched_at: None,
                completed_at: None,
                response_size_bytes: None,
                error: None,
            })
            .send()
            .await
            .into_sdk_error()
            .context(ArcErrorData::HttpOperationFailed {
                message: format!(
                    "Failed to increment attempt for command {} in Platform API",
                    command_id
                ),
                method: Some("PATCH".to_string()),
                url: Some(format!("/commands/{}", command_id)),
            })?;

        Ok(new_attempt)
    }
}
