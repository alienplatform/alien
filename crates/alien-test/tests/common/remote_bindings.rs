//! Live-cloud verification of the public remote Storage API.
//!
//! The local discovery fixture stands in only for the Platform API. It points
//! the public client at the in-process manager that owns the real deployment;
//! manager authorization, credential attenuation, and object operations all
//! run through their production paths.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

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
const ENTERPRISE_KEY_BINDING: &str = "enterprise-key";
const STORAGE_KEY_BINDING: &str = "storage-key";

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
        "releaseChannel": "production",
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

/// Resolve the deployment's real cloud Key through the same public remote API
/// used by the hosted Encryption Gateway, then prove provider cryptography and
/// portable context behavior rather than merely inspecting generated setup.
pub fn check_remote_key<'a>(
    deployment: &'a TestDeployment,
    platform: Platform,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        info!(
            platform = %platform.as_str(),
            "Checking remote Enterprise Key through assigned-manager discovery"
        );
        let discovery = DiscoveryServer::start(deployment, platform).await?;
        let bindings =
            RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
                .await
                .context("discover assigned manager for remote Key binding")?;
        let key = bindings
            .key(ENTERPRISE_KEY_BINDING)
            .await
            .context("resolve real remote Key binding")?;
        anyhow::ensure!(
            bindings.key(STORAGE_KEY_BINDING).await.is_err(),
            "native Storage Key must not be remotely accessible"
        );

        let plaintext = [0x5au8; 128];
        let context = BTreeMap::from([
            ("purpose".to_string(), "application-root".to_string()),
            ("test".to_string(), deployment.id.clone()),
        ]);
        let ciphertext = key
            .encrypt(&plaintext, Some(&context))
            .await
            .context("encrypt through real remote Enterprise Key")?;
        let decrypted = key
            .decrypt(&ciphertext, Some(&context))
            .await
            .context("decrypt through real remote Enterprise Key")?;
        anyhow::ensure!(decrypted == plaintext, "remote Key plaintext changed");

        let wrong_context = BTreeMap::from([
            ("purpose".to_string(), "different-purpose".to_string()),
            ("test".to_string(), deployment.id.clone()),
        ]);
        anyhow::ensure!(
            key.decrypt(&ciphertext, Some(&wrong_context))
                .await
                .is_err(),
            "remote Key must reject the wrong portable context"
        );
        anyhow::ensure!(
            key.encrypt(&[0u8; 129], Some(&context)).await.is_err(),
            "remote Key must reject values above the portable 128-byte limit"
        );
        info!(platform = %platform.as_str(), "Remote Enterprise Key check passed");
        Ok(())
    })
}

/// Encrypt before a provider-native rotation, then resolve a fresh remote
/// client and decrypt the old ciphertext. The caller performs the serialized
/// provider mutation so this helper exercises the same public discovery and
/// credential path used by a newly started Gateway replica.
pub async fn check_remote_key_after_rotation<F, Fut>(
    deployment: &TestDeployment,
    platform: Platform,
    rotate: F,
) -> anyhow::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    info!(
        platform = %platform.as_str(),
        "Checking remote Enterprise Key across provider-native rotation"
    );
    let discovery = DiscoveryServer::start(deployment, platform).await?;
    let before =
        RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
            .await
            .context("discover assigned manager before Key rotation")?;
    let before_key = before
        .key(ENTERPRISE_KEY_BINDING)
        .await
        .context("resolve Enterprise Key before rotation")?;
    let context = BTreeMap::from([
        ("purpose".to_string(), "rotation-qualification".to_string()),
        ("test".to_string(), deployment.id.clone()),
    ]);
    let plaintext = [0xa5u8; 32];
    let ciphertext = before_key
        .encrypt(&plaintext, Some(&context))
        .await
        .context("encrypt before provider-native Key rotation")?;
    drop(before_key);
    drop(before);

    rotate().await?;

    let after =
        RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
            .await
            .context("discover assigned manager after Key rotation")?;
    let after_key = after
        .key(ENTERPRISE_KEY_BINDING)
        .await
        .context("resolve Enterprise Key after rotation")?;
    let decrypted = after_key
        .decrypt(&ciphertext, Some(&context))
        .await
        .context("decrypt pre-rotation ciphertext with a fresh remote client")?;
    anyhow::ensure!(
        decrypted == plaintext,
        "provider-native rotation changed the remote Key plaintext"
    );
    let new_ciphertext = after_key
        .encrypt(&plaintext, Some(&context))
        .await
        .context("encrypt after provider-native Key rotation")?;
    anyhow::ensure!(
        after_key
            .decrypt(&new_ciphertext, Some(&context))
            .await
            .context("decrypt post-rotation ciphertext")?
            == plaintext,
        "post-rotation remote Key plaintext changed"
    );
    info!(
        platform = %platform.as_str(),
        "Remote Enterprise Key rotation check passed"
    );
    Ok(())
}

