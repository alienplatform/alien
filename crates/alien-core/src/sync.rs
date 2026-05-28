//! Sync protocol types for agent ↔ manager communication.
//!
//! The agent periodically calls `POST /v1/sync` with a `SyncRequest` and
//! receives a `SyncResponse` containing the target deployment state.

use serde::{Deserialize, Serialize};

use crate::{DeploymentConfig, DeploymentState, ReleaseInfo};

/// Request sent by the agent to the manager during periodic sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRequest {
    /// The deployment ID this agent is managing.
    pub deployment_id: String,
    /// Current deployment state as seen by the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state: Option<DeploymentState>,
    /// Agent binary version, from `env!("CARGO_PKG_VERSION")` at build time.
    /// Lets the manager render fleet inventory and decide whether to send
    /// an `agent_target`. Optional for backward compatibility with old agents.
    /// See `internal-docs/alien/02-manager/12-agent-self-update.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    /// `linux` / `macos` / `windows`. From `std::env::consts::OS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_os: Option<String>,
    /// `x86_64` / `aarch64`. From `std::env::consts::ARCH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_arch: Option<String>,
    /// How the agent is supervised — `os-service` (launcher) or `kubernetes`
    /// (Helm). Detected at runtime from `KUBERNETES_SERVICE_HOST`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<AgentRegime>,
}

/// Supervisor regime for an agent. Drives which `agent_target` payload
/// (`binary` vs `helm`) the manager sends and how the agent actuates the
/// upgrade. See `internal-docs/alien/02-manager/12-agent-self-update.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRegime {
    /// Native OS service — the launcher swaps the binary on disk.
    OsService,
    /// Kubernetes pod — agent creates a Helm-runner Job that runs `helm upgrade --atomic`.
    Kubernetes,
}

/// Response from the manager to the agent sync request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    /// Authoritative deployment state from the manager.
    ///
    /// Pull agents use this to hydrate local state when attaching to an
    /// already-imported deployment. Absent means the agent's local state is
    /// already authoritative or no state has been established yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state: Option<DeploymentState>,
    /// Target deployment the agent should converge toward.
    /// None means no changes needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetDeployment>,
    /// Public URL for the commands API (e.g. `https://manager.example.com/v1`).
    /// Cloud-deployed workers use this to poll for pending commands.
    /// When absent, the agent falls back to its sync URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands_url: Option<String>,
    /// Desired agent self-update target. The agent acts on whichever payload
    /// matches its regime: `binary` for `os-service`, `helm` for `kubernetes`.
    /// None means no upgrade pending.
    /// See `internal-docs/alien/02-manager/12-agent-self-update.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_target: Option<AgentTarget>,
}

/// Desired agent upgrade payload, sent by the manager on the sync exchange.
///
/// The agent reads `binary` or `helm` depending on its regime. The manager is
/// the single source of truth for the target version per deployment per
/// channel, and on Kubernetes also for the full desired values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTarget {
    /// Target agent version (e.g. "1.4.0").
    pub version: String,
    /// The agent should refuse the upgrade if its own version is older than
    /// this — used by the manager to enforce a floor on incremental migrations.
    pub min_supported_version: String,
    /// OS-service actuation payload. Present iff the deployment's regime is `os-service`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<AgentBinaryTarget>,
    /// Kubernetes actuation payload. Present iff the deployment's regime is `kubernetes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helm: Option<AgentHelmTarget>,
}

/// OS-service binary upgrade payload — the agent downloads, verifies the
/// SHA-256, stages the binary, and exits; the launcher performs the
/// health-gated swap. The `signature` field is **future work** (see
/// `internal-docs/alien/02-manager/12-agent-self-update.md` →
/// "Publishing and signing"): it rides along on the wire so newer agents
/// can enforce it once the signing infrastructure lands, but the current
/// launcher relies on the SHA-256 and HTTPS download path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBinaryTarget {
    /// `(os, arch)` → download URL. Keyed as `"<os>/<arch>"` (e.g. `"linux/x86_64"`).
    pub artifacts: std::collections::BTreeMap<String, String>,
    /// SHA-256 digest of the binary, lowercase hex.
    pub sha256: String,
    /// ed25519 detached signature over the binary, base64-encoded.
    /// **Future:** verified against the launcher's pinned public key before
    /// the binary is exec'd. Not enforced in the current iteration.
    pub signature: String,
}

