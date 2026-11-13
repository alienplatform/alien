//! Deployment tracking — stores tracked deployments in a local JSON file.
//!
//! The deploy CLI stores deployment info in `~/.config/alien/deployments.json`.

use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Information about a tracked deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedDeployment {
    /// User-provided name for the deployment
    pub name: String,
    /// Deployment ID from the manager
    pub deployment_id: String,
    /// Authentication token. A Kubernetes install keeps none: the Operator
    /// enrolls and the installation's key is retired, so a rerun asks the
    /// running Operator for a short-lived key instead. Files written before
    /// that carry one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Manager URL
    pub manager_url: String,
    /// Target platform
    pub platform: String,
    /// Helm release and namespace of a Kubernetes install, where a rerun finds
    /// the Operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helm: Option<TrackedHelmRelease>,
    /// Local platform metadata needed to destroy pull-model resources later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<TrackedLocalDeployment>,
    /// When the deployment was tracked
    pub tracked_at: String,
}

/// Where a Kubernetes install's Operator runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackedHelmRelease {
    pub release: String,
    pub namespace: String,
}

/// Metadata for deployments managed by the local operator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackedLocalDeployment {
    /// Data directory used by the local operator for runtime state.
    pub data_dir: String,
    /// Whether deploy installed the local operator as an OS service.
    pub service_managed: bool,
}

/// Deployment tracking manager
pub struct DeploymentTracker {
    deployments: HashMap<String, TrackedDeployment>,
    file_path: PathBuf,
}

impl DeploymentTracker {
    /// Create a new deployment tracker and load existing deployments
    pub fn new() -> Result<Self> {
        let file_path = tracking_file_path()?;
        let deployments = load_deployments(&file_path)?;
        Ok(Self {
            deployments,
            file_path,
        })
    }

    /// Create a tracker with a custom file path (useful for testing)
    pub fn with_path(file_path: PathBuf) -> Result<Self> {
        let deployments = load_deployments(&file_path)?;
        Ok(Self {
            deployments,
            file_path,
        })
    }

    /// Track a new deployment
    pub fn track(
        &mut self,
        name: String,
        deployment_id: String,
        token: Option<String>,
        manager_url: String,
        platform: String,
        helm: Option<TrackedHelmRelease>,
        local: Option<TrackedLocalDeployment>,
    ) -> Result<TrackedDeployment> {
        let tracked = TrackedDeployment {
            name: name.clone(),
            deployment_id,
            token,
            manager_url,
            platform,
            helm,
            local,
            tracked_at: chrono::Utc::now().to_rfc3339(),
        };

        self.deployments.insert(name, tracked.clone());
        save_deployments(&self.file_path, &self.deployments)?;

        Ok(tracked)
    }

    /// Get a tracked deployment by name
    pub fn get(&self, name: &str) -> Option<&TrackedDeployment> {
        self.deployments.get(name)
    }

    /// List all tracked deployments
    pub fn list(&self) -> Vec<&TrackedDeployment> {
        self.deployments.values().collect()
    }

    /// Remove a tracked deployment
    pub fn remove(&mut self, name: &str) -> Result<Option<TrackedDeployment>> {
        let removed = self.deployments.remove(name);
        if removed.is_some() {
            save_deployments(&self.file_path, &self.deployments)?;
        }
        Ok(removed)
    }
}

fn tracking_file_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine config directory".to_string(),
        })
    })?;

    Ok(config_dir.join("alien").join("deployments.json"))
}

fn load_deployments(path: &PathBuf) -> Result<HashMap<String, TrackedDeployment>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read deployments file".to_string(),
        },
    )?;

    serde_json::from_str(&content)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse deployments file".to_string(),
        })
}

