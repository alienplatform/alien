use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    HeartbeatBackend, KubernetesSecretVaultHeartbeatData, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Vault, VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Kubernetes Vault controller that uses Kubernetes Secrets as the backing store.
///
/// This controller creates a logical vault using the Kubernetes Secrets API.
/// Each vault entry is stored as a separate Kubernetes Secret resource with
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
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        if let (Some(namespace), Some(vault_prefix)) = (&self.namespace, &self.vault_prefix) {
            emit_kubernetes_secret_vault_heartbeat(
                ctx,
                &config.id,
                namespace.as_str(),
                vault_prefix.as_str(),
            );
        }

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

        if let (Some(namespace), Some(prefix)) = (&self.namespace, &self.vault_prefix) {
            let client = ctx
                .service_provider
                .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
                .await?;
            let selector = format!("managed-by=operator,vault-prefix={prefix}");
            let secrets = match client.list_secrets(namespace, Some(selector), None).await {
                Ok(secrets) => secrets.items,
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    Vec::new()
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to list vault Secrets in namespace '{namespace}'"),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            };
            // Re-listing on retry is safe: already deleted children are absent.
            for secret in secrets {
                let name = secret.metadata.name.ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: config.id.clone(),
                        message: "Listed vault Secret has no name".to_string(),
                    })
                })?;
                match client.delete_secret(namespace, &name).await {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete vault Secret '{namespace}/{name}'"),
                            resource_id: Some(config.id.clone()),
                        }))
                    }
                }
            }
        }

        self.namespace = None;
        self.vault_prefix = None;

        info!("Kubernetes Vault and its managed Secrets deleted");

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

