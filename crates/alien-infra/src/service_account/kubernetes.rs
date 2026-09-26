use std::time::Duration;

use alien_core::{
    kubernetes_service_account_name, permission_profile_from_service_account_id,
    KubernetesClientConfig, Platform, ResourceOutputs, ResourceStatus, ServiceAccount,
    ServiceAccountOutputs,
};
use alien_error::{AlienError, Context};
use alien_k8s_clients::kubernetes::{
    kubernetes_client::KubernetesClient, kubernetes_request_utils::sign_send_json,
};
use alien_macros::controller;
use k8s_openapi::api::core::v1::ServiceAccount as KubernetesServiceAccount;
use reqwest::Method;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

/// Observes ServiceAccounts owned by the Helm release. It never creates or deletes them.
#[controller]
pub struct KubernetesServiceAccountController {
    pub(crate) name: Option<String>,
    pub(crate) namespace: Option<String>,
    pub(crate) identity: Option<String>,
}

#[controller]
impl KubernetesServiceAccountController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.observe(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.observe(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.observe(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Helm owns the account and removes it when the chart is uninstalled.
        self.name = None;
        self.namespace = None;
        self.identity = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.name
            .as_ref()
            .zip(self.identity.as_ref())
            .map(|(name, identity)| {
                ResourceOutputs::new(ServiceAccountOutputs {
                    identity: identity.clone(),
                    resource_id: name.clone(),
                })
            })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }
}

impl KubernetesServiceAccountController {
    async fn observe(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let profile = permission_profile_from_service_account_id(config.id());
        let name = kubernetes_service_account_name(ctx.resource_prefix, &profile);
        let kubernetes_config = ctx.get_kubernetes_config()?.clone();
        let namespace = namespace(&kubernetes_config, config.id())?;
        let client = KubernetesClient::new(kubernetes_config).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to configure Kubernetes ServiceAccount read".to_string(),
                resource_id: Some(config.id().to_string()),
            },
        )?;
        let url = format!(
            "{}/api/v1/namespaces/{namespace}/serviceaccounts/{name}",
            client.get_base_url()
        );
        let request = client.client().request(Method::GET, url);
        let account: KubernetesServiceAccount = sign_send_json(request, &client.auth_config())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Helm-owned ServiceAccount {namespace}/{name} is unavailable"),
                resource_id: Some(config.id().to_string()),
            })?;
        if account.metadata.name.as_deref() != Some(&name) {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id().to_string(),
                message: format!(
                    "Kubernetes returned the wrong ServiceAccount for {namespace}/{name}"
                ),
            }));
        }

        let identity = match ctx.deployment_config.base_platform {
            Some(base_platform) => {
                let annotation = match base_platform {
                    Platform::Aws => "eks.amazonaws.com/role-arn",
                    Platform::Gcp => "iam.gke.io/gcp-service-account",
                    Platform::Azure => "azure.workload.identity/client-id",
                    _ => {
                        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: config.id().to_string(),
                            message: format!(
                                "Unsupported Kubernetes base platform: {base_platform:?}"
                            ),
                        }));
                    }
                };
                account
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|annotations| annotations.get(annotation))
                    .filter(|value| !value.is_empty())
                    .cloned()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceControllerConfigError {
                            resource_id: config.id().to_string(),
                            message: format!(
                                "ServiceAccount {namespace}/{name} is missing {annotation}"
                            ),
                        })
                    })?
            }
            None => name.clone(),
        };

        self.name = Some(name);
        self.namespace = Some(namespace);
        self.identity = Some(identity);
        Ok(())
    }
}

fn namespace(config: &KubernetesClientConfig, resource_id: &str) -> Result<String> {
    let namespace = match config {
        KubernetesClientConfig::InCluster { namespace, .. }
        | KubernetesClientConfig::Kubeconfig { namespace, .. }
        | KubernetesClientConfig::Manual { namespace, .. } => namespace,
    };
    namespace.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: "Kubernetes namespace is required for ServiceAccount observation".to_string(),
        })
    })
}
