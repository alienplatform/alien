//! Embedded configuration support for alien-deploy-cli and alien-agent binaries.
//!
//! The package builder appends a JSON-encoded config struct to the end of the
//! binary, followed by a 4-byte little-endian length and 8-byte magic trailer.
//! This allows a single binary to be customized per deployment group without
//! recompilation.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Magic bytes at the end of a binary with embedded config.
pub const MAGIC_BYTES: &[u8; 8] = b"ALIENCFG";

/// Size of the footer: 4 bytes (length) + 8 bytes (magic).
pub const FOOTER_SIZE: usize = 12;

/// Configuration embedded in alien-deploy-cli binaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployCliConfig {
    // --- Connection (for pre-configured/OSS binaries) ---
    /// Manager URL to connect to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manager_url: Option<String>,
    /// Authentication token for the manager API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Deployment group ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_group_id: Option<String>,
    /// Default platform for deployments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_platform: Option<String>,
    // --- Branding (for white-labeled SaaS binaries) ---
    /// Binary name (e.g., "acme-deploy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-friendly display name (e.g., "Acme Deploy CLI").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Configuration embedded in alien-agent binaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    // --- Connection (for pre-configured/OSS binaries) ---
    /// Manager URL to connect to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manager_url: Option<String>,
    /// Authentication token for the manager API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Deployment ID this agent manages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    /// Sync interval in seconds (default: 30).
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u64,
    // --- Branding (for white-labeled SaaS binaries) ---
    /// Binary name (e.g., "acme-agent").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-friendly display name (e.g., "Acme Agent").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

fn default_sync_interval() -> u64 {
    30
}

/// Load embedded configuration from the current binary.
///
/// Reads the binary's own file, checks for the magic trailer, extracts the
/// JSON payload, and deserializes it into the requested type.
///
/// Returns `None` if the binary has no embedded config (no magic trailer).
pub fn load_embedded_config<T: DeserializeOwned>() -> Result<Option<T>, EmbeddedConfigError> {
    let exe_path = std::env::current_exe().map_err(EmbeddedConfigError::Io)?;
    load_embedded_config_from_path(&exe_path)
}

/// Load embedded configuration from a specific binary path.
pub fn load_embedded_config_from_path<T: DeserializeOwned>(
    path: &std::path::Path,
) -> Result<Option<T>, EmbeddedConfigError> {
    let data = std::fs::read(path).map_err(EmbeddedConfigError::Io)?;

    if data.len() < FOOTER_SIZE {
        return Ok(None);
    }

    // Check magic bytes at the end
    let magic_start = data.len() - MAGIC_BYTES.len();
    if &data[magic_start..] != MAGIC_BYTES {
        return Ok(None);
    }

    // Read the 4-byte little-endian length before the magic
    let len_start = magic_start - 4;
    let len_bytes: [u8; 4] = data[len_start..magic_start]
        .try_into()
        .map_err(|_| EmbeddedConfigError::InvalidFormat("invalid length bytes".into()))?;
    let json_len = u32::from_le_bytes(len_bytes) as usize;

    if json_len == 0 || len_start < json_len {
        return Err(EmbeddedConfigError::InvalidFormat(
            "config length exceeds binary size".into(),
        ));
    }

    let json_start = len_start - json_len;
    let json_bytes = &data[json_start..len_start];

    let config: T =
        serde_json::from_slice(json_bytes).map_err(EmbeddedConfigError::Deserialization)?;

    Ok(Some(config))
}

/// Append embedded configuration to a binary.
///
/// Writes: original binary bytes + JSON payload + 4-byte LE length + magic bytes.
pub fn append_embedded_config<T: Serialize>(
    binary_data: &[u8],
    config: &T,
) -> Result<Vec<u8>, EmbeddedConfigError> {
    let json_bytes = serde_json::to_vec(config).map_err(EmbeddedConfigError::Deserialization)?;
    let json_len = json_bytes.len() as u32;

    let mut result = Vec::with_capacity(binary_data.len() + json_bytes.len() + FOOTER_SIZE);
    result.extend_from_slice(binary_data);
    result.extend_from_slice(&json_bytes);
    result.extend_from_slice(&json_len.to_le_bytes());
    result.extend_from_slice(MAGIC_BYTES);

    Ok(result)
}