fn emit_kubernetes_secret_vault_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    namespace: &str,
    prefix: &str,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Vault::RESOURCE_TYPE,
        controller_platform: Platform::Kubernetes,
        backend: HeartbeatBackend::Kubernetes,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Vault(VaultHeartbeatData::KubernetesSecret(
            KubernetesSecretVaultHeartbeatData {
                status: VaultHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(
                        "Kubernetes Secret namespace prefix is configured; secret metadata was not listed"
                            .to_string(),
                    ),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                namespace: namespace.to_string(),
                prefix: prefix.to_string(),
                secret_metadata_listed: false,
            },
        )),
        raw: vec![],
    });
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
    use crate::core::{
        kubernetes_manifest_test_support::KubernetesManifestTestHarness,
        MockPlatformServiceProvider,
    };
    use alien_bindings::{
        providers::vault::kubernetes_secret::KubernetesSecretVault, traits::Vault as _,
    };
    use alien_core::{ClientConfig, KubernetesClientConfig, Resource};
    use alien_k8s_clients::secrets::{MockSecretsApi, SecretsApi};
    use k8s_openapi::{
        api::core::v1::Secret,
        apimachinery::pkg::apis::meta::v1::{ListMeta, ObjectMeta},
        List,
    };
    use std::{collections::BTreeMap, sync::Arc};

    fn ready(namespace: &str) -> KubernetesVaultController {
        KubernetesVaultController {
            state: KubernetesVaultState::Ready,
            namespace: Some(namespace.into()),
            vault_prefix: Some("test-vault".into()),
            _internal_stay_count: None,
        }
    }

    #[tokio::test]
    async fn delete_failure_preserves_identifiers_and_retry_tolerates_missing_secrets() {
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_list_secrets()
            .withf(|ns, selector, field| {
                ns == "test-ns"
                    && selector.as_deref() == Some("managed-by=operator,vault-prefix=test-vault")
                    && field.is_none()
            })
            .times(2)
            .returning(|_, _, _| {
                Ok(List {
                    metadata: ListMeta::default(),
                    items: vec![Secret {
                        metadata: ObjectMeta {
                            name: Some("test-vault-key".into()),
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                })
            });
        let mut sequence = mockall::Sequence::new();
        secrets
            .expect_delete_secret()
            .withf(|ns, name| ns == "test-ns" && name == "test-vault-key")
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::AuthenticationError {
                        message: "delete denied".into(),
                    },
                ))
            });
        secrets
            .expect_delete_secret()
            .withf(|ns, name| ns == "test-ns" && name == "test-vault-key")
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceNotFound {
                        resource_type: "Secret".into(),
                        resource_name: "test-vault-key".into(),
                    },
                ))
            });
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_secrets_client()
            .times(2)
            .returning(move |_| Ok(secrets.clone()));
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("vault".into()).build()),
            vec![],
        )
        .with_service_provider(Arc::new(provider));
        let mut controller = ready("test-ns");
        let error = controller.delete_start(&harness.ctx()).await.unwrap_err();
        assert!(error.to_string().contains("test-ns/test-vault-key"));
        assert_eq!(controller.namespace.as_deref(), Some("test-ns"));
        assert_eq!(controller.vault_prefix.as_deref(), Some("test-vault"));
        controller.delete_start(&harness.ctx()).await.unwrap();
        assert!(controller.namespace.is_none());
        assert!(controller.vault_prefix.is_none());
    }

    #[rstest::rstest]
    #[case::namespace_missing(true)]
    #[case::access_denied(false)]
    #[tokio::test]
    async fn list_failure_only_completes_cleanup_when_namespace_is_missing(#[case] missing: bool) {
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_list_secrets()
            .withf(|ns, selector, field| {
                ns == "test-ns"
                    && selector.as_deref() == Some("managed-by=operator,vault-prefix=test-vault")
                    && field.is_none()
            })
            .times(1)
            .return_once(move |_, _, _| {
                Err(AlienError::new(if missing {
                    alien_client_core::ErrorData::RemoteResourceNotFound {
                        resource_type: "Namespace".into(),
                        resource_name: "test-ns".into(),
                    }
                } else {
                    alien_client_core::ErrorData::AuthenticationError {
                        message: "list denied".into(),
                    }
                }))
            });
        secrets.expect_delete_secret().times(0);
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_secrets_client()
            .times(1)
            .return_once(move |_| Ok(secrets));
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("vault".into()).build()),
            vec![],
        )
        .with_service_provider(Arc::new(provider));
        let mut controller = ready("test-ns");
        let result = controller.delete_start(&harness.ctx()).await;
        if missing {
            result.unwrap();
            assert!(controller.namespace.is_none());
            assert!(controller.vault_prefix.is_none());
        } else {
            assert!(result.unwrap_err().to_string().contains("test-ns"));
            assert_eq!(controller.namespace.as_deref(), Some("test-ns"));
            assert_eq!(controller.vault_prefix.as_deref(), Some("test-vault"));
        }
    }

    /// Run against an empty disposable namespace:
    /// KUBECONFIG=/path/to/config VAULT_TEST_NAMESPACE=vault-test cargo test -p alien-infra
    /// --all-features vault::kubernetes::tests::delete_removes_binding_secrets -- --ignored
    #[tokio::test]
    #[ignore = "requires a disposable Kubernetes namespace and KUBECONFIG"]
    async fn delete_removes_binding_secrets_and_preserves_site_and_other_vault_secrets() {
        let namespace = std::env::var("VAULT_TEST_NAMESPACE").expect("VAULT_TEST_NAMESPACE");
        let config = KubernetesClientConfig::Kubeconfig {
            kubeconfig_path: Some(std::env::var("KUBECONFIG").expect("KUBECONFIG")),
            context: None,
            cluster: None,
            user: None,
            namespace: Some(namespace.clone()),
            additional_headers: None,
        };
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("vault".into()).build()),
            vec![],
        );
        let mut ctx = harness.ctx();
        ctx.client_config = ClientConfig::Kubernetes(Box::new(config));
        let client = ctx
            .service_provider
            .get_kubernetes_secrets_client(ctx.get_kubernetes_config().unwrap())
            .await
            .unwrap();
        let vault =
            KubernetesSecretVault::new(client.clone(), namespace.clone(), "test-vault".into());
        vault.set_secret("one", "first").await.unwrap();
        vault.set_secret("two", "second").await.unwrap();
        let other =
            KubernetesSecretVault::new(client.clone(), namespace.clone(), "other-vault".into());
        other.set_secret("keep", "other").await.unwrap();
        for (name, labels) in [
            ("site-input", BTreeMap::new()),
            (
                "site-with-prefix",
                BTreeMap::from([("vault-prefix".into(), "test-vault".into())]),
            ),
        ] {
            client
                .create_secret(
                    &namespace,
                    &Secret {
                        metadata: ObjectMeta {
                            name: Some(name.into()),
                            labels: Some(labels),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }
        let mut controller = ready(&namespace);
        controller.delete_start(&ctx).await.unwrap();
        let remaining = client.list_secrets(&namespace, None, None).await.unwrap();
        let mut names: Vec<_> = remaining
            .items
            .into_iter()
            .map(|s| s.metadata.name.unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["other-vault-keep", "site-input", "site-with-prefix"]
        );
        assert!(controller.namespace.is_none());
        assert!(controller.vault_prefix.is_none());
        // Replaying a saved delete checkpoint is safe after all matching Secrets are gone.
        ready(&namespace).delete_start(&ctx).await.unwrap();
    }
}
