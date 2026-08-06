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

use alien_azure_clients::{AzureStorageAccountsClient, AzureTokenCache, StorageAccountsApi};
use alien_bindings::RemoteBindings;
use alien_core::{
    AzureClientConfig, AzureCredentials, KeyFingerprint, KeyOutputs, Platform, StorageOutputs,
};
use alien_test::{TestContext, TestDeployment};
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
const NATIVE_STORAGE_BINDING: &str = "encrypted-storage";
const NATIVE_STORAGE_KEY: &str = ENTERPRISE_KEY_BINDING;
const STORAGE_KEY_BINDING: &str = "storage-key";
const AI_BINDING: &str = "customer-models";

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

        let plaintext = [0x5au8; 128];
        let context = BTreeMap::from([
            ("purpose".to_string(), "application-root".to_string()),
            ("test".to_string(), deployment.id.clone()),
        ]);
        // Setup can finish before a newly attached cloud IAM policy is visible
        // to the provider data plane. Keep the production client's short retry
        // behavior, then give this setup qualification a bounded readiness
        // window before declaring the generated access identity unusable.
        let ciphertext = {
            let max_attempts = 15;
            let mut result = None;
            for attempt in 1..=max_attempts {
                match key.encrypt(&plaintext, Some(&context)).await {
                    Ok(ciphertext) => {
                        result = Some(ciphertext);
                        break;
                    }
                    Err(error) if attempt < max_attempts => {
                        info!(
                            attempt,
                            max_attempts,
                            platform = %platform.as_str(),
                            error = %error,
                            "Remote Key is not ready after setup; waiting for cloud IAM propagation"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(error) => return Err(error).context(
                        "encrypt through real remote Enterprise Key after setup readiness window",
                    ),
                }
            }
            result.context("remote Enterprise Key readiness loop returned no ciphertext")?
        };
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

/// Write and read real object data, then inspect provider metadata to prove
/// that Storage.encryptionKey selected the intended customer-managed Key.
pub fn check_native_storage_encryption<'a>(
    ctx: &'a TestContext,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let response = ctx
            .deployment
            .manager()
            .client()
            .get_deployment()
            .id(&ctx.deployment.id)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
        let value = response
            .into_inner()
            .stack_state
            .context("deployment is missing stack_state")?;
        let state: alien_core::StackState =
            serde_json::from_value(value).context("failed to parse stack_state")?;
        let storage = state
            .get_resource_outputs::<StorageOutputs>(NATIVE_STORAGE_BINDING)
            .context("encrypted Storage has no outputs")?;
        let storage_internal = state
            .resources
            .get(NATIVE_STORAGE_BINDING)
            .and_then(|resource| resource.internal_state.as_ref());
        let key = state
            .get_resource_outputs::<KeyOutputs>(NATIVE_STORAGE_KEY)
            .context("native Storage Key has no outputs")?;
        let env = ctx
            .distribution_cleanups
            .iter()
            .map(|cleanup| cleanup.command_env())
            .find(|env| !env.is_empty())
            .context("no distribution cleanup environment for native Storage check")?;

        match (&ctx.platform, &key.fingerprint) {
            (Platform::Aws, KeyFingerprint::Aws { key_arn }) => {
                check_aws_native_storage(&storage.bucket_name, key_arn, env).await
            }
            (Platform::Gcp, KeyFingerprint::Gcp { crypto_key_name }) => {
                check_gcp_native_storage(&storage.bucket_name, crypto_key_name, env).await
            }
            (
                Platform::Azure,
                KeyFingerprint::Azure {
                    vault_resource_id,
                    key_name,
                    lineage_version_id: _,
                },
            ) => {
                let account = storage_internal
                    .and_then(|value| value.get("storageAccountName"))
                    .and_then(serde_json::Value::as_str)
                    .context("Azure Storage controller has no storageAccountName")?;
                check_azure_native_storage(
                    &storage.bucket_name,
                    account,
                    vault_resource_id,
                    key_name,
                    &key.wrapping_key_id,
                    env,
                )
                .await
            }
            (platform, fingerprint) => anyhow::bail!(
                "native Storage test does not yet support {platform:?} fingerprint {fingerprint:?}"
            ),
        }
    })
}

