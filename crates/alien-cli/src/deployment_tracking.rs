//! Deployment tracking and storage functionality for the Alien CLI
//!
//! This module handles securely storing deployment information (name, ID, API key)
//! and managing deployment registration with the platform.

use crate::error::{ErrorData, Result};
use alien_platform_api::SdkResultExt;
use alien_platform_api::{
    types::{Subject, SubjectScope},
    Client as SdkClient,
};
use alien_error::{AlienError, Context, IntoAlienError};
use dirs::config_dir;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(debug_assertions)]
use debug_keyring::Entry;
#[cfg(not(debug_assertions))]
use keyring::Entry;

const SERVICE: &str = "alien-cli";
const DEPLOYMENTS_KEY: &str = "tracked_deployments";

/// Information about a tracked deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedDeployment {
    /// User-provided name for the deployment
    pub name: String,
    /// Deployment ID from the platform
    pub deployment_id: String,
    /// Deployment API key for authentication
    pub api_key: String,
    /// Workspace ID the deployment belongs to
    pub workspace_id: String,
    /// Project ID the deployment belongs to
    pub project_id: String,
}

/// Deployment tracking manager
pub struct DeploymentTracker {
    /// All tracked deployments by name
    deployments: HashMap<String, TrackedDeployment>,
}

impl DeploymentTracker {
    /// Create a new deployment tracker and load existing deployments
    pub fn new() -> Result<Self> {
        let deployments = load_tracked_deployments()?;
        Ok(Self { deployments })
    }

    /// Add a new deployment after validating it with the platform
    pub async fn add_deployment(
        &mut self,
        name: String,
        api_key: String,
        base_url: &str,
    ) -> Result<TrackedDeployment> {
        // Call whoami to validate the deployment and get info
        let deployment_info = validate_deployment_api_key(&api_key, base_url).await?;

        let tracked = TrackedDeployment {
            name: name.clone(),
            deployment_id: deployment_info.deployment_id,
            api_key,
            workspace_id: deployment_info.workspace_id,
            project_id: deployment_info.project_id,
        };

        // Store in memory
        self.deployments.insert(name, tracked.clone());

        // Persist to keyring
        save_tracked_deployments(&self.deployments)?;

        Ok(tracked)
    }

    /// Get a tracked deployment by name
    pub fn get_deployment(&self, name: &str) -> Option<&TrackedDeployment> {
        self.deployments.get(name)
    }

    /// List all tracked deployments
    pub fn list_deployments(&self) -> Vec<&TrackedDeployment> {
        self.deployments.values().collect()
    }

    /// Remove a tracked deployment
    pub fn remove_deployment(&mut self, name: &str) -> Result<Option<TrackedDeployment>> {
        let removed = self.deployments.remove(name);
        if removed.is_some() {
            save_tracked_deployments(&self.deployments)?;
        }
        Ok(removed)
    }
}

/// Information about a deployment token (deployment or deployment-group scoped)
#[derive(Debug, Clone)]
pub enum DeploymentToken {
    /// Deployment-scoped token (for existing deployments)
    Deployment {
        deployment_id: String,
        project_id: String,
        workspace_id: String,
    },
    /// Deployment-group-scoped token (for creating new deployments)
    DeploymentGroup {
        deployment_group_id: String,
        deployment_group_name: String,
        project_id: String,
        workspace_id: String,
        max_deployments: u32,
    },
}

/// Information returned from deployment validation
#[derive(Debug)]
pub struct ValidatedDeploymentInfo {
    pub deployment_id: String,
    pub workspace_id: String,
    pub project_id: String,
}

/// Validate a deployment token (agent or deployment-group scoped)
pub async fn validate_token(api_key: &str, base_url: &str) -> Result<DeploymentToken> {
    // Create authenticated reqwest client
    let auth_value = format!("Bearer {}", api_key);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let reqwest_client = Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    // Create SDK client with the authenticated reqwest client
    let sdk_client = SdkClient::new_with_client(base_url, reqwest_client);

    // Call whoami endpoint
    let response =
        sdk_client
            .whoami()
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Failed to validate token with platform".to_string(),
            })?;

    let subject = response.into_inner();

    // Extract token type and information based on the subject scope
    match subject {
        Subject::ServiceAccountSubject(sa) => {
            match sa.scope {
                // Deployment-scoped token: for existing deployments
                SubjectScope::Deployment {
                    deployment_id,
                    project_id,
                } => Ok(DeploymentToken::Deployment {
                    deployment_id,
                    project_id,
                    workspace_id: sa.workspace_id,
                }),
                // Deployment-group-scoped token: for creating new deployments
                SubjectScope::DeploymentGroup {
                    deployment_group_id,
                    project_id: _,
                } => {
                    // Fetch deployment group details
                    let deployment_group = fetch_deployment_group(
                        &deployment_group_id,
                        &sa.workspace_id,
                        api_key,
                        base_url,
                    )
                    .await?;

                    Ok(DeploymentToken::DeploymentGroup {
                        deployment_group_id: deployment_group.id.to_string(),
                        deployment_group_name: deployment_group.name.to_string(),
                        project_id: deployment_group.project_id.to_string(),
                        workspace_id: sa.workspace_id,
                        max_deployments: deployment_group.max_deployments.get() as u32,
                    })
                }
                _ => Err(AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: "Token must be deployment-scoped or deployment-group-scoped"
                        .to_string(),
                })),
            }
        }
        Subject::UserSubject(_) => Err(AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: "API key must be for a service account, not a user".to_string(),
        })),
    }
}

