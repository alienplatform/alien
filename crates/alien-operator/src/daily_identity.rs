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
    /// The key the manager last accepted, kept while the manager refuses the
    /// keys made after it. The refusal may be a revoked row or an outage that
    /// answers 401, so nothing proves the newer keys yet. Cleared once a
    /// newer key works.
    #[serde(default)]
    pub previous: Option<String>,
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
    /// Set when an enrollment request may have reached the manager without its
    /// answer reaching this Operator: a transport failure, a server error or an
    /// unreadable answer. The saved candidate key is then the only way to
    /// finish the enrollment, so it is kept.
    enrollment_uncertain: std::sync::atomic::AtomicBool,
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
            enrollment_uncertain: std::sync::atomic::AtomicBool::new(false),
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
                    previous: None,
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
        let response = match self
            .post_outcome(
                &state,
                "enroll",
                json!({"code":code,"initialKeyHash":key_hash(candidate)}),
            )
            .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(status)) => {
                if status.is_server_error() || matches!(status.as_u16(), 408 | 429) {
                    self.mark_enrollment_uncertain();
                }
                return Err(failed(&format!(
                    "Component identity request refused (HTTP {})",
                    status.as_u16()
                )));
            }
            Err(error) => {
                self.mark_enrollment_uncertain();
                return Err(error);
            }
        };
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
        state.previous = None;
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
    ///
    /// A working key also uses up a new configured code. Left unused in the
    /// release Secret, the code would let anyone who reads that Secret within
    /// its hour replace this operator's key. A code that no longer works is
    /// ignored, so it never stops a working operator.
    pub(crate) async fn connect(
        self: Arc<Self>,
        request: EnrollmentRequest<'_>,
    ) -> Result<(String, String)> {
        let deployment_id = self.enroll(request).await?;
        let error = match self.clone().sync_token(&request, &deployment_id).await {
            Ok(token) => {
                if let Err(error) = self.recover(request).await {
                    warn!(error = %error, "Configured enrollment code was not used; keeping the current key");
                }
                return Ok((deployment_id, token));
            }
            Err(error) => error,
        };
        let Some(deployment_id) = self.recover(request).await? else {
            return Err(error);
        };
        let token = self.sync_token(&request, &deployment_id).await?;
        Ok((deployment_id, token))
    }

    /// Enroll an Operator that runs with a deployment token, once. It asks
    /// the manager for a one-use code with that token and enrolls with it.
    /// From then on it uses its daily key, and the manager revokes the token
    /// after the first rollout this Operator finishes.
    ///
    /// Returns `false` and keeps the token when the manager issues no code:
    /// an older manager, or one that refuses the token. A failed enrollment
    /// leaves no half-made identity behind, so the token keeps working.
    pub(crate) async fn enroll_with_deployment_token(
        &self,
        manager_url: &str,
        deployment_id: &str,
        token: &str,
    ) -> Result<bool> {
        self.enrollment_uncertain
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if self
            .finish_installed_enrollment(manager_url, deployment_id)
            .await?
        {
            return Ok(true);
        }
        let mut endpoint =
            reqwest::Url::parse(manager_url).map_err(|_| failed("Invalid manager URL"))?;
        endpoint
            .path_segments_mut()
            .map_err(|_| failed("Invalid manager URL"))?
            .pop_if_empty()
            .extend(["v1", "deployments", deployment_id, "enrollment-code"]);
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| failed("Enrollment code request transport failed"))?;
        if matches!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND
                | reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
        ) {
            return Ok(false);
        }
        if !response.status().is_success() {
            return Err(failed(&format!(
                "Enrollment code request failed with {}",
                response.status()
            )));
        }
        let issued: Value = response
            .json()
            .await
            .map_err(|_| failed("Invalid enrollment code response"))?;
        let field = |name: &str| {
            issued
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| failed("Invalid enrollment code response"))
        };
        let (code, audience) = (field("code")?, field("managerAudience")?);
        let enrolled = self
            .enroll(EnrollmentRequest {
                manager_url,
                audience: &audience,
                code: &code,
                deployment_id: Some(deployment_id),
            })
            .await;
        if let Err(error) = enrolled {
            // After an uncertain answer the manager may have installed the
            // key, and the saved candidate is what proves it. Only a refusal
            // the manager confirmed leaves nothing to keep.
            if !self
                .enrollment_uncertain
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                self.db.clear_unenrolled_daily_identity().await?;
            }
            return Err(error);
        }
        Ok(true)
    }

    fn mark_enrollment_uncertain(&self) {
        self.enrollment_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// An earlier attempt may have enrolled with the manager and lost the
    /// answer, leaving only its candidate key saved. The manager then refuses
    /// another token-made code, since the deployment has a key, but it accepts
    /// the candidate if it installed it. Prove it through the identity
    /// exchange and finish the enrollment. Returns `false` when there is no
    /// candidate, or the manager confirms it never installed it (the candidate
    /// is dropped and a new enrollment can start). An unreachable manager is an
    /// error and keeps the candidate.
    async fn finish_installed_enrollment(
        &self,
        manager_url: &str,
        deployment_id: &str,
    ) -> Result<bool> {
        let _operation = self.operation.lock().await;
        let Some(mut state) = self.db.read_daily_identity().await? else {
            return Ok(false);
        };
        if !state.deployment_id.is_empty() {
            return Ok(false);
        }
        if state.manager_url != manager_url {
            self.db.clear_unenrolled_daily_identity().await?;
            return Ok(false);
        }
        let probe = self
            .post_outcome(
                &state,
                "exchange",
                json!({"deploymentId":deployment_id,"purpose":{"type":"telemetry"},"proof":{"type":"dailyKey","key":state.current}}),
            )
            .await?;
        match probe {
            Ok(response) => {
                let expires_at = response_expiry(&response)?;
                if response
                    .get("token")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                    || expires_at > Utc::now() + chrono::Duration::hours(1) + ALLOWED_CLOCK_SKEW
                {
                    return Err(failed(
                        "Saved enrollment key did not obtain bounded authority",
                    ));
                }
                // The deadline of the key itself is not known here. Treat it as
                // due, so the first use rotates it, which the manager allows
                // until a week after it ended.
                state.deployment_id = deployment_id.to_owned();
                state.expires_at = Utc::now();
                state.enrolled_code_hash = None;
                state.pending_enrollment = None;
                state.previous = None;
                state.pending = None;
                state.pending_started_at = None;
                self.db.persist_enrolled_identity(&state).await?;
                Ok(true)
            }
            Err(status) if matches!(status.as_u16(), 401 | 403 | 404) => {
                self.db.clear_unenrolled_daily_identity().await?;
                Ok(false)
            }
            Err(status) => Err(failed(&format!(
                "Saved enrollment key could not be verified (HTTP {})",
                status.as_u16()
            ))),
        }
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
        let had_pending = state.pending.is_some();
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
                state.previous = None;
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
        let mut pending = state
            .pending
            .clone()
            .ok_or_else(|| failed("Pending key was not saved"))?;
        let rotation = json!({"deploymentId":state.deployment_id,"key":state.current,"successorHash":key_hash(&pending)});
        let mut outcome = match self.post_outcome(&state, "rotate", rotation).await {
            Ok(outcome) => outcome,
            Err(error) => return keep_current(&state, error),
        };
        if had_pending && matches!(outcome, Err(status) if status.as_u16() == 401) {
            // The manager refused the current key, so it may have replaced it:
            // an earlier rotation committed and its answer was lost. The saved
            // successor is then the latest installed key, and it still rotates
            // for a week after it ended, though its exchange no longer works.
            // Make it the current key, save a new successor, and rotate again.
            // The refused key stays saved as `previous`: a revoked row refuses
            // it too, and nothing proves the successor until it rotates.
            // All keys are on disk before the request is sent.
            let installed_at = state.pending_started_at.unwrap_or(now);
            if state.previous.is_none() {
                state.previous = Some(state.current.clone());
            }
            state.current = state
                .pending
                .take()
                .ok_or_else(|| failed("Pending key was not saved"))?;
            pending = new_key()?;
            state.pending = Some(pending.clone());
            state.pending_started_at = Some(now);
            state.expires_at = installed_at + chrono::Duration::hours(24);
            self.db.write_daily_identity(&state).await?;
            warn!("Component key was refused; rotating with the saved successor");
            let rotation = json!({"deploymentId":state.deployment_id,"key":state.current,"successorHash":key_hash(&pending)});
            outcome = match self.post_outcome(&state, "rotate", rotation).await {
                Ok(outcome) => outcome,
                Err(error) => return keep_current(&state, error),
            };
        }
        if matches!(outcome, Err(status) if status.as_u16() == 401) {
            // No newer key works. Try the last key the manager accepted: it
            // rotates again if the refusal was lifted.
            if let Some(previous) = state.previous.clone() {
                let rotation = json!({"deploymentId":state.deployment_id,"key":previous,"successorHash":key_hash(&pending)});
                outcome = match self.post_outcome(&state, "rotate", rotation).await {
                    Ok(outcome) => outcome,
                    Err(error) => return keep_current(&state, error),
                };
            }
        }
        let response = match outcome {
            Ok(response) => response,
            Err(status) => {
                return keep_current(
                    &state,
                    failed(&format!(
                        "Component identity request refused (HTTP {})",
                        status.as_u16()
                    )),
                )
            }
        };
        state.expires_at = response_expiry(&response)?;
        state.current = pending.clone();
        state.previous = None;
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
        match self.post_outcome(state, operation, body).await? {
            Ok(value) => Ok(value),
            Err(status) => Err(failed(&format!(
                "Component identity request refused (HTTP {})",
                status.as_u16()
            ))),
        }
    }

    /// Like [`Self::post`], but hands back the refusal status so a caller can
    /// tell a key the manager no longer accepts (401) from an outage.
    async fn post_outcome(
        &self,
        state: &DailyIdentityState,
        operation: &str,
        body: Value,
    ) -> Result<std::result::Result<Value, reqwest::StatusCode>> {
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
            return Ok(Err(response.status()));
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
        serde_json::from_slice(&bytes)
            .map(Ok)
            .map_err(|_| failed("Invalid component identity response"))
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
    Ok(db.read_enrolled_daily_identity().await?.is_some())
}

pub(crate) async fn request_token(state: &crate::OperatorState, purpose: Value) -> Result<String> {
    if state.db.read_enrolled_daily_identity().await?.is_none() {
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
            .is_some_and(|existing| existing.current.expires_at > credential.expires_at)
        {
            return Err(AlienError::new(invalid()));
        }
        let previous = match published.remove(&key) {
            Some(existing) if existing.current.token != credential.token => Some(existing.current),
            Some(existing) => existing.previous,
            None => None,
        };
        published.insert(
            key,
            PublishedCapability {
                current: credential,
                previous,
            },
        );
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

/// The newest written leaf for one purpose and the leaf it replaced. A pod
/// sees a Secret change up to a minute or two after the write, so its runtime
/// may still hold the replaced leaf.
pub(crate) struct PublishedCapability {
    pub(crate) current: alien_client_core::runtime_credentials::RuntimeCapability,
    pub(crate) previous: Option<alien_client_core::runtime_credentials::RuntimeCapability>,
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
        .map(|published| &published.current)
        .filter(|credential| credential.expires_at > Utc::now())
        .ok_or_else(|| {
            failed("Workload runtime authority has not been published or has expired")
        })?;
    Ok(credential.clone())
}

/// The generation the newest published leaf replaced, while it has not expired.
pub(crate) async fn previous_published_capability(
    db: &OperatorDb,
    purpose: &Value,
) -> Option<alien_client_core::runtime_credentials::RuntimeCapability> {
    let key = serde_json::to_string(purpose).ok()?;
    let published = db.published_runtime_credentials.lock().await;
    published
        .get(&key)?
        .previous
        .clone()
        .filter(|credential| credential.expires_at > Utc::now())
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
        .read_enrolled_daily_identity()
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
    if state.db.read_enrolled_daily_identity().await?.is_none() {
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
            previous: None,
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
            previous: None,
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

    /// Issues a code only for the deployment's own token, like the
    /// manager's `/v1/deployments/{id}/enrollment-code`.
    async fn code_for_token(
        axum::extract::State(manager): axum::extract::State<RecoveryManager>,
        axum::extract::Path(deployment): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if deployment != "deployment-one"
            || headers.get("authorization").and_then(|v| v.to_str().ok())
                != Some("Bearer ax_deploy_old")
        {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        manager
            .codes
            .lock()
            .await
            .insert("ax_enroll_from_token".to_owned(), None);
        axum::Json(json!({
            "deploymentId": "deployment-one",
            "code": "ax_enroll_from_token",
            "expiresAt": Utc::now() + chrono::Duration::hours(1),
            "managerAudience": "manager-one",
        }))
        .into_response()
    }

    /// An Operator moved to this image by self-update still has only its
    /// deployment token. It enrolls with it once and then runs on its key.
    /// A refused token or an older manager leaves it on the token, with no
    /// half-made identity that would stop it from starting.
    #[tokio::test]
    async fn a_token_operator_enrolls_with_its_deployment_token_once() {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let manager = RecoveryManager {
            current: Arc::new(Mutex::new(String::new())),
            codes: Arc::new(Mutex::new(Default::default())),
            enrolls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            rotate_unavailable: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let app = axum::Router::new()
            .route(
                "/v1/deployments/{id}/enrollment-code",
                axum::routing::post(code_for_token),
            )
            .route("/v1/identity/enroll", axum::routing::post(recovery_enroll))
            .route(
                "/v1/identity/exchange",
                axum::routing::post(recovery_exchange),
            )
            .with_state(manager.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        db.set_deployment_id("deployment-one").await.unwrap();
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());

        // A token the manager refuses: no code, no saved identity.
        assert!(!identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_other")
            .await
            .unwrap());
        assert!(db.read_daily_identity().await.unwrap().is_none());

        // A manager without the route (an older one) issues no code either.
        let missing = format!("{url}/older");
        assert!(!identity
            .enroll_with_deployment_token(&missing, "deployment-one", "ax_deploy_old")
            .await
            .unwrap());
        assert!(db.read_daily_identity().await.unwrap().is_none());

        // The deployment's own token gets a code and enrolls.
        assert!(identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_old")
            .await
            .unwrap());
        let saved = db.read_daily_identity().await.unwrap().expect("enrolled");
        assert_eq!(saved.deployment_id, "deployment-one");
        assert_eq!(saved.audience, "manager-one");
        assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 1);

        // The next start takes the enrolled path with no code and gets its
        // sync credential from the daily key.
        let (deployment, token) = identity
            .clone()
            .connect(EnrollmentRequest {
                manager_url: &url,
                audience: "manager-one",
                code: "",
                deployment_id: Some("deployment-one"),
            })
            .await
            .unwrap();
        assert_eq!(deployment, "deployment-one");
        assert_eq!(token, "ax_cap_v1_recovered");
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A code that the enroll step refuses leaves no saved key, so the
    /// Operator still starts in token mode.
    #[tokio::test]
    async fn a_refused_token_enrollment_leaves_no_identity() {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/v1/deployments/{id}/enrollment-code",
                axum::routing::post(|| async {
                    axum::Json(json!({"code": "ax_enroll_spent", "managerAudience": "manager-one"}))
                }),
            )
            .route(
                "/v1/identity/enroll",
                axum::routing::post(|| async { axum::http::StatusCode::UNAUTHORIZED }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        db.set_deployment_id("deployment-one").await.unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();

        identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_old")
            .await
            .expect_err("the enroll step is refused");
        assert!(db.read_daily_identity().await.unwrap().is_none());
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

    /// `helm upgrade` put a new code in the release Secret while the key
    /// still works. The operator uses the code up, so a reader of the Secret
    /// cannot enroll with it. A code that does not work changes nothing.
    #[tokio::test]
    async fn a_working_key_uses_up_a_new_code() {
        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() + chrono::Duration::hours(20)).await;
        *manager.current.lock().await = key_hash(&state.current);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        identity
            .clone()
            .connect(request(&state, "ax_enroll_new"))
            .await
            .unwrap();
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert!(saved.current != state.current);
        assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
        assert!(manager.codes.lock().await["ax_enroll_new"].is_some());
        // Restarts do not send the spent code again.
        identity
            .clone()
            .connect(request(&state, "ax_enroll_new"))
            .await
            .unwrap();
        assert_eq!(manager.enrolls.load(std::sync::atomic::Ordering::SeqCst), 1);

        // A code the manager refuses leaves the working key in place.
        let (db, state, manager, _directory) =
            recovery_fixture(Utc::now() + chrono::Duration::hours(20)).await;
        *manager.current.lock().await = key_hash(&state.current);
        let identity = Arc::new(DailyIdentity::new(db.clone()).unwrap());
        identity
            .connect(request(&state, "ax_enroll_expired"))
            .await
            .unwrap();
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert!(saved.current == state.current);
        assert_eq!(key_hash(&saved.current), *manager.current.lock().await);
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
                previous: None,
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
                previous: None,
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
            previous: None,
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

/// A manager that keeps one key per deployment and follows the real rules:
/// exchange needs an unexpired current key; rotation takes the current key
/// until a week after it ended.
#[cfg(test)]
mod grace_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone)]
    struct GraceManager {
        key: Arc<Mutex<(String, DateTime<Utc>)>>,
        rotations: Arc<AtomicUsize>,
        lose_next_answer: Arc<AtomicBool>,
        /// While set, the manager refuses every key, as for a revoked row.
        revoked: Arc<AtomicBool>,
    }

    async fn grace_rotate(
        axum::extract::State(manager): axum::extract::State<GraceManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let mut key = manager.key.lock().await;
        let successor = body["successorHash"].as_str().unwrap().to_owned();
        if manager.revoked.load(Ordering::SeqCst)
            || key_hash(body["key"].as_str().unwrap()) != key.0
            || key.1 <= Utc::now() - ROTATION_GRACE
            || successor == key.0
        {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        let expires_at = Utc::now() + chrono::Duration::hours(24);
        *key = (successor, expires_at);
        manager.rotations.fetch_add(1, Ordering::SeqCst);
        if manager.lose_next_answer.swap(false, Ordering::SeqCst) {
            return http::Response::builder()
                .status(200)
                .body(axum::body::Body::from_stream(futures::stream::once(
                    async {
                        Err::<axum::body::Bytes, _>(std::io::Error::new(
                            std::io::ErrorKind::ConnectionReset,
                            "fixture response lost",
                        ))
                    },
                )))
                .unwrap();
        }
        axum::Json(json!({"expiresAt": expires_at, "revision": 2})).into_response()
    }

    async fn grace_exchange(
        axum::extract::State(manager): axum::extract::State<GraceManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let key = manager.key.lock().await;
        if manager.revoked.load(Ordering::SeqCst)
            || key_hash(body["proof"]["key"].as_str().unwrap()) != key.0
            || key.1 <= Utc::now()
        {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        axum::Json(json!({
            "token": "ax_cap_v1_grace",
            "expiresAt": Utc::now() + chrono::Duration::minutes(5),
        }))
        .into_response()
    }

    async fn fixture(
        server_key: &str,
        server_key_expires: DateTime<Utc>,
    ) -> (Arc<OperatorDb>, String, GraceManager, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let manager = GraceManager {
            key: Arc::new(Mutex::new((key_hash(server_key), server_key_expires))),
            rotations: Arc::new(AtomicUsize::new(0)),
            lose_next_answer: Arc::new(AtomicBool::new(false)),
            revoked: Arc::new(AtomicBool::new(false)),
        };
        let app = axum::Router::new()
            .route("/v1/identity/rotate", axum::routing::post(grace_rotate))
            .route("/v1/identity/exchange", axum::routing::post(grace_exchange))
            .with_state(manager.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        (db, url, manager, directory)
    }

    fn saved_state(
        url: &str,
        current: &str,
        pending: Option<&str>,
        started: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> DailyIdentityState {
        DailyIdentityState {
            manager_url: url.into(),
            audience: "manager-one".into(),
            deployment_id: "deployment-one".into(),
            current: current.into(),
            pending: pending.map(str::to_owned),
            pending_started_at: pending.map(|_| started),
            expires_at,
            enrolled_code_hash: None,
            pending_enrollment: None,
            previous: None,
        }
    }

    fn key(digit: &str) -> String {
        format!("ax_day_v1_{}", digit.repeat(64))
    }

    /// The manager committed a rotation and the answer was lost. The manager
    /// was then unreachable for three days, so the saved successor ended and
    /// its exchange is refused. The old key is replaced. The operator still
    /// holds the latest key inside the seven days and must rotate with it.
    #[tokio::test]
    async fn a_lost_rotation_answer_and_a_long_outage_recover_without_a_code() {
        let now = Utc::now();
        let (db, url, manager, _directory) =
            fixture(&key("1"), now + chrono::Duration::hours(11)).await;
        db.write_daily_identity(&saved_state(
            &url,
            &key("1"),
            None,
            now,
            now + chrono::Duration::hours(11),
        ))
        .await
        .unwrap();
        manager.lose_next_answer.store(true, Ordering::SeqCst);
        let identity = DailyIdentity::new(db.clone()).unwrap();
        // The old key still has hours left, so this attempt keeps using it.
        assert_eq!(identity.current_key("manager-one").await.unwrap(), key("1"));
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        let successor = saved.pending.clone().expect("the successor is saved first");
        assert_eq!(key_hash(&successor), manager.key.lock().await.0);

        // Three days pass: the successor ended two days ago, and so did the
        // old key's local deadline.
        let started = now - chrono::Duration::days(3);
        manager.key.lock().await.1 = now - chrono::Duration::days(2);
        db.write_daily_identity(&saved_state(
            &url,
            &key("1"),
            Some(&successor),
            started,
            started + chrono::Duration::hours(11),
        ))
        .await
        .unwrap();

        let recovered = identity
            .current_key("manager-one")
            .await
            .expect("the latest key rotates inside the seven days");
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert_eq!(saved.current, recovered);
        assert_ne!(recovered, successor);
        assert_ne!(recovered, key("1"));
        assert!(saved.pending.is_none());
        assert!(
            saved.previous.is_none(),
            "the new key worked, so the old one is dropped"
        );
        assert_eq!(key_hash(&recovered), manager.key.lock().await.0);
        assert!(saved.expires_at > Utc::now() + chrono::Duration::hours(23));
        assert_eq!(manager.rotations.load(Ordering::SeqCst), 2);
    }

    /// The manager refuses every key for a while (a revoked row), then accepts
    /// the old key again. Two refusals must not make the operator throw the old
    /// key away: nothing proved the keys it made since, and the old key is the
    /// only one the manager knows.
    #[tokio::test]
    async fn a_refusal_does_not_discard_the_old_key_before_a_new_one_works() {
        let now = Utc::now();
        let (db, url, manager, _directory) =
            fixture(&key("1"), now + chrono::Duration::hours(11)).await;
        db.write_daily_identity(&saved_state(
            &url,
            &key("1"),
            Some(&key("2")),
            now,
            now + chrono::Duration::hours(11),
        ))
        .await
        .unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();

        manager.revoked.store(true, Ordering::SeqCst);
        for _ in 0..2 {
            // The manager answers 401 to every key; the operator keeps working
            // with what it has until its own deadline.
            let _ = identity.current_key("manager-one").await;
        }
        assert_eq!(manager.rotations.load(Ordering::SeqCst), 0);

        manager.revoked.store(false, Ordering::SeqCst);
        identity
            .current_key("manager-one")
            .await
            .expect("the old key is accepted again and rotates");
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert_eq!(
            manager.key.lock().await.0,
            key_hash(&saved.current),
            "the saved current key is the one the manager holds"
        );
        assert_eq!(manager.rotations.load(Ordering::SeqCst), 1);
        assert!(saved.previous.is_none() && saved.pending.is_none());
    }

    /// The rotation never reached the manager, so the old key is still the
    /// latest one. It rotates, and the saved successor is installed by it.
    #[tokio::test]
    async fn an_uninstalled_successor_is_installed_by_the_old_key_within_the_grace() {
        let now = Utc::now();
        let ended = now - chrono::Duration::days(2);
        let (db, url, manager, _directory) = fixture(&key("1"), ended).await;
        db.write_daily_identity(&saved_state(&url, &key("1"), Some(&key("2")), ended, ended))
            .await
            .unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        assert_eq!(identity.current_key("manager-one").await.unwrap(), key("2"));
        assert_eq!(manager.key.lock().await.0, key_hash(&key("2")));
        assert_eq!(manager.rotations.load(Ordering::SeqCst), 1);
    }

    /// Past the seven days no saved key rotates. The operator reports it and
    /// keeps the saved keys, so a new enrollment code can still replace them.
    #[tokio::test]
    async fn no_saved_key_rotates_after_the_seven_days() {
        let now = Utc::now();
        let (db, url, manager, _directory) =
            fixture(&key("2"), now - chrono::Duration::days(9)).await;
        let started = now - chrono::Duration::days(10);
        db.write_daily_identity(&saved_state(
            &url,
            &key("1"),
            Some(&key("2")),
            started,
            started + chrono::Duration::hours(11),
        ))
        .await
        .unwrap();
        let identity = DailyIdentity::new(db.clone()).unwrap();
        identity
            .current_key("manager-one")
            .await
            .expect_err("both keys are past the grace");
        assert_eq!(manager.rotations.load(Ordering::SeqCst), 0);
        assert_eq!(manager.key.lock().await.0, key_hash(&key("2")));
    }
}

/// Enrollment with the deployment token, where the manager installs the key
/// and the reply is lost.
#[cfg(test)]
mod lost_enrollment_tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    const LOST: u8 = 0;
    const REFUSED: u8 = 1;

    #[derive(Clone)]
    struct EnrollManager {
        /// Hash of the key the manager installed, once it has one.
        installed: Arc<Mutex<Option<String>>>,
        /// What the next `enroll` answer is, after the key is installed (or not).
        answer: Arc<AtomicU8>,
    }

    async fn issue_code(
        axum::extract::State(manager): axum::extract::State<EnrollManager>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if manager.installed.lock().await.is_some() {
            return (
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(json!({"code": "ENROLLMENT_KEY_EXISTS", "message": "key exists"})),
            )
                .into_response();
        }
        axum::Json(json!({"code": "ax_enroll_fixture", "managerAudience": "manager-one"}))
            .into_response()
    }

    async fn enroll(
        axum::extract::State(manager): axum::extract::State<EnrollManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if manager.answer.load(Ordering::SeqCst) == REFUSED {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
        *manager.installed.lock().await = Some(body["initialKeyHash"].as_str().unwrap().to_owned());
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

    async fn exchange(
        axum::extract::State(manager): axum::extract::State<EnrollManager>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let installed = manager.installed.lock().await;
        if installed.as_deref() != body["proof"]["key"].as_str().map(key_hash).as_deref() {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        axum::Json(json!({
            "token": "ax_cap_v1_fixture",
            "expiresAt": Utc::now() + chrono::Duration::minutes(5),
        }))
        .into_response()
    }

    async fn fixture(answer: u8) -> (Arc<OperatorDb>, String, EnrollManager, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let manager = EnrollManager {
            installed: Arc::new(Mutex::new(None)),
            answer: Arc::new(AtomicU8::new(answer)),
        };
        let app = axum::Router::new()
            .route(
                "/v1/deployments/deployment-one/enrollment-code",
                axum::routing::post(issue_code),
            )
            .route("/v1/identity/enroll", axum::routing::post(enroll))
            .route("/v1/identity/exchange", axum::routing::post(exchange))
            .with_state(manager.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(
            OperatorDb::new(directory.path().to_str().unwrap(), &"a".repeat(64))
                .await
                .unwrap(),
        );
        (db, url, manager, directory)
    }

    /// The manager installed the key and its reply was lost. The next start
    /// cannot get another code, since the deployment has a key, but it holds
    /// the key and proves it to the manager, which finishes the enrollment.
    #[tokio::test]
    async fn a_lost_enrollment_reply_is_finished_by_proving_the_saved_key() {
        let (db, url, _manager, _directory) = fixture(LOST).await;
        let identity = DailyIdentity::new(db.clone()).unwrap();

        identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_fixture")
            .await
            .expect_err("the reply was lost");

        // Until the enrollment is confirmed the Operator is in token mode: a
        // saved candidate is no identity to authenticate with.
        assert!(!has_runtime_identity(&db).await.unwrap());

        let identity = DailyIdentity::new(db.clone()).unwrap();
        let enrolled = identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_fixture")
            .await
            .expect("the saved key proves the installed enrollment");
        assert!(has_runtime_identity(&db).await.unwrap());
        assert!(enrolled);
        let saved = db.read_daily_identity().await.unwrap().unwrap();
        assert_eq!(saved.deployment_id, "deployment-one");
        assert!(saved.current.starts_with("ax_day_v1_"));
    }

    /// A refusal the manager confirms leaves no half-made identity behind.
    #[tokio::test]
    async fn a_refused_enrollment_clears_the_candidate_key() {
        let (db, url, _manager, _directory) = fixture(REFUSED).await;
        let identity = DailyIdentity::new(db.clone()).unwrap();
        identity
            .enroll_with_deployment_token(&url, "deployment-one", "ax_deploy_fixture")
            .await
            .expect_err("the manager refuses the enrollment");
        assert!(db.read_daily_identity().await.unwrap().is_none());
    }
}
