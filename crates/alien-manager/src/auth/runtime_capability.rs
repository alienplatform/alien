//! Manager-bound runtime credentials with a fixed authority deadline.
//!
//! Only trusted identity adapters may supply grants to the issuer. Incoming
//! requests never choose their own grant or extend a parent deadline.

use std::sync::Arc;

use alien_commands::server::command_registry::is_operations_command_target;
use alien_core::CommandTarget;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    auth::{CommandCapability, Role, Scope, Subject, SubjectKind},
    error::{ErrorData, Result},
    traits::AuthValidator,
};

const PREFIX: &str = "ax_cap_v1_";
const MAX_TOKEN_BYTES: usize = 8192;

/// Rights established by a trusted identity adapter, never by the exchange caller.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGrant {
    /// Tenant owning the recorded component.
    pub workspace_id: String,
    /// Project owning the recorded component.
    pub project_id: String,
    /// Exact deployment from authoritative resource state.
    pub deployment_id: String,
    /// Exact enrolled component or verified cloud resource.
    pub component_id: String,
    /// Single operation class allowed for this credential.
    pub purpose: RuntimePurpose,
}

/// Runtime operations cannot administer deployments or mint credentials.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum RuntimePurpose {
    /// Ingest telemetry for the recorded deployment.
    Telemetry,
    /// Reconcile the recorded operator deployment.
    OperatorSync,
    /// Pull deployment images through the registry receiver.
    RegistryPull,
    /// Connect the recorded operator tunnel.
    OperatorTunnel,
    /// Receive the operator's fixed operations queue.
    OperatorOperations,
    /// Relay commands to one app worker, without dispatch or administrative rights.
    OperationRelay { target: CommandTarget },
    /// Receive commands for one app-owned target.
    ReceiveCommands { target: CommandTarget },
}