fn save_deployments(
    path: &PathBuf,
    deployments: &HashMap<String, TrackedDeployment>,
) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "create directory".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to create config directory".to_string(),
            },
        )?;
    }

    let content = serde_json::to_string_pretty(deployments)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize deployments".to_string(),
        })?;

    alien_core::file_utils::write_secret_file(path, content.as_bytes())
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write deployments file".to_string(),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_tracking_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "alien-deploy-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_empty_tracker() {
        let path = temp_tracking_path();
        let tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        assert!(tracker.list().is_empty());
        assert!(tracker.get("anything").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_track_and_get() {
        let path = temp_tracking_path();
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();

        tracker
            .track(
                "prod".to_string(),
                "dep_123".to_string(),
                Some("ax_dg_abc".to_string()),
                "https://manager.example.com".to_string(),
                "aws".to_string(),
                None,
                None,
            )
            .unwrap();

        let dep = tracker.get("prod").unwrap();
        assert_eq!(dep.deployment_id, "dep_123");
        assert_eq!(dep.token.as_deref(), Some("ax_dg_abc"));
        assert_eq!(dep.manager_url, "https://manager.example.com");
        assert_eq!(dep.platform, "aws");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_persistence_across_instances() {
        let path = temp_tracking_path();

        {
            let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();
            tracker
                .track(
                    "staging".to_string(),
                    "dep_456".to_string(),
                    Some("ax_dg_def".to_string()),
                    "https://manager.test.com".to_string(),
                    "local".to_string(),
                    None,
                    Some(TrackedLocalDeployment { data_dir: "/tmp/alien-agent-state".to_string(), service_managed: true, }),
                )
                .unwrap();
        }

        // New instance should load from disk
        let tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        let dep = tracker.get("staging").unwrap();
        assert_eq!(dep.deployment_id, "dep_456");
        assert_eq!(
            dep.local,
            Some(TrackedLocalDeployment {
                data_dir: "/tmp/alien-agent-state".to_string(),
                service_managed: true,
            })
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_remove_deployment() {
        let path = temp_tracking_path();
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();

        tracker
            .track(
                "prod".to_string(),
                "dep_1".to_string(),
                Some("tok".to_string()),
                "url".to_string(),
                "aws".to_string(),
                None,
                None,
            )
            .unwrap();

        let removed = tracker.remove("prod").unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().deployment_id, "dep_1");
        assert!(tracker.get("prod").is_none());

        // Remove non-existent returns None
        let removed = tracker.remove("nonexistent").unwrap();
        assert!(removed.is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_list_multiple() {
        let path = temp_tracking_path();
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();

        tracker
            .track(
                "a".into(),
                "dep_a".into(),
                Some("t".into()),
                "u".into(),
                "aws".into(),
                None,
                None,
            )
            .unwrap();
        tracker
            .track(
                "b".into(),
                "dep_b".into(),
                Some("t".into()),
                "u".into(),
                "gcp".into(),
                None,
                None,
            )
            .unwrap();

        let list = tracker.list();
        assert_eq!(list.len(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_track_overwrites_existing() {
        let path = temp_tracking_path();
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();

        tracker
            .track(
                "prod".into(),
                "dep_old".into(),
                Some("t".into()),
                "u".into(),
                "aws".into(),
                None,
                None,
            )
            .unwrap();
        tracker
            .track(
                "prod".into(),
                "dep_new".into(),
                Some("t".into()),
                "u".into(),
                "aws".into(),
                None,
                None,
            )
            .unwrap();

        assert_eq!(tracker.list().len(), 1);
        assert_eq!(tracker.get("prod").unwrap().deployment_id, "dep_new");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_tracked_deployment_serialization() {
        let dep = TrackedDeployment {
            name: "test".to_string(),
            deployment_id: "dep_123".to_string(),
            token: Some("ax_dg_abc".to_string()),
            helm: None,
            manager_url: "https://example.com".to_string(),
            platform: "aws".to_string(),
            local: None,
            tracked_at: "2025-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_value(&dep).unwrap();
        assert_eq!(json["deploymentId"], "dep_123");
        assert_eq!(json["managerUrl"], "https://example.com");

        let deserialized: TrackedDeployment = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.deployment_id, "dep_123");
    }

    #[test]
    fn a_file_written_before_helm_tracking_still_loads_and_keeps_its_token() {
        let path = temp_tracking_path();
        std::fs::write(
            &path,
            r#"{"prod":{"name":"prod","deploymentId":"dep_1","token":"ax_dep_saved","managerUrl":"https://m","platform":"kubernetes","trackedAt":"2026-01-01T00:00:00Z"}}"#,
        )
        .unwrap();
        let tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        let tracked = tracker.get("prod").unwrap();
        assert_eq!(tracked.token.as_deref(), Some("ax_dep_saved"));
        assert!(tracked.helm.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_kubernetes_install_is_saved_without_a_token_and_with_its_release() {
        let path = temp_tracking_path();
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        tracker
            .track(
                "prod".into(),
                "dep_1".into(),
                None,
                "https://m".into(),
                "kubernetes".into(),
                Some(TrackedHelmRelease {
                    release: "rel".into(),
                    namespace: "ns".into(),
                }),
                None,
            )
            .unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("token"), "no token on disk: {written}");
        let reloaded = DeploymentTracker::with_path(path.clone()).unwrap();
        assert_eq!(
            reloaded.get("prod").unwrap().helm,
            Some(TrackedHelmRelease {
                release: "rel".into(),
                namespace: "ns".into()
            })
        );
        let _ = std::fs::remove_file(&path);
    }
}
