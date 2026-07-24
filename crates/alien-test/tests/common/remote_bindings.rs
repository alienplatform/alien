//! Live-cloud verification of the public remote Storage API.
//!
//! The local discovery fixture stands in only for the Platform API. It points
//! the public client at the in-process manager that owns the real deployment;
//! manager authorization, credential attenuation, and object operations all
//! run through their production paths.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;

use alien_bindings::RemoteBindings;
use alien_core::Platform;
use alien_test::TestDeployment;
use anyhow::{bail, Context};
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::TryStreamExt;
use object_store::path::Path;
use object_store::{Error as ObjectStoreError, PutPayload};
use serde_json::json;
use tracing::info;

use super::bindings::STORAGE_BINDING;

const MANAGER_ID: &str = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PROJECT_ID: &str = "prj_cccccccccccccccccccccccccccc";
const DEPLOYMENT_GROUP_ID: &str = "dg_dddddddddddddddddddddddddddd";
const WORKSPACE_ID: &str = "ws_eeeeeeeeeeeeeeeeeeeeeeee";
const PAYLOAD: &[u8] = b"alien remote storage live-cloud e2e";

#[derive(Clone)]
struct DiscoveryState {
    deployment_id: String,
    manager_url: String,
    platform: Platform,
    authorization: HeaderValue,
    manager_access_token: String,
}

struct DiscoveryServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DiscoveryServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl DiscoveryServer {
    async fn start(deployment: &TestDeployment, platform: Platform) -> anyhow::Result<Self> {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", deployment.token))
            .context("build discovery authorization header")?;
        authorization.set_sensitive(true);
        let state = DiscoveryState {
            deployment_id: deployment.id.clone(),
            manager_url: deployment.manager().url.clone(),
            platform,
            authorization,
            manager_access_token: deployment.token.clone(),
        };
        let app = Router::new()
            .route("/v1/deployments/{id}", get(deployment_handler))
            .route(
                "/v1/managers/{id}/binding-token",
                post(manager_binding_token_handler),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .context("bind Platform discovery fixture")?;
        let address = listener
            .local_addr()
            .context("read Platform discovery fixture address")?;
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                tracing::error!(%error, "Platform discovery fixture failed");
            }
        });

        Ok(Self {
            url: format!("http://{address}"),
            task,
        })
    }
}

fn is_authorized(state: &DiscoveryState, headers: &HeaderMap) -> bool {
    headers.get(reqwest::header::AUTHORIZATION) == Some(&state.authorization)
}

async fn deployment_handler(
    State(state): State<DiscoveryState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if id != state.deployment_id {
        return StatusCode::NOT_FOUND.into_response();
    }

    Json(json!({
        "id": state.deployment_id,
        "name": "remote-storage-live-cloud-e2e",
        "status": "running",
        "projectId": PROJECT_ID,
        "platform": state.platform.as_str(),
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": DEPLOYMENT_GROUP_ID,
        "stackSettings": {},
        "retryRequested": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": MANAGER_ID,
        "workspaceId": WORKSPACE_ID,
    }))
    .into_response()
}

async fn manager_binding_token_handler(
    State(state): State<DiscoveryState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if !is_authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if id != MANAGER_ID {
        return StatusCode::NOT_FOUND.into_response();
    }
    if body.get("deploymentId").and_then(serde_json::Value::as_str)
        != Some(state.deployment_id.as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    Json(json!({
        "accessToken": state.manager_access_token,
        "expiresIn": 300,
        "tokenType": "Bearer",
        "managerUrl": state.manager_url,
        "databaseId": null,
        "controlPlaneUrl": null,
    }))
    .into_response()
}

/// Resolve the deployment's real cloud Storage through the public remote API
/// and exercise every operation in its intentionally narrow v0 surface.
pub fn check_remote_storage<'a>(
    deployment: &'a TestDeployment,
    platform: Platform,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    // This check includes generated SDK and provider futures that are large
    // enough to overflow nextest's test-thread stack when embedded directly in
    // the comprehensive runner's async state machine. Keep that state on the
    // heap; this is also the boundary between the generic runner and the
    // feature-specific live-cloud flow.
    Box::pin(async move {
        info!(
            platform = %platform.as_str(),
            "Checking remote Storage through assigned-manager discovery"
        );
        let discovery = DiscoveryServer::start(deployment, platform).await?;
        let bindings =
            RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
                .await
                .context("discover assigned manager for remote bindings")?;
        let storage = bindings
            .storage(STORAGE_BINDING)
            .await
            .context("resolve real remote Storage binding")?;

        let prefix = Path::from(format!(
            "alien-e2e/remote-bindings/{}/{}",
            deployment.id,
            uuid::Uuid::new_v4().simple()
        ));
        let object = prefix.child("payload.txt");

        let verification = verify_before_delete(storage.as_ref(), &prefix, &object).await;
        let deletion = storage.delete(&object).await;
        match (verification, deletion) {
            // A failed PUT may leave no object; NotFound still proves cleanup is safe.
            (Err(verification), Err(ObjectStoreError::NotFound { .. })) => {
                return Err(verification)
            }
            (Err(verification), Err(deletion)) => {
                bail!("remote Storage verification failed: {verification:#}; cleanup also failed: {deletion:#}")
            }
            (Err(verification), Ok(())) => return Err(verification),
            (Ok(()), Err(deletion)) => {
                return Err(deletion)
                    .context("delete remote Storage object during mandatory cleanup")
            }
            (Ok(()), Ok(())) => {}
        }

        verify_deleted(storage.as_ref(), &prefix, &object).await?;
        info!(
            platform = %platform.as_str(),
            "Remote Storage put/head/get/list/delete check passed"
        );
        Ok(())
    })
}

async fn verify_before_delete(
    storage: &dyn alien_bindings::RemoteStorage,
    prefix: &Path,
    object: &Path,
) -> anyhow::Result<()> {
    storage
        .put(object, PutPayload::from_static(PAYLOAD))
        .await
        .context("put remote Storage object")?;

    let metadata = storage
        .head(object)
        .await
        .context("head remote Storage object")?;
    if metadata.location != *object || metadata.size != PAYLOAD.len() as u64 {
        bail!(
            "remote Storage head mismatch: expected path {object} and {} bytes, got {} and {} bytes",
            PAYLOAD.len(),
            metadata.location,
            metadata.size
        );
    }

    let bytes = storage
        .get(object)
        .await
        .context("get remote Storage object")?
        .bytes()
        .await
        .context("read remote Storage object body")?;
    if bytes.as_ref() != PAYLOAD {
        bail!("remote Storage get returned different object bytes");
    }

    let listed = storage
        .list(Some(prefix))
        .try_collect::<Vec<_>>()
        .await
        .context("list remote Storage prefix")?;
    if listed.len() != 1 || listed[0].location != *object {
        bail!("remote Storage list did not return exactly the written object");
    }

    Ok(())
}

async fn verify_deleted(
    storage: &dyn alien_bindings::RemoteStorage,
    prefix: &Path,
    object: &Path,
) -> anyhow::Result<()> {
    match storage.head(object).await {
        Err(ObjectStoreError::NotFound { .. }) => {}
        Err(error) => return Err(error).context("verify remote Storage object deletion"),
        Ok(_) => bail!("remote Storage object still exists after delete"),
    }

    let listed = storage
        .list(Some(prefix))
        .try_collect::<Vec<_>>()
        .await
        .context("list remote Storage prefix after delete")?;
    if listed.iter().any(|metadata| metadata.location == *object) {
        bail!("remote Storage list still contains the deleted object");
    }

    Ok(())
}
