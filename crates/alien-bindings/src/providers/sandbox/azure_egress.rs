//! Azure's native proxy transport and durable gateway identity mapping.
use super::{AzureSandbox, CREATE};
use crate::{
    error::{ErrorData, Result},
    sandbox_egress::SandboxEgressManager,
};
use alien_azure_clients::azure::sandbox_data_plane::{EgressHostRule, EgressPolicy, Sandbox};
use alien_core::{EgressPolicy as RuntimePolicy, SANDBOX_EXPIRY_LABEL};
use alien_error::{AlienError, ContextError};
use std::{collections::BTreeMap, sync::Arc};

const IDENTITY_LABEL: &str = "alien-egress-identity";
const MAX_TOKEN_LIFETIME: u32 = 24 * 60 * 60;

/// Azure echoes the effective HTTP policy twice and adds its enforcement mode.
/// Both copies must match, including rule order; unknown fields remain refused.
fn gateway_policy_holds(expected: &EgressPolicy, effective: &EgressPolicy) -> bool {
    let enforced_matches = |mut policy: EgressPolicy| {
        let mode = policy.unmodelled.remove("enforcementMode");
        mode.is_none_or(|mode| mode.as_str() == Some("Enforced")) && policy == *expected
    };
    let mut flat = effective.clone();
    let http = flat.unmodelled.remove("http");
    enforced_matches(flat)
        && match http {
            None => true,
            Some(http) => serde_json::from_value(http).is_ok_and(enforced_matches),
        }
}

impl AzureSandbox {
    /// Use the deployment's gateway for policy writes and create-time proxy credentials.
    pub fn with_egress_manager(
        mut self,
        manager: Arc<SandboxEgressManager>,
        default_policy: Option<RuntimePolicy>,
    ) -> Self {
        self.egress_manager = Some(manager);
        self.default_policy = default_policy;
        self
    }

    /// The same declaration deadline enforced by the external lifecycle reconciler.
    pub fn with_lifetime_ceiling(mut self, seconds: Option<u32>) -> Self {
        self.max_lifetime_seconds = seconds;
        self
    }

    pub(super) fn requested_policy(
        &self,
        requested: Option<&RuntimePolicy>,
    ) -> Result<Option<RuntimePolicy>> {
        let policy = requested.or(self.default_policy.as_ref());
        if self.egress_manager.is_none() {
            super::super::egress::require_default(policy, Some(&self.egress))?;
            return Ok(None);
        }
        let policy = policy.cloned().unwrap_or(RuntimePolicy::Internet);
        super::super::egress::validate(&policy, Some(&self.egress))?;
        Ok(Some(policy))
    }

    pub(super) fn creation_deadline(&self, timeout_ms: Option<u64>) -> Result<Option<i64>> {
        let ceiling = if self.egress_manager.is_some() {
            Some(
                self.max_lifetime_seconds
                    .unwrap_or(MAX_TOKEN_LIFETIME)
                    .min(MAX_TOKEN_LIFETIME),
            )
        } else {
            self.max_lifetime_seconds
        };
        let seconds = timeout_ms
            .map(|timeout| super::super::requested_lifetime_seconds(timeout, ceiling, CREATE))
            .transpose()?
            .or(ceiling);
        Ok(
            seconds
                .map(|seconds| chrono::Utc::now().timestamp_millis() + i64::from(seconds) * 1000),
        )
    }

    pub(super) fn native_policy(&self) -> Option<EgressPolicy> {
        let Some(manager) = &self.egress_manager else {
            return super::egress_policy(&self.egress);
        };
        // Full/default-Deny inspects the inner CONNECT destination and rejects the gateway.
        // Partial requires default-Allow. The ordered catch-all blocks other HTTP requests;
        // setup's NSG allows the linked gateway ports and denies platform DNS/IMDS and other
        // traffic. The admin port is reachable through the link but requires an admin JWT.
        Some(EgressPolicy {
            default_action: "Allow".into(),
            traffic_inspection: Some("Partial".into()),
            host_rules: vec![
                EgressHostRule {
                    pattern: manager.gateway_authority().into(),
                    action: "Allow".into(),
                },
                EgressHostRule {
                    pattern: manager.gateway_admin_authority().into(),
                    action: "Allow".into(),
                },
                EgressHostRule {
                    pattern: "*".into(),
                    action: "Deny".into(),
                },
            ],
            rules: Vec::new(),
            unmodelled: Default::default(),
        })
    }

