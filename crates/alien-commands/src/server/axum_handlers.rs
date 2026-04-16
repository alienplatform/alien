use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use tracing::error;

use alien_error::AlienError;

use crate::{
    error::{Error, ErrorData},
    server::CommandServer,
    types::*,
};

/// Trait to extract CommandServer from any state type
pub trait HasCommandServer {
    fn command_server(&self) -> &Arc<CommandServer>;
}

impl HasCommandServer for Arc<CommandServer> {
    fn command_server(&self) -> &Arc<CommandServer> {
        self
    }
}

/// Create an Axum router with all command endpoints using a generic state type
pub fn create_axum_router<S>() -> Router<S>
where
    S: HasCommandServer + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/commands", post(create_command::<S>))
        .route(
            "/commands/{command_id}/upload-complete",
            post(upload_complete::<S>),
        )
        .route("/commands/{command_id}/response", put(submit_response::<S>))
        .route("/commands/{command_id}", get(get_command_status::<S>))
        .route(
            "/commands/{command_id}/payload",
            get(get_command_payload::<S>).put(store_command_payload::<S>),
        )
        .route("/commands/leases", post(acquire_leases::<S>))
        .route(
            "/commands/leases/{lease_id}/release",
            post(release_lease::<S>),
        )
}

/// Create a new command
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/commands",
    request_body = CreateCommandRequest,
    responses(
        (status = 200, description = "Command created successfully", body = CreateCommandResponse),
        (status = 400, description = "Invalid command", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "create_command",
    tag = "commands"
))]
async fn create_command<S>(
    State(state): State<S>,
    Json(request): Json<CreateCommandRequest>,
) -> Result<Json<CreateCommandResponse>, ErrorResponse>
where
    S: HasCommandServer,
{
    let response = state.command_server().create_command(request).await?;
    Ok(Json(response))
}

/// Mark upload as complete
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/commands/{command_id}/upload-complete",
    params(
        ("command_id" = String, Path, description = "Command identifier")
    ),
    request_body = UploadCompleteRequest,
    responses(
        (status = 200, description = "Upload marked complete", body = UploadCompleteResponse),
        (status = 400, description = "Invalid command or state", body = ErrorResponse),
        (status = 404, description = "Command not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "upload_complete",
    tag = "commands"
))]
async fn upload_complete<S>(
    State(state): State<S>,
    Path(command_id): Path<String>,
    Json(upload_request): Json<UploadCompleteRequest>,
) -> Result<Json<UploadCompleteResponse>, ErrorResponse>
where
    S: HasCommandServer,
{
    let response = state
        .command_server()
        .upload_complete(&command_id, upload_request)
        .await?;
    Ok(Json(response))
}

/// Get command status
#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/commands/{command_id}",
    params(
        ("command_id" = String, Path, description = "Command identifier")
    ),
    responses(
        (status = 200, description = "Command status", body = CommandStatusResponse),
        (status = 404, description = "Command not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "get_command_status",
    tag = "commands"
))]
async fn get_command_status<S>(
    State(state): State<S>,
    Path(command_id): Path<String>,
) -> Result<Json<CommandStatusResponse>, ErrorResponse>
where
    S: HasCommandServer,
{
    let response = state
        .command_server()
        .get_command_status(&command_id)
        .await?;
    Ok(Json(response))
}

/// Payload response containing params and response data from KV
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CommandPayloadResponse {
    pub command_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BodySpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<CommandResponse>,
}

/// Get command payload (params and response) from KV
///
/// Returns the raw params and response data stored in the manager's KV store.
/// Returns 404 if neither params nor response exist for this command.
#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/commands/{command_id}/payload",
    params(
        ("command_id" = String, Path, description = "Command identifier")
    ),
    responses(
        (status = 200, description = "Command payload data", body = CommandPayloadResponse),
        (status = 404, description = "Command payload not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "get_command_payload",
    tag = "commands"
))]
async fn get_command_payload<S>(
    State(state): State<S>,
    Path(command_id): Path<String>,
) -> Result<Json<CommandPayloadResponse>, ErrorResponse>
where
    S: HasCommandServer,
{
    let params = state.command_server().get_params(&command_id).await?;
    let response = state.command_server().get_response(&command_id).await?;

    // If neither params nor response exist, the command payload doesn't exist in this AM
    if params.is_none() && response.is_none() {
        return Err(AlienError::new(ErrorData::CommandNotFound {
            command_id: command_id.clone(),
        })
        .into());
    }

    Ok(Json(CommandPayloadResponse {
        command_id,
        params,
        response,
    }))
}

/// Request to store payload data directly in KV by command_id.
///
/// This bypasses the normal command lifecycle (create → dispatch → respond)
/// and writes params/response directly into KV. Used by the demo service
/// to populate payload data for commands created outside the command flow.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorePayloadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BodySpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<CommandResponse>,
}

