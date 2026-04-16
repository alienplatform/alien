use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Vault, VaultOutputs};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

/// Kubernetes Vault controller that uses Kubernetes Secrets as the backing store.
///
/// This controller creates a logical vault using the Kubernetes Secrets API.
/// Each secret value is stored as a separate Kubernetes Secret resource with
/// a naming convention: {release-name}-{vault-id}-{secret-key}
///
/// For external vaults (HashiCorp Vault, Azure KeyVault, AWS Secrets Manager),
/// users should provide an external binding in values.yaml instead of provisioning
/// a Vault resource.
#[controller]
pub struct KubernetesVaultController {
    /// The namespace where secrets are stored
    pub(crate) namespace: Option<String>,
    /// The vault prefix used for naming secrets
    pub(crate) vault_prefix: Option<String>,
}

#[controller]
impl KubernetesVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(vault_id=%config.id, "Creating Kubernetes Vault (using Kubernetes Secrets)");

        let namespace = self.get_kubernetes_namespace(ctx)?;

        // Generate vault prefix for naming secrets
        // Format: {release-name}-{vault-id}
        let vault_prefix = format!("{}-{}", ctx.resource_prefix, config.id)
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_lowercase();

        // Kubernetes doesn't require upfront vault creation
        // Secrets are created on-demand when values are written
        // This controller just stores the prefix and namespace

        self.namespace = Some(namespace.clone());
        self.vault_prefix = Some(vault_prefix.clone());

        info!(
            vault_id=%config.id,
            namespace=%namespace,
            vault_prefix=%vault_prefix,
            "Kubernetes Vault initialized (secrets will be created on-demand)"
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // No heartbeat check needed - secrets are created/verified on-demand
        // during secret sync operations
        debug!("Kubernetes Vault is ready");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Vault has no mutable fields on Kubernetes — update is a no-op that also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;
        info!(vault_id=%config.id, "Kubernetes Vault update (no-op — no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(vault_id=%config.id, "Deleting Kubernetes Vault");

        // Note: We don't delete individual secrets here because:
        // 1. Secrets are ephemeral (deleted when namespace is deleted)
        // 2. Deleting all secrets with prefix would require listing and deleting each one
        // 3. When Helm chart is uninstalled, the entire namespace is deleted anyway

        // For best-effort cleanup, we could list and delete secrets with our prefix
        // But for now, rely on namespace deletion for cleanup

        self.namespace = None;
        self.vault_prefix = None;

        info!("Kubernetes Vault deleted (individual secrets remain until namespace deletion)");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(namespace), Some(vault_prefix)) = (&self.namespace, &self.vault_prefix) {
            Some(ResourceOutputs::new(VaultOutputs {
                vault_id: format!("kubernetes-secret:{}:{}", namespace, vault_prefix),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Binding params for Kubernetes vault using native Kubernetes Secrets
        // The runtime will use these to read/write secrets via the K8s API
        if let (Some(namespace), Some(vault_prefix)) = (&self.namespace, &self.vault_prefix) {
            let binding = alien_core::bindings::VaultBinding::kubernetes_secret(
                namespace.clone(),
                vault_prefix.clone(),
            );
            Ok(Some(
                serde_json::to_value(&binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

impl KubernetesVaultController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(vault_prefix: &str, namespace: &str) -> Self {
        Self {
            state: KubernetesVaultState::Ready,
            namespace: Some(namespace.to_string()),
            vault_prefix: Some(vault_prefix.to_string()),
            _internal_stay_count: None,
        }
    }

    /// Gets the Kubernetes namespace from KubernetesClientConfig
    fn get_kubernetes_namespace(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        let k8s_config = ctx.get_kubernetes_config()?;
        match k8s_config {
            alien_core::KubernetesClientConfig::InCluster { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-vault".to_string(),
                        message: "Kubernetes namespace not configured in InCluster config"
                            .to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-vault".to_string(),
                        message: "Kubernetes namespace not configured in Kubeconfig".to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-vault".to_string(),
                        message: "Kubernetes namespace not configured in Manual config".to_string(),
                    })
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_prefix_generation() {
        // The vault prefix is generated from resource_prefix + vault_id
        // It must be lowercase and alphanumeric with hyphens only
        let prefix = "My-App_123";
        let vault_id = "secrets";

        let vault_prefix = format!("{}-{}", prefix, vault_id)
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_lowercase();

        assert_eq!(vault_prefix, "my-app123-secrets");
    }
}