impl RuntimePurpose {
    fn lifetime(&self) -> Duration {
        match self {
            Self::Telemetry | Self::OperatorSync | Self::OperatorTunnel => Duration::hours(1),
            // Pods pull with this credential from a Kubernetes Secret that the
            // operator refreshes. It may last as long as the operator's daily
            // key, which caps it through the authority deadline, so new pods
            // still start while the operator is down. Cloud proofs and the
            // manager's own reconciliation pass a one hour deadline.
            Self::RegistryPull => Duration::hours(24),
            Self::ReceiveCommands { .. }
            | Self::OperationRelay { .. }
            | Self::OperatorOperations => Duration::minutes(5),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claims {
    audience: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    grant: RuntimeGrant,
}

/// Shared issuer/verifier for one manager. The signing key must be stable across
/// its replicas and loaded from protected storage. It is never sent to runtimes.
/// No Debug implementation: it owns signing material.
pub struct RuntimeCredentials {
    audience: String,
    key: [u8; 32],
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

/// Response credential and its hard deadline. Never log or persist a debug dump.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct IssuedRuntimeCredential {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

impl RuntimeCredentials {
    pub fn audience(&self) -> &str {
        &self.audience
    }

    pub fn new(audience: String, key: [u8; 32]) -> Result<Self> {
        Self::with_clock(audience, key, Arc::new(Utc::now))
    }

    /// Supply a controlled clock for deterministic deadline verification.
    pub fn with_clock(
        audience: String,
        key: [u8; 32],
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self> {
        if audience.is_empty() {
            return Err(ErrorData::bad_request("Runtime audience must not be empty"));
        }
        Ok(Self {
            audience,
            key,
            clock,
        })
    }

    /// Issue only after verifying identity and deriving its grant from recorded
    /// resources. `authority_deadline` is the verified proof or parent bound;
    /// replay must reuse that bound, never recompute it from receipt time.
    pub fn issue(
        &self,
        grant: RuntimeGrant,
        authority_deadline: DateTime<Utc>,
    ) -> Result<IssuedRuntimeCredential> {
        let now = (self.clock)();
        let claims = Claims {
            audience: self.audience.clone(),
            issued_at: now,
            expires_at: authority_deadline.min(now + grant.purpose.lifetime()),
            grant,
        };
        self.validate_claims(&claims, now)?;
        let payload =
            serde_json::to_vec(&claims)
                .into_alien_error()
                .context(ErrorData::InternalError {
                    message: "Cannot encode runtime credential".into(),
                })?;
        let payload = URL_SAFE_NO_PAD.encode(payload);
        let mut mac = self.mac()?;
        mac.update(PREFIX.as_bytes());
        mac.update(payload.as_bytes());
        let token = format!(
            "{PREFIX}{payload}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        );
        if token.len() > MAX_TOKEN_BYTES {
            return Err(ErrorData::bad_request("Runtime grant is too large"));
        }
        Ok(IssuedRuntimeCredential {
            token,
            expires_at: claims.expires_at,
        })
    }

    fn mac(&self) -> Result<Hmac<Sha256>> {
        Hmac::<Sha256>::new_from_slice(&self.key)
            .into_alien_error()
            .context(ErrorData::InternalError {
                message: "Cannot initialize credential signer".into(),
            })
    }

    fn validate_claims(&self, claims: &Claims, now: DateTime<Utc>) -> Result<()> {
        let grant = &claims.grant;
        if let RuntimePurpose::ReceiveCommands { target }
        | RuntimePurpose::OperationRelay { target } = &grant.purpose
        {
            if target.resource_id.is_empty() || is_operations_command_target(&target.resource_id) {
                return Err(invalid());
            }
        }
        if let RuntimePurpose::OperationRelay { target } = &grant.purpose {
            if target.resource_type != alien_core::CommandTargetType::Worker {
                return Err(invalid());
            }
        }
        if claims.audience != self.audience
            || claims.issued_at > now
            || claims.expires_at <= now
            || claims.expires_at <= claims.issued_at
            || claims.expires_at - claims.issued_at > grant.purpose.lifetime()
            || [
                &grant.workspace_id,
                &grant.project_id,
                &grant.deployment_id,
                &grant.component_id,
            ]
            .iter()
            .any(|value| value.is_empty())
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn verified_claims(&self, token: &str) -> Result<Claims> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(invalid());
        }
        let (payload, signature) = token
            .strip_prefix(PREFIX)
            .and_then(|value| value.split_once('.'))
            .ok_or_else(invalid)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?;
        let mut mac = self.mac()?;
        mac.update(PREFIX.as_bytes());
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).map_err(|_| invalid())?;
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?;
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        self.validate_claims(&claims, (self.clock)())?;
        Ok(claims)
    }

    pub(crate) fn verify(&self, token: &str) -> Result<Subject> {
        let claims = self.verified_claims(token)?;
        let grant = claims.grant;
        let (scope, role) = match grant.purpose {
            RuntimePurpose::Telemetry => (
                Scope::Deployment {
                    project_id: grant.project_id,
                    deployment_id: grant.deployment_id,
                },
                Role::DeploymentTelemetryWriter,
            ),
            purpose @ (RuntimePurpose::OperatorSync
            | RuntimePurpose::RegistryPull
            | RuntimePurpose::OperatorTunnel) => (
                Scope::Deployment {
                    project_id: grant.project_id,
                    deployment_id: grant.deployment_id,
                },
                match purpose {
                    RuntimePurpose::OperatorSync => Role::OperatorSync,
                    RuntimePurpose::RegistryPull => Role::RegistryPull,
                    RuntimePurpose::OperatorTunnel => Role::OperatorTunnel,
                    _ => unreachable!(),
                },
            ),
            RuntimePurpose::OperatorOperations => (
                Scope::Commands {
                    project_id: grant.project_id,
                    deployment_id: grant.deployment_id,
                    capability: CommandCapability::Receive {
                        target: CommandTarget::new(
                            alien_commands::server::command_registry::OPERATOR_COMMAND_TARGET_ID,
                            alien_core::CommandTargetType::Daemon,
                        ),
                    },
                },
                Role::CommandCapability,
            ),
            RuntimePurpose::ReceiveCommands { target }
            | RuntimePurpose::OperationRelay { target } => (
                Scope::Commands {
                    project_id: grant.project_id,
                    deployment_id: grant.deployment_id,
                    capability: CommandCapability::Receive { target },
                },
                Role::CommandCapability,
            ),
        };
        Ok(Subject {
            kind: SubjectKind::ServiceAccount {
                id: grant.component_id,
            },
            workspace_id: grant.workspace_id,
            scope,
            role,
            bearer_token: token.to_owned(),
        })
    }
}

fn invalid() -> AlienError<ErrorData> {
    ErrorData::unauthorized("Invalid or expired runtime credential")
}

/// True for a bearer that only the runtime validator accepts. Legacy validators
/// never see this prefix, so an authenticated subject with it is a capability.
pub fn is_runtime_capability(token: &str) -> bool {
    token.starts_with("ax_cap_")
}

/// Recognizes runtime credentials before delegating legacy authentication.
/// A rejected runtime credential never falls back to another validator.
pub struct RuntimeAuthValidator {
    credentials: Arc<RuntimeCredentials>,
    legacy: Arc<dyn AuthValidator>,
}

impl RuntimeAuthValidator {
    pub fn new(credentials: Arc<RuntimeCredentials>, legacy: Arc<dyn AuthValidator>) -> Self {
        Self {
            credentials,
            legacy,
        }
    }
}

#[async_trait]
impl AuthValidator for RuntimeAuthValidator {
    fn authority_deadline(
        &self,
        subject: &Subject,
    ) -> std::result::Result<Option<DateTime<Utc>>, AlienError> {
        if is_runtime_capability(&subject.bearer_token) {
            return self
                .credentials
                .verified_claims(&subject.bearer_token)
                .map(|claims| Some(claims.expires_at))
                .map_err(AlienError::into_generic);
        }
        self.legacy.authority_deadline(subject)
    }

    async fn validate(
        &self,
        headers: &http::HeaderMap,
    ) -> std::result::Result<Option<Subject>, AlienError> {
        let authorization = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        // OCI clients carry a credential as the Basic password. Reserve the
        // capability prefix there too, so it cannot reach legacy authentication.
        let basic = authorization
            .and_then(|value| value.strip_prefix("Basic "))
            .map(|value| {
                let bytes = STANDARD
                    .decode(value)
                    .map_err(|_| invalid().into_generic())?;
                String::from_utf8(bytes).map_err(|_| invalid().into_generic())
            })
            .transpose()?;
        let token = authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .or_else(|| {
                basic.as_deref().map(|value| {
                    value
                        .split_once(':')
                        .map_or(value, |(_, password)| password)
                })
            });
        if let Some(token) = token.filter(|token| is_runtime_capability(token)) {
            return self
                .credentials
                .verify(token)
                .map(Some)
                .map_err(AlienError::into_generic);
        }
        self.legacy.validate(headers).await
    }
}

/// Transient registry authority for the manager's current reconciliation only.
/// The loop constructs this from its acquired deployment record; it is never
/// serialized into target configuration or controller state.
pub(crate) struct DeploymentRegistryCredentials {
    credentials: Arc<RuntimeCredentials>,
    grant: RuntimeGrant,
    command_targets: Vec<CommandTarget>,
}

impl DeploymentRegistryCredentials {
    pub(crate) fn new(
        credentials: Arc<RuntimeCredentials>,
        deployment: &crate::traits::DeploymentRecord,
        command_targets: Vec<CommandTarget>,
    ) -> Self {
        Self {
            credentials,
            command_targets,
            grant: RuntimeGrant {
                workspace_id: deployment.workspace_id.clone(),
                project_id: deployment.project_id.clone(),
                deployment_id: deployment.id.clone(),
                component_id: "registry-controller".into(),
                purpose: RuntimePurpose::RegistryPull,
            },
        }
    }
}

#[async_trait]
impl alien_infra::RuntimeRegistryCredentialProvider for DeploymentRegistryCredentials {
    async fn runtime_credential(
        &self,
        deployment_id: &str,
        purpose: serde_json::Value,
    ) -> alien_infra::Result<alien_client_core::runtime_credentials::RuntimeCapability> {
        let invalid = || alien_infra::ErrorData::ResourceConfigInvalid {
            message: "Runtime credential is outside this reconciliation's authority".into(),
            resource_id: None,
        };
        let purpose: RuntimePurpose =
            serde_json::from_value(purpose).map_err(|_| AlienError::new(invalid()))?;
        if deployment_id != self.grant.deployment_id {
            return Err(AlienError::new(invalid()));
        }
        match &purpose {
            RuntimePurpose::Telemetry | RuntimePurpose::RegistryPull => {}
            RuntimePurpose::ReceiveCommands { target }
                if target.resource_type != alien_core::CommandTargetType::Worker
                    && self.command_targets.contains(target) => {}
            _ => return Err(AlienError::new(invalid())),
        }
        let mut grant = self.grant.clone();
        grant.component_id = "runtime-controller".into();
        grant.purpose = purpose;
        let issued = self
            .credentials
            .issue(grant, Utc::now() + Duration::hours(1))
            .context(invalid())?;
        Ok(alien_client_core::runtime_credentials::RuntimeCapability {
            token: issued.token,
            expires_at: issued.expires_at,
        })
    }

    async fn runtime_credential_published(
        &self,
        deployment_id: &str,
        purpose: serde_json::Value,
        credential: alien_client_core::runtime_credentials::RuntimeCapability,
    ) -> alien_infra::Result<()> {
        let invalid = || alien_infra::ErrorData::ResourceConfigInvalid {
            message: "Runtime publication is outside this reconciliation's authority".into(),
            resource_id: None,
        };
        let purpose: RuntimePurpose =
            serde_json::from_value(purpose).map_err(|_| AlienError::new(invalid()))?;
        if deployment_id != self.grant.deployment_id
            || !matches!(purpose,
            RuntimePurpose::ReceiveCommands { ref target }
                if target.resource_type != alien_core::CommandTargetType::Worker
                    && self.command_targets.contains(target))
        {
            return Err(AlienError::new(invalid()));
        }
        let claims = self
            .credentials
            .verified_claims(&credential.token)
            .context(invalid())?;
        if claims.grant.workspace_id != self.grant.workspace_id
            || claims.grant.project_id != self.grant.project_id
            || claims.grant.deployment_id != self.grant.deployment_id
            || claims.grant.purpose != purpose
            || claims.expires_at != credential.expires_at
        {
            return Err(AlienError::new(invalid()));
        }
        // The manager keeps no copy. Every reconciliation issues a fresh leaf and
        // an older written leaf stays valid until its own short deadline, so a
        // restart or another replica needs no shared state.
        if credential.expires_at <= (self.credentials.clock)() {
            return Err(AlienError::new(invalid()));
        }
        Ok(())
    }

    async fn registry_pull_credential(
        &self,
        deployment_id: &str,
    ) -> alien_infra::Result<alien_client_core::runtime_credentials::RuntimeCapability> {
        if deployment_id != self.grant.deployment_id {
            return Err(AlienError::new(
                alien_infra::ErrorData::ResourceConfigInvalid {
                    message: "Registry credential deployment mismatch".into(),
                    resource_id: None,
                },
            ));
        }
        let issued = self
            .credentials
            .issue(self.grant.clone(), Utc::now() + Duration::hours(1))
            .context(alien_infra::ErrorData::ResourceConfigInvalid {
                message: "Cannot issue registry credential".into(),
                resource_id: None,
            })?;
        Ok(alien_client_core::runtime_credentials::RuntimeCapability {
            token: issued.token,
            expires_at: issued.expires_at,
        })
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;
    use alien_infra::RuntimeRegistryCredentialProvider;
    use serde_json::json;

    #[tokio::test]
    async fn controller_issues_only_recorded_pull_targets() {
        let credentials = Arc::new(RuntimeCredentials::new("manager-one".into(), [7; 32]).unwrap());
        let target = CommandTarget {
            resource_id: "app".into(),
            resource_type: alien_core::CommandTargetType::Container,
        };
        let provider = DeploymentRegistryCredentials {
            credentials: credentials.clone(),
            command_targets: vec![target.clone()],
            grant: RuntimeGrant {
                workspace_id: "workspace-one".into(),
                project_id: "project-one".into(),
                deployment_id: "deployment-one".into(),
                component_id: "runtime-controller".into(),
                purpose: RuntimePurpose::RegistryPull,
            },
        };
        let purpose = json!({"type":"receiveCommands", "target":target});
        let issued = provider
            .runtime_credential("deployment-one", purpose.clone())
            .await
            .unwrap();
        assert!(credentials.verify(&issued.token).is_ok());
        assert!(issued.expires_at <= Utc::now() + Duration::minutes(5));
        provider
            .runtime_credential_published("deployment-one", purpose.clone(), issued)
            .await
            .unwrap();
        assert!(provider
            .runtime_credential("other-deployment", purpose)
            .await
            .is_err());
        for denied in [
            json!({"type":"receiveCommands", "target":{"resourceId":"other", "resourceType":"container"}}),
            json!({"type":"receiveCommands", "target":{"resourceId":"app", "resourceType":"worker"}}),
            json!({"type":"operatorSync"}),
            json!({"type":"operatorTunnel"}),
            json!({"type":"operationRelay", "target":target}),
        ] {
            assert!(provider
                .runtime_credential("deployment-one", denied)
                .await
                .is_err());
        }
        for purpose in [json!({"type":"telemetry"}), json!({"type":"registryPull"})] {
            let issued = provider
                .runtime_credential("deployment-one", purpose)
                .await
                .unwrap();
            assert!(credentials.verify(&issued.token).is_ok());
            assert!(issued.expires_at <= Utc::now() + Duration::hours(1));
        }
    }
}
