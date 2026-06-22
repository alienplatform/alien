use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::kubernetes_client::DeploymentApi;
#[cfg(feature = "kubernetes")]
use crate::kubernetes_cluster_heartbeat::{
    emit_kubernetes_cluster_heartbeat, KubernetesClusterHeartbeatInput,
};
use alien_core::{
    import::data::AzureApplicationGatewayForContainersBootstrap, KubernetesCluster,
    KubernetesClusterOutputs, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, Platform, ResourceOutputs as CoreResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context};
use alien_macros::controller;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AzureLongRunningOperation {
    pub(crate) url: String,
    pub(crate) retry_after: Option<Duration>,
    #[serde(default)]
    pub(crate) location_url: Option<String>,
}

/// Kubernetes cluster controller.
///
/// The cluster itself is created or selected during setup. At runtime the pull
/// agent verifies Kubernetes API, namespace, and RBAC access from inside the
/// target Kubernetes environment.
#[controller]
pub struct KubernetesClusterController {
    pub(crate) provider: Option<alien_core::KubernetesClusterProvider>,
    pub(crate) ownership: Option<alien_core::KubernetesClusterOwnership>,
    pub(crate) namespace: Option<String>,
    pub(crate) cluster_name: Option<String>,
    pub(crate) cluster_id: Option<String>,
    pub(crate) kubernetes_api_reachable: Option<bool>,
    pub(crate) namespace_ready: Option<bool>,
    pub(crate) rbac_ready: Option<bool>,
    pub(crate) agent_ready: Option<bool>,
    pub(crate) cloud_metadata_ready: Option<bool>,
    pub(crate) azure_application_gateway_for_containers:
        Option<AzureApplicationGatewayForContainersBootstrap>,
    pub(crate) cloud_operation_id: Option<String>,
    pub(crate) cloud_cluster_status: Option<String>,
    pub(crate) azure_delete_operation: Option<AzureLongRunningOperation>,
    pub(crate) aws_vpc_id: Option<String>,
    pub(crate) aws_vpc_dns_configured: Option<bool>,
    pub(crate) aws_internet_gateway_id: Option<String>,
    pub(crate) aws_internet_gateway_detached: Option<bool>,
    pub(crate) aws_nat_eip_allocation_id: Option<String>,
    pub(crate) aws_nat_gateway_id: Option<String>,
    pub(crate) aws_public_subnet_ids: Vec<String>,
    pub(crate) aws_private_subnet_ids: Vec<String>,
    pub(crate) aws_public_route_table_id: Option<String>,
    pub(crate) aws_private_route_table_id: Option<String>,
    pub(crate) aws_public_route_configured: Option<bool>,
    pub(crate) aws_private_route_configured: Option<bool>,
    pub(crate) aws_route_table_association_ids: Vec<String>,
    pub(crate) aws_cluster_role_name: Option<String>,
    pub(crate) aws_cluster_role_arn: Option<String>,
    pub(crate) aws_node_role_name: Option<String>,
    pub(crate) aws_node_role_arn: Option<String>,
    pub(crate) aws_managed_node_role_name: Option<String>,
    pub(crate) aws_managed_node_role_arn: Option<String>,
    pub(crate) aws_cluster_role_policies_attached: Option<bool>,
    pub(crate) aws_node_role_policies_attached: Option<bool>,
    pub(crate) aws_managed_node_role_policies_attached: Option<bool>,
    pub(crate) aws_oidc_provider_arn: Option<String>,
    pub(crate) aws_vpc_cni_addon_ready: Option<bool>,
    pub(crate) aws_nodegroup_name: Option<String>,
    pub(crate) aws_nodegroup_ready: Option<bool>,
    pub(crate) aws_kube_proxy_addon_ready: Option<bool>,
    pub(crate) aws_coredns_addon_ready: Option<bool>,
    pub(crate) agent_helm_installed: Option<bool>,
    pub(crate) agent_helm_release: Option<String>,
    pub(crate) agent_helm_namespace: Option<String>,
    pub(crate) status_message: Option<String>,
}

