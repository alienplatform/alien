//! Agent configuration.
//!
//! Configuration can be built from CLI arguments or programmatically via the builder.

use alien_core::{Platform, StackSettings, TelemetryMode, UpdatesMode};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// Sync configuration for connecting to remote management server
///
/// When `None`, the agent runs in airgapped mode:
/// - No sync loop (doesn't connect to management server)
/// - No telemetry push loop
/// - Deployment loop runs with locally-stored target
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    /// Management server URL to sync with
    pub url: Url,
    /// Authentication token (agent token)
    pub token: String,
}

/// Agent configuration
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into))]
pub struct AgentConfig {
    /// Target cloud platform
    pub platform: Platform,

    /// Sync configuration (None = airgapped mode)
    pub sync: Option<SyncConfig>,

    /// Data directory for encrypted database
    #[builder(default = ".alien-agent".to_string())]
    pub data_dir: String,

    /// Encryption key for database (64-character hex string for AEGIS-256)
    pub encryption_key: String,

    /// Sync interval in seconds
    #[builder(default = 30)]
    pub sync_interval_seconds: u64,

    /// Deployment check interval in seconds
    #[builder(default = 1)]
    pub deployment_interval_seconds: u64,

    /// Telemetry push interval in seconds
    #[builder(default = 10)]
    pub telemetry_interval_seconds: u64,

    /// Commands dispatch poll interval in seconds (for cloud function platforms).
    /// The agent polls the manager's lease API at this interval and dispatches
    /// leased commands to the deployed function via platform-native push.
    #[builder(default = 5)]
    pub commands_interval_seconds: u64,

    /// OTLP server port (for local functions to send telemetry)
    #[builder(default = 4318)]
    pub otlp_server_port: u16,

    /// HTTP server port for airgapped CLI APIs (None = disabled)
    pub api_server_port: Option<u16>,

    /// Kubernetes namespace (Kubernetes platform only)
    pub namespace: Option<String>,

    /// Public URLs for exposed resources (Kubernetes platform only).
    /// Maps resource ID to public URL (e.g., {"api": "https://api.acme.com"}).
    pub public_urls: Option<HashMap<String, String>>,

    /// Stack settings for deployment customization.
    pub stack_settings: Option<StackSettings>,
}

impl AgentConfig {
    /// Check if running in airgapped mode (no sync configuration)
    pub fn is_airgapped(&self) -> bool {
        self.sync.is_none()
    }

    /// Check if deployment updates require manual approval.
    pub fn requires_deployment_approval(&self) -> bool {
        self.stack_settings
            .as_ref()
            .map(|s| s.updates == UpdatesMode::ApprovalRequired)
            .unwrap_or(false)
    }

    /// Check if telemetry collection requires manual approval.
    pub fn requires_telemetry_approval(&self) -> bool {
        self.stack_settings
            .as_ref()
            .map(|s| s.telemetry == TelemetryMode::ApprovalRequired)
            .unwrap_or(false)
    }

    /// Check if telemetry collection is enabled.
    pub fn is_telemetry_enabled(&self) -> bool {
        self.stack_settings
            .as_ref()
            .map(|s| s.telemetry != TelemetryMode::Off)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder_with_defaults() {
        let config = AgentConfig::builder()
            .platform(Platform::Aws)
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_agent_xyz".to_string(),
            }))
            .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .build();

        assert_eq!(config.data_dir, ".alien-agent");
        assert_eq!(config.sync_interval_seconds, 30);
        assert_eq!(config.deployment_interval_seconds, 1);
        assert_eq!(config.otlp_server_port, 4318);
        assert!(!config.is_airgapped());
        assert!(!config.requires_deployment_approval());
    }

    #[test]
    fn test_config_builder_with_overrides() {
        let config = AgentConfig::builder()
            .platform(Platform::Kubernetes)
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_agent_abc".to_string(),
            }))
            .encryption_key("key")
            .data_dir("/var/agent")
            .sync_interval_seconds(60)
            .maybe_stack_settings(Some(alien_core::StackSettings {
                updates: alien_core::UpdatesMode::ApprovalRequired,
                telemetry: alien_core::TelemetryMode::Auto,
                heartbeats: alien_core::HeartbeatsMode::On,
                deployment_model: alien_core::DeploymentModel::Pull,
                network: None,
                domains: None,
                external_bindings: None,
            }))
            .api_server_port(8080)
            .build();

        assert_eq!(config.data_dir, "/var/agent");
        assert_eq!(config.sync_interval_seconds, 60);
        assert!(config.requires_deployment_approval());
        assert!(!config.requires_telemetry_approval());
        assert!(config.is_telemetry_enabled());
        assert_eq!(config.api_server_port, Some(8080));
    }

    #[test]
    fn test_airgapped_mode() {
        let config = AgentConfig::builder()
            .platform(Platform::Local)
            .encryption_key("key")
            .api_server_port(8080)
            .build();

        assert!(config.is_airgapped());
    }

    #[test]
    fn test_deployment_approval_required() {
        let config = AgentConfig::builder()
            .platform(Platform::Kubernetes)
            .encryption_key("key")
            .maybe_stack_settings(Some(alien_core::StackSettings {
                updates: alien_core::UpdatesMode::ApprovalRequired,
                telemetry: alien_core::TelemetryMode::Auto,
                heartbeats: alien_core::HeartbeatsMode::On,
                deployment_model: alien_core::DeploymentModel::Pull,
                network: None,
                domains: None,
                external_bindings: None,
            }))
            .build();

        assert!(config.requires_deployment_approval());
        assert!(!config.requires_telemetry_approval());
        assert!(config.is_telemetry_enabled());
    }

    #[test]
    fn test_telemetry_approval_required() {
        let config = AgentConfig::builder()
            .platform(Platform::Kubernetes)
            .encryption_key("key")
            .maybe_stack_settings(Some(alien_core::StackSettings {
                updates: alien_core::UpdatesMode::Auto,
                telemetry: alien_core::TelemetryMode::ApprovalRequired,
                heartbeats: alien_core::HeartbeatsMode::On,
                deployment_model: alien_core::DeploymentModel::Pull,
                network: None,
                domains: None,
                external_bindings: None,
            }))
            .build();

        assert!(!config.requires_deployment_approval());
        assert!(config.requires_telemetry_approval());
        assert!(config.is_telemetry_enabled());
    }

    #[test]
    fn test_telemetry_disabled() {
        let config = AgentConfig::builder()
            .platform(Platform::Kubernetes)
            .encryption_key("key")
            .maybe_stack_settings(Some(alien_core::StackSettings {
                updates: alien_core::UpdatesMode::Auto,
                telemetry: alien_core::TelemetryMode::Off,
                heartbeats: alien_core::HeartbeatsMode::On,
                deployment_model: alien_core::DeploymentModel::Pull,
                network: None,
                domains: None,
                external_bindings: None,
            }))
            .build();

        assert!(!config.requires_telemetry_approval());
        assert!(!config.is_telemetry_enabled());
    }

    #[test]
    fn test_no_stack_settings_defaults() {
        let config = AgentConfig::builder()
            .platform(Platform::Kubernetes)
            .encryption_key("key")
            .build();

        assert!(!config.requires_deployment_approval());
        assert!(!config.requires_telemetry_approval());
        assert!(config.is_telemetry_enabled());
    }
}