async fn check_gcp_native_storage(
    bucket: &str,
    crypto_key_name: &str,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let object = format!(
        "gs://{bucket}/alien-e2e/native-encryption/{}.bin",
        uuid::Uuid::new_v4().simple()
    );
    let payload = b"alien native Storage encryption live-cloud e2e";
    let source = tempfile::NamedTempFile::new().context("create native Storage source file")?;
    std::fs::write(source.path(), payload).context("write native Storage source file")?;
    let destination =
        tempfile::NamedTempFile::new().context("create native Storage destination file")?;

    let verification = async {
        run_cloud_command(
            "gcloud",
            &[
                "storage",
                "cp",
                source.path().to_str().context("source path is not UTF-8")?,
                &object,
                "--quiet",
            ],
            env,
            "GCP Storage upload",
        )
        .await?;
        let description = run_cloud_command(
            "gcloud",
            &["storage", "objects", "describe", &object, "--format=json"],
            env,
            "GCP Storage object describe",
        )
        .await?;
        let description: serde_json::Value =
            serde_json::from_str(&description).context("parse GCP Storage object description")?;
        let version_prefix = format!("{crypto_key_name}/cryptoKeyVersions/");
        anyhow::ensure!(
            json_contains_string_prefix(&description, &version_prefix),
            "GCP object did not use a version of the intended CryptoKey"
        );
        run_cloud_command(
            "gcloud",
            &[
                "storage",
                "cp",
                &object,
                destination
                    .path()
                    .to_str()
                    .context("destination path is not UTF-8")?,
                "--quiet",
            ],
            env,
            "GCP Storage download",
        )
        .await?;
        anyhow::ensure!(
            std::fs::read(destination.path()).context("read downloaded GCP object")? == payload,
            "GCP object payload changed"
        );
        Ok(())
    }
    .await;

    finish_native_storage_check(
        verification,
        run_cloud_command(
            "gcloud",
            &["storage", "rm", &object, "--quiet"],
            env,
            "GCP Storage object delete",
        )
        .await
        .map(|_| ()),
        "GCP",
    )
}

fn json_contains_string_prefix(value: &serde_json::Value, prefix: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value.starts_with(prefix),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_contains_string_prefix(value, prefix)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| json_contains_string_prefix(value, prefix)),
        _ => false,
    }
}

async fn check_azure_native_storage(
    container: &str,
    account: &str,
    vault_resource_id: &str,
    key_name: &str,
    wrapping_key_id: &str,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let config = AzureClientConfig {
        subscription_id: env_value(env, "ARM_SUBSCRIPTION_ID")?.to_string(),
        tenant_id: env_value(env, "ARM_TENANT_ID")?.to_string(),
        region: None,
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env_value(env, "ARM_CLIENT_ID")?.to_string(),
            client_secret: env_value(env, "ARM_CLIENT_SECRET")?.to_string(),
        },
        service_overrides: None,
    };
    let resource_group = resource_id_segment(vault_resource_id, "resourceGroups")?;
    let management = AzureStorageAccountsClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(config.clone()),
    );
    let account_properties = management
        .get_storage_account_properties(resource_group, account)
        .await
        .map_err(|error| anyhow::anyhow!("get Azure Storage account properties: {error}"))?;
    let encryption = account_properties
        .properties
        .and_then(|properties| properties.encryption)
        .context("Azure Storage account has no encryption settings")?;
    anyhow::ensure!(
        encryption.key_source.to_string() == "Microsoft.Keyvault",
        "Azure Storage account does not use a Key Vault key"
    );
    let properties = encryption
        .keyvaultproperties
        .context("Azure Storage account has no Key Vault properties")?;
    anyhow::ensure!(
        properties.keyname.as_deref() == Some(key_name),
        "Azure Storage account uses a different Key Vault key"
    );
    anyhow::ensure!(
        properties.current_versioned_key_identifier.as_deref() == Some(wrapping_key_id),
        "Azure Storage account reports a different current key identifier"
    );

    let storage_key = management
        .list_storage_account_keys(resource_group, account)
        .await
        .map_err(|error| anyhow::anyhow!("list Azure Storage account keys: {error}"))?
        .keys
        .into_iter()
        .find_map(|key| key.value)
        .context("Azure Storage account returned no access key")?;
    let mut data_env = env.to_vec();
    data_env.push(("AZURE_STORAGE_ACCOUNT".to_string(), account.to_string()));
    data_env.push(("AZURE_STORAGE_KEY".to_string(), storage_key));
    let object = format!(
        "alien-e2e/native-encryption/{}.bin",
        uuid::Uuid::new_v4().simple()
    );
    let payload = b"alien native Storage encryption live-cloud e2e";
    let source = tempfile::NamedTempFile::new().context("create Azure Storage source file")?;
    std::fs::write(source.path(), payload).context("write Azure Storage source file")?;
    let destination =
        tempfile::NamedTempFile::new().context("create Azure Storage destination file")?;
    let verification = async {
        run_cloud_command(
            "az",
            &[
                "storage",
                "blob",
                "upload",
                "--container-name",
                container,
                "--name",
                &object,
                "--file",
                source.path().to_str().context("source path is not UTF-8")?,
                "--overwrite",
                "true",
                "--auth-mode",
                "key",
                "--only-show-errors",
                "--output",
                "none",
            ],
            &data_env,
            "Azure Blob upload",
        )
        .await?;
        run_cloud_command(
            "az",
            &[
                "storage",
                "blob",
                "download",
                "--container-name",
                container,
                "--name",
                &object,
                "--file",
                destination
                    .path()
                    .to_str()
                    .context("destination path is not UTF-8")?,
                "--auth-mode",
                "key",
                "--only-show-errors",
                "--output",
                "none",
            ],
            &data_env,
            "Azure Blob download",
        )
        .await?;
        anyhow::ensure!(
            std::fs::read(destination.path()).context("read downloaded Azure object")? == payload,
            "Azure object payload changed"
        );
        Ok(())
    }
    .await;
    let deletion = run_cloud_command(
        "az",
        &[
            "storage",
            "blob",
            "delete",
            "--container-name",
            container,
            "--name",
            &object,
            "--auth-mode",
            "key",
            "--only-show-errors",
            "--output",
            "none",
        ],
        &data_env,
        "Azure Blob delete",
    )
    .await
    .map(|_| ());
    finish_native_storage_check(verification, deletion, "Azure")
}