/// Kubernetes upgrade payload — the agent writes the full values to a
/// ConfigMap (sensitive values to a Secret) and creates a Helm-runner Job
/// that runs `helm upgrade --atomic`. Helm's revision-scoped rollback covers
/// both the image and the values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHelmTarget {
    /// OCI registry reference for the chart (e.g. `oci://ghcr.io/alienplatform/alien-agent`).
    pub chart_repo: String,
    /// Chart version (e.g. `"1.4.0"`).
    pub chart_version: String,
    /// Full desired values document (manager-owned). Stored verbatim by Helm
    /// in the release Secret, so MUST NOT contain raw secrets — those go via
    /// `sensitive_values` as references.
    pub values: serde_json::Value,
    /// Sensitive values, expressed as references to existing Kubernetes
    /// Secrets in the namespace. Keyed by JSON-Pointer path into `values`.
    /// The agent materializes these into a Secret the chart mounts via
    /// `valueFrom: secretKeyRef:`, so the material never reaches the release
    /// history Secret base64-encoded.
    #[serde(default)]
    pub sensitive_values: std::collections::BTreeMap<String, SecretRef>,
}

/// Reference to a Kubernetes Secret key holding sensitive data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub name: String,
    pub key: String,
}

/// Target deployment state for the agent to converge toward.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDeployment {
    /// Release information (ID, version, stack definition).
    pub release_info: ReleaseInfo,
    /// Full deployment configuration (settings, env vars, etc.).
    pub config: DeploymentConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_sync_request() -> SyncRequest {
        SyncRequest {
            deployment_id: "dep_abc123".to_string(),
            current_state: None,
            agent_version: None,
            agent_os: None,
            agent_arch: None,
            regime: None,
        }
    }

    fn empty_sync_response() -> SyncResponse {
        SyncResponse {
            current_state: None,
            target: None,
            commands_url: None,
            agent_target: None,
        }
    }

    #[test]
    fn test_sync_request_serialization() {
        let req = empty_sync_request();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["deploymentId"], "dep_abc123");
        // current_state is None → should be omitted
        assert!(json.get("currentState").is_none());
    }

    #[test]
    fn test_sync_request_deserialization() {
        let json = r#"{"deploymentId": "dep_xyz"}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_xyz");
        assert!(req.current_state.is_none());
    }

    #[test]
    fn test_sync_response_empty() {
        let resp = empty_sync_response();
        let json = serde_json::to_value(&resp).unwrap();
        // target is None → should be omitted
        assert!(json.get("target").is_none());
        assert!(json.get("currentState").is_none());
    }

    #[test]
    fn test_sync_response_roundtrip_no_target() {
        let resp = empty_sync_response();
        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: SyncResponse = serde_json::from_str(&serialized).unwrap();
        assert!(deserialized.target.is_none());
        assert!(deserialized.current_state.is_none());
    }

    #[test]
    fn test_sync_request_with_camel_case() {
        // Verify camelCase renaming works correctly
        let json = r#"{"deploymentId": "dep_1", "currentState": null}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_1");
        assert!(req.current_state.is_none());

        // snake_case should NOT work
        let json = r#"{"deployment_id": "dep_1"}"#;
        assert!(serde_json::from_str::<SyncRequest>(json).is_err());
    }

    // --- ALIEN-59 agent self-update wire format tests --------------------

    /// A new agent that fills in the self-update fields produces JSON the
    /// new manager can deserialize, with the expected camelCase + kebab-case
    /// values from the design doc.
    #[test]
    fn test_sync_request_with_self_update_fields_roundtrip() {
        let req = SyncRequest {
            deployment_id: "dep_abc".to_string(),
            current_state: None,
            agent_version: Some("1.3.5".to_string()),
            agent_os: Some("linux".to_string()),
            agent_arch: Some("aarch64".to_string()),
            regime: Some(AgentRegime::Kubernetes),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["agentVersion"], "1.3.5");
        assert_eq!(json["agentOs"], "linux");
        assert_eq!(json["agentArch"], "aarch64");
        assert_eq!(json["regime"], "kubernetes"); // kebab-case enum
        let back: SyncRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.agent_version.as_deref(), Some("1.3.5"));
        assert_eq!(back.regime, Some(AgentRegime::Kubernetes));
    }

    /// **Backward compat (new manager, old agent):** an old agent on the
    /// wire doesn't send agentVersion/Os/Arch/regime. The new manager must
    /// deserialize without complaint — the four fields default to None.
    #[test]
    fn test_sync_request_old_agent_no_self_update_fields() {
        let json =
            r#"{"deploymentId": "dep_old", "currentState": null}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_old");
        assert!(req.agent_version.is_none());
        assert!(req.agent_os.is_none());
        assert!(req.agent_arch.is_none());
        assert!(req.regime.is_none());
    }

    /// **Backward compat (old client, new payload):** the SyncResponse's
    /// new `agentTarget` field is omitted (skip_serializing_if = None), so
    /// an old agent that does not know about it still gets a clean response.
    #[test]
    fn test_sync_response_old_client_no_agent_target() {
        let resp = empty_sync_response();
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("agentTarget").is_none());
    }

    #[test]
    fn test_agent_regime_kebab_case() {
        assert_eq!(
            serde_json::to_value(AgentRegime::OsService).unwrap(),
            serde_json::json!("os-service")
        );
        assert_eq!(
            serde_json::to_value(AgentRegime::Kubernetes).unwrap(),
            serde_json::json!("kubernetes")
        );
        let r: AgentRegime = serde_json::from_str("\"os-service\"").unwrap();
        assert_eq!(r, AgentRegime::OsService);
    }

    /// AgentTarget.helm carries the full structure for the Kubernetes
    /// regime — verify the chart_repo/values/sensitive_values shape
    /// survives a JSON roundtrip.
    #[test]
    fn test_agent_target_helm_roundtrip() {
        let mut sensitive = std::collections::BTreeMap::new();
        sensitive.insert(
            "/management/token".to_string(),
            SecretRef {
                name: "alien-agent".to_string(),
                key: "sync-token".to_string(),
            },
        );
        let helm = AgentHelmTarget {
            chart_repo: "oci://ghcr.io/alienplatform/alien-agent".to_string(),
            chart_version: "1.4.0".to_string(),
            values: serde_json::json!({"runtime": {"image": {"tag": "1.4.0"}}}),
            sensitive_values: sensitive,
        };
        let target = AgentTarget {
            version: "1.4.0".to_string(),
            min_supported_version: "1.3.0".to_string(),
            binary: None,
            helm: Some(helm),
        };
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["minSupportedVersion"], "1.3.0");
        assert!(json.get("binary").is_none(), "binary must be omitted");
        assert_eq!(json["helm"]["chartVersion"], "1.4.0");
        assert_eq!(
            json["helm"]["sensitiveValues"]["/management/token"]["name"],
            "alien-agent"
        );
        let back: AgentTarget = serde_json::from_value(json).unwrap();
        assert!(back.binary.is_none());
        assert_eq!(back.helm.unwrap().chart_version, "1.4.0");
    }

    /// AgentTarget.binary carries the OS-service payload (artifacts/sha256/
    /// signature).
    #[test]
    fn test_agent_target_binary_roundtrip() {
        let mut artifacts = std::collections::BTreeMap::new();
        artifacts.insert(
            "linux/aarch64".to_string(),
            "https://releases.alien.dev/1.4.0/linux-aarch64/alien-agent".to_string(),
        );
        let bin = AgentBinaryTarget {
            artifacts,
            sha256: "abc123".to_string(),
            signature: "base64sig==".to_string(),
        };
        let target = AgentTarget {
            version: "1.4.0".to_string(),
            min_supported_version: "1.3.0".to_string(),
            binary: Some(bin),
            helm: None,
        };
        let json = serde_json::to_value(&target).unwrap();
        assert!(json.get("helm").is_none(), "helm must be omitted");
        assert_eq!(json["binary"]["sha256"], "abc123");
        assert_eq!(
            json["binary"]["artifacts"]["linux/aarch64"],
            "https://releases.alien.dev/1.4.0/linux-aarch64/alien-agent"
        );
    }
}
