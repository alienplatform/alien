//! Release channels and deployment routing.
//!
//! - `GET/POST /v1/release-channels`, `DELETE /v1/release-channels/{name}`
//! - `POST /v1/releases/{id}/promote` points a channel at an existing release
//!   and rolls it out to the deployments following the channel.
//! - `GET /v1/deployments/{id}/routing`, `PUT /v1/deployments/{id}/channel`
//!   and `PUT /v1/deployments/{id}/pin` choose what one deployment runs.
//!
//! Served when the manager has a [`ReleaseChannelStore`]; otherwise every
//! release goes to every deployment and these routes answer 404.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::{
    auth::Subject,
    error::ErrorData,
    traits::{
        DeploymentRecord, DeploymentRouting, ReleaseChannelStore, ReleaseRecord, DEFAULT_CHANNEL,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/release-channels",
            get(list_channels).post(create_channel),
        )
        .route("/v1/release-channels/{name}", delete(delete_channel))
        .route("/v1/releases/{id}/promote", post(promote_release))
        .route("/v1/deployments/{id}/routing", get(get_routing))
        .route("/v1/deployments/{id}/channel", put(set_channel))
        .route("/v1/deployments/{id}/pin", put(set_pin))
}

/// A release channel.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseChannelResponse {
    pub name: String,
    /// Release the channel points at; absent until one is published or promoted to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release_id: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// Deployments following the channel, pinned or not.
    pub deployments: u64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListReleaseChannelsResponse {
    pub items: Vec<ReleaseChannelResponse>,
}

/// Body of `POST /v1/release-channels`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateReleaseChannelRequest {
    /// Lowercase letters, digits and hyphens, starting with a letter.
    pub name: String,
    /// Release the channel starts at.
    #[serde(default)]
    pub release_id: Option<String>,
}

/// Body of `POST /v1/releases/{id}/promote`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PromoteReleaseRequest {
    pub channel: String,
}

/// Body of `PUT /v1/deployments/{id}/channel`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetDeploymentChannelRequest {
    pub channel: String,
}

/// Body of `PUT /v1/deployments/{id}/pin`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetDeploymentPinRequest {
    /// Release to pin to; absent unpins and returns to the channel's release.
    #[serde(default)]
    pub release_id: Option<String>,
}

/// What a deployment follows and the release that makes it run.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRoutingResponse {
    pub deployment_id: String,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_release_id: Option<String>,
    /// The release the deployment is sent: the pin, else the channel's release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
}

/// The release a deployment should run: its pin, else its channel's release.
/// Without channels, the release the manager already chose for it. Else the
/// latest release. `deployment` is `None` for one being created, which
/// follows the default channel.
pub(crate) async fn release_for_deployment(
    state: &AppState,
    subject: &Subject,
    deployment: Option<&DeploymentRecord>,
) -> Result<Option<ReleaseRecord>, alien_error::AlienError> {
    if let Some(channels) = &state.release_channels {
        let routing = match deployment {
            Some(deployment) => channels.deployment_routing(&deployment.id).await?,
            None => DeploymentRouting {
                channel: DEFAULT_CHANNEL.to_string(),
                pinned_release_id: None,
            },
        };
        let release_id = match routing.pinned_release_id {
            Some(pinned) => Some(pinned),
            None => channels
                .get_channel(&routing.channel)
                .await?
                .and_then(|channel| channel.current_release_id),
        };
        if let Some(release_id) = release_id {
            return state.release_store.get_release(subject, &release_id).await;
        }
    } else if let Some(release_id) = deployment.and_then(|d| d.desired_release_id.as_deref()) {
        return state.release_store.get_release(subject, release_id).await;
    }
    state.release_store.get_latest_release(subject).await
}