/// Errors from embedded config operations.
#[derive(Debug)]
pub enum EmbeddedConfigError {
    Io(std::io::Error),
    InvalidFormat(String),
    Deserialization(serde_json::Error),
}

impl std::fmt::Display for EmbeddedConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error reading embedded config: {}", e),
            Self::InvalidFormat(msg) => write!(f, "invalid embedded config format: {}", msg),
            Self::Deserialization(e) => write!(f, "failed to deserialize embedded config: {}", e),
        }
    }
}

impl std::error::Error for EmbeddedConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_deploy_cli_config() {
        let config = DeployCliConfig {
            manager_url: Some("https://manager.example.com".into()),
            token: Some("tok_abc123".into()),
            deployment_group_id: Some("dg_xyz".into()),
            display_name: Some("Production".into()),
            default_platform: Some("aws".into()),
            name: Some("acme-deploy".into()),
        };

        let binary = b"fake binary content";
        let embedded = append_embedded_config(binary, &config).unwrap();

        let loaded: Option<DeployCliConfig> =
            load_embedded_config_from_path_bytes(&embedded).unwrap();
        let loaded = loaded.unwrap();

        assert_eq!(loaded.manager_url, config.manager_url);
        assert_eq!(loaded.token, config.token);
        assert_eq!(loaded.deployment_group_id, config.deployment_group_id);
        assert_eq!(loaded.display_name, config.display_name);
        assert_eq!(loaded.name, config.name);
    }

    #[test]
    fn test_no_embedded_config() {
        let binary = b"just a regular binary";
        let result: Option<DeployCliConfig> = load_embedded_config_from_path_bytes(binary).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_roundtrip_agent_config() {
        let config = AgentConfig {
            manager_url: Some("https://manager.example.com".into()),
            token: Some("tok_agent123".into()),
            deployment_id: Some("ag_abc".into()),
            sync_interval_secs: 60,
            name: Some("acme-agent".into()),
            display_name: Some("Acme Agent".into()),
        };

        let binary = b"agent binary";
        let embedded = append_embedded_config(binary, &config).unwrap();

        let loaded: Option<AgentConfig> = load_embedded_config_from_path_bytes(&embedded).unwrap();
        let loaded = loaded.unwrap();

        assert_eq!(loaded.manager_url, config.manager_url);
        assert_eq!(loaded.deployment_id, config.deployment_id);
        assert_eq!(loaded.sync_interval_secs, 60);
        assert_eq!(loaded.name, config.name);
    }

    /// Helper that works on in-memory bytes (for tests that don't need files).
    fn load_embedded_config_from_path_bytes<T: DeserializeOwned>(
        data: &[u8],
    ) -> Result<Option<T>, EmbeddedConfigError> {
        if data.len() < FOOTER_SIZE {
            return Ok(None);
        }

        let magic_start = data.len() - MAGIC_BYTES.len();
        if &data[magic_start..] != MAGIC_BYTES {
            return Ok(None);
        }

        let len_start = magic_start - 4;
        let len_bytes: [u8; 4] = data[len_start..magic_start]
            .try_into()
            .map_err(|_| EmbeddedConfigError::InvalidFormat("invalid length bytes".into()))?;
        let json_len = u32::from_le_bytes(len_bytes) as usize;

        if json_len == 0 || len_start < json_len {
            return Err(EmbeddedConfigError::InvalidFormat(
                "config length exceeds binary size".into(),
            ));
        }

        let json_start = len_start - json_len;
        let json_bytes = &data[json_start..len_start];

        let config: T =
            serde_json::from_slice(json_bytes).map_err(EmbeddedConfigError::Deserialization)?;

        Ok(Some(config))
    }
}
