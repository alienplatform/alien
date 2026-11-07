//! Durable daily component key replacement. Server responses never supply keys.

use std::sync::Arc;

use alien_client_core::runtime_credentials::{RuntimeProofProvider, ALLOWED_CLOCK_SKEW};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::warn;

use crate::{
    db::OperatorDb,
    error::{ErrorData, Result},
};

/// How long after its expiry the manager still rotates the latest daily key,
/// so an operator survives a manager outage longer than a day.
pub const ROTATION_GRACE: chrono::Duration = chrono::Duration::days(7);

/// Stored only in the encrypted local database; intentionally not Debug.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyIdentityState {
    pub manager_url: String,
    pub audience: String,
    pub deployment_id: String,
    pub current: String,
    pub pending: Option<String>,
    pub pending_started_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    /// Hash of the code that enrolled the current key. A configured code with
    /// this hash is spent and is never sent again.
    #[serde(default)]
    pub enrolled_code_hash: Option<String>,
    #[serde(default)]
    pub pending_enrollment: Option<PendingEnrollment>,
}

/// A re-enrollment whose response may have been lost. A retry with the same
/// code sends the same key hash, so the manager answers it again.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingEnrollment {
    code_hash: String,
    key: String,
}

#[derive(Clone, Copy)]
pub(crate) struct EnrollmentRequest<'a> {
    pub manager_url: &'a str,
    pub audience: &'a str,
    pub code: &'a str,
    pub deployment_id: Option<&'a str>,
}

pub(crate) struct DailyIdentity {
    db: Arc<OperatorDb>,
    operation: Arc<Mutex<()>>,
    client: Client,
}

