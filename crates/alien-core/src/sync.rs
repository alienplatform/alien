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
}

/// Response from the manager to the agent sync request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    /// Target deployment the agent should converge toward.
    /// None means no changes needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetDeployment>,
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

    #[test]
    fn test_sync_request_serialization() {
        let req = SyncRequest {
            deployment_id: "dep_abc123".to_string(),
            current_state: None,
        };

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
        let resp = SyncResponse { target: None };
        let json = serde_json::to_value(&resp).unwrap();
        // target is None → should be omitted
        assert!(json.get("target").is_none());
    }

    #[test]
    fn test_sync_response_roundtrip_no_target() {
        let resp = SyncResponse { target: None };
        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: SyncResponse = serde_json::from_str(&serialized).unwrap();
        assert!(deserialized.target.is_none());
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
}
