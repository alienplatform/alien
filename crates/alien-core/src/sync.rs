//! Sync protocol types for agent ↔ manager communication.
//!
//! The agent periodically calls `POST /v1/sync` with a `SyncRequest` and
//! receives a `SyncResponse` containing the target deployment state.

use serde::{Deserialize, Serialize};

use crate::{DeploymentConfig, DeploymentState, ReleaseInfo, ResourceHeartbeat};

/// Request sent by the agent to the manager during periodic sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRequest {
    /// The deployment ID this agent is managing.
    pub deployment_id: String,
    /// Current deployment state as seen by the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state: Option<DeploymentState>,
    /// Resource heartbeats emitted by the Operator's deployment step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heartbeats: Vec<ResourceHeartbeat>,
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
    /// None means no changes needed or this is an observe-only deployment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetDeployment>,
    /// Public URL for the commands API (e.g. `https://manager.example.com/v1`).
    /// Cloud-deployed workers use this to poll for pending commands.
    /// When absent, the agent falls back to its sync URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands_url: Option<String>,
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
            heartbeats: Vec::new(),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["deploymentId"], "dep_abc123");
        // current_state is None → should be omitted
        assert!(json.get("currentState").is_none());
        assert!(json.get("heartbeats").is_none());
    }

    #[test]
    fn test_sync_request_deserialization() {
        let json = r#"{"deploymentId": "dep_xyz"}"#;
        let req: SyncRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.deployment_id, "dep_xyz");
        assert!(req.current_state.is_none());
        assert!(req.heartbeats.is_empty());
    }

    #[test]
    fn test_sync_response_empty() {
        let resp = SyncResponse {
            current_state: None,
            target: None,
            commands_url: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        // target is None → should be omitted
        assert!(json.get("target").is_none());
        assert!(json.get("currentState").is_none());
    }

    #[test]
    fn test_sync_response_roundtrip_no_target() {
        let resp = SyncResponse {
            current_state: None,
            target: None,
            commands_url: None,
        };
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
        assert!(req.heartbeats.is_empty());

        // snake_case should NOT work
        let json = r#"{"deployment_id": "dep_1"}"#;
        assert!(serde_json::from_str::<SyncRequest>(json).is_err());
    }

    #[test]
    fn test_sync_request_heartbeats_roundtrip() {
        let json = serde_json::json!({
            "deploymentId": "dep_1",
            "heartbeats": [{
                "deploymentId": "dep_1",
                "resourceId": "api",
                "resourceType": "container",
                "controllerPlatform": "kubernetes",
                "backend": "kubernetes",
                "observedAt": "2026-01-01T00:00:00Z",
                "data": {
                    "resourceType": "container",
                    "data": {
                        "backend": "kubernetes",
                        "status": {
                            "health": "healthy",
                            "lifecycle": "running",
                            "message": null,
                            "stale": false,
                            "partial": false,
                            "collectionIssues": []
                        },
                        "namespace": "default",
                        "name": "api",
                        "workloadKind": "deployment",
                        "replicas": { "desired": 1, "current": 1, "ready": 1, "available": 1, "updated": null, "misscheduled": null },
                        "restarts": 0,
                        "cpu": null,
                        "memory": null,
                        "workload": null,
                        "pods": [],
                        "instances": [],
                        "events": []
                    }
                },
                "raw": []
            }]
        });

        let req: SyncRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.heartbeats.len(), 1);
        assert_eq!(req.heartbeats[0].resource_id, "api");

        let serialized = serde_json::to_value(&req).unwrap();
        assert_eq!(serialized["heartbeats"][0]["resourceId"], "api");
    }

    #[test]
    fn test_sync_response_observe_only_state_roundtrip() {
        let state = DeploymentState {
            status: crate::DeploymentStatus::Running,
            platform: crate::Platform::Kubernetes,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: crate::DEPLOYMENT_PROTOCOL_VERSION,
        };
        assert!(!state.has_desired());

        let resp = SyncResponse {
            current_state: Some(state),
            target: None,
            commands_url: None,
        };

        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: SyncResponse = serde_json::from_str(&serialized).unwrap();
        let current_state = deserialized.current_state.unwrap();

        assert_eq!(current_state.status, crate::DeploymentStatus::Running);
        assert!(!current_state.has_desired());
        assert!(deserialized.target.is_none());
    }
}