#[controller]
impl KubernetesClusterController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if ctx.platform != Platform::Kubernetes {
            let config = ctx.desired_resource_config::<KubernetesCluster>()?;
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "KubernetesCluster setup must be completed by Terraform, CloudFormation, or another setup path before runtime handoff".to_string(),
            }));
        }
        let ready = record_cluster_status(self, ctx, "Creating").await?;
        let state = if ready { Ready } else { CreateStart };
        Ok(HandlerAction::Continue {
            state,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if ctx.platform == Platform::Kubernetes {
            record_cluster_status(self, ctx, "Refreshing").await?;
            #[cfg(feature = "kubernetes")]
            {
                let config = ctx.desired_resource_config::<KubernetesCluster>()?;
                emit_kubernetes_cluster_heartbeat(
                    ctx,
                    KubernetesClusterHeartbeatInput {
                        config,
                        cluster_name: self.cluster_name.as_deref(),
                        api_reachable: self.kubernetes_api_reachable.unwrap_or(false),
                        namespace_ready: self.namespace_ready.unwrap_or(false),
                        rbac_ready: self.rbac_ready.unwrap_or(false),
                        agent_ready: self.agent_ready.unwrap_or(false),
                        status_message: self.status_message.clone(),
                    },
                )
                .await?;
            }
        } else {
            debug!("Skipping KubernetesCluster runtime refresh outside Kubernetes agent");
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let ready = record_cluster_status(self, ctx, "Updating").await?;
        let state = if ready { Ready } else { UpdateStart };
        Ok(HandlerAction::Continue {
            state,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<KubernetesCluster>()?;
        info!(cluster_id = %config.id, "Deleting KubernetesCluster state");

        self.provider = None;
        self.ownership = None;
        self.namespace = None;
        self.cluster_name = None;
        self.cluster_id = None;
        self.kubernetes_api_reachable = None;
        self.namespace_ready = None;
        self.rbac_ready = None;
        self.agent_ready = None;
        self.cloud_metadata_ready = None;
        self.cloud_operation_id = None;
        self.cloud_cluster_status = None;
        self.azure_delete_operation = None;
        self.aws_vpc_id = None;
        self.aws_vpc_dns_configured = None;
        self.aws_internet_gateway_id = None;
        self.aws_internet_gateway_detached = None;
        self.aws_nat_eip_allocation_id = None;
        self.aws_nat_gateway_id = None;
        self.aws_public_subnet_ids.clear();
        self.aws_private_subnet_ids.clear();
        self.aws_public_route_table_id = None;
        self.aws_private_route_table_id = None;
        self.aws_public_route_configured = None;
        self.aws_private_route_configured = None;
        self.aws_route_table_association_ids.clear();
        self.aws_cluster_role_name = None;
        self.aws_cluster_role_arn = None;
        self.aws_node_role_name = None;
        self.aws_node_role_arn = None;
        self.aws_managed_node_role_name = None;
        self.aws_managed_node_role_arn = None;
        self.aws_cluster_role_policies_attached = None;
        self.aws_node_role_policies_attached = None;
        self.aws_managed_node_role_policies_attached = None;
        self.aws_oidc_provider_arn = None;
        self.aws_vpc_cni_addon_ready = None;
        self.aws_nodegroup_name = None;
        self.aws_nodegroup_ready = None;
        self.aws_kube_proxy_addon_ready = None;
        self.aws_coredns_addon_ready = None;
        self.agent_helm_installed = None;
        self.agent_helm_release = None;
        self.agent_helm_namespace = None;
        self.status_message = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        let default_ready = matches!(self.state, KubernetesClusterState::Ready);
        Some(CoreResourceOutputs::new(KubernetesClusterOutputs {
            provider: self.provider?,
            ownership: self.ownership?,
            namespace: self.namespace.clone()?,
            cluster_name: self.cluster_name.clone(),
            cluster_id: self.cluster_id.clone(),
            kubernetes_api_reachable: self.kubernetes_api_reachable.unwrap_or(default_ready),
            namespace_ready: self.namespace_ready.unwrap_or(default_ready),
            rbac_ready: self.rbac_ready.unwrap_or(default_ready),
            agent_ready: self.agent_ready.unwrap_or(default_ready),
            cloud_metadata_ready: self.cloud_metadata_ready,
            azure_application_gateway_for_containers: self
                .azure_application_gateway_for_containers
                .clone(),
            version: None,
            status_message: self.status_message.clone().or_else(|| {
                self.cloud_cluster_status
                    .as_ref()
                    .map(|status| format!("Cloud cluster status: {status}"))
            }),
        }))
    }
}

async fn record_cluster_status(
    controller: &mut KubernetesClusterController,
    ctx: &ResourceControllerContext<'_>,
    action: &str,
) -> Result<bool> {
    let config = ctx.desired_resource_config::<KubernetesCluster>()?;
    validate_setup_controller_boundary(config, controller, ctx.platform)?;

    info!(
        cluster_id = %config.id,
        provider = ?config.provider,
        ownership = ?config.ownership,
        namespace = %config.namespace,
        "{} KubernetesCluster runtime",
        action
    );

    controller.provider = Some(config.provider);
    controller.ownership = Some(config.ownership);
    controller.namespace = Some(config.namespace.clone());
    if controller
        .cluster_name
        .as_ref()
        .is_none_or(|name| name.is_empty())
    {
        controller.cluster_name = config
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.cluster_name.clone());
    }
    if controller
        .cluster_id
        .as_ref()
        .is_none_or(|id| id.is_empty())
    {
        controller.cluster_id = config
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.cluster_id.clone());
    }

    let cloud_metadata_ready_before_runtime = controller.cloud_metadata_ready;
    let status_message_before_runtime = controller.status_message.clone();
    let runtime_status = verify_agent_runtime(config, ctx).await?;
    controller.kubernetes_api_reachable = Some(runtime_status.kubernetes_api_reachable);
    controller.namespace_ready = Some(runtime_status.namespace_ready);
    controller.rbac_ready = Some(runtime_status.rbac_ready);
    controller.agent_ready = Some(runtime_status.agent_ready);
    controller.status_message = if ctx.platform != Platform::Kubernetes
        && cloud_metadata_ready_before_runtime == Some(false)
    {
        status_message_before_runtime
    } else {
        runtime_status.status_message
    };
    controller.cloud_metadata_ready =
        if config.heartbeat_mode == KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata {
            if ctx.platform == Platform::Kubernetes {
                Some(runtime_status.cloud_metadata_ready)
            } else {
                cloud_metadata_ready_before_runtime.or(Some(runtime_status.cloud_metadata_ready))
            }
        } else {
            None
        };

    debug!(cluster_id = %config.id, "KubernetesCluster runtime ready");

    Ok(runtime_status.agent_ready)
}

struct KubernetesClusterRuntimeStatus {
    kubernetes_api_reachable: bool,
    namespace_ready: bool,
    rbac_ready: bool,
    agent_ready: bool,
    cloud_metadata_ready: bool,
    status_message: Option<String>,
}

async fn verify_agent_runtime(
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<KubernetesClusterRuntimeStatus> {
    if ctx.platform != Platform::Kubernetes {
        return Ok(KubernetesClusterRuntimeStatus {
            kubernetes_api_reachable: false,
            namespace_ready: false,
            rbac_ready: false,
            agent_ready: false,
            cloud_metadata_ready: true,
            status_message: Some(
                "Kubernetes cluster identity is available; waiting for alien-agent to report from inside the cluster"
                    .into(),
            ),
        });
    }

    let kubernetes_config = ctx.get_kubernetes_config()?;
    let deployment_client = ctx
        .service_provider
        .get_kubernetes_client(kubernetes_config)
        .await?;
    deployment_client
        .list_deployments(&config.namespace, None, None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to verify alien-agent Kubernetes API access in namespace '{}'",
                config.namespace
            ),
            resource_id: Some(config.id.clone()),
        })?;

    Ok(KubernetesClusterRuntimeStatus {
        kubernetes_api_reachable: true,
        namespace_ready: true,
        rbac_ready: true,
        agent_ready: true,
        cloud_metadata_ready: true,
        status_message: Some(
            "alien-agent verified Kubernetes API, namespace, and RBAC from inside the cluster"
                .into(),
        ),
    })
}