/// Store command payload data (params and/or response) directly into KV.
///
/// Bypasses the command registry — useful for populating demo data or
/// migrating payload data. Does not validate command existence or state.
#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/commands/{command_id}/payload",
    params(
        ("command_id" = String, Path, description = "Command identifier")
    ),
    request_body = StorePayloadRequest,
    responses(
        (status = 200, description = "Payload stored successfully"),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "store_command_payload",
    tag = "commands"
))]
async fn store_command_payload<S>(
    State(state): State<S>,
    Path(command_id): Path<String>,
    Json(request): Json<StorePayloadRequest>,
) -> Result<StatusCode, ErrorResponse>
where
    S: HasCommandServer,
{
    if let Some(params) = &request.params {
        state
            .command_server()
            .store_params(&command_id, params)
            .await?;
    }

    if let Some(response) = &request.response {
        state
            .command_server()
            .store_response(&command_id, response)
            .await?;
    }

    Ok(StatusCode::OK)
}

/// Submit response from deployment
#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/commands/{command_id}/response",
    params(
        ("command_id" = String, Path, description = "Command identifier")
    ),
    request_body = SubmitResponseRequest,
    responses(
        (status = 200, description = "Response submitted successfully"),
        (status = 400, description = "Invalid command or state", body = ErrorResponse),
        (status = 404, description = "Command not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "submit_response",
    tag = "commands"
))]
async fn submit_response<S>(
    State(state): State<S>,
    Path(command_id): Path<String>,
    Json(request): Json<SubmitResponseRequest>,
) -> Result<StatusCode, ErrorResponse>
where
    S: HasCommandServer,
{
    state
        .command_server()
        .submit_command_response(&command_id, request.response)
        .await?;
    Ok(StatusCode::OK)
}

/// Acquire leases for polling deployments
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/commands/leases",
    request_body = LeaseRequest,
    responses(
        (status = 200, description = "Leases acquired", body = LeaseResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "acquire_leases",
    tag = "leases"
))]
async fn acquire_leases<S>(
    State(state): State<S>,
    Json(lease_request): Json<LeaseRequest>,
) -> Result<Json<LeaseResponse>, ErrorResponse>
where
    S: HasCommandServer,
{
    let response = state
        .command_server()
        .acquire_lease(&lease_request.deployment_id, &lease_request)
        .await?;
    Ok(Json(response))
}

/// Release a lease
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/commands/leases/{lease_id}/release",
    params(
        ("lease_id" = String, Path, description = "Lease identifier")
    ),
    responses(
        (status = 200, description = "Lease released successfully"),
        (status = 404, description = "Lease not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    operation_id = "release_lease",
    tag = "leases"
))]
async fn release_lease<S>(
    State(state): State<S>,
    Path(lease_id): Path<String>,
) -> Result<StatusCode, ErrorResponse>
where
    S: HasCommandServer,
{
    state
        .command_server()
        .release_lease_by_id(&lease_id)
        .await?;
    Ok(StatusCode::OK)
}

// Error handling

/// Error response wrapper for API endpoints
#[derive(Debug, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
struct ErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl From<Error> for ErrorResponse {
    fn from(error: Error) -> Self {
        ErrorResponse {
            code: error.code.clone(),
            message: error.message.clone(),
            details: None,
        }
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "INVALID_COMMAND" | "INVALID_STATE_TRANSITION" | "INVALID_ENVELOPE" => {
                StatusCode::BAD_REQUEST
            }
            "COMMAND_NOT_FOUND" | "LEASE_NOT_FOUND" => StatusCode::NOT_FOUND,
            "COMMAND_EXPIRED" => StatusCode::GONE,
            "CONFLICT" => StatusCode::CONFLICT,
            "OPERATION_NOT_SUPPORTED" => StatusCode::NOT_IMPLEMENTED,
            "STORAGE_OPERATION_FAILED"
            | "KV_OPERATION_FAILED"
            | "TRANSPORT_DISPATCH_FAILED"
            | "AGENT_ERROR"
            | "COMMANDS_ERROR"
            | "SERIALIZATION_FAILED"
            | "HTTP_OPERATION_FAILED" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = match serde_json::to_string(&self) {
            Ok(json) => json,
            Err(e) => {
                error!("Failed to serialize error response: {}", e);
                r#"{"code":"COMMANDS_ERROR","message":"Serialization error"}"#.to_string()
            }
        };

        (status, body).into_response()
    }
}

#[cfg(feature = "openapi")]
mod openapi {
    use super::*;
    use utoipa::OpenApi;

    #[derive(OpenApi)]
    #[openapi(
        paths(
            create_command,
            upload_complete,
            get_command_status,
            get_command_payload,
            store_command_payload,
            submit_response,
            acquire_leases,
            release_lease,
        ),
        components(
            schemas(
                CreateCommandRequest,
                CreateCommandResponse,
                UploadCompleteRequest,
                UploadCompleteResponse,
                CommandStatusResponse,
                CommandPayloadResponse,
                StorePayloadRequest,
                SubmitResponseRequest,
                CommandResponse,
                LeaseRequest,
                LeaseResponse,
                ReleaseRequest,
                ErrorResponse,
                // Re-export common types
                BodySpec,
                CommandState,
                StorageUpload,
                ResponseHandling,
                Envelope,
                LeaseInfo,
            )
        ),
        tags(
            (name = "commands", description = "Command management"),
            (name = "leases", description = "Lease management for polling deployments")
        ),
        info(
            title = "Commands API",
            description = "Alien Commands API",
            version = "1.0.0"
        ),
    )]
    pub struct ApiDoc;
}

#[cfg(feature = "openapi")]
pub use openapi::ApiDoc;
