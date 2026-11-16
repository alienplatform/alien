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
    /// The chart may provide the account again (a `helm upgrade` after a rollback), so a
    /// failed check keeps looking instead of staying failed until someone retries.
    #[handler(
        state = RefreshFailed,
        on_failure = RefreshFailed,
        status = ResourceStatus::RefreshFailed,
    )]
    async fn refresh_failed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.observe(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }
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
        let account: KubernetesServiceAccount = match sign_send_json(request, &client.auth_config())
            .await
        {
            Ok(account) => account,
            // The chart creates the account before the Operator runs, so a missing
            // one is not a race to wait out: the installed chart no longer provides it
            // (for example after a rollback). Failing at once lets the health check
            // report it, instead of spending its retry budget while the deployment
            // still shows Running.
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: config.id().to_string(),
                        message: format!(
                            "Helm-owned ServiceAccount {namespace}/{name} is unavailable: the installed chart does not provide it"
                        ),
                    }));
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Helm-owned ServiceAccount {namespace}/{name} is unavailable"),
                    resource_id: Some(config.id().to_string()),
                });
            }
        };
        if account.metadata.name.as_deref() != Some(&name) {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id().to_string(),
                message: format!(
                    "Kubernetes returned the wrong ServiceAccount for {namespace}/{name}"
                ),
            }));
        }

        let identity = observed_identity(
            ctx.deployment_config.base_platform,
            config,
            &namespace,
            &name,
            account.metadata.annotations.as_ref(),
        )?;

        self.name = Some(name);
        self.namespace = Some(namespace);
        self.identity = Some(identity);
        Ok(())
    }
}

/// The identity a Helm-owned ServiceAccount carries: the cloud identity annotated on it, or the
/// account's own name when there is no base cloud or its profile grants nothing on it.
fn observed_identity(
    base_platform: Option<Platform>,
    config: &ServiceAccount,
    namespace: &str,
    name: &str,
    annotations: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<String> {
    let Some(base_platform) = base_platform else {
        return Ok(name.to_string());
    };
    let annotation = match base_platform {
        Platform::Aws => "eks.amazonaws.com/role-arn",
        Platform::Gcp => "iam.gke.io/gcp-service-account",
        Platform::Azure => "azure.workload.identity/client-id",
        _ => {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id().to_string(),
                message: format!("Unsupported Kubernetes base platform: {base_platform:?}"),
            }));
        }
    };
    match annotations
        .and_then(|annotations| annotations.get(annotation))
        .filter(|value| !value.is_empty())
    {
        Some(identity) => Ok(identity.clone()),
        // A profile that grants nothing on the base cloud has no cloud identity to carry: the
        // account is a plain Kubernetes ServiceAccount.
        None if !crate::core::service_account_grants_on(config, base_platform) => {
            Ok(name.to_string())
        }
        // The release grants this account something on the base cloud, but the account was set
        // up (or recorded) as a plain Kubernetes ServiceAccount: it has no cloud identity yet,
        // and only the installation's setup can create one. Say so, so it is neither a bare
        // "missing annotation" nor a grant that is silently left out.
        None => Err(AlienError::new(ErrorData::SetupRerunRequired {
            resource_id: config.id().to_string(),
            message: format!(
                "this release grants the service account {namespace}/{name} permissions on {base_platform}, but the account has no cloud identity ({annotation} is not set). Run the installation's setup again (for example helm upgrade with the cloud role for this account) so it gets one"
            ),
        })),
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

#[cfg(test)]
mod identity_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn account(permission_set: Option<&str>) -> ServiceAccount {
        let mut profile = alien_core::PermissionProfile::new();
        if let Some(id) = permission_set {
            profile
                .0
                .entry("*".to_string())
                .or_default()
                .push(alien_core::PermissionSetReference::from_name(id));
        }
        ServiceAccount::from_permission_profile("p-sa".to_string(), &profile, |id| {
            alien_permissions::get_permission_set(id).cloned()
        })
        .unwrap()
    }

    #[test]
    fn a_grantless_account_has_its_own_name_as_identity() {
        let identity =
            observed_identity(Some(Platform::Aws), &account(None), "ns", "app-p", None).unwrap();
        assert_eq!(identity, "app-p");
    }

    #[test]
    fn an_annotated_account_keeps_its_cloud_identity() {
        let annotations = BTreeMap::from([(
            "eks.amazonaws.com/role-arn".to_string(),
            "arn:aws:iam::1:role/x".to_string(),
        )]);
        for permission_set in [None, Some("storage/data-read")] {
            let identity = observed_identity(
                Some(Platform::Aws),
                &account(permission_set),
                "ns",
                "app-p",
                Some(&annotations),
            )
            .unwrap();
            assert_eq!(identity, "arn:aws:iam::1:role/x");
        }
    }

    #[test]
    fn an_account_with_grants_and_no_annotation_is_refused() {
        let error = observed_identity(
            Some(Platform::Aws),
            &account(Some("storage/data-read")),
            "ns",
            "app-p",
            None,
        )
        .unwrap_err();
        assert!(
            error.message.contains("eks.amazonaws.com/role-arn"),
            "{}",
            error.message
        );
        assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
        assert!(error.message.contains("setup again"), "{}", error.message);
    }
}
