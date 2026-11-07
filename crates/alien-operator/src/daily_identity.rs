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

use crate::{
    db::OperatorDb,
    error::{ErrorData, Result},
};

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
    #[serde(default)]
    pub enrollment_revision: u64,
    #[serde(default)]
    pub pending_enrollment: Option<PendingEnrollment>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingEnrollment {
    revision: u64,
    code_hash: String,
    key: String,
}

pub(crate) struct EnrollmentRequest<'a> {
    pub manager_url: &'a str,
    pub audience: &'a str,
    pub code: &'a str,
    pub deployment_id: Option<&'a str>,
    pub revision: u64,
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

    /// Save the initial key before enrollment so a lost response can be retried
    /// with the same key hash and one-use code.
    pub async fn enroll(&self, request: EnrollmentRequest<'_>) -> Result<String> {
        let EnrollmentRequest {
            manager_url,
            audience,
            code,
            deployment_id,
            revision,
        } = request;
        let expected_revision = revision
            .checked_add(1)
            .ok_or_else(|| failed("Enrollment revision exceeds supported range"))?;
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
        let mut state = match self.db.read_daily_identity().await? {
            Some(state) => {
                if state.manager_url != manager_url || state.audience != audience {
                    return Err(failed(
                        "Saved component identity belongs to another manager",
                    ));
                }
                if deployment_id.is_some_and(|expected| {
                    !state.deployment_id.is_empty() && expected != state.deployment_id
                }) {
                    return Err(failed("Saved component belongs to another deployment"));
                }
                if revision < state.enrollment_revision {
                    return Err(failed(
                        "Enrollment revision is older than the saved identity",
                    ));
                }
                if !state.deployment_id.is_empty() && revision == state.enrollment_revision {
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
                    enrollment_revision: 0,
                    pending_enrollment: None,
                };
                self.db.write_daily_identity(&state).await?;
                state
            }
        };
        if code.trim().is_empty() {
            return Err(failed("A fresh component enrollment code is required"));
        }
        let replacing = !state.deployment_id.is_empty();
        if replacing {
            let code_hash = key_hash(code);
            match &state.pending_enrollment {
                Some(pending) if pending.revision > revision => {
                    return Err(failed("A newer enrollment replacement is already pending"));
                }
                Some(pending) if pending.revision == revision => {
                    if pending.code_hash != code_hash {
                        return Err(failed("Enrollment code changed for the pending revision"));
                    }
                }
                _ => {
                    state.pending_enrollment = Some(PendingEnrollment {
                        revision,
                        code_hash,
                        key: new_key()?,
                    });
                    self.db.write_daily_identity(&state).await?;
                }
            }
        }
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
        // Issuance increments the server enrollment revision exactly once.
        // An idempotent lost-response retry returns the same revision.
        if revision > 0
            && response.get("revision").and_then(Value::as_u64) != Some(expected_revision)
        {
            return Err(failed(
                "Enrollment response revision does not match this replacement",
            ));
        }
        let expires_at = response_expiry(&response)?;
        state.current = candidate.to_owned();
        state.deployment_id = verified_deployment.to_owned();
        state.expires_at = expires_at;
        state.enrollment_revision = revision;
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
        if let Some(pending) = &state.pending {
            let mut probe = self.client.post(format!("{}/v1/identity/exchange", state.manager_url.trim_end_matches('/')))
                .json(&json!({"deploymentId":state.deployment_id,"purpose":{"type":"telemetry"},"proof":{"type":"dailyKey","key":pending}}))
                .send().await.map_err(|_| failed("Cannot recover pending component key"))?;
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
                return Err(failed("Pending component key verification is unavailable"));
            }
        } else {
            if now >= state.expires_at {
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
        let response = self.post(&state, "rotate", json!({"deploymentId":state.deployment_id,"key":state.current,"successorHash":key_hash(pending)})).await?;
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
            enrollment_revision: 0,
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

    #[tokio::test]
    async fn changed_enrollment_code_alone_preserves_existing_identity() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        let state = DailyIdentityState {
            manager_url: "https://manager.example".into(),
            audience: "manager-one".into(),
            deployment_id: "deployment-one".into(),
            current: format!("ax_day_v1_{}", "1".repeat(64)),
            pending: None,
            pending_started_at: None,
            enrollment_revision: 0,
            pending_enrollment: None,
            expires_at: Utc::now() - chrono::Duration::seconds(1),
        };
        db.write_daily_identity(&state).await.unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        let deployment = identity
            .enroll(EnrollmentRequest {
                manager_url: &state.manager_url,
                audience: &state.audience,
                code: "replacement-code-fixture",
                deployment_id: Some(&state.deployment_id),
                revision: 0,
            })
            .await
            .unwrap();
        assert_eq!(deployment, state.deployment_id);
        let after = db.read_daily_identity().await.unwrap().unwrap();
        assert!(
            after.current == state.current,
            "changing code alone must not silently replace a durable identity"
        );
        assert_eq!(after.expires_at, state.expires_at);
        assert!(
            identity.proof(&state.audience).await.is_err(),
            "expired saved authority cannot recover merely from a changed code"
        );
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
                enrollment_revision: 0,
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
        assert_eq!(attempt(directory.path().to_owned()).await, (false, true));
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
            .enroll(EnrollmentRequest {
                manager_url: &before.manager_url,
                audience: &before.audience,
                code: "replacement-code-fixture",
                deployment_id: Some("deployment-one"),
                revision: if mode == "stale" { 1 } else { 2 },
            })
            .await;
        let after = db.read_daily_identity().await.unwrap().unwrap();
        if mode == "lost" {
            assert!(result.is_err());
            assert!(
                after.current == before.current,
                "lost response must preserve prior identity until verified retry"
            );
            assert!(after.pending_enrollment.is_some());
        } else if mode == "stale" {
            assert!(result.is_err());
            assert!(after.current == before.current);
            assert_eq!(after.enrollment_revision, 2);
        } else {
            assert_eq!(result.unwrap(), "deployment-one");
            assert!(after.pending_enrollment.is_none());
            assert_eq!(after.enrollment_revision, 2);
        }
    }

    #[tokio::test]
    async fn explicit_replacement_survives_response_loss_and_rejects_stale_restart() {
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
                enrollment_revision: 1,
                pending_enrollment: None,
            })
            .await
            .unwrap();
        }
        for mode in ["lost", "recover", "repeat", "stale"] {
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
            enrollment_revision: 1,
            pending_enrollment: None,
        };
        db.write_daily_identity(&original).await.unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        for code in [401, 200] {
            status.store(code, std::sync::atomic::Ordering::SeqCst);
            let error = identity
                .enroll(EnrollmentRequest {
                    manager_url: &url,
                    audience: "manager-one",
                    code: "replacement-fixture",
                    deployment_id: Some("deployment-one"),
                    revision: 2,
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, "CONFIGURATION_ERROR");
            let saved = db.read_daily_identity().await.unwrap().unwrap();
            assert!(saved.current == original.current);
            assert_eq!(saved.deployment_id, original.deployment_id);
            assert_eq!(saved.expires_at, original.expires_at);
            assert_eq!(saved.enrollment_revision, 1);
            assert!(saved.pending_enrollment.is_some());
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