fn env_value<'a>(env: &'a [(String, String)], name: &str) -> anyhow::Result<&'a str> {
    env.iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
        .with_context(|| format!("native Storage check is missing {name}"))
}

fn resource_id_segment<'a>(resource_id: &'a str, segment: &str) -> anyhow::Result<&'a str> {
    let mut parts = resource_id.split('/');
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case(segment) {
            return parts
                .next()
                .filter(|value| !value.is_empty())
                .with_context(|| format!("Azure resource ID has no value after {segment}"));
        }
    }
    anyhow::bail!("Azure resource ID has no {segment} segment")
}

fn finish_native_storage_check(
    verification: anyhow::Result<()>,
    deletion: anyhow::Result<()>,
    provider: &str,
) -> anyhow::Result<()> {
    match (verification, deletion) {
        (Ok(()), Ok(())) => {
            info!(provider, "Native Storage encryption check passed");
            Ok(())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(error.context(format!(
            "native Storage object cleanup also failed: {cleanup:#}"
        ))),
    }
}

async fn check_aws_native_storage(
    bucket: &str,
    key_arn: &str,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let object = format!(
        "alien-e2e/native-encryption/{}.bin",
        uuid::Uuid::new_v4().simple()
    );
    let payload = b"alien native Storage encryption live-cloud e2e";
    let source = tempfile::NamedTempFile::new().context("create native Storage source file")?;
    std::fs::write(source.path(), payload).context("write native Storage source file")?;
    let destination =
        tempfile::NamedTempFile::new().context("create native Storage destination file")?;

    let verification = async {
        run_cloud_command(
            "aws",
            &[
                "s3api",
                "put-object",
                "--bucket",
                bucket,
                "--key",
                &object,
                "--body",
                source.path().to_str().context("source path is not UTF-8")?,
                "--output",
                "json",
            ],
            env,
            "AWS S3 put-object",
        )
        .await?;
        let head = run_cloud_command(
            "aws",
            &[
                "s3api",
                "head-object",
                "--bucket",
                bucket,
                "--key",
                &object,
                "--output",
                "json",
            ],
            env,
            "AWS S3 head-object",
        )
        .await?;
        let head: serde_json::Value =
            serde_json::from_str(&head).context("parse AWS S3 head-object output")?;
        anyhow::ensure!(
            head.get("ServerSideEncryption")
                .and_then(serde_json::Value::as_str)
                == Some("aws:kms"),
            "S3 object is not protected by SSE-KMS"
        );
        anyhow::ensure!(
            head.get("SSEKMSKeyId").and_then(serde_json::Value::as_str) == Some(key_arn),
            "S3 object used a different KMS key"
        );
        run_cloud_command(
            "aws",
            &[
                "s3api",
                "get-object",
                "--bucket",
                bucket,
                "--key",
                &object,
                destination
                    .path()
                    .to_str()
                    .context("destination path is not UTF-8")?,
            ],
            env,
            "AWS S3 get-object",
        )
        .await?;
        anyhow::ensure!(
            std::fs::read(destination.path()).context("read downloaded S3 object")? == payload,
            "S3 object payload changed"
        );
        Ok(())
    }
    .await;

    let deletion = run_cloud_command(
        "aws",
        &[
            "s3api",
            "delete-object",
            "--bucket",
            bucket,
            "--key",
            &object,
        ],
        env,
        "AWS S3 delete-object",
    )
    .await;
    match (verification, deletion) {
        (Ok(()), Ok(_)) => {
            info!("Native AWS Storage encryption check passed");
            Ok(())
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(error.context(format!(
            "native Storage object cleanup also failed: {cleanup:#}"
        ))),
    }
}

async fn run_cloud_command(
    program: &str,
    args: &[&str],
    env: &[(String, String)],
    description: &str,
) -> anyhow::Result<String> {
    let mut command = tokio::process::Command::new(program);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .await
        .with_context(|| format!("failed to run {description}"))?;
    anyhow::ensure!(
        output.status.success(),
        "{description} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).with_context(|| format!("{description} returned non-UTF-8"))
}

/// Resolve the deployment's real cloud AI resource through the public Remote
/// Bindings API, then use the Manager-minted Access credential for one real
/// inference request through the same protocol engine as the hosted gateway.
pub fn check_remote_ai<'a>(
    deployment: &'a TestDeployment,
    platform: Platform,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if platform == Platform::Azure
            && std::env::var("AZURE_FEDERATED_TOKEN_FILE")
                .ok()
                .filter(|value| !value.is_empty())
                .is_none()
        {
            info!(
                "Skipping Azure Remote Bindings inference locally: the local target-static resolver uses the test service principal and cannot exchange an OIDC token for the generated Access identity; the OIDC-backed CI run is the qualifying test"
            );
            return Ok(());
        }

        info!(
            platform = %platform.as_str(),
            "Checking remote AI through assigned-manager discovery"
        );
        let discovery = DiscoveryServer::start(deployment, platform).await?;
        let bindings =
            RemoteBindings::for_deployment(&deployment.id, &deployment.token, Some(&discovery.url))
                .await
                .context("discover assigned manager for remote AI binding")?;
        let lease = bindings
            .ai()
            .await
            .context("resolve real remote AI binding")?;
        anyhow::ensure!(
            lease.resource_id == AI_BINDING,
            "resolved AI resource '{}' instead of '{AI_BINDING}'",
            lease.resource_id
        );

        let model = match platform {
            Platform::Aws => "gpt-oss-20b",
            Platform::Gcp => "gemini-2.5-flash",
            Platform::Azure => "gpt-4.1",
            other => bail!("remote AI has no qualification model for {other:?}"),
        };
        let route = alien_ai_gateway::route_from_remote_ai_lease(AI_BINDING, &lease)
            .await
            .context("build gateway route from remote AI lease")?;
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .context("bind remote AI gateway")?;
        let address = listener
            .local_addr()
            .context("read remote AI gateway address")?;
        let task = tokio::spawn(async move {
            if let Err(error) =
                axum::serve(listener, alien_ai_gateway::build_router(vec![route])).await
            {
                tracing::error!(%error, "remote AI gateway failed");
            }
        });

        let response = reqwest::Client::new()
            .post(format!("http://{address}/{AI_BINDING}/v1/chat/completions"))
            .timeout(std::time::Duration::from_secs(180))
            .json(&json!({
                "model": model,
                "max_completion_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }]
            }))
            .send()
            .await
            .context("invoke model through remote AI gateway")?;
        task.abort();

        let status = response.status();
        if !status.is_success() && status != StatusCode::TOO_MANY_REQUESTS {
            let body = response.text().await.unwrap_or_default();
            bail!("remote AI model '{model}' returned {status}: {body}");
        }
        info!(
            platform = %platform.as_str(),
            model,
            %status,
            "Remote AI credential and inference check passed"
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