    pub(super) async fn prepare_identity(
        &self,
        environment: &mut BTreeMap<String, String>,
        labels: &mut BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        let Some(manager) = &self.egress_manager else {
            return Ok(None);
        };
        let expires = labels
            .get(SANDBOX_EXPIRY_LABEL)
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                not_as_declared("new", "gateway identity requires an expiry deadline")
            })?;
        // Azure chooses its vendor id after create; the label persists the pre-created identity.
        let identity = uuid::Uuid::new_v4().to_string();
        let token = manager.mint_identity(&identity, expires / 1000).await?;
        let proxy = format!("http://alien:{}@{}", token, manager.gateway_authority());
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            environment.insert(name.into(), proxy.clone());
        }
        for name in ["NO_PROXY", "no_proxy"] {
            environment.insert(name.into(), "localhost,127.0.0.1".into());
        }
        labels.insert(IDENTITY_LABEL.into(), identity.clone());
        // The gateway has no policy for this identity yet and denies all outbound requests.
        // Create publishes the policy only after Azure returns the expected floor and label.
        Ok(Some(identity))
    }

    pub(super) fn check_gateway_floor(&self, sandbox: &Sandbox) -> Result<()> {
        if !self
            .native_policy()
            .as_ref()
            .zip(sandbox.egress_policy.as_ref())
            .is_some_and(|(expected, effective)| gateway_policy_holds(expected, effective))
        {
            return Err(not_as_declared(
                &sandbox.id,
                "native gateway-only policy differs from the binding",
            ));
        }
        identity(sandbox)?;
        let expires = sandbox
            .labels
            .get(SANDBOX_EXPIRY_LABEL)
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                not_as_declared(&sandbox.id, "gateway expiry label is missing or invalid")
            })?;
        if expires <= chrono::Utc::now().timestamp_millis() {
            return Err(not_as_declared(
                &sandbox.id,
                "gateway identity has expired; create a new sandbox",
            ));
        }
        Ok(())
    }

    pub(super) async fn revoke_identity(&self, sandbox_id: &str) -> Result<()> {
        let Some(manager) = &self.egress_manager else {
            return Ok(());
        };
        let Some(sandbox) = self.read_sandbox("sandbox.terminate", sandbox_id).await? else {
            return Ok(());
        };
        manager.delete(identity(&sandbox)?).await
    }

    pub(super) async fn discard_with_policy(
        &self,
        sandbox_id: &str,
        identity: Option<&str>,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        let reason = if let (Some(manager), Some(identity)) = (&self.egress_manager, identity) {
            match manager.delete(identity).await {
                Ok(()) => reason,
                Err(_) => reason.context(ErrorData::SandboxCommandFailed {
                    failure: "egressRevocationFailed".into(),
                    reason: "could not revoke the failed create's gateway identity; terminating its sandbox".into(),
                }),
            }
        } else {
            reason
        };
        self.discard(sandbox_id, reason).await
    }
}

pub(super) fn identity(sandbox: &Sandbox) -> Result<&str> {
    let identity = sandbox
        .labels
        .get(IDENTITY_LABEL)
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .ok_or_else(|| {
            not_as_declared(&sandbox.id, "gateway identity label is missing or invalid")
        })?;
    Ok(identity)
}

pub(super) fn not_as_declared(id: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxNotAsDeclared {
        sandbox_id: id.into(),
        restriction: "gateway identity and transport".into(),
        reason: reason.into(),
    })
}

#[cfg(all(test, feature = "local"))]
#[path = "azure_egress_tests.rs"]
mod tests;
