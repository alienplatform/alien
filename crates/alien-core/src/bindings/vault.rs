//! Vault binding definitions for secure secret management
//!
//! This module defines the binding parameters for different vault services:
//! - AWS SSM Parameter Store (SecureString)
//! - GCP Secret Manager
//! - Azure Key Vault
//! - Kubernetes Secrets (native K8s secret storage)

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents a vault binding for secure secret management
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "kebab-case")]
pub enum VaultBinding {
    /// AWS SSM Parameter Store binding (SecureString)
    ParameterStore(ParameterStoreVaultBinding),
    /// GCP Secret Manager binding
    SecretManager(SecretManagerVaultBinding),
    /// Azure Key Vault binding
    KeyVault(KeyVaultBinding),
    /// Kubernetes Secrets binding (native K8s secret storage)
    KubernetesSecret(KubernetesSecretVaultBinding),
    /// Local development vault
    #[serde(rename = "local-vault")]
    Local(LocalVaultBinding),
}

/// AWS SSM Parameter Store vault binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ParameterStoreVaultBinding {
    /// The vault prefix for parameter names (e.g., "my-stack-my-vault")
    pub vault_prefix: BindingValue<String>,
}

/// GCP Secret Manager vault binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SecretManagerVaultBinding {
    /// The vault prefix for secret names (e.g., "my-stack-my-vault")
    pub vault_prefix: BindingValue<String>,
}

/// Azure Key Vault binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct KeyVaultBinding {
    /// The Key Vault name
    pub vault_name: BindingValue<String>,
}

/// Kubernetes Secrets vault binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesSecretVaultBinding {
    /// The Kubernetes namespace where secrets are stored
    pub namespace: BindingValue<String>,
    /// The vault prefix for secret names (e.g., "acme-monitoring-secrets")
    pub vault_prefix: BindingValue<String>,
}

/// Local development vault binding (for testing/development)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalVaultBinding {
    /// The vault name for local storage
    pub vault_name: String,
    /// Directory where vault secrets are stored
    pub data_dir: BindingValue<String>,
}

impl VaultBinding {
    /// Creates an AWS SSM Parameter Store vault binding
    pub fn parameter_store(vault_prefix: impl Into<String>) -> Self {
        Self::ParameterStore(ParameterStoreVaultBinding {
            vault_prefix: vault_prefix.into().into(),
        })
    }

    /// Creates a GCP Secret Manager vault binding
    pub fn secret_manager(vault_prefix: impl Into<String>) -> Self {
        Self::SecretManager(SecretManagerVaultBinding {
            vault_prefix: vault_prefix.into().into(),
        })
    }

    /// Creates an Azure Key Vault binding
    pub fn key_vault(vault_name: impl Into<String>) -> Self {
        Self::KeyVault(KeyVaultBinding {
            vault_name: vault_name.into().into(),
        })
    }

    /// Creates a Kubernetes Secrets vault binding
    pub fn kubernetes_secret(
        namespace: impl Into<String>,
        vault_prefix: impl Into<String>,
    ) -> Self {
        Self::KubernetesSecret(KubernetesSecretVaultBinding {
            namespace: namespace.into().into(),
            vault_prefix: vault_prefix.into().into(),
        })
    }

    /// Creates a local vault binding for development/testing
    pub fn local(vault_name: impl Into<String>, data_dir: impl Into<String>) -> Self {
        Self::Local(LocalVaultBinding {
            vault_name: vault_name.into(),
            data_dir: BindingValue::value(data_dir.into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_store_binding_creation() {
        let binding = VaultBinding::parameter_store("my-vault");

        match binding {
            VaultBinding::ParameterStore(config) => {
                assert_eq!(
                    config.vault_prefix,
                    BindingValue::Value("my-vault".to_string())
                );
            }
            _ => panic!("Expected ParameterStore binding"),
        }
    }

    #[test]
    fn test_secret_manager_binding_creation() {
        let binding = VaultBinding::secret_manager("my-vault");

        match binding {
            VaultBinding::SecretManager(config) => {
                assert_eq!(
                    config.vault_prefix,
                    BindingValue::Value("my-vault".to_string())
                );
            }
            _ => panic!("Expected SecretManager binding"),
        }
    }

    #[test]
    fn test_key_vault_binding_creation() {
        let binding = VaultBinding::key_vault("my-key-vault");

        match binding {
            VaultBinding::KeyVault(config) => {
                assert_eq!(
                    config.vault_name,
                    BindingValue::Value("my-key-vault".to_string())
                );
            }
            _ => panic!("Expected KeyVault binding"),
        }
    }

    #[test]
    fn test_local_binding_creation() {
        let binding = VaultBinding::local("dev-vault", "/tmp/vault-data");

        match binding {
            VaultBinding::Local(config) => {
                assert_eq!(config.vault_name, "dev-vault");
                assert_eq!(
                    config.data_dir,
                    BindingValue::Value("/tmp/vault-data".to_string())
                );
            }
            _ => panic!("Expected Local binding"),
        }
    }

    #[test]
    fn test_serialization_roundtrip() {
        let original = VaultBinding::parameter_store("test-vault");

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: VaultBinding = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }
}
