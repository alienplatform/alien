//! Deployment tracking — stores tracked deployments in a local JSON file.
//!
//! Unlike the platform project-cli which uses keyring, the OSS deploy-cli
//! stores deployment info in `~/.config/alien/deployments.json`.

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
    /// Authentication token
    pub token: String,
    /// Manager URL
    pub manager_url: String,
    /// Target platform
    pub platform: String,
    /// When the deployment was tracked
    pub tracked_at: String,
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
        token: String,
        manager_url: String,
        platform: String,
    ) -> Result<TrackedDeployment> {
        let tracked = TrackedDeployment {
            name: name.clone(),
            deployment_id,
            token,
            manager_url,
            platform,
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
                "tok_abc".to_string(),
                "https://manager.example.com".to_string(),
                "aws".to_string(),
            )
            .unwrap();

        let dep = tracker.get("prod").unwrap();
        assert_eq!(dep.deployment_id, "dep_123");
        assert_eq!(dep.token, "tok_abc");
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
                    "tok_def".to_string(),
                    "https://manager.test.com".to_string(),
                    "local".to_string(),
                )
                .unwrap();
        }

        // New instance should load from disk
        let tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        let dep = tracker.get("staging").unwrap();
        assert_eq!(dep.deployment_id, "dep_456");

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
                "tok".to_string(),
                "url".to_string(),
                "aws".to_string(),
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
                "t".into(),
                "u".into(),
                "aws".into(),
            )
            .unwrap();
        tracker
            .track(
                "b".into(),
                "dep_b".into(),
                "t".into(),
                "u".into(),
                "gcp".into(),
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
                "t".into(),
                "u".into(),
                "aws".into(),
            )
            .unwrap();
        tracker
            .track(
                "prod".into(),
                "dep_new".into(),
                "t".into(),
                "u".into(),
                "aws".into(),
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
            token: "tok_abc".to_string(),
            manager_url: "https://example.com".to_string(),
            platform: "aws".to_string(),
            tracked_at: "2025-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_value(&dep).unwrap();
        assert_eq!(json["deploymentId"], "dep_123");
        assert_eq!(json["managerUrl"], "https://example.com");

        let deserialized: TrackedDeployment = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.deployment_id, "dep_123");
    }
}