/// Send a newly created release through its channel (or to every deployment
/// when the manager has no channels).
pub(crate) async fn route_new_release(
    state: &AppState,
    subject: &Subject,
    release_id: &str,
    channel: &str,
) -> Result<(), alien_error::AlienError> {
    match &state.release_channels {
        Some(channels) => {
            channels.set_channel_release(channel, release_id).await?;
            channels.roll_out_channel(channel, release_id).await
        }
        None => {
            state
                .deployment_store
                .set_desired_release(subject, release_id, None)
                .await
        }
    }
}

/// Channel names: lowercase letters, digits and hyphens, starting with a
/// letter, at most 63 characters.
pub(crate) fn valid_channel_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn channel_name_error() -> Response {
    ErrorData::bad_request(
        "Channel names start with a letter and contain only lowercase letters, digits and hyphens",
    )
    .into_response()
}

// --- Handlers ---

type Handler<T> = std::result::Result<T, Response>;

async fn setup(
    state: &AppState,
    headers: &HeaderMap,
    manage: bool,
) -> Handler<(Subject, Arc<dyn ReleaseChannelStore>)> {
    let subject = auth::require_auth(state, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let Some(channels) = state.release_channels.clone() else {
        return Err(ErrorData::not_found_release(
            "release channels are not available on this manager",
        )
        .into_response());
    };
    let allowed = if manage {
        state.authz.can_manage_release_channels(&subject)
    } else {
        state.authz.can_read_release_channels(&subject)
    };
    if !allowed {
        return Err(ErrorData::forbidden("Cannot manage release channels").into_response());
    }
    Ok((subject, channels))
}

async fn release(state: &AppState, subject: &Subject, id: &str) -> Handler<ReleaseRecord> {
    match state.release_store.get_release(subject, id).await {
        Ok(Some(release)) if state.authz.can_read_release(subject, &release) => Ok(release),
        Ok(_) => Err(ErrorData::not_found_release(id).into_response()),
        Err(e) => Err(e.into_response()),
    }
}

async fn deployment(state: &AppState, subject: &Subject, id: &str) -> Handler<DeploymentRecord> {
    match state.deployment_store.get_deployment(subject, id).await {
        Ok(Some(deployment)) if state.authz.can_update_deployment(subject, &deployment) => {
            Ok(deployment)
        }
        Ok(Some(_)) => Err(ErrorData::forbidden("Cannot route this deployment").into_response()),
        Ok(None) => Err(ErrorData::not_found_deployment(id).into_response()),
        Err(e) => Err(e.into_response()),
    }
}

async fn channel_response(
    channels: &dyn ReleaseChannelStore,
    record: crate::traits::ReleaseChannelRecord,
) -> Handler<ReleaseChannelResponse> {
    let deployments = channels
        .count_following(&record.name)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(ReleaseChannelResponse {
        name: record.name,
        current_release_id: record.current_release_id,
        updated_at: record.updated_at,
        deployments,
    })
}

async fn routing_response(
    channels: &dyn ReleaseChannelStore,
    deployment_id: &str,
) -> Handler<DeploymentRoutingResponse> {
    let routing = channels
        .deployment_routing(deployment_id)
        .await
        .map_err(IntoResponse::into_response)?;
    let release_id = match &routing.pinned_release_id {
        Some(pinned) => Some(pinned.clone()),
        None => channels
            .get_channel(&routing.channel)
            .await
            .map_err(IntoResponse::into_response)?
            .and_then(|channel| channel.current_release_id),
    };
    Ok(DeploymentRoutingResponse {
        deployment_id: deployment_id.to_string(),
        channel: routing.channel,
        pinned_release_id: routing.pinned_release_id,
        release_id,
    })
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/release-channels",
    operation_id = "list_manager_release_channels",
    tag = "releases",
    responses((status = 200, description = "Release channels", body = ListReleaseChannelsResponse)),
    security(("bearer" = []))
))]
async fn list_channels(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (_, channels) = match setup(&state, &headers, false).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    let records = match channels.list_channels().await {
        Ok(records) => records,
        Err(e) => return e.into_response(),
    };
    let mut items = Vec::with_capacity(records.len());
    for record in records {
        match channel_response(channels.as_ref(), record).await {
            Ok(item) => items.push(item),
            Err(response) => return response,
        }
    }
    Json(ListReleaseChannelsResponse { items }).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/release-channels",
    operation_id = "create_manager_release_channel",
    tag = "releases",
    request_body = CreateReleaseChannelRequest,
    responses((status = 201, description = "Channel created", body = ReleaseChannelResponse)),
    security(("bearer" = []))
))]
async fn create_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateReleaseChannelRequest>,
) -> Response {
    let (subject, channels) = match setup(&state, &headers, true).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if !valid_channel_name(&body.name) {
        return channel_name_error();
    }
    match channels.get_channel(&body.name).await {
        Ok(Some(_)) => {
            return ErrorData::bad_request(format!("Channel '{}' already exists", body.name))
                .into_response()
        }
        Ok(None) => {}
        Err(e) => return e.into_response(),
    }
    if let Some(release_id) = &body.release_id {
        if let Err(response) = release(&state, &subject, release_id).await {
            return response;
        }
    }
    let record = match channels
        .create_channel(&body.name, body.release_id.as_deref())
        .await
    {
        Ok(record) => record,
        Err(e) => return e.into_response(),
    };
    match channel_response(channels.as_ref(), record).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(response) => response,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/v1/release-channels/{name}",
    operation_id = "delete_manager_release_channel",
    tag = "releases",
    params(("name" = String, Path, description = "Channel name")),
    responses((status = 204, description = "Channel deleted")),
    security(("bearer" = []))
))]
async fn delete_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let (_, channels) = match setup(&state, &headers, true).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if name == DEFAULT_CHANNEL {
        return ErrorData::bad_request(format!("The '{DEFAULT_CHANNEL}' channel can't be deleted"))
            .into_response();
    }
    match channels.count_following(&name).await {
        Ok(0) => {}
        Ok(count) => {
            return ErrorData::bad_request(format!(
                "{count} deployment(s) follow '{name}'; move them to another channel first"
            ))
            .into_response()
        }
        Err(e) => return e.into_response(),
    }
    match channels.delete_channel(&name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => ErrorData::bad_request(format!("No channel named '{name}'")).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/releases/{id}/promote",
    operation_id = "promote_manager_release",
    tag = "releases",
    params(("id" = String, Path, description = "Release ID")),
    request_body = PromoteReleaseRequest,
    responses((status = 200, description = "The channel now points at the release", body = ReleaseChannelResponse)),
    security(("bearer" = []))
))]
async fn promote_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PromoteReleaseRequest>,
) -> Response {
    let (subject, channels) = match setup(&state, &headers, true).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if !valid_channel_name(&body.channel) {
        return channel_name_error();
    }
    if let Err(response) = release(&state, &subject, &id).await {
        return response;
    }
    match channels.get_channel(&body.channel).await {
        Ok(Some(_)) => {}
        Ok(None) if body.channel == DEFAULT_CHANNEL => {}
        Ok(None) => {
            return ErrorData::bad_request(format!(
                "No channel named '{}'; create it first",
                body.channel
            ))
            .into_response()
        }
        Err(e) => return e.into_response(),
    }
    let record = match channels.set_channel_release(&body.channel, &id).await {
        Ok(record) => record,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = channels.roll_out_channel(&body.channel, &id).await {
        return e.into_response();
    }
    match channel_response(channels.as_ref(), record).await {
        Ok(item) => Json(item).into_response(),
        Err(response) => response,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/routing",
    operation_id = "get_deployment_routing",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID")),
    responses((status = 200, description = "Channel and pin", body = DeploymentRoutingResponse)),
    security(("bearer" = []))
))]
async fn get_routing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (subject, channels) = match setup(&state, &headers, false).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(deployment)) if state.authz.can_read_deployment(&subject, &deployment) => {
            deployment
        }
        Ok(_) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };
    match routing_response(channels.as_ref(), &deployment.id).await {
        Ok(routing) => Json(routing).into_response(),
        Err(response) => response,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/v1/deployments/{id}/channel",
    operation_id = "set_deployment_channel",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID")),
    request_body = SetDeploymentChannelRequest,
    responses((status = 200, description = "The deployment follows the channel", body = DeploymentRoutingResponse)),
    security(("bearer" = []))
))]
async fn set_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetDeploymentChannelRequest>,
) -> Response {
    let (subject, channels) = match setup(&state, &headers, true).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if !valid_channel_name(&body.channel) {
        return channel_name_error();
    }
    let deployment = match deployment(&state, &subject, &id).await {
        Ok(deployment) => deployment,
        Err(response) => return response,
    };
    let channel = match channels.get_channel(&body.channel).await {
        Ok(Some(channel)) => Some(channel),
        Ok(None) if body.channel == DEFAULT_CHANNEL => None,
        Ok(None) => {
            return ErrorData::bad_request(format!(
                "No channel named '{}'; create it first",
                body.channel
            ))
            .into_response()
        }
        Err(e) => return e.into_response(),
    };
    let mut routing = match channels.deployment_routing(&deployment.id).await {
        Ok(routing) => routing,
        Err(e) => return e.into_response(),
    };
    routing.channel = body.channel.clone();
    if let Err(e) = channels
        .set_deployment_routing(&deployment.id, &routing)
        .await
    {
        return e.into_response();
    }
    // A pin keeps the deployment where it is until it is unpinned.
    if routing.pinned_release_id.is_none() {
        if let Some(release_id) = channel.and_then(|channel| channel.current_release_id) {
            if let Err(e) = channels
                .roll_out_deployment(&deployment.id, &release_id)
                .await
            {
                return e.into_response();
            }
        }
    }
    match routing_response(channels.as_ref(), &deployment.id).await {
        Ok(routing) => Json(routing).into_response(),
        Err(response) => response,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/v1/deployments/{id}/pin",
    operation_id = "set_deployment_pin",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID")),
    request_body = SetDeploymentPinRequest,
    responses((status = 200, description = "The deployment's pin", body = DeploymentRoutingResponse)),
    security(("bearer" = []))
))]
async fn set_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetDeploymentPinRequest>,
) -> Response {
    let (subject, channels) = match setup(&state, &headers, true).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    let deployment = match deployment(&state, &subject, &id).await {
        Ok(deployment) => deployment,
        Err(response) => return response,
    };
    if let Some(release_id) = &body.release_id {
        if let Err(response) = release(&state, &subject, release_id).await {
            return response;
        }
    }
    let mut routing = match channels.deployment_routing(&deployment.id).await {
        Ok(routing) => routing,
        Err(e) => return e.into_response(),
    };
    routing.pinned_release_id = body.release_id.clone();
    if let Err(e) = channels
        .set_deployment_routing(&deployment.id, &routing)
        .await
    {
        return e.into_response();
    }
    let target = match release_for_deployment(&state, &subject, Some(&deployment)).await {
        Ok(target) => target,
        Err(e) => return e.into_response(),
    };
    if let Some(target) = target {
        if let Err(e) = channels
            .roll_out_deployment(&deployment.id, &target.id)
            .await
        {
            return e.into_response();
        }
    }
    match routing_response(channels.as_ref(), &deployment.id).await {
        Ok(routing) => Json(routing).into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::valid_channel_name;

    #[test]
    fn channel_names() {
        for good in ["production", "staging", "canary-2", "a"] {
            assert!(valid_channel_name(good), "{good}");
        }
        for bad in ["", "Prod", "2nd", "-x", "a_b", "a.b", &"a".repeat(64)] {
            assert!(!valid_channel_name(bad), "{bad}");
        }
    }
}