/// Fetch deployment group details from the API
async fn fetch_deployment_group(
    deployment_group_id: &str,
    workspace_id: &str,
    api_key: &str,
    base_url: &str,
) -> Result<alien_platform_api::types::GetDeploymentGroupResponse> {
    // Create authenticated reqwest client
    let auth_value = format!("Bearer {}", api_key);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let reqwest_client = Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    // Create SDK client
    let sdk_client = SdkClient::new_with_client(base_url, reqwest_client);

    // Fetch deployment group
    let response = sdk_client
        .get_deployment_group()
        .id(deployment_group_id)
        .workspace(workspace_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_group".to_string(),
            message: format!("Failed to fetch deployment group: {}", deployment_group_id),
        })?;

    Ok(response.into_inner())
}

/// Validate a deployment API key by calling the whoami endpoint
async fn validate_deployment_api_key(
    api_key: &str,
    base_url: &str,
) -> Result<ValidatedDeploymentInfo> {
    let token = validate_token(api_key, base_url).await?;

    match token {
        DeploymentToken::Deployment {
            deployment_id,
            workspace_id,
            project_id,
        } => Ok(ValidatedDeploymentInfo {
            deployment_id,
            workspace_id,
            project_id,
        }),
        DeploymentToken::DeploymentGroup { .. } => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Expected deployment token, got deployment-group token".to_string(),
            }))
        }
    }
}

/// Load tracked deployments from keyring
fn load_tracked_deployments() -> Result<HashMap<String, TrackedDeployment>> {
    let entry = Entry::new(SERVICE, DEPLOYMENTS_KEY)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create keyring entry for deployments".to_string(),
        })?;

    match entry.get_password() {
        Ok(data) => serde_json::from_str(&data)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialize".to_string(),
                reason: "Failed to parse tracked deployments data".to_string(),
            }),
        Err(_) => {
            // No existing data, return empty map
            Ok(HashMap::new())
        }
    }
}

/// Save tracked deployments to keyring
fn save_tracked_deployments(deployments: &HashMap<String, TrackedDeployment>) -> Result<()> {
    let entry = Entry::new(SERVICE, DEPLOYMENTS_KEY)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create keyring entry for deployments".to_string(),
        })?;

    let data = serde_json::to_string(deployments)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize tracked deployments data".to_string(),
        })?;

    entry
        .set_password(&data)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to store tracked deployments in keyring".to_string(),
        })?;

    Ok(())
}

/// Simple file-based keyring for debug builds to avoid macOS keychain prompts
#[cfg(debug_assertions)]
mod debug_keyring {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug)]
    pub struct DebugKeyringError(String);

    impl std::fmt::Display for DebugKeyringError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for DebugKeyringError {}

    pub struct Entry {
        service: String,
        user: String,
    }

    impl Entry {
        pub fn new(service: &str, user: &str) -> std::result::Result<Self, DebugKeyringError> {
            Ok(Self {
                service: service.to_string(),
                user: user.to_string(),
            })
        }

        pub fn set_password(&self, password: &str) -> std::result::Result<(), DebugKeyringError> {
            let mut store = self.load_store()?;
            let key = format!("{}:{}", self.service, self.user);
            store.insert(key, password.to_string());
            self.save_store(&store)
        }

        pub fn get_password(&self) -> std::result::Result<String, DebugKeyringError> {
            let store = self.load_store()?;
            let key = format!("{}:{}", self.service, self.user);
            store
                .get(&key)
                .cloned()
                .ok_or_else(|| DebugKeyringError("No entry found".to_string()))
        }

        pub fn delete_password(&self) -> std::result::Result<(), DebugKeyringError> {
            let mut store = self.load_store()?;
            let key = format!("{}:{}", self.service, self.user);
            store.remove(&key);
            self.save_store(&store)
        }

        fn keyring_path(&self) -> PathBuf {
            super::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("alien")
                .join("cli-keyring.json")
        }

        fn load_store(&self) -> std::result::Result<HashMap<String, String>, DebugKeyringError> {
            let path = self.keyring_path();
            if path.exists() {
                let content = fs::read_to_string(path).map_err(|e| {
                    DebugKeyringError(format!("Failed to read keyring file: {}", e))
                })?;
                Ok(serde_json::from_str(&content).unwrap_or_default())
            } else {
                Ok(HashMap::new())
            }
        }

        fn save_store(
            &self,
            store: &HashMap<String, String>,
        ) -> std::result::Result<(), DebugKeyringError> {
            let path = self.keyring_path();
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| {
                    DebugKeyringError(format!("Failed to create config dir: {}", e))
                })?;
            }
            let content = serde_json::to_string_pretty(store)
                .map_err(|e| DebugKeyringError(format!("Failed to serialize keyring: {}", e)))?;
            fs::write(path, content)
                .map_err(|e| DebugKeyringError(format!("Failed to write keyring file: {}", e)))?;
            Ok(())
        }
    }
}