impl DailyIdentity {
    pub fn new(db: Arc<OperatorDb>) -> Result<Self> {
        let client = Client::builder()
            .redirect(Policy::none())
            .http1_only()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| failed("Cannot initialize component identity transport"))?;
        Ok(Self {
            db: db.clone(),
            operation: db.daily_identity_operation.clone(),
            client,
        })
    }

    /// Enroll with the configured code unless this operator is already
    /// enrolled. The initial key is saved before the request, so a lost
    /// response can be retried with the same key hash and one-use code.
    pub async fn enroll(&self, request: EnrollmentRequest<'_>) -> Result<String> {
        let EnrollmentRequest {
            manager_url,
            audience,
            code,
            deployment_id,
        } = request;
        let _operation = self.operation.lock().await;
        let endpoint =
            reqwest::Url::parse(manager_url).map_err(|_| failed("Invalid manager URL"))?;
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !(endpoint.scheme() == "https"
                || (endpoint.scheme() == "http"
                    && matches!(
                        endpoint.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]")
                    )))
        {
            return Err(failed("Manager identity transport requires HTTPS"));
        }
        if audience.is_empty() {
            return Err(failed("Manager audience is required"));
        }
        if let (Some(expected), Some(canonical)) =
            (deployment_id, self.db.get_deployment_id().await?)
        {
            if canonical != expected {
                return Err(failed("Saved operator belongs to another deployment"));
            }
        }
        let state = match self.db.read_daily_identity().await? {
            Some(state) => {
                check_saved_identity(&state, manager_url, audience, deployment_id)?;
                if !state.deployment_id.is_empty() {
                    self.db.repair_enrolled_deployment_id(&state).await?;
                    return Ok(state.deployment_id);
                }
                state
            }
            None => {
                let state = DailyIdentityState {
                    manager_url: manager_url.into(),
                    audience: audience.into(),
                    deployment_id: String::new(),
                    current: new_key()?,
                    pending: None,
                    pending_started_at: None,
                    expires_at: Utc::now(),
                    enrolled_code_hash: None,
                    pending_enrollment: None,
                };
                self.db.write_daily_identity(&state).await?;
                state
            }
        };
        self.enroll_with_code(state, code, deployment_id).await
    }

    /// Enroll again with the configured code when the saved key no longer
    /// works. Returns `None` when there is no code, or when the code is the
    /// one that enrolled the current key. The current key stays saved until
    /// the manager confirms the replacement.
    pub async fn recover(&self, request: EnrollmentRequest<'_>) -> Result<Option<String>> {
        let EnrollmentRequest {
            manager_url,
            audience,
            code,
            deployment_id,
        } = request;
        let _operation = self.operation.lock().await;
        let Some(mut state) = self.db.read_daily_identity().await? else {
            return Ok(None);
        };
        check_saved_identity(&state, manager_url, audience, deployment_id)?;
        let code_hash = key_hash(code);
        if code.trim().is_empty()
            || state.deployment_id.is_empty()
            || state.enrolled_code_hash.as_deref() == Some(code_hash.as_str())
        {
            return Ok(None);
        }
        if state
            .pending_enrollment
            .as_ref()
            .is_none_or(|pending| pending.code_hash != code_hash)
        {
            state.pending_enrollment = Some(PendingEnrollment {
                code_hash,
                key: new_key()?,
            });
            self.db.write_daily_identity(&state).await?;
        }
        self.enroll_with_code(state, code, deployment_id)
            .await
            .map(Some)
    }

    /// Send the enrollment and save the confirmed key. The caller holds the
    /// operation lock and has saved the candidate key.
    async fn enroll_with_code(
        &self,
        mut state: DailyIdentityState,
        code: &str,
        deployment_id: Option<&str>,
    ) -> Result<String> {
        if code.trim().is_empty() {
            return Err(failed("A fresh component enrollment code is required"));
        }
        let replacing = !state.deployment_id.is_empty();
        let candidate = state
            .pending_enrollment
            .as_ref()
            .map(|pending| pending.key.as_str())
            .unwrap_or(&state.current);
        let response = self
            .post(
                &state,
                "enroll",
                json!({"code":code,"initialKeyHash":key_hash(candidate)}),
            )
            .await?;
        let verified_deployment = response
            .get("deploymentId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| failed("Enrollment returned no deployment identity"))?;
        if deployment_id.is_some_and(|expected| expected != verified_deployment)
            || (replacing && state.deployment_id != verified_deployment)
        {
            return Err(failed("Enrollment belongs to another deployment"));
        }
        let expires_at = response_expiry(&response)?;
        state.current = candidate.to_owned();
        state.deployment_id = verified_deployment.to_owned();
        state.expires_at = expires_at;
        state.enrolled_code_hash = Some(key_hash(code));
        state.pending_enrollment = None;
        state.pending = None;
        state.pending_started_at = None;
        #[cfg(test)]
        if std::env::var_os("ALIEN_TEST_ENROLLMENT_COMMIT_EXIT").is_some() {
            // Reproduce the partial durable state created by older clients.
            self.db.write_daily_identity(&state).await?;
            std::process::exit(73);
        }
        self.db.persist_enrolled_identity(&state).await?;
        Ok(state.deployment_id)
    }

    /// Enroll if needed, then get the first sync credential. If the saved key
    /// is expired or refused, enroll again with a configured code this
    /// operator has not used yet. So `helm upgrade` with a new code recovers
    /// an operator whose key expired while it was down, or was replaced.
    pub(crate) async fn connect(
        self: Arc<Self>,
        request: EnrollmentRequest<'_>,
    ) -> Result<(String, String)> {
        let deployment_id = self.enroll(request).await?;
        let error = match self.clone().sync_token(&request, &deployment_id).await {
            Ok(token) => return Ok((deployment_id, token)),
            Err(error) => error,
        };
        let Some(deployment_id) = self.recover(request).await? else {
            return Err(error);
        };
        let token = self.sync_token(&request, &deployment_id).await?;
        Ok((deployment_id, token))
    }

    async fn sync_token(
        self: Arc<Self>,
        request: &EnrollmentRequest<'_>,
        deployment_id: &str,
    ) -> Result<String> {
        alien_client_core::runtime_credentials::RuntimeCredentials::new(
            alien_client_core::runtime_credentials::RuntimeCredentialConfig {
                manager_url: request.manager_url.to_owned(),
                audience: request.audience.to_owned(),
                deployment_id: deployment_id.to_owned(),
                purpose: json!({"type":"operatorSync"}),
            },
            self,
        )
        .context(ErrorData::ConfigurationError {
            message: "Invalid component manager configuration".into(),
        })?
        .token()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Cannot authenticate enrolled component".into(),
        })
    }

    async fn current_key(&self, audience: &str) -> Result<String> {
        let _operation = self.operation.lock().await;
        let mut state = self
            .db
            .read_daily_identity()
            .await?
            .ok_or_else(|| failed("Component identity has not been enrolled"))?;
        if state.audience != audience || state.deployment_id.is_empty() {
            return Err(failed(
                "Component identity is not enrolled for this manager",
            ));
        }
        let now = Utc::now();
        if state.pending.is_none() && state.expires_at - now > chrono::Duration::hours(12) {
            return Ok(state.current);
        }
        // Until the key expires, a manager outage or a failed rotation only
        // delays the swap. The current key keeps working meanwhile.
        let keep_current = |state: &DailyIdentityState, error: AlienError<ErrorData>| {
            if now < state.expires_at {
                warn!(error = %error, "Daily key rotation failed; using the current key");
                Ok(state.current.clone())
            } else {
                Err(error)
            }
        };
        if let Some(pending) = &state.pending {
            let probe = self.client.post(format!("{}/v1/identity/exchange", state.manager_url.trim_end_matches('/')))
                .json(&json!({"deploymentId":state.deployment_id,"purpose":{"type":"telemetry"},"proof":{"type":"dailyKey","key":pending}}))
                .send().await;
            let mut probe = match probe {
                Ok(probe) => probe,
                Err(_) => {
                    return keep_current(&state, failed("Cannot recover pending component key"))
                }
            };
            if probe.status().is_success() {
                let mut bytes = Vec::new();
                while let Some(chunk) = probe
                    .chunk()
                    .await
                    .map_err(|_| failed("Pending key response failed"))?
                {
                    if bytes.len() + chunk.len() > 16 * 1024 {
                        return Err(failed("Pending key response exceeds limit"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let response: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| failed("Invalid pending key response"))?;
                let expires_at = response_expiry(&response)?;
                if response
                    .get("token")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                    || expires_at > Utc::now() + chrono::Duration::hours(1) + ALLOWED_CLOCK_SKEW
                {
                    return Err(failed("Pending key did not obtain bounded authority"));
                }
                let started = state
                    .pending_started_at
                    .ok_or_else(|| failed("Pending key has no durable start time"))?;
                state.current = pending.clone();
                state.pending = None;
                state.pending_started_at = None;
                // A lost rotation response cannot extend the locally remembered
                // deadline. The server independently checks its exact deadline.
                state.expires_at = started + chrono::Duration::hours(24);
                self.db.write_daily_identity(&state).await?;
                return Ok(state.current);
            }
            if probe.status().as_u16() != 401 {
                return keep_current(
                    &state,
                    failed("Pending component key verification is unavailable"),
                );
            }
        } else {
            // After a manager outage the manager still rotates the latest key
            // for a week past its expiry. Rotation stays one-use, so a copy of
            // the key still loses the race and is detected.
            if now >= state.expires_at + ROTATION_GRACE {
                return Err(failed("Component identity expired; enroll a replacement"));
            }
            state.pending = Some(new_key()?);
            state.pending_started_at = Some(now);
            self.db.write_daily_identity(&state).await?;
        }
        let pending = state
            .pending
            .as_ref()
            .ok_or_else(|| failed("Pending key was not saved"))?;
        let response = match self.post(&state, "rotate", json!({"deploymentId":state.deployment_id,"key":state.current,"successorHash":key_hash(pending)})).await {
            Ok(response) => response,
            Err(error) => return keep_current(&state, error),
        };
        state.expires_at = response_expiry(&response)?;
        state.current = pending.clone();
        state.pending = None;
        state.pending_started_at = None;
        self.db.write_daily_identity(&state).await?;
        Ok(state.current)
    }

    async fn post(
        &self,
        state: &DailyIdentityState,
        operation: &str,
        body: Value,
    ) -> Result<Value> {
        let mut response = self
            .client
            .post(format!(
                "{}/v1/identity/{operation}",
                state.manager_url.trim_end_matches('/')
            ))
            .json(&body)
            .send()
            .await
            .map_err(|_| failed("Component identity request failed"))?;
        if !response.status().is_success() {
            return Err(failed(&format!(
                "Component identity request refused (HTTP {})",
                response.status().as_u16()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failed("Component identity response failed"))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 {
                return Err(failed("Component identity response exceeds limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| failed("Invalid component identity response"))
    }
}

#[async_trait]
impl RuntimeProofProvider for DailyIdentity {
    async fn proof(&self, audience: &str) -> alien_client_core::Result<Value> {
        let key = self.current_key(audience).await.context(
            alien_client_core::ErrorData::AuthenticationError {
                message: "Daily component identity cannot establish authority".into(),
            },
        )?;
        Ok(json!({"type":"dailyKey","key":key}))
    }
}

fn check_saved_identity(
    state: &DailyIdentityState,
    manager_url: &str,
    audience: &str,
    deployment_id: Option<&str>,
) -> Result<()> {
    if state.manager_url != manager_url || state.audience != audience {
        return Err(failed(
            "Saved component identity belongs to another manager",
        ));
    }
    if deployment_id
        .is_some_and(|expected| !state.deployment_id.is_empty() && expected != state.deployment_id)
    {
        return Err(failed("Saved component belongs to another deployment"));
    }
    Ok(())
}

fn key_hash(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}
fn new_key() -> Result<String> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|_| failed("Cannot generate a component key"))?;
    Ok(format!(
        "ax_day_v1_{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}
fn response_expiry(response: &Value) -> Result<DateTime<Utc>> {
    let expiry = response
        .get("expiresAt")
        .and_then(Value::as_str)
        .ok_or_else(|| failed("Component identity response has no deadline"))?;
    let expiry = DateTime::parse_from_rfc3339(expiry)
        .map_err(|_| failed("Invalid component identity deadline"))?
        .with_timezone(&Utc);
    if expiry <= Utc::now()
        || expiry > Utc::now() + chrono::Duration::hours(24) + ALLOWED_CLOCK_SKEW
    {
        return Err(failed("Component identity deadline is out of bounds"));
    }
    Ok(expiry)
}
fn failed(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: message.into(),
    })
}

/// Obtain the exact-purpose credential for one operator request. Legacy mode
/// remains explicit while installations migrate.
pub(crate) async fn has_runtime_identity(db: &OperatorDb) -> Result<bool> {
    Ok(db.read_daily_identity().await?.is_some())
}

pub(crate) async fn request_token(state: &crate::OperatorState, purpose: Value) -> Result<String> {
    if state.db.read_daily_identity().await?.is_none() {
        return state
            .config
            .sync
            .as_ref()
            .map(|sync| sync.token.clone())
            .ok_or_else(|| failed("Operator is not connected"));
    }
    Ok(request_capability(&state.db, purpose).await?.token)
}

pub(crate) struct OperatorRegistryCredentials(pub Arc<OperatorDb>);
#[async_trait]
impl alien_infra::RuntimeRegistryCredentialProvider for OperatorRegistryCredentials {
    async fn runtime_credential_published(
        &self,
        deployment_id: &str,
        purpose: Value,
        credential: alien_client_core::runtime_credentials::RuntimeCapability,
    ) -> alien_infra::Result<()> {
        let invalid = || alien_infra::ErrorData::ResourceConfigInvalid {
            message: "Cannot publish workload runtime authority".into(),
            resource_id: None,
        };
        if self
            .0
            .get_deployment_id()
            .await
            .context(invalid())?
            .as_deref()
            != Some(deployment_id)
        {
            return Err(AlienError::new(invalid()));
        }
        let key = serde_json::to_string(&purpose).map_err(|_| AlienError::new(invalid()))?;
        let mut published = self.0.published_runtime_credentials.lock().await;
        let now = Utc::now();
        if credential.token.is_empty()
            || credential.expires_at <= now
            || credential.expires_at > now + chrono::Duration::hours(1) + ALLOWED_CLOCK_SKEW
        {
            return Err(AlienError::new(invalid()));
        }
        if published
            .get(&key)
            .is_some_and(|existing| existing.expires_at > credential.expires_at)
        {
            return Err(AlienError::new(invalid()));
        }
        published.insert(key, credential);
        Ok(())
    }

    async fn runtime_credential(
        &self,
        deployment_id: &str,
        purpose: Value,
    ) -> alien_infra::Result<alien_client_core::runtime_credentials::RuntimeCapability> {
        let actual = self.0.get_deployment_id().await.context(
            alien_infra::ErrorData::ResourceConfigInvalid {
                message: "Cannot resolve component deployment".into(),
                resource_id: None,
            },
        )?;
        if actual.as_deref() != Some(deployment_id) {
            return Err(AlienError::new(
                alien_infra::ErrorData::ResourceConfigInvalid {
                    message: "Runtime authority belongs to another deployment".into(),
                    resource_id: None,
                },
            ));
        }
        // Keep a published generation stable while the projection converges.
        // A new exchange on every maintenance tick could otherwise outrun it.
        if purpose.get("type").and_then(Value::as_str) == Some("receiveCommands") {
            if let Ok(published) = published_capability(&self.0, &purpose).await {
                if published.expires_at > Utc::now() + chrono::Duration::minutes(2) {
                    return Ok(published);
                }
            }
        }
        request_capability(&self.0, purpose).await.context(
            alien_infra::ErrorData::ResourceConfigInvalid {
                message: "Cannot obtain bounded runtime authority".into(),
                resource_id: None,
            },
        )
    }

    async fn registry_pull_credential(
        &self,
        deployment_id: &str,
    ) -> alien_infra::Result<alien_client_core::runtime_credentials::RuntimeCapability> {
        self.runtime_credential(deployment_id, json!({"type":"registryPull"}))
            .await
    }
}

/// Read the successfully written generation without acquiring new authority.
/// A failed writer leaves a still-valid generation usable until its hard deadline.
pub(crate) async fn published_capability(
    db: &OperatorDb,
    purpose: &Value,
) -> Result<alien_client_core::runtime_credentials::RuntimeCapability> {
    let key =
        serde_json::to_string(purpose).map_err(|_| failed("Cannot identify workload authority"))?;
    let published = db.published_runtime_credentials.lock().await;
    let credential = published
        .get(&key)
        .filter(|credential| credential.expires_at > Utc::now())
        .ok_or_else(|| {
            failed("Workload runtime authority has not been published or has expired")
        })?;
    Ok(credential.clone())
}

pub(crate) async fn request_capability(
    db: &Arc<OperatorDb>,
    purpose: Value,
) -> Result<alien_client_core::runtime_credentials::RuntimeCapability> {
    request_credentials(db, purpose)
        .await?
        .capability()
        .await
        .context(ErrorData::SyncFailed {
            message: "Cannot refresh operator authorization".into(),
        })
}

async fn request_credentials(
    db: &Arc<OperatorDb>,
    purpose: Value,
) -> Result<Arc<alien_client_core::runtime_credentials::RuntimeCredentials>> {
    let identity = db
        .read_daily_identity()
        .await?
        .ok_or_else(|| failed("Component identity is required"))?;
    let (manager_url, audience, deployment_id) = (
        identity.manager_url,
        identity.audience,
        identity.deployment_id,
    );
    let cache_key = serde_json::to_string(&json!({"managerUrl":manager_url,"audience":audience,"deploymentId":deployment_id,"purpose":purpose}))
        .map_err(|_| failed("Cannot identify runtime credential scope"))?;
    let credentials = {
        let mut cache = db.runtime_credentials.lock().await;
        if let Some(credentials) = cache.get(&cache_key) {
            credentials.clone()
        } else {
            let proof: Arc<dyn RuntimeProofProvider> = Arc::new(DailyIdentity::new(db.clone())?);
            let credentials = Arc::new(
                alien_client_core::runtime_credentials::RuntimeCredentials::new(
                    alien_client_core::runtime_credentials::RuntimeCredentialConfig {
                        manager_url,
                        audience,
                        deployment_id,
                        purpose,
                    },
                    proof,
                )
                .context(ErrorData::ConfigurationError {
                    message: "Invalid component exchange configuration".into(),
                })?,
            );
            cache.insert(cache_key, credentials.clone());
            credentials
        }
    };
    Ok(credentials)
}

/// Validate advertised manager endpoints before reading any authority.
pub(crate) fn validate_manager_endpoint(
    state: &crate::OperatorState,
    endpoint: &reqwest::Url,
) -> Result<()> {
    let sync = state
        .config
        .sync
        .as_ref()
        .ok_or_else(|| failed("Operator is not connected"))?;
    let mut normalized = endpoint.clone();
    if normalized.scheme() == "wss" {
        normalized
            .set_scheme("https")
            .map_err(|_| failed("Invalid tunnel scheme"))?;
    }
    if normalized.scheme() == "ws" {
        normalized
            .set_scheme("http")
            .map_err(|_| failed("Invalid tunnel scheme"))?;
    }
    if !normalized.username().is_empty()
        || normalized.password().is_some()
        || normalized.origin() != sync.url.origin()
    {
        return Err(failed("Manager request targets an unauthorized origin"));
    }
    Ok(())
}

/// A rejected generation is replaced once; concurrent callers share the winner.
pub(crate) async fn send_manager_request(
    state: &crate::OperatorState,
    purpose: Value,
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let (client, request) = builder.build_split();
    let mut request = request.map_err(|_| failed("Cannot construct manager request"))?;
    if state.db.read_daily_identity().await?.is_none() {
        // A legacy Operator's client already carries its sync token.
        return client
            .execute(request)
            .await
            .into_alien_error()
            .context(ErrorData::SyncFailed {
                message: "Failed to send manager request".into(),
            });
    }
    validate_manager_endpoint(state, request.url())?;
    let credentials = request_credentials(&state.db, purpose.clone()).await?;
    if !credentials.allows_endpoint(request.url().as_str()) {
        return Err(failed(
            "Manager request differs from enrolled identity origin",
        ));
    }
    let token = request_capability(&state.db, purpose).await?.token;
    request.headers_mut().insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .map_err(|_| failed("Invalid manager authorization"))?,
    );
    let retry = request
        .try_clone()
        .ok_or_else(|| failed("Manager request cannot be retried safely"))?;
    let response = client
        .execute(request)
        .await
        .map_err(|_| failed("Manager request transport failed"))?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(response);
    }
    let replacement =
        credentials
            .token_after_rejection(&token)
            .await
            .context(ErrorData::SyncFailed {
                message: "Cannot renew rejected operator authorization".into(),
            })?;
    let mut retry = retry;
    retry.headers_mut().insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {replacement}")
            .parse()
            .map_err(|_| failed("Invalid manager authorization"))?,
    );
    let response = client
        .execute(retry)
        .await
        .map_err(|_| failed("Manager retry transport failed"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        credentials.reject(&replacement).await;
    }
    Ok(response)
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use std::process::Command;

    /// Invoked only by the parent test, with a fresh OS process and DB handle.
    #[tokio::test]
    #[ignore = "subprocess crash fixture"]
    async fn durable_pending_child() {
        let directory = std::env::var("ALIEN_TEST_DAILY_DIRECTORY").unwrap();
        let db = OperatorDb::new(&directory, &"a".repeat(64)).await.unwrap();
        let state = DailyIdentityState {
            manager_url: "https://manager.example".into(),
            audience: "manager-one".into(),
            deployment_id: "deployment-one".into(),
            current: format!("ax_day_v1_{}", "1".repeat(64)),
            pending: Some(format!("ax_day_v1_{}", "2".repeat(64))),
            pending_started_at: Some(Utc::now()),
            enrolled_code_hash: None,
            pending_enrollment: None,
            expires_at: Utc::now() + chrono::Duration::hours(12),
        };
        db.write_daily_identity(&state).await.unwrap();
        // Simulate loss immediately before the remote CAS without graceful
        // database close, checkpoint, or Rust destructors.
        std::process::exit(0);
    }

    #[tokio::test]
    async fn pending_successor_survives_process_loss_before_server_swap() {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let path = directory.path().to_owned();
        let result = tokio::task::spawn_blocking(move || {
            Command::new(executable)
                .args([
                    "--exact",
                    "daily_identity::persistence_tests::durable_pending_child",
                    "--ignored",
                ])
                .env("ALIEN_TEST_DAILY_DIRECTORY", path)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            result.status.success(),
            "subprocess must commit and exit successfully"
        );
        let db = OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
            .await
            .unwrap();
        let state = db
            .read_daily_identity()
            .await
            .unwrap()
            .expect("identity must survive process loss");
        assert!(state.current.ends_with(&"1".repeat(64)));
        assert!(state
            .pending
            .as_ref()
            .is_some_and(|key| key.ends_with(&"2".repeat(64))));
        assert!(state.pending_started_at.is_some());
        assert_eq!(state.deployment_id, "deployment-one");
    }

    /// A manager that knows one current key per deployment, like the real
    /// one: exchange answers 401 for any other key, and a code enrolls once.
    #[derive(Clone)]
    struct RecoveryManager {
        current: Arc<Mutex<String>>,
        codes: Arc<Mutex<std::collections::HashMap<String, Option<String>>>>,
        enrolls: Arc<std::sync::atomic::AtomicUsize>,
        /// Answer rotations with 503, as during a manager outage.
        rotate_unavailable: Arc<std::sync::atomic::AtomicBool>,
    }

    /// Rotates the latest key once, like the manager inside its grace week.
    async fn recovery_rotate(
        axum::extract::State(manager): axum::extract::State<RecoveryManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if manager
            .rotate_unavailable
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let mut current = manager.current.lock().await;
        if key_hash(body["key"].as_str().unwrap()) != *current {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        *current = body["successorHash"].as_str().unwrap().to_owned();
        axum::Json(json!({
            "expiresAt": Utc::now() + chrono::Duration::hours(24),
            "revision": 3,
        }))
        .into_response()
    }

    async fn recovery_enroll(
        axum::extract::State(manager): axum::extract::State<RecoveryManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        manager
            .enrolls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let hash = body["initialKeyHash"].as_str().unwrap().to_owned();
        let mut codes = manager.codes.lock().await;
        match codes.get_mut(body["code"].as_str().unwrap()) {
            Some(used @ None) => *used = Some(hash.clone()),
            Some(Some(used)) if *used == hash => {}
            _ => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
        }
        *manager.current.lock().await = hash;
        axum::Json(json!({
            "deploymentId": "deployment-one",
            "revision": 2,
            "expiresAt": Utc::now() + chrono::Duration::hours(24),
        }))
        .into_response()
    }

    async fn recovery_exchange(
        axum::extract::State(manager): axum::extract::State<RecoveryManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if key_hash(body["proof"]["key"].as_str().unwrap()) != *manager.current.lock().await {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        axum::Json(json!({
            "token": "ax_cap_v1_recovered",
            "expiresAt": Utc::now() + chrono::Duration::hours(1),
        }))
        .into_response()
    }

    async fn recovery_fixture(
        expires_at: DateTime<Utc>,
    ) -> (
        Arc<OperatorDb>,
        DailyIdentityState,
        RecoveryManager,
        tempfile::TempDir,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let manager = RecoveryManager {
            // Someone else holds the deployment's key now.
            current: Arc::new(Mutex::new(key_hash("ax_day_v1_other"))),
            codes: Arc::new(Mutex::new(
                [
                    ("ax_enroll_original".to_owned(), Some("spent".to_owned())),
                    ("ax_enroll_new".to_owned(), None),
                ]
                .into(),
            )),
            enrolls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            rotate_unavailable: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let app = axum::Router::new()
            .route("/v1/identity/enroll", axum::routing::post(recovery_enroll))
            .route(
                "/v1/identity/exchange",
                axum::routing::post(recovery_exchange),
            )
            .route("/v1/identity/rotate", axum::routing::post(recovery_rotate))
            .with_state(manager.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        let state = DailyIdentityState {
            manager_url: url,
            audience: "manager-one".into(),
            deployment_id: "deployment-one".into(),
            current: format!("ax_day_v1_{}", "1".repeat(64)),
            pending: None,
            pending_started_at: None,
            enrolled_code_hash: Some(key_hash("ax_enroll_original")),
            pending_enrollment: None,
            expires_at,
        };
        db.write_daily_identity(&state).await.unwrap();
        db.persist_enrolled_identity(&state).await.unwrap();
        (db, state, manager, directory)
    }

    fn request<'a>(state: &'a DailyIdentityState, code: &'a str) -> EnrollmentRequest<'a> {
        EnrollmentRequest {
            manager_url: &state.manager_url,
            audience: &state.audience,
            code,
            deployment_id: Some("deployment-one"),
        }
    }

    #[tokio::test]
    async fn new_code_recovers_an_operator_whose_key_expired_or_was_refused() {
        for expires_at in [
            // Down for longer than the key's life.
            Utc::now() - chrono::Duration::hours(1),
            // Locally valid, but the manager refuses it.
            Utc::now() + chrono::Duration::hours(20),
        ] {
            let (db, state, manager, _directory) = recovery_fixture(expires_at).await;
            let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
            // Restarting with the spent code cannot recover.
            assert!(identity
                .clone()
                .connect(request(&state, "ax_enroll_original"))
                .await
                .is_err());
            // `helm upgrade` with a new code recovers on the next start.
            let (deployment, token) = identity
                .clone()
                .connect(request(&state, "ax_enroll_new"))
                .await
                .unwrap();
            assert_eq!(deployment, "deployment-one");
            assert_eq!(token, "ax_cap_v1_recovered");
            let saved = db.read_daily_identity().await.unwrap().unwrap();
            assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
            assert_eq!(
                saved.enrolled_code_hash.as_deref(),
                Some(key_hash("ax_enroll_new").as_str())
            );
            // Later restarts keep the new key and do not send the code again.
            let enrolls = manager.enrolls.load(std::sync::atomic::Ordering::SeqCst);
            identity
                .clone()
                .connect(request(&state, "ax_enroll_new"))
                .await
                .unwrap();
            assert_eq!(
                manager.enrolls.load(std::sync::atomic::Ordering::SeqCst),
                enrolls
            );
        }
    }

    /// The manager was down for two days, so the key expired. Its key is still
    /// the latest issued, so the operator rotates it on restart without a new
    /// code. After the grace week it needs a new code.
    #[tokio::test]
    async fn an_operator_rotates_its_expired_key_after_a_manager_outage() {
        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() - chrono::Duration::days(2)).await;
        *manager.current.lock().await = key_hash(&state.current);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        let (_, token) = identity
            .clone()
            .connect(request(&state, "ax_enroll_original"))
            .await
            .expect("the operator rotates within the grace week");
        assert_eq!(token, "ax_cap_v1_recovered");
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert!(saved.current != state.current);
        assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
        assert!(saved.expires_at > Utc::now() + chrono::Duration::hours(23));

        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() - chrono::Duration::days(8)).await;
        *manager.current.lock().await = key_hash(&state.current);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        assert!(identity
            .connect(request(&state, "ax_enroll_original"))
            .await
            .is_err());
    }

    /// The manager cannot rotate for hours. The key still has six hours, so
    /// the operator keeps working with it and does not need a new code.
    #[tokio::test]
    async fn a_failed_rotation_does_not_stop_a_key_that_still_works() {
        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() + chrono::Duration::hours(6)).await;
        *manager.current.lock().await = key_hash(&state.current);
        manager
            .rotate_unavailable
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        let (_, token) = identity
            .clone()
            .connect(request(&state, "ax_enroll_original"))
            .await
            .expect("the current key keeps working");
        assert_eq!(token, "ax_cap_v1_recovered");
        // The next rotation succeeds once the manager answers again.
        manager
            .rotate_unavailable
            .store(false, std::sync::atomic::Ordering::SeqCst);
        identity.current_key("manager-one").await.unwrap();
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert!(saved.current != state.current);
        assert!(saved.pending.is_none());
        assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
    }

    #[tokio::test]
    async fn a_working_key_ignores_a_new_code() {
        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() + chrono::Duration::hours(20)).await;
        *manager.current.lock().await = key_hash(&state.current);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        identity
            .connect(request(&state, "ax_enroll_new"))
            .await
            .unwrap();
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert!(saved.current == state.current);
    }

    #[derive(Clone)]
    struct LostResponseServer {
        hash: Arc<Mutex<String>>,
        rotations: Arc<std::sync::atomic::AtomicUsize>,
        probes: Arc<std::sync::atomic::AtomicUsize>,
    }

    async fn rotate_without_response(
        axum::extract::State(state): axum::extract::State<LostResponseServer>,
        axum::Json(request): axum::Json<Value>,
    ) -> axum::response::Response {
        let mut hash = state.hash.lock().await;
        assert_eq!(key_hash(request["key"].as_str().unwrap()), *hash);
        *hash = request["successorHash"].as_str().unwrap().to_string();
        state
            .rotations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // The server committed the replacement, but the response stream fails.
        http::Response::builder()
            .status(200)
            .body(axum::body::Body::from_stream(futures::stream::once(
                async {
                    Err::<axum::body::Bytes, _>(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "fixture response lost",
                    ))
                },
            )))
            .unwrap()
    }

    async fn probe_successor(
        axum::extract::State(state): axum::extract::State<LostResponseServer>,
        axum::Json(request): axum::Json<Value>,
    ) -> axum::Json<Value> {
        assert_eq!(request["purpose"]["type"], "telemetry");
        assert_eq!(
            key_hash(request["proof"]["key"].as_str().unwrap()),
            *state.hash.lock().await
        );
        state
            .probes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        axum::Json(
            json!({"token":"ax_cap_v1_recovery-fixture", "expiresAt":Utc::now() + chrono::Duration::minutes(5)}),
        )
    }

    #[tokio::test]
    async fn saved_successor_recovers_after_server_swap_and_response_loss() {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let initial = format!("ax_day_v1_{}", "1".repeat(64));
        let fixture = LostResponseServer {
            hash: Arc::new(Mutex::new(key_hash(&initial))),
            rotations: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            probes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let app = axum::Router::new()
            .route(
                "/v1/identity/rotate",
                axum::routing::post(rotate_without_response),
            )
            .route(
                "/v1/identity/exchange",
                axum::routing::post(probe_successor),
            )
            .with_state(fixture.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        {
            let db = OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap();
            db.write_daily_identity(&DailyIdentityState {
                manager_url: server_url,
                audience: "manager-one".into(),
                deployment_id: "deployment-one".into(),
                current: initial,
                pending: None,
                pending_started_at: None,
                enrolled_code_hash: None,
                pending_enrollment: None,
                expires_at: Utc::now() + chrono::Duration::hours(11),
            })
            .await
            .unwrap();
        }
        // Each attempt reopens the database, as a restarted Operator does.
        let attempt = |directory: std::path::PathBuf| async move {
            let db = Arc::new(
                OperatorDb::new(directory.to_str().unwrap(), &"a".repeat(64))
                    .await
                    .unwrap(),
            );
            let result = DailyIdentity::new(db.clone())
                .unwrap()
                .proof("manager-one")
                .await;
            let pending = db.read_daily_identity().await.unwrap().unwrap().pending;
            (result.is_ok(), pending.is_some())
        };
        // The server swapped keys but the response was lost: the saved
        // successor is not promoted yet and the old key is not rotated again.
        // The old key has not expired, so this attempt still offers it; the
        // manager refuses it and the next attempt checks the successor.
        assert_eq!(attempt(directory.path().to_owned()).await, (true, true));
        assert_eq!(
            fixture.rotations.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(fixture.probes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(attempt(directory.path().to_owned()).await, (true, false));
        assert_eq!(
            fixture.rotations.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(fixture.probes.load(std::sync::atomic::Ordering::SeqCst), 1);
        let db = OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
            .await
            .unwrap();
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert_eq!(key_hash(&saved.current), *fixture.hash.lock().await);
        assert!(saved.pending.is_none());
        assert!(saved.expires_at <= Utc::now() + chrono::Duration::hours(24));
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    /// One enrollment replacement attempt from a freshly opened database.
    async fn replacement_attempt(directory: &std::path::Path, mode: &str) {
        let db = Arc::new(
            OperatorDb::new(directory.to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        let before = db.read_daily_identity().await.unwrap().unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        let result = identity
            .recover(EnrollmentRequest {
                manager_url: &before.manager_url,
                audience: &before.audience,
                code: "replacement-code-fixture",
                deployment_id: Some("deployment-one"),
            })
            .await;
        let after = db.read_daily_identity().await.unwrap().unwrap();
        match mode {
            "lost" => {
                assert!(result.is_err());
                assert!(
                    after.current == before.current,
                    "lost response must preserve prior identity until verified retry"
                );
                assert!(after.pending_enrollment.is_some());
            }
            "recover" => {
                assert_eq!(result.unwrap().as_deref(), Some("deployment-one"));
                assert!(after.pending_enrollment.is_none());
            }
            _ => {
                // The code is spent: nothing is sent and nothing changes.
                assert!(result.unwrap().is_none());
                assert!(after.current == before.current);
            }
        }
    }

    #[tokio::test]
    async fn replacement_survives_response_loss_and_never_reuses_the_code() {
        let directory = tempfile::tempdir().unwrap();
        let hash = Arc::new(Mutex::new(String::new()));
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_hash = hash.clone();
        let server_requests = requests.clone();
        let deadline = Utc::now() + chrono::Duration::hours(23);
        let app = axum::Router::new().route("/v1/identity/enroll", axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let hash = server_hash.clone();
            let requests = server_requests.clone();
            async move {
                assert_eq!(body["code"], "replacement-code-fixture");
                let mut hash = hash.lock().await;
                let candidate = body["initialKeyHash"].as_str().unwrap();
                let first = requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
                if first {
                    *hash = candidate.to_owned();
                    http::Response::builder().status(200).body(axum::body::Body::from_stream(futures::stream::once(async {
                        Err::<axum::body::Bytes, _>(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "fixture response lost"))
                    }))).unwrap()
                } else {
                    assert!(candidate == hash.as_str(), "retry must use the same durably saved key hash");
                    http::Response::builder().status(200).header("content-type", "application/json")
                        .body(axum::body::Body::from(json!({"deploymentId":"deployment-one", "revision":3, "expiresAt":deadline}).to_string())).unwrap()
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        {
            let db = OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap();
            db.write_daily_identity(&DailyIdentityState {
                manager_url: url,
                audience: "manager-one".into(),
                deployment_id: "deployment-one".into(),
                current: format!("ax_day_v1_{}", "1".repeat(64)),
                pending: None,
                pending_started_at: None,
                expires_at: Utc::now() - chrono::Duration::seconds(1),
                enrolled_code_hash: None,
                pending_enrollment: None,
            })
            .await
            .unwrap();
        }
        for mode in ["lost", "recover", "repeat"] {
            replacement_attempt(directory.path(), mode).await;
        }
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);
        let db = OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
            .await
            .unwrap();
        let state = db.read_daily_identity().await.unwrap().unwrap();
        assert!(key_hash(&state.current) == *hash.lock().await);
        assert_eq!(state.expires_at, deadline);
        assert!(state.pending_enrollment.is_none());
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn replacement_refusal_and_wrong_deployment_preserve_saved_identity() {
        let directory = tempfile::tempdir().unwrap();
        let status = Arc::new(std::sync::atomic::AtomicUsize::new(401));
        let server_status = status.clone();
        let app = axum::Router::new().route("/v1/identity/enroll", axum::routing::post(move || {
            let status = server_status.load(std::sync::atomic::Ordering::SeqCst);
            async move { (
                axum::http::StatusCode::from_u16(status as u16).unwrap(),
                axum::Json(json!({"deploymentId":"different-deployment", "revision":3, "expiresAt":Utc::now()+chrono::Duration::hours(23)})),
            ) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        let original = DailyIdentityState {
            manager_url: url.clone(),
            audience: "manager-one".into(),
            deployment_id: "deployment-one".into(),
            current: format!("ax_day_v1_{}", "1".repeat(64)),
            pending: None,
            pending_started_at: None,
            expires_at: Utc::now() + chrono::Duration::hours(1),
            enrolled_code_hash: None,
            pending_enrollment: None,
        };
        db.write_daily_identity(&original).await.unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        for code in [401, 200] {
            status.store(code, std::sync::atomic::Ordering::SeqCst);
            let error = identity
                .recover(EnrollmentRequest {
                    manager_url: &url,
                    audience: "manager-one",
                    code: "replacement-fixture",
                    deployment_id: Some("deployment-one"),
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, "CONFIGURATION_ERROR");
            let saved = db.read_daily_identity().await.unwrap().unwrap();
            assert!(saved.current == original.current);
            assert_eq!(saved.deployment_id, original.deployment_id);
            assert_eq!(saved.expires_at, original.expires_at);
            assert!(saved.enrolled_code_hash.is_none());
            assert!(saved.pending_enrollment.is_some());
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