/// Disable the provider Key itself and prove that fresh remote operations stop,
/// then restore it and decrypt ciphertext created before the interruption.
/// Unlike an IAM-grant test, this kill switch applies to every provider
/// identity and is therefore valid in Azure local `target-static` mode.
pub async fn check_remote_key_disable_restore<D, DFut, R, RFut>(
    deployment: &TestDeployment,
    platform: Platform,
    disable: D,
    restore: R,
    timeout: Duration,
) -> anyhow::Result<Duration>
where
    D: FnOnce() -> DFut,
    DFut: Future<Output = anyhow::Result<()>>,
    R: FnOnce() -> RFut,
    RFut: Future<Output = anyhow::Result<()>>,
{
    let discovery = DiscoveryServer::start(deployment, platform).await?;
    let before =
        RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
            .await
            .context("discover assigned manager before disabling the provider Key")?;
    let before_key = before
        .key(ENTERPRISE_KEY_BINDING)
        .await
        .context("resolve Enterprise Key before disabling it")?;
    let context = BTreeMap::from([
        (
            "purpose".to_string(),
            "provider-disable-qualification".to_string(),
        ),
        ("test".to_string(), deployment.id.clone()),
    ]);
    let plaintext = [0x3cu8; 32];
    let ciphertext = before_key
        .encrypt(&plaintext, Some(&context))
        .await
        .context("encrypt before disabling the provider Key")?;
    drop(before_key);
    drop(before);

    disable().await?;
    let denied = wait_for_remote_key_data_unavailable(deployment, platform, timeout).await;
    let restored = restore().await;
    let elapsed = match (denied, restored) {
        (Err(error), Err(restore_error)) => bail!(
            "provider Key disable check failed: {error:#}; restoring the Key also failed: {restore_error:#}"
        ),
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error.context("restore disabled provider Key")),
        (Ok(elapsed), Ok(())) => elapsed,
    };

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let attempt = async {
            let bindings = RemoteBindings::for_deployment(
                &deployment.id,
                &deployment.token,
                Some(&discovery.url),
            )
            .await?;
            let key = bindings.key(ENTERPRISE_KEY_BINDING).await?;
            let decrypted = key.decrypt(&ciphertext, Some(&context)).await?;
            anyhow::ensure!(decrypted == plaintext, "restored Key changed the plaintext");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if attempt.is_ok() {
            return Ok(elapsed);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(attempt
                .expect_err("failed recovery attempt")
                .context("provider Key did not recover before the deadline"));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Wait for a provider-disabled Key to reject a fresh data operation. Key
/// disablement is distinct from IAM denial: providers use different structured
/// errors for those states. The caller separately verifies and owns the
/// provider state transition, and successful restoration must decrypt the
/// pre-disable ciphertext before this qualification passes.
async fn wait_for_remote_key_data_unavailable(
    deployment: &TestDeployment,
    platform: Platform,
    timeout: Duration,
) -> anyhow::Result<Duration> {
    let discovery = DiscoveryServer::start(deployment, platform).await?;
    let started_at = tokio::time::Instant::now();
    let deadline = started_at + timeout;
    loop {
        let attempt = async {
            let bindings = RemoteBindings::for_deployment(
                &deployment.id,
                &deployment.token,
                Some(&discovery.url),
            )
            .await?;
            let key = bindings.key(ENTERPRISE_KEY_BINDING).await?;
            let context =
                BTreeMap::from([("purpose".to_string(), "disabled-key-probe".to_string())]);
            key.encrypt(b"disabled key probe", Some(&context)).await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if attempt.is_err() {
            return Ok(started_at.elapsed());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "disabled provider Key still accepted fresh operations after {timeout:?}"
            );
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Wait until a fresh remote client can still resolve the published Key but
/// the provider rejects its data operation. This distinguishes revoking the
/// exact cryptographic grant from deleting discovery or the access identity.
pub async fn wait_for_remote_key_data_denied(
    deployment: &TestDeployment,
    platform: Platform,
    timeout: Duration,
) -> anyhow::Result<Duration> {
    let discovery = DiscoveryServer::start(deployment, platform).await?;
    let started_at = tokio::time::Instant::now();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let bindings =
            RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
                .await
                .context("discovery must remain available while Key data access is revoked")?;
        let key = bindings
            .key(ENTERPRISE_KEY_BINDING)
            .await
            .context("the published Key must remain resolvable while its data grant is revoked")?;
        let context = BTreeMap::from([("purpose".to_string(), "revocation-probe".to_string())]);
        match key.encrypt(b"revocation probe", Some(&context)).await {
            Err(error) if error_chain_has_code(&error, "REMOTE_ACCESS_DENIED") => {
                return Ok(started_at.elapsed());
            }
            Err(error) if tokio::time::Instant::now() >= deadline => {
                return Err(error).context(
                    "remote Key failed for a reason other than the expected provider access denial",
                );
            }
            Err(_) => {}
            Ok(_) if tokio::time::Instant::now() >= deadline => {
                anyhow::bail!("remote Key data access still succeeded after {timeout:?}");
            }
            Ok(_) => {}
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

fn error_chain_has_code<T>(error: &alien_error::AlienError<T>, expected: &str) -> bool
where
    T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    if error.code == expected {
        return true;
    }

    let mut source = error.source.as_deref();
    while let Some(cause) = source {
        if cause.code == expected {
            return true;
        }
        source = cause.source.as_deref();
    }
    false
}

/// Wait for the exact restored provider grant to become usable through a new
/// discovery and credential exchange.
pub async fn wait_for_remote_key_data_recovered(
    deployment: &TestDeployment,
    platform: Platform,
    timeout: Duration,
) -> anyhow::Result<()> {
    let discovery = DiscoveryServer::start(deployment, platform).await?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let attempt = async {
            let bindings = RemoteBindings::for_deployment(
                &deployment.id,
                &deployment.token,
                Some(&discovery.url),
            )
            .await?;
            let key = bindings.key(ENTERPRISE_KEY_BINDING).await?;
            let context = BTreeMap::from([("purpose".to_string(), "recovery-probe".to_string())]);
            let ciphertext = key.encrypt(b"recovery probe", Some(&context)).await?;
            let plaintext = key.decrypt(&ciphertext, Some(&context)).await?;
            anyhow::ensure!(plaintext == b"recovery probe");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if attempt.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return attempt.context("remote Key data access did not recover before the deadline");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
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