fn validate_setup_controller_boundary(
    config: &KubernetesCluster,
    controller: &KubernetesClusterController,
    platform: Platform,
) -> Result<()> {
    if platform == Platform::Kubernetes || config.ownership != KubernetesClusterOwnership::Managed {
        return Ok(());
    }

    if has_cluster_identity(config, controller) {
        return Ok(());
    }

    let provider = match config.provider {
        KubernetesClusterProvider::Eks => "EKS Auto Mode",
        KubernetesClusterProvider::Gke => "GKE Autopilot",
        KubernetesClusterProvider::Aks => "AKS Base",
        KubernetesClusterProvider::Generic => "generic Kubernetes",
    };

    Err(AlienError::new(ErrorData::ResourceControllerConfigError {
        resource_id: config.id.clone(),
        message: format!(
            "Managed {provider} setup requires setup-time cluster provisioning before runtime handoff. \
             Terraform/imported clusters are supported when the setup artifact provides \
             clusterName or clusterId."
        ),
    }))
}

fn has_cluster_identity(
    config: &KubernetesCluster,
    controller: &KubernetesClusterController,
) -> bool {
    has_controller_cluster_identity(controller) || has_config_cluster_identity(config)
}

fn has_controller_cluster_identity(controller: &KubernetesClusterController) -> bool {
    controller
        .cluster_name
        .as_ref()
        .is_some_and(|name| !name.is_empty())
        || controller
            .cluster_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
}

fn has_config_cluster_identity(config: &KubernetesCluster) -> bool {
    config.cloud.as_ref().is_some_and(|cloud| {
        cloud
            .cluster_name
            .as_ref()
            .is_some_and(|name| !name.is_empty())
            || cloud.cluster_id.as_ref().is_some_and(|id| !id.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{KubernetesCloudReference, KubernetesHeartbeatMode};

    fn managed_cluster(provider: KubernetesClusterProvider) -> KubernetesCluster {
        KubernetesCluster::new("kubernetes".to_string())
            .provider(provider)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .maybe_cloud(None)
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build()
    }

    #[test]
    fn managed_cloud_cluster_without_imported_identity_requires_setup_provisioning() {
        let cluster = managed_cluster(KubernetesClusterProvider::Eks);
        let controller = KubernetesClusterController::default();

        let err =
            validate_setup_controller_boundary(&cluster, &controller, Platform::Aws).unwrap_err();

        assert_eq!(err.code, "RESOURCE_CONTROLLER_CONFIG_ERROR");
        assert!(err
            .message
            .contains("requires setup-time cluster provisioning"));
    }

    #[test]
    fn managed_cloud_cluster_with_imported_identity_is_allowed() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .cloud(KubernetesCloudReference {
                cluster_name: Some("alien-test".to_string()),
                ..KubernetesCloudReference::default()
            })
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build();
        let controller = KubernetesClusterController::default();

        validate_setup_controller_boundary(&cluster, &controller, Platform::Aws).unwrap();
    }
}
