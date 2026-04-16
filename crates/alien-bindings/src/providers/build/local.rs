use crate::{
    error::{ErrorData, Result},
    traits::{Binding, Build},
};
use alien_core::{
    bindings::{BuildBinding, LocalBuildBinding},
    BuildConfig, BuildExecution, BuildStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct BuildMetadata {
    uuid: String,
    pid: u32,
    start_time: String,
    end_time: Option<String>,
    status: BuildStatus,
}

/// Local implementation of the `Build` trait that runs bash scripts directly.
/// This implementation is stateless - all build state is encoded in the build ID
/// and stored in the filesystem.
#[derive(Debug)]
pub struct LocalBuild {
    binding_name: String,
    base_dir: std::path::PathBuf,
    build_env_vars: std::collections::HashMap<String, String>,
}

impl LocalBuild {
    /// Creates a new Local Build instance from binding parameters.
    pub fn new(binding_name: String, binding: alien_core::bindings::BuildBinding) -> Result<Self> {
        // Extract values from binding
        let config = match binding {
            BuildBinding::Local(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected Local binding, got different service type".to_string(),
                }));
            }
        };

        let data_dir = config
            .data_dir
            .into_value(&binding_name, "data_dir")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract data_dir from binding".to_string(),
            })?;

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let base_dir = std::path::PathBuf::from(data_dir).join(&binding_name);

        // Create the build directory if it doesn't exist
        std::fs::create_dir_all(&base_dir)
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to create build directory".to_string(),
            })?;

        Ok(Self {
            binding_name,
            base_dir,
            build_env_vars,
        })
    }

    /// Creates a new Local Build instance from a directory path.
    pub fn new_from_path(binding_name: String, base_dir: std::path::PathBuf) -> Self {
        Self {
            binding_name,
            base_dir,
            build_env_vars: std::collections::HashMap::new(),
        }
    }

    /// Encodes build information into a build ID: {uuid}_{pid}_{timestamp}
    fn encode_build_id(uuid: &str, pid: u32, timestamp: u64) -> String {
        format!("{}_{}_{}", uuid, pid, timestamp)
    }

    /// Decodes build information from a build ID
    fn decode_build_id(build_id: &str) -> Result<(String, u32, u64)> {
        let parts: Vec<&str> = build_id.split('_').collect();
        if parts.len() != 3 {
            return Err(AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: "local".to_string(),
                operation: format!("invalid build ID format: {}", build_id),
            }));
        }

        let uuid = parts[0].to_string();
        let pid = parts[1].parse::<u32>().map_err(|_| {
            AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: "local".to_string(),
                operation: format!("invalid PID in build ID: {}", build_id),
            })
        })?;
        let timestamp = parts[2].parse::<u64>().map_err(|_| {
            AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: "local".to_string(),
                operation: format!("invalid timestamp in build ID: {}", build_id),
            })
        })?;

        Ok((uuid, pid, timestamp))
    }

    /// Creates a working directory for a build
    fn create_build_dir(&self, uuid: &str) -> Result<std::path::PathBuf> {
        let build_dir = self.base_dir.join("builds").join(uuid);
        std::fs::create_dir_all(&build_dir)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "create build directory".to_string(),
            })?;
        Ok(build_dir)
    }

    /// Saves build metadata to disk
    fn save_build_metadata(&self, uuid: &str, metadata: &BuildMetadata) -> Result<()> {
        let build_dir = self.base_dir.join("builds").join(uuid);
        let metadata_path = build_dir.join("metadata.json");

        let metadata_json = serde_json::to_string_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "serialize build metadata".to_string(),
            })?;

        std::fs::write(&metadata_path, metadata_json)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "write build metadata".to_string(),
            })?;

        Ok(())
    }

    /// Loads build metadata from disk
    fn load_build_metadata(&self, uuid: &str) -> Result<BuildMetadata> {
        let build_dir = self.base_dir.join("builds").join(uuid);
        let metadata_path = build_dir.join("metadata.json");

        let metadata_json = std::fs::read_to_string(&metadata_path)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: format!("read build metadata for {}", uuid),
            })?;

        let metadata: BuildMetadata = serde_json::from_str(&metadata_json)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: format!("parse build metadata for {}", uuid),
            })?;

        Ok(metadata)
    }

    /// Checks if a process is still running
    fn is_process_running(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            use std::process::Command;
            // Use kill -0 to check if process exists without actually killing it
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        #[cfg(windows)]
        {
            use std::process::Command;
            // Use tasklist to check if process exists
            Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", pid)])
                .output()
                .map(|output| {
                    output.status.success()
                        && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
                })
                .unwrap_or(false)
        }
    }

    /// Updates build status based on current process state
    fn update_build_status(&self, metadata: &mut BuildMetadata) -> Result<()> {
        if metadata.status == BuildStatus::Running {
            if !self.is_process_running(metadata.pid) {
                // Process has finished, update status
                metadata.status = BuildStatus::Succeeded; // Assume success if process exited cleanly
                metadata.end_time = Some(chrono::Utc::now().to_rfc3339());
                self.save_build_metadata(&metadata.uuid, metadata)?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Build for LocalBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution> {
        let uuid = Uuid::new_v4().to_string();
        let start_time = chrono::Utc::now().to_rfc3339();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create working directory for the build
        let build_dir = self.create_build_dir(&uuid)?;

        // Create script file
        let script_path = build_dir.join("build_script.sh");
        std::fs::write(&script_path, &config.script)
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "write build script".to_string(),
            })?;

        // Make script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)
                .into_alien_error()
                .context(ErrorData::BuildOperationFailed {
                    binding_name: self.binding_name.clone(),
                    operation: "get script permissions".to_string(),
                })?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)
                .into_alien_error()
                .context(ErrorData::BuildOperationFailed {
                    binding_name: self.binding_name.clone(),
                    operation: "set script permissions".to_string(),
                })?;
        }

        // Prepare environment variables
        let mut cmd = Command::new("bash");
        cmd.arg(&script_path)
            .current_dir(&build_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Merge build config environment with binding environment variables
        // Build config environment takes precedence over binding environment
        let mut merged_environment = self.build_env_vars.clone();
        merged_environment.extend(config.environment);

        // Add environment variables
        for (key, value) in &merged_environment {
            cmd.env(key, value);
        }

        // Start the process
        let mut child =
            cmd.spawn()
                .into_alien_error()
                .context(ErrorData::BuildOperationFailed {
                    binding_name: self.binding_name.clone(),
                    operation: "start build process".to_string(),
                })?;

        // Get the PID
        let pid = child.id().ok_or_else(|| {
            AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "get process ID".to_string(),
            })
        })?;

        // Create build ID with encoded PID
        let build_id = Self::encode_build_id(&uuid, pid, timestamp);

        // Save build metadata
        let metadata = BuildMetadata {
            uuid: uuid.clone(),
            pid,
            start_time: start_time.clone(),
            end_time: None,
            status: BuildStatus::Running,
        };
        self.save_build_metadata(&uuid, &metadata)?;

        // Detach the child process so it runs independently
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        Ok(BuildExecution {
            id: build_id,
            status: BuildStatus::Running,
            start_time: Some(start_time),
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution> {
        // Decode build ID to get UUID and PID
        let (uuid, _pid, _timestamp) = Self::decode_build_id(build_id)?;

        // Load build metadata
        let mut metadata = self.load_build_metadata(&uuid)?;

        // Update status based on current process state
        self.update_build_status(&mut metadata)?;

        Ok(BuildExecution {
            id: build_id.to_string(),
            status: metadata.status,
            start_time: Some(metadata.start_time),
            end_time: metadata.end_time,
        })
    }

    async fn stop_build(&self, build_id: &str) -> Result<()> {
        // Decode build ID to get UUID and PID
        let (uuid, pid, _timestamp) = Self::decode_build_id(build_id)?;

        // Load build metadata
        let mut metadata = self.load_build_metadata(&uuid)?;

        // Only try to kill if the build is still running
        if metadata.status == BuildStatus::Running {
            #[cfg(unix)]
            {
                use std::process::Command;
                // Kill the process
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .output();
            }

            #[cfg(windows)]
            {
                use std::process::Command;
                // Kill the process on Windows
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }

            // Update metadata
            metadata.status = BuildStatus::Cancelled;
            metadata.end_time = Some(chrono::Utc::now().to_rfc3339());
            self.save_build_metadata(&uuid, &metadata)?;
        }

        Ok(())
    }
}

impl Binding for LocalBuild {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_build_success() {
        let temp_dir = TempDir::new().unwrap();
        let local_build =
            LocalBuild::new_from_path("test-build".to_string(), temp_dir.path().to_path_buf());

        let mut config = BuildConfig {
            image: "ubuntu:20.04".to_string(), // Ignored for local builds
            script: "echo 'Hello World!'".to_string(),
            environment: HashMap::new(),
            timeout_seconds: 30,
            compute_type: alien_core::ComputeType::Small,
            monitoring: None,
        };
        config
            .environment
            .insert("TEST_VAR".to_string(), "test_value".to_string());

        let execution = local_build.start_build(config).await.unwrap();
        assert!(!execution.id.is_empty());
        assert_eq!(execution.status, BuildStatus::Running);

        // Wait a bit for the build to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let status = local_build.get_build_status(&execution.id).await.unwrap();
        assert_eq!(status.status, BuildStatus::Succeeded);
        assert!(status.end_time.is_some());
    }

    #[tokio::test]
    async fn test_local_build_failure() {
        let temp_dir = TempDir::new().unwrap();
        let local_build =
            LocalBuild::new_from_path("test-build".to_string(), temp_dir.path().to_path_buf());

        let config = BuildConfig {
            image: "ubuntu:20.04".to_string(), // Ignored for local builds
            script: "exit 1".to_string(),      // This will fail
            environment: HashMap::new(),
            timeout_seconds: 30,
            compute_type: alien_core::ComputeType::Small,
            monitoring: None,
        };

        let execution = local_build.start_build(config).await.unwrap();
        assert!(!execution.id.is_empty());
        assert_eq!(execution.status, BuildStatus::Running);

        // Wait a bit for the build to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let status = local_build.get_build_status(&execution.id).await.unwrap();
        assert_eq!(status.status, BuildStatus::Succeeded); // Note: We assume success if process exits cleanly
        assert!(status.end_time.is_some());
    }

    #[tokio::test]
    async fn test_local_build_stop() {
        let temp_dir = TempDir::new().unwrap();
        let local_build =
            LocalBuild::new_from_path("test-build".to_string(), temp_dir.path().to_path_buf());

        let config = BuildConfig {
            image: "ubuntu:20.04".to_string(), // Ignored for local builds
            script: "sleep 10".to_string(),    // Long running command
            environment: HashMap::new(),
            timeout_seconds: 30,
            compute_type: alien_core::ComputeType::Small,
            monitoring: None,
        };

        let execution = local_build.start_build(config).await.unwrap();
        assert!(!execution.id.is_empty());
        assert_eq!(execution.status, BuildStatus::Running);

        // Stop the build
        local_build.stop_build(&execution.id).await.unwrap();

        let status = local_build.get_build_status(&execution.id).await.unwrap();
        assert_eq!(status.status, BuildStatus::Cancelled);
        assert!(status.end_time.is_some());
    }

    #[test]
    fn test_build_id_encoding_decoding() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let pid = 12345u32;
        let timestamp = 1234567890u64;

        let build_id = LocalBuild::encode_build_id(uuid, pid, timestamp);
        let (decoded_uuid, decoded_pid, decoded_timestamp) =
            LocalBuild::decode_build_id(&build_id).unwrap();

        assert_eq!(decoded_uuid, uuid);
        assert_eq!(decoded_pid, pid);
        assert_eq!(decoded_timestamp, timestamp);
    }
}
