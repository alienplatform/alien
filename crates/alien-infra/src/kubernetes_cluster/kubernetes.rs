use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::{
    ec2::{
        AllocateAddressRequest, AssociateRouteTableRequest, AttachInternetGatewayRequest,
        CreateInternetGatewayRequest, CreateNatGatewayRequest, CreateRouteRequest,
        CreateRouteTableRequest, CreateSubnetRequest, CreateVpcRequest,
        DescribeAvailabilityZonesRequest, DescribeNatGatewaysRequest, DescribeRouteTablesRequest,
        DetachInternetGatewayRequest, Ec2Api, ModifyVpcAttributeRequest, Tag, TagSpecification,
    },
    eks::{
        BlockStorageRequest, ComputeConfigRequest, CreateAccessConfigRequest, CreateAddonRequest,
        CreateClusterRequest, CreateNodegroupRequest, EksApi, ElasticLoadBalancingRequest,
        KubernetesNetworkConfigRequest, NodegroupScalingConfig, NodegroupUpdateConfig,
        StorageConfigRequest, VpcConfigRequest,
    },
    iam::{CreateOpenIdConnectProviderRequest, CreateRoleRequest, CreateRoleTag, IamApi},
};
use alien_azure_clients::long_running_operation::{
    LongRunningOperation as AzureLongRunningOperation, OperationResult as AzureOperationResult,
};
use alien_azure_clients::managed_identity::{
    FederatedCredentialProperties, FederatedIdentityCredential,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_manager_service_account_name, kubernetes_service_account_name,
    permission_profile_from_service_account_id, DeploymentModel, KubernetesCluster,
    KubernetesClusterOutputs, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, Platform, RemoteStackManagement, RemoteStackManagementOutputs,
    ResourceOutputs as CoreResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs,
    StackSettings,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::container::{
    Autopilot, CreateClusterRequest as GkeCreateClusterRequest, GkeCluster, IpAllocationPolicy,
    ReleaseChannel, WorkloadIdentityConfig,
};
use alien_gcp_clients::iam::{Binding as GcpIamBinding, IamPolicy as GcpIamPolicy};
use alien_gcp_clients::longrunning::OperationResult as GcpOperationResult;
use alien_gcp_clients::GcpClientConfigExt;
use alien_macros::controller;
use base64::Engine as _;
use serde_json::json;

/// Kubernetes substrate controller.
///
/// The cluster itself is created or selected during setup. At runtime the
/// agent's ability to reconcile this resource means the Helm chart is installed
/// and reporting from inside the target Kubernetes environment.
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

enum SetupClusterProgress {
    InProgress,
    ReadyForHandoff,
}

const EKS_VPC_CIDR: &str = "10.251.0.0/16";
const EKS_CLUSTER_POLICY_ARNS: &[&str] = &[
    "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy",
    "arn:aws:iam::aws:policy/AmazonEKSBlockStoragePolicy",
    "arn:aws:iam::aws:policy/AmazonEKSComputePolicy",
    "arn:aws:iam::aws:policy/AmazonEKSLoadBalancingPolicy",
    "arn:aws:iam::aws:policy/AmazonEKSNetworkingPolicy",
];
const EKS_AUTO_NODE_POLICY_ARNS: &[&str] = &[
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
    "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
    "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodeMinimalPolicy",
];
const EKS_MANAGED_NODE_POLICY_ARNS: &[&str] = &[
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
    "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
    "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
];

#[controller]
impl KubernetesClusterController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if matches!(
            ensure_setup_cluster(self, ctx).await?,
            SetupClusterProgress::InProgress
        ) {
            return Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }
        if ctx.platform != Platform::Kubernetes {
            if matches!(
                ensure_setup_agent_helm(self, ctx).await?,
                SetupClusterProgress::InProgress
            ) {
                return Ok(HandlerAction::Continue {
                    state: CreateStart,
                    suggested_delay: Some(Duration::from_secs(30)),
                });
            }

            return Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: Some(Duration::from_secs(30)),
            });
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

        if !delete_setup_cluster(self, config, ctx).await? {
            return Ok(HandlerAction::Continue {
                state: DeleteStart,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }

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
            version: None,
            status_message: self.status_message.clone().or_else(|| {
                self.cloud_cluster_status
                    .as_ref()
                    .map(|status| format!("Cloud cluster status: {status}"))
            }),
        }))
    }
}

async fn delete_setup_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<bool> {
    if ctx.platform == Platform::Kubernetes
        || config.ownership != KubernetesClusterOwnership::Managed
        || has_config_cluster_identity(config)
        || !has_controller_setup_state(controller)
    {
        return Ok(true);
    }

    match (ctx.platform, config.provider) {
        (Platform::Gcp, KubernetesClusterProvider::Gke) => {
            delete_gke_autopilot_cluster(controller, config, ctx).await
        }
        (Platform::Azure, KubernetesClusterProvider::Aks) => {
            delete_aks_base_cluster(controller, config, ctx).await
        }
        (Platform::Aws, KubernetesClusterProvider::Eks) => {
            delete_eks_auto_mode_cluster(controller, config, ctx).await
        }
        (_, KubernetesClusterProvider::Generic)
        | (Platform::Local | Platform::Test | Platform::Kubernetes, _) => Ok(true),
        (_, provider) => Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!(
                "KubernetesCluster provider {provider:?} cannot be deleted on controller platform {}",
                ctx.platform
            ),
        })),
    }
}

async fn delete_gke_autopilot_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<bool> {
    let gcp_config = ctx.get_gcp_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let location = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.region.clone())
        .unwrap_or_else(|| gcp_config.region.clone());
    let client = ctx.service_provider.get_gcp_container_client(gcp_config)?;

    if controller.cloud_cluster_status.as_deref() == Some("DELETING") {
        if let Some(operation_id) = controller.cloud_operation_id.clone() {
            let operation = client
                .get_operation(&location, &operation_id)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to poll GKE Autopilot cluster deletion operation '{operation_id}'"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            if operation.done != Some(true) {
                controller.status_message =
                    Some("Waiting for GKE Autopilot cluster deletion".to_string());
                return Ok(false);
            }
            if let Some(GcpOperationResult::Error { error }) = operation.result {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "GKE Autopilot cluster '{cluster_name}' deletion failed: {}",
                        error.message
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }
        controller.cluster_name = None;
        controller.cluster_id = None;
        controller.cloud_metadata_ready = None;
        controller.cloud_operation_id = None;
        controller.cloud_cluster_status = None;
        controller.status_message = Some("Deleted GKE Autopilot cluster".to_string());
        return Ok(true);
    }

    match client.delete_cluster(&location, &cluster_name).await {
        Ok(operation) => {
            controller.cloud_operation_id = operation.name;
            controller.cloud_cluster_status = Some("DELETING".to_string());
            controller.cloud_metadata_ready = Some(false);
            controller.status_message = Some("Started GKE Autopilot cluster deletion".to_string());
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            controller.cluster_name = None;
            controller.cluster_id = None;
            controller.cloud_metadata_ready = None;
            controller.cloud_operation_id = None;
            controller.cloud_cluster_status = None;
            controller.status_message = Some("Deleted GKE Autopilot cluster".to_string());
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete GKE cluster '{cluster_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_aks_base_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<bool> {
    let azure_config = ctx.get_azure_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let resource_group = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.resource_group.clone())
        .unwrap_or_else(|| format!("{}-k8s", ctx.resource_prefix));
    let client = ctx
        .service_provider
        .get_azure_managed_clusters_client(azure_config)?;
    let lro_client = ctx
        .service_provider
        .get_azure_long_running_operation_client(azure_config)?;

    if let Some(operation) = controller.azure_delete_operation.clone() {
        match lro_client
            .check_status(&operation, "DeleteManagedCluster", &cluster_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to poll AKS cluster '{cluster_name}' deletion"),
                resource_id: Some(config.id.clone()),
            })? {
            Some(_) => {
                controller.cluster_name = None;
                controller.cluster_id = None;
                controller.cloud_metadata_ready = None;
                controller.cloud_cluster_status = None;
                controller.azure_delete_operation = None;
                controller.status_message = Some("Deleted AKS cluster".to_string());
                return Ok(true);
            }
            None => {
                controller.status_message = Some("Waiting for AKS cluster deletion".to_string());
                return Ok(false);
            }
        }
    }

    match client
        .delete_managed_cluster(&resource_group, &cluster_name)
        .await
    {
        Ok(AzureOperationResult::Completed(())) => {
            controller.cluster_name = None;
            controller.cluster_id = None;
            controller.cloud_metadata_ready = None;
            controller.cloud_cluster_status = None;
            controller.status_message = Some("Deleted AKS cluster".to_string());
            Ok(true)
        }
        Ok(AzureOperationResult::LongRunning(operation)) => {
            controller.azure_delete_operation = Some(operation);
            controller.cloud_cluster_status = Some("Deleting".to_string());
            controller.cloud_metadata_ready = Some(false);
            controller.status_message = Some("Started AKS cluster deletion".to_string());
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            controller.cluster_name = None;
            controller.cluster_id = None;
            controller.cloud_metadata_ready = None;
            controller.cloud_cluster_status = None;
            controller.azure_delete_operation = None;
            controller.status_message = Some("Deleted AKS cluster".to_string());
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete AKS cluster '{cluster_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_eks_auto_mode_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<bool> {
    let aws_config = ctx.get_aws_config()?;
    let iam_client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
    let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
    let eks_client = ctx.service_provider.get_aws_eks_client(aws_config).await?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);

    if let Some(nodegroup_name) = controller.aws_nodegroup_name.clone() {
        if !delete_eks_nodegroup_if_present(
            controller,
            config,
            eks_client.as_ref(),
            &cluster_name,
            &nodegroup_name,
        )
        .await?
        {
            return Ok(false);
        }
    }

    for addon_name in ["coredns", "kube-proxy", "vpc-cni"] {
        if !delete_eks_addon_if_present(
            controller,
            config,
            eks_client.as_ref(),
            &cluster_name,
            addon_name,
        )
        .await?
        {
            return Ok(false);
        }
    }

    if controller.cluster_name.is_some() || controller.cluster_id.is_some() {
        if !delete_eks_cluster_if_present(controller, config, eks_client.as_ref(), &cluster_name)
            .await?
        {
            return Ok(false);
        }
    }

    if let Some(nat_gateway_id) = controller.aws_nat_gateway_id.clone() {
        if !delete_nat_gateway_if_present(controller, config, ec2_client.as_ref(), &nat_gateway_id)
            .await?
        {
            return Ok(false);
        }
    }

    if let Some(association_id) = controller.aws_route_table_association_ids.last().cloned() {
        ec2_client
            .disassociate_route_table(&association_id)
            .await
            .or_else(|error| {
                if is_cloud_not_found(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to disassociate EKS route table association '{association_id}'"
                ),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_route_table_association_ids.pop();
        controller.status_message = Some("Disassociated EKS subnet route table".to_string());
        return Ok(false);
    }

    if let Some(route_table_id) = controller.aws_private_route_table_id.clone() {
        if !delete_route_table_if_present(
            controller,
            config,
            ec2_client.as_ref(),
            &route_table_id,
            true,
        )
        .await?
        {
            return Ok(false);
        }
    }

    if let Some(route_table_id) = controller.aws_public_route_table_id.clone() {
        if !delete_route_table_if_present(
            controller,
            config,
            ec2_client.as_ref(),
            &route_table_id,
            false,
        )
        .await?
        {
            return Ok(false);
        }
    }

    if let Some(internet_gateway_id) = controller.aws_internet_gateway_id.clone() {
        if !delete_internet_gateway_if_present(
            controller,
            config,
            ec2_client.as_ref(),
            &internet_gateway_id,
        )
        .await?
        {
            return Ok(false);
        }
    }

    if let Some(subnet_id) = controller.aws_private_subnet_ids.last().cloned() {
        if !delete_subnet_if_present(controller, config, ec2_client.as_ref(), &subnet_id, true)
            .await?
        {
            return Ok(false);
        }
    }

    if let Some(subnet_id) = controller.aws_public_subnet_ids.last().cloned() {
        if !delete_subnet_if_present(controller, config, ec2_client.as_ref(), &subnet_id, false)
            .await?
        {
            return Ok(false);
        }
    }

    if let Some(allocation_id) = controller.aws_nat_eip_allocation_id.clone() {
        ec2_client
            .release_address(&allocation_id)
            .await
            .or_else(|error| {
                if is_cloud_not_found(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to release EKS NAT gateway address '{allocation_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_nat_eip_allocation_id = None;
        controller.status_message = Some("Released EKS NAT gateway address".to_string());
        return Ok(false);
    }

    if let Some(vpc_id) = controller.aws_vpc_id.clone() {
        if !delete_vpc_if_present(controller, config, ec2_client.as_ref(), &vpc_id).await? {
            return Ok(false);
        }
    }

    if let Some(role_name) = controller.aws_managed_node_role_name.clone() {
        if !delete_iam_role_if_present(
            controller,
            config,
            iam_client.as_ref(),
            &role_name,
            EKS_MANAGED_NODE_POLICY_ARNS,
            "EKS managed node group IAM role",
        )
        .await?
        {
            return Ok(false);
        }
        controller.aws_managed_node_role_name = None;
        controller.aws_managed_node_role_arn = None;
        controller.aws_managed_node_role_policies_attached = None;
        return Ok(false);
    }

    if let Some(role_name) = controller.aws_node_role_name.clone() {
        if !delete_iam_role_if_present(
            controller,
            config,
            iam_client.as_ref(),
            &role_name,
            EKS_AUTO_NODE_POLICY_ARNS,
            "EKS Auto Mode node IAM role",
        )
        .await?
        {
            return Ok(false);
        }
        controller.aws_node_role_name = None;
        controller.aws_node_role_arn = None;
        controller.aws_node_role_policies_attached = None;
        return Ok(false);
    }

    if let Some(role_name) = controller.aws_cluster_role_name.clone() {
        if !delete_iam_role_if_present(
            controller,
            config,
            iam_client.as_ref(),
            &role_name,
            EKS_CLUSTER_POLICY_ARNS,
            "EKS Auto Mode cluster IAM role",
        )
        .await?
        {
            return Ok(false);
        }
        controller.aws_cluster_role_name = None;
        controller.aws_cluster_role_arn = None;
        controller.aws_cluster_role_policies_attached = None;
        return Ok(false);
    }

    if let Some(oidc_provider_arn) = controller.aws_oidc_provider_arn.clone() {
        delete_oidc_provider_if_present(
            controller,
            config,
            iam_client.as_ref(),
            &oidc_provider_arn,
        )
        .await?;
        controller.aws_oidc_provider_arn = None;
        return Ok(false);
    }

    controller.status_message = Some("Deleted EKS Auto Mode setup resources".to_string());
    Ok(true)
}

async fn delete_oidc_provider_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn IamApi,
    oidc_provider_arn: &str,
) -> Result<()> {
    match client
        .delete_open_id_connect_provider(oidc_provider_arn)
        .await
    {
        Ok(()) => {}
        Err(error) if is_cloud_not_found(&error) => {}
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete IAM OIDC provider '{oidc_provider_arn}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    }
    controller.status_message = Some("Deleted EKS IAM OIDC provider".to_string());
    Ok(())
}

async fn delete_eks_nodegroup_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn EksApi,
    cluster_name: &str,
    nodegroup_name: &str,
) -> Result<bool> {
    match client
        .describe_nodegroup(cluster_name, nodegroup_name)
        .await
    {
        Ok(response) => {
            let status = response
                .nodegroup
                .status
                .unwrap_or_else(|| "UNKNOWN".to_string());
            if status != "DELETING" {
                client
                    .delete_nodegroup(cluster_name, nodegroup_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete EKS managed node group '{nodegroup_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                controller.status_message =
                    Some("Started EKS managed node group deletion".to_string());
            } else {
                controller.status_message =
                    Some("Waiting for EKS managed node group deletion".to_string());
            }
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            controller.aws_nodegroup_name = None;
            controller.aws_nodegroup_ready = None;
            controller.status_message = Some("Deleted EKS managed node group".to_string());
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS managed node group '{nodegroup_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_eks_addon_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn EksApi,
    cluster_name: &str,
    addon_name: &str,
) -> Result<bool> {
    let ready_slot = match addon_name {
        "vpc-cni" => &mut controller.aws_vpc_cni_addon_ready,
        "kube-proxy" => &mut controller.aws_kube_proxy_addon_ready,
        "coredns" => &mut controller.aws_coredns_addon_ready,
        _ => {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: format!("Unsupported EKS add-on '{addon_name}'"),
            }));
        }
    };
    if ready_slot.is_none() {
        return Ok(true);
    }

    match client.describe_addon(cluster_name, addon_name).await {
        Ok(response) => {
            let status = response
                .addon
                .status
                .unwrap_or_else(|| "UNKNOWN".to_string());
            if status != "DELETING" {
                client
                    .delete_addon(cluster_name, addon_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete EKS add-on '{addon_name}'"),
                        resource_id: Some(config.id.clone()),
                    })?;
                controller.status_message =
                    Some(format!("Started EKS add-on '{addon_name}' deletion"));
            } else {
                controller.status_message =
                    Some(format!("Waiting for EKS add-on '{addon_name}' deletion"));
            }
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            *ready_slot = None;
            controller.status_message = Some(format!("Deleted EKS add-on '{addon_name}'"));
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS add-on '{addon_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_eks_cluster_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn EksApi,
    cluster_name: &str,
) -> Result<bool> {
    match client.describe_cluster(cluster_name).await {
        Ok(response) => {
            let status = response
                .cluster
                .status
                .unwrap_or_else(|| "UNKNOWN".to_string());
            if status != "DELETING" {
                client.delete_cluster(cluster_name).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to delete EKS cluster '{cluster_name}'"),
                        resource_id: Some(config.id.clone()),
                    },
                )?;
                controller.cloud_cluster_status = Some("DELETING".to_string());
                controller.status_message =
                    Some("Started EKS Auto Mode cluster deletion".to_string());
            } else {
                controller.cloud_cluster_status = Some(status);
                controller.status_message =
                    Some("Waiting for EKS Auto Mode cluster deletion".to_string());
            }
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            controller.cluster_name = None;
            controller.cluster_id = None;
            controller.cloud_metadata_ready = None;
            controller.cloud_cluster_status = None;
            controller.status_message = Some("Deleted EKS Auto Mode cluster".to_string());
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS cluster '{cluster_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_nat_gateway_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
    nat_gateway_id: &str,
) -> Result<bool> {
    let response = client
        .describe_nat_gateways(DescribeNatGatewaysRequest {
            nat_gateway_ids: Some(vec![nat_gateway_id.to_string()]),
            filters: None,
            max_results: None,
            next_token: None,
        })
        .await;

    match response {
        Ok(response) => {
            let state = response
                .nat_gateway_set
                .and_then(|set| set.items.into_iter().next())
                .and_then(|gateway| gateway.state)
                .unwrap_or_else(|| "unknown".to_string());
            if state == "deleted" {
                controller.aws_nat_gateway_id = None;
                controller.status_message = Some("Deleted EKS NAT gateway".to_string());
                return Ok(true);
            }
            if state != "deleting" {
                client.delete_nat_gateway(nat_gateway_id).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to delete EKS NAT gateway '{nat_gateway_id}'"),
                        resource_id: Some(config.id.clone()),
                    },
                )?;
                controller.status_message = Some("Started EKS NAT gateway deletion".to_string());
            } else {
                controller.status_message =
                    Some("Waiting for EKS NAT gateway deletion".to_string());
            }
            Ok(false)
        }
        Err(error) if is_cloud_not_found(&error) => {
            controller.aws_nat_gateway_id = None;
            controller.status_message = Some("Deleted EKS NAT gateway".to_string());
            Ok(true)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS NAT gateway '{nat_gateway_id}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn delete_route_table_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
    route_table_id: &str,
    private: bool,
) -> Result<bool> {
    let route_table = match client
        .describe_route_tables(DescribeRouteTablesRequest {
            route_table_ids: Some(vec![route_table_id.to_string()]),
            filters: None,
            max_results: None,
            next_token: None,
        })
        .await
    {
        Ok(response) => response
            .route_table_set
            .and_then(|set| set.items.into_iter().next()),
        Err(error) if is_cloud_not_found(&error) => None,
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to describe EKS route table '{route_table_id}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    };

    if let Some(association_id) = route_table
        .and_then(|table| table.association_set)
        .and_then(|set| {
            set.items
                .into_iter()
                .find(|association| association.main != Some(true))
                .and_then(|association| association.route_table_association_id)
        })
    {
        client
            .disassociate_route_table(&association_id)
            .await
            .or_else(|error| {
                if is_cloud_not_found(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to disassociate EKS route table association '{association_id}'"
                ),
                resource_id: Some(config.id.clone()),
            })?;
        controller
            .aws_route_table_association_ids
            .retain(|id| id != &association_id);
        controller.status_message = Some("Disassociated EKS route table".to_string());
        return Ok(false);
    }

    match client.delete_route_table(route_table_id).await {
        Ok(()) => {}
        Err(error) if is_cloud_not_found(&error) => {}
        Err(error) if is_cloud_conflict(&error) => {
            controller.status_message = Some(format!(
                "Waiting for EKS route table '{route_table_id}' dependencies"
            ));
            return Ok(false);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete EKS route table '{route_table_id}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    }
    if private {
        controller.aws_private_route_table_id = None;
        controller.aws_private_route_configured = None;
    } else {
        controller.aws_public_route_table_id = None;
        controller.aws_public_route_configured = None;
    }
    controller.status_message = Some("Deleted EKS route table".to_string());
    Ok(false)
}

async fn delete_internet_gateway_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
    internet_gateway_id: &str,
) -> Result<bool> {
    if controller.aws_internet_gateway_detached != Some(true) {
        let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
        client
            .detach_internet_gateway(DetachInternetGatewayRequest {
                internet_gateway_id: internet_gateway_id.to_string(),
                vpc_id: vpc_id.clone(),
            })
            .await
            .or_else(|error| {
                if is_cloud_not_found(&error) || is_cloud_conflict(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to detach EKS internet gateway '{internet_gateway_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_internet_gateway_detached = Some(true);
        controller.status_message = Some("Detached EKS internet gateway".to_string());
        return Ok(false);
    }

    client
        .delete_internet_gateway(internet_gateway_id)
        .await
        .or_else(|error| {
            if is_cloud_not_found(&error) {
                Ok(())
            } else {
                Err(error)
            }
        })
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete EKS internet gateway '{internet_gateway_id}'"),
            resource_id: Some(config.id.clone()),
        })?;
    controller.aws_internet_gateway_id = None;
    controller.aws_internet_gateway_detached = None;
    controller.status_message = Some("Deleted EKS internet gateway".to_string());
    Ok(false)
}

async fn delete_subnet_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
    subnet_id: &str,
    private: bool,
) -> Result<bool> {
    match client.delete_subnet(subnet_id).await {
        Ok(()) => {}
        Err(error) if is_cloud_not_found(&error) => {}
        Err(error) if is_cloud_conflict(&error) => {
            controller.status_message =
                Some(format!("Waiting for EKS subnet '{subnet_id}' dependencies"));
            return Ok(false);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete EKS subnet '{subnet_id}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    }
    if private {
        controller.aws_private_subnet_ids.pop();
    } else {
        controller.aws_public_subnet_ids.pop();
    }
    controller.status_message = Some("Deleted EKS subnet".to_string());
    Ok(false)
}

async fn delete_vpc_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
    vpc_id: &str,
) -> Result<bool> {
    match client.delete_vpc(vpc_id).await {
        Ok(()) => {}
        Err(error) if is_cloud_not_found(&error) => {}
        Err(error) if is_cloud_conflict(&error) => {
            controller.status_message = Some("Waiting for EKS VPC dependencies".to_string());
            return Ok(false);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete EKS VPC '{vpc_id}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    }
    controller.aws_vpc_id = None;
    controller.aws_vpc_dns_configured = None;
    controller.status_message = Some("Deleted EKS VPC".to_string());
    Ok(false)
}

async fn delete_iam_role_if_present(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn IamApi,
    role_name: &str,
    managed_policy_arns: &[&str],
    label: &str,
) -> Result<bool> {
    let attached_policies = match client.list_attached_role_policies(role_name).await {
        Ok(response) => response
            .list_attached_role_policies_result
            .attached_policies
            .map(|policies| policies.member)
            .unwrap_or_default(),
        Err(error) if is_cloud_not_found(&error) => {
            controller.status_message = Some(format!("Deleted {label}"));
            return Ok(true);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to list policies attached to IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    };

    if let Some(policy) = attached_policies
        .into_iter()
        .find(|policy| managed_policy_arns.contains(&policy.policy_arn.as_str()))
    {
        client
            .detach_role_policy(role_name, &policy.policy_arn)
            .await
            .or_else(|error| {
                if is_cloud_not_found(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to detach IAM policy '{}' from role '{role_name}'",
                    policy.policy_arn
                ),
                resource_id: Some(config.id.clone()),
            })?;
        controller.status_message = Some(format!("Detached IAM policy from {label}"));
        return Ok(false);
    }

    match client.delete_role(role_name).await {
        Ok(()) => {}
        Err(error) if is_cloud_not_found(&error) => {}
        Err(error) if is_cloud_conflict(&error) => {
            controller.status_message = Some(format!("Waiting for {label} dependencies"));
            return Ok(false);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            }));
        }
    }
    controller.status_message = Some(format!("Deleted {label}"));
    Ok(true)
}

async fn ensure_setup_cluster(
    controller: &mut KubernetesClusterController,
    ctx: &ResourceControllerContext<'_>,
) -> Result<SetupClusterProgress> {
    let config = ctx.desired_resource_config::<KubernetesCluster>()?;
    if ctx.platform == Platform::Kubernetes
        || config.ownership != KubernetesClusterOwnership::Managed
        || has_config_cluster_identity(config)
        || controller.cloud_metadata_ready == Some(true)
    {
        return Ok(SetupClusterProgress::ReadyForHandoff);
    }

    match (ctx.platform, config.provider) {
        (Platform::Gcp, KubernetesClusterProvider::Gke) => {
            ensure_gke_autopilot_cluster(controller, config, ctx).await
        }
        (Platform::Azure, KubernetesClusterProvider::Aks) => {
            ensure_aks_base_cluster(controller, config, ctx).await
        }
        (Platform::Aws, KubernetesClusterProvider::Eks) => {
            ensure_eks_auto_mode_cluster(controller, config, ctx).await
        }
        (_, provider) => Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!(
                "KubernetesCluster provider {provider:?} cannot be provisioned on controller platform {}",
                ctx.platform
            ),
        })),
    }
}

async fn ensure_setup_agent_helm(
    controller: &mut KubernetesClusterController,
    ctx: &ResourceControllerContext<'_>,
) -> Result<SetupClusterProgress> {
    let config = ctx.desired_resource_config::<KubernetesCluster>()?;
    if controller.agent_helm_installed == Some(true) {
        return Ok(SetupClusterProgress::ReadyForHandoff);
    }

    let manager_url = ctx
        .deployment_config
        .manager_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Kubernetes setup controller requires manager_url to install alien-agent"
                    .to_string(),
            })
        })?;
    let deployment_token = ctx
        .deployment_config
        .deployment_token
        .as_deref()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message:
                    "Kubernetes setup controller requires deployment_token to install alien-agent"
                        .to_string(),
            })
        })?;

    let Some(identity_values) = ensure_setup_workload_identity(controller, config, ctx).await?
    else {
        return Ok(SetupClusterProgress::InProgress);
    };

    let work_dir = std::env::temp_dir().join(format!(
        "alien-k8s-setup-{}-{}",
        sanitize_kubernetes_dns_label(ctx.resource_prefix),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&work_dir)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create temporary Kubernetes setup directory '{}'",
                work_dir.display()
            ),
            resource_id: Some(config.id.clone()),
        })?;

    let install_result: Result<()> = async {
        let kubeconfig = write_setup_kubeconfig(controller, config, ctx, &work_dir).await?;
        let chart_dir = write_setup_agent_chart(config, ctx, &work_dir)?;
        let values_file = write_setup_agent_values(
            config,
            ctx,
            &work_dir,
            manager_url,
            deployment_token,
            identity_values,
        )?;
        let release = "alien-agent";

        run_helm_upgrade_install(
            &chart_dir,
            &values_file,
            release,
            &config.namespace,
            &kubeconfig,
        )
        .await?;

        controller.agent_helm_installed = Some(true);
        controller.agent_helm_release = Some(release.to_string());
        controller.agent_helm_namespace = Some(config.namespace.clone());
        controller.kubernetes_api_reachable = Some(true);
        controller.namespace_ready = Some(true);
        controller.rbac_ready = Some(true);
        controller.agent_ready = Some(false);
        controller.status_message = Some(
            "alien-agent Helm release installed; waiting for in-cluster heartbeat".to_string(),
        );
        Ok(())
    }
    .await;

    let _ = std::fs::remove_dir_all(&work_dir);
    install_result?;

    Ok(SetupClusterProgress::ReadyForHandoff)
}

async fn write_setup_kubeconfig(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
) -> Result<PathBuf> {
    match (ctx.platform, config.provider) {
        (Platform::Aws, KubernetesClusterProvider::Eks) => {
            write_eks_setup_kubeconfig(controller, config, ctx, work_dir).await
        }
        (Platform::Gcp, KubernetesClusterProvider::Gke) => {
            write_gke_setup_kubeconfig(controller, config, ctx, work_dir).await
        }
        (Platform::Azure, KubernetesClusterProvider::Aks) => {
            write_aks_setup_kubeconfig(controller, config, ctx, work_dir).await
        }
        (_, KubernetesClusterProvider::Generic) => write_generic_setup_kubeconfig(config),
        (_, provider) => Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!(
                "Kubernetes setup controller cannot derive a kubeconfig for provider {provider:?} on platform {}",
                ctx.platform
            ),
        })),
    }
}

fn write_generic_setup_kubeconfig(config: &KubernetesCluster) -> Result<PathBuf> {
    let kubeconfig = std::env::var("KUBECONFIG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Generic Kubernetes setup requires KUBECONFIG in the setup environment"
                    .to_string(),
            })
        })?;
    let path = PathBuf::from(kubeconfig);
    if !path.exists() {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!(
                "Generic Kubernetes setup KUBECONFIG '{}' does not exist",
                path.display()
            ),
        }));
    }
    Ok(path)
}

#[derive(Debug, Clone)]
struct SetupServiceAccountIdentity {
    values_key: String,
    kubernetes_name: String,
    identity: String,
    resource_id: String,
}

#[derive(Debug, Clone)]
struct SetupManagerServiceAccountIdentity {
    kubernetes_name: String,
    identity: String,
    resource_id: String,
    access_configuration: String,
}

#[derive(Debug, Clone)]
struct SetupAgentIdentityValues {
    service_accounts: serde_json::Map<String, serde_json::Value>,
    manager_service_account: serde_json::Value,
}

async fn ensure_setup_workload_identity(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<SetupAgentIdentityValues>> {
    let Some((service_accounts, manager_service_account)) =
        collect_setup_workload_identities(controller, config, ctx)?
    else {
        return Ok(None);
    };

    match (ctx.platform, config.provider) {
        (Platform::Aws, KubernetesClusterProvider::Eks) => ensure_eks_workload_identity(
            controller,
            config,
            ctx,
            &service_accounts,
            manager_service_account.as_ref(),
        )
        .await
        .map(Some),
        (Platform::Gcp, KubernetesClusterProvider::Gke) => ensure_gke_workload_identity(
            controller,
            config,
            ctx,
            &service_accounts,
            manager_service_account.as_ref(),
        )
        .await
        .map(Some),
        (Platform::Azure, KubernetesClusterProvider::Aks) => ensure_aks_workload_identity(
            controller,
            config,
            ctx,
            &service_accounts,
            manager_service_account.as_ref(),
        )
        .await
        .map(Some),
        _ => Ok(Some(build_empty_identity_values())),
    }
}

fn collect_setup_workload_identities(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<
    Option<(
        Vec<SetupServiceAccountIdentity>,
        Option<SetupManagerServiceAccountIdentity>,
    )>,
> {
    let mut service_accounts = Vec::new();
    for (resource_id, entry) in ctx.desired_stack.resources() {
        let Some(service_account) = entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        let Some(outputs) = ctx
            .state
            .resource(resource_id)
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())
        else {
            controller.status_message = Some(format!(
                "Waiting for ServiceAccount '{resource_id}' before installing alien-agent"
            ));
            return Ok(None);
        };
        let permission_profile = permission_profile_from_service_account_id(service_account.id());
        service_accounts.push(SetupServiceAccountIdentity {
            values_key: permission_profile.clone(),
            kubernetes_name: kubernetes_service_account_name(
                ctx.resource_prefix,
                &permission_profile,
            ),
            identity: outputs.identity.clone(),
            resource_id: outputs.resource_id.clone(),
        });
    }

    let mut manager_service_account = None;
    for (resource_id, entry) in ctx.desired_stack.resources() {
        if entry
            .config
            .downcast_ref::<RemoteStackManagement>()
            .is_none()
        {
            continue;
        }
        let Some(outputs) = ctx
            .state
            .resource(resource_id)
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
        else {
            controller.status_message = Some(format!(
                "Waiting for RemoteStackManagement '{resource_id}' before installing alien-agent"
            ));
            return Ok(None);
        };
        manager_service_account = Some(SetupManagerServiceAccountIdentity {
            kubernetes_name: kubernetes_manager_service_account_name(ctx.resource_prefix),
            identity: outputs.management_resource_id.clone(),
            resource_id: outputs.management_resource_id.clone(),
            access_configuration: outputs.access_configuration.clone(),
        });
        break;
    }

    let _ = config;
    Ok(Some((service_accounts, manager_service_account)))
}

fn build_empty_identity_values() -> SetupAgentIdentityValues {
    SetupAgentIdentityValues {
        service_accounts: serde_json::Map::new(),
        manager_service_account: json!({
            "annotations": {},
            "labels": {},
        }),
    }
}

async fn ensure_eks_workload_identity(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    service_accounts: &[SetupServiceAccountIdentity],
    manager_service_account: Option<&SetupManagerServiceAccountIdentity>,
) -> Result<SetupAgentIdentityValues> {
    let aws_config = ctx.get_aws_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let eks_client = ctx.service_provider.get_aws_eks_client(aws_config).await?;
    let cluster = eks_client
        .describe_cluster(&cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read EKS cluster '{cluster_name}' for workload identity"),
            resource_id: Some(config.id.clone()),
        })?
        .cluster;
    let issuer = cluster
        .identity
        .and_then(|identity| identity.oidc)
        .and_then(|oidc| oidc.issuer)
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("EKS cluster '{cluster_name}' did not report an OIDC issuer"),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let issuer_host_path = issuer.trim_start_matches("https://").to_string();
    let provider_arn = format!(
        "arn:aws:iam::{}:oidc-provider/{issuer_host_path}",
        aws_config.account_id
    );
    let iam_client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

    if controller.aws_oidc_provider_arn.as_deref() != Some(provider_arn.as_str()) {
        let request = CreateOpenIdConnectProviderRequest::builder()
            .url(issuer.clone())
            .client_id_list(vec!["sts.amazonaws.com".to_string()])
            .tags(vec![
                CreateRoleTag {
                    key: "alien-resource".to_string(),
                    value: config.id.clone(),
                },
                CreateRoleTag {
                    key: "alien-resource-prefix".to_string(),
                    value: ctx.resource_prefix.to_string(),
                },
            ])
            .build();
        match iam_client.create_open_id_connect_provider(request).await {
            Ok(response) => {
                controller.aws_oidc_provider_arn = Some(
                    response
                        .create_open_id_connect_provider_result
                        .open_id_connect_provider_arn,
                );
            }
            Err(error) if is_cloud_conflict(&error) => {
                controller.aws_oidc_provider_arn = Some(provider_arn.clone());
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create IAM OIDC provider for EKS cluster '{cluster_name}'"
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }
    }

    let mut values = build_empty_identity_values();
    for service_account in service_accounts {
        let policy = eks_web_identity_trust_policy(
            controller
                .aws_oidc_provider_arn
                .as_deref()
                .unwrap_or(&provider_arn),
            &issuer_host_path,
            &config.namespace,
            &service_account.kubernetes_name,
        )?;
        iam_client
            .update_assume_role_policy(&service_account.resource_id, &policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to configure IRSA trust for IAM role '{}'",
                    service_account.resource_id
                ),
                resource_id: Some(config.id.clone()),
            })?;
        values.service_accounts.insert(
            service_account.values_key.clone(),
            service_account_values_json(
                [(
                    "eks.amazonaws.com/role-arn",
                    service_account.identity.as_str(),
                )],
                [],
            ),
        );
    }

    if let Some(manager) = manager_service_account {
        let Some(role_name) = aws_role_name_from_arn(&manager.identity) else {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "RemoteStackManagement output '{}' is not an IAM role ARN",
                    manager.identity
                ),
                resource_id: Some(config.id.clone()),
            }));
        };
        let policy = eks_web_identity_trust_policy(
            controller
                .aws_oidc_provider_arn
                .as_deref()
                .unwrap_or(&provider_arn),
            &issuer_host_path,
            &config.namespace,
            &manager.kubernetes_name,
        )?;
        iam_client
            .update_assume_role_policy(&role_name, &policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to configure IRSA trust for IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        values.manager_service_account = service_account_values_json(
            [("eks.amazonaws.com/role-arn", manager.identity.as_str())],
            [],
        );
    }

    Ok(values)
}

async fn ensure_gke_workload_identity(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    service_accounts: &[SetupServiceAccountIdentity],
    manager_service_account: Option<&SetupManagerServiceAccountIdentity>,
) -> Result<SetupAgentIdentityValues> {
    let gcp_config = ctx.get_gcp_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let location = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.region.clone())
        .unwrap_or_else(|| gcp_config.region.clone());
    let container_client = ctx.service_provider.get_gcp_container_client(gcp_config)?;
    let cluster = container_client
        .get_cluster(&location, &cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read GKE cluster '{cluster_name}' for workload identity"),
            resource_id: Some(config.id.clone()),
        })?;
    let workload_pool = cluster
        .workload_identity_config
        .and_then(|config| config.workload_pool)
        .unwrap_or_else(|| format!("{}.svc.id.goog", gcp_config.project_id));
    let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

    let mut values = build_empty_identity_values();
    for service_account in service_accounts {
        let member = format!(
            "serviceAccount:{workload_pool}[{}/{}]",
            config.namespace, service_account.kubernetes_name
        );
        add_gke_workload_identity_binding(
            iam_client.as_ref(),
            &service_account.identity,
            &member,
            config,
        )
        .await?;
        values.service_accounts.insert(
            service_account.values_key.clone(),
            service_account_values_json(
                [(
                    "iam.gke.io/gcp-service-account",
                    service_account.identity.as_str(),
                )],
                [],
            ),
        );
    }

    if let Some(manager) = manager_service_account {
        let member = format!(
            "serviceAccount:{workload_pool}[{}/{}]",
            config.namespace, manager.kubernetes_name
        );
        add_gke_workload_identity_binding(iam_client.as_ref(), &manager.identity, &member, config)
            .await?;
        values.manager_service_account = service_account_values_json(
            [("iam.gke.io/gcp-service-account", manager.identity.as_str())],
            [],
        );
    }

    Ok(values)
}

async fn ensure_aks_workload_identity(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    service_accounts: &[SetupServiceAccountIdentity],
    manager_service_account: Option<&SetupManagerServiceAccountIdentity>,
) -> Result<SetupAgentIdentityValues> {
    let azure_config = ctx.get_azure_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let cluster_resource_group = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.resource_group.clone())
        .unwrap_or_else(|| format!("{}-k8s", ctx.resource_prefix));
    let cluster_client = ctx
        .service_provider
        .get_azure_managed_clusters_client(azure_config)?;
    let cluster = cluster_client
        .get_managed_cluster(&cluster_resource_group, &cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read AKS cluster '{cluster_name}' for workload identity"),
            resource_id: Some(config.id.clone()),
        })?;
    let cluster_value = serde_json::to_value(&cluster).into_alien_error().context(
        ErrorData::CloudPlatformError {
            message: format!("Failed to inspect AKS cluster '{cluster_name}' workload identity"),
            resource_id: Some(config.id.clone()),
        },
    )?;
    let issuer = cluster_value
        .pointer("/properties/oidcIssuerProfile/issuerURL")
        .and_then(|value| value.as_str())
        .filter(|issuer| !issuer.trim().is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("AKS cluster '{cluster_name}' did not report an OIDC issuer URL"),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let identity_client = ctx
        .service_provider
        .get_azure_managed_identity_client(azure_config)?;

    let mut values = build_empty_identity_values();
    for service_account in service_accounts {
        ensure_aks_federated_credential(
            identity_client.as_ref(),
            &service_account.resource_id,
            &format!("{}-{}", ctx.resource_prefix, service_account.values_key),
            issuer,
            &config.namespace,
            &service_account.kubernetes_name,
            config,
        )
        .await?;
        values.service_accounts.insert(
            service_account.values_key.clone(),
            service_account_values_json(
                [(
                    "azure.workload.identity/client-id",
                    service_account.identity.as_str(),
                )],
                [("azure.workload.identity/use", "true")],
            ),
        );
    }

    if let Some(manager) = manager_service_account {
        let client_id = azure_manager_client_id(manager)?;
        ensure_aks_federated_credential(
            identity_client.as_ref(),
            &manager.resource_id,
            &format!("{}-manager", ctx.resource_prefix),
            issuer,
            &config.namespace,
            &manager.kubernetes_name,
            config,
        )
        .await?;
        values.manager_service_account = service_account_values_json(
            [("azure.workload.identity/client-id", client_id.as_str())],
            [("azure.workload.identity/use", "true")],
        );
    }

    Ok(values)
}

fn service_account_values_json<const A: usize, const L: usize>(
    annotations: [(&str, &str); A],
    labels: [(&str, &str); L],
) -> serde_json::Value {
    let annotations = annotations
        .into_iter()
        .map(|(key, value)| (key.to_string(), json!(value)))
        .collect::<serde_json::Map<_, _>>();
    let labels = labels
        .into_iter()
        .map(|(key, value)| (key.to_string(), json!(value)))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "annotations": annotations,
        "labels": labels,
    })
}

fn eks_web_identity_trust_policy(
    provider_arn: &str,
    issuer_host_path: &str,
    namespace: &str,
    service_account_name: &str,
) -> Result<String> {
    serde_json::to_string(&json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Federated": provider_arn },
            "Action": "sts:AssumeRoleWithWebIdentity",
            "Condition": {
                "StringEquals": {
                    format!("{issuer_host_path}:sub"): format!("system:serviceaccount:{namespace}:{service_account_name}"),
                    format!("{issuer_host_path}:aud"): "sts.amazonaws.com",
                },
            },
        }],
    }))
    .into_alien_error()
    .context(ErrorData::CloudPlatformError {
        message: format!(
            "Failed to serialize EKS web identity trust policy for service account '{service_account_name}'"
        ),
        resource_id: None,
    })
}

fn aws_role_name_from_arn(arn: &str) -> Option<String> {
    arn.split_once(":role/").map(|(_, role_path)| {
        role_path
            .rsplit('/')
            .next()
            .unwrap_or(role_path)
            .to_string()
    })
}

async fn add_gke_workload_identity_binding(
    iam_client: &dyn alien_gcp_clients::iam::IamApi,
    service_account_email: &str,
    member: &str,
    config: &KubernetesCluster,
) -> Result<()> {
    let mut policy = iam_client
        .get_service_account_iam_policy(service_account_email.to_string())
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to read IAM policy for GCP service account '{service_account_email}'"
            ),
            resource_id: Some(config.id.clone()),
        })?;

    let role = "roles/iam.workloadIdentityUser";
    if let Some(binding) = policy
        .bindings
        .iter_mut()
        .find(|binding| binding.role == role && binding.condition.is_none())
    {
        if !binding.members.iter().any(|existing| existing == member) {
            binding.members.push(member.to_string());
        }
    } else {
        policy.bindings.push(GcpIamBinding {
            role: role.to_string(),
            members: vec![member.to_string()],
            condition: None,
        });
    }

    let policy = GcpIamPolicy {
        version: policy.version.or(Some(3)),
        kind: policy.kind,
        resource_id: policy.resource_id,
        bindings: policy.bindings,
        etag: policy.etag,
    };
    iam_client
        .set_service_account_iam_policy(service_account_email.to_string(), policy)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to bind Kubernetes service account '{member}' to GCP service account '{service_account_email}'"
            ),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(())
}

async fn ensure_aks_federated_credential(
    identity_client: &dyn alien_azure_clients::managed_identity::ManagedIdentityApi,
    identity_resource_id: &str,
    credential_name: &str,
    issuer: &str,
    namespace: &str,
    service_account_name: &str,
    config: &KubernetesCluster,
) -> Result<()> {
    let (resource_group, identity_name) =
        azure_identity_resource_group_and_name(identity_resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Managed identity resource ID '{identity_resource_id}' is not a user-assigned identity ID"
                ),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let credential_name = sanitize_kubernetes_dns_label(credential_name);
    let credential = FederatedIdentityCredential {
        id: None,
        name: Some(credential_name.clone()),
        type_: None,
        properties: Some(FederatedCredentialProperties {
            issuer: issuer.to_string(),
            subject: format!("system:serviceaccount:{namespace}:{service_account_name}"),
            audiences: vec!["api://AzureADTokenExchange".to_string()],
        }),
    };
    identity_client
        .create_or_update_federated_credential(
            &resource_group,
            &identity_name,
            &credential_name,
            &credential,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to configure AKS federated credential '{credential_name}' for managed identity '{identity_name}'"
            ),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(())
}

fn azure_identity_resource_group_and_name(resource_id: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = resource_id
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let resource_group = parts
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("resourceGroups"))
        .map(|window| window[1].to_string())?;
    let identity_name = parts
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("userAssignedIdentities"))
        .map(|window| window[1].to_string())?;
    Some((resource_group, identity_name))
}

fn azure_manager_client_id(manager: &SetupManagerServiceAccountIdentity) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(&manager.access_configuration)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse Azure RemoteStackManagement access configuration".to_string(),
            resource_id: None,
        })?;
    value
        .get("uamiClientId")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message:
                    "Azure RemoteStackManagement access configuration did not include uamiClientId"
                        .to_string(),
                resource_id: None,
            })
        })
}

async fn write_eks_setup_kubeconfig(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
) -> Result<PathBuf> {
    let aws_config = ctx.get_aws_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let client = ctx.service_provider.get_aws_eks_client(aws_config).await?;
    let cluster = client
        .describe_cluster(&cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read EKS cluster '{cluster_name}' for kubeconfig"),
            resource_id: Some(config.id.clone()),
        })?
        .cluster;
    let endpoint = cluster.endpoint.ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("EKS cluster '{cluster_name}' did not report an API endpoint"),
            resource_id: Some(config.id.clone()),
        })
    })?;
    let ca_data = cluster
        .certificate_authority
        .and_then(|ca| ca.data)
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("EKS cluster '{cluster_name}' did not report certificate data"),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let kubeconfig = json!({
        "apiVersion": "v1",
        "kind": "Config",
        "clusters": [{
            "name": cluster_name,
            "cluster": {
                "server": endpoint,
                "certificate-authority-data": ca_data,
            },
        }],
        "users": [{
            "name": "alien-setup",
            "user": {
                "exec": {
                    "apiVersion": "client.authentication.k8s.io/v1beta1",
                    "command": "aws",
                    "args": ["eks", "get-token", "--cluster-name", cluster_name, "--region", aws_config.region],
                },
            },
        }],
        "contexts": [{
            "name": "alien-setup",
            "context": {
                "cluster": cluster_name,
                "user": "alien-setup",
            },
        }],
        "current-context": "alien-setup",
    });
    write_json_kubeconfig(work_dir, config, kubeconfig)
}

async fn write_gke_setup_kubeconfig(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
) -> Result<PathBuf> {
    let gcp_config = ctx.get_gcp_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let location = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.region.clone())
        .unwrap_or_else(|| gcp_config.region.clone());
    let client = ctx.service_provider.get_gcp_container_client(gcp_config)?;
    let cluster = client.get_cluster(&location, &cluster_name).await.context(
        ErrorData::CloudPlatformError {
            message: format!("Failed to read GKE cluster '{cluster_name}' for kubeconfig"),
            resource_id: Some(config.id.clone()),
        },
    )?;
    let endpoint = cluster.endpoint.ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("GKE cluster '{cluster_name}' did not report an API endpoint"),
            resource_id: Some(config.id.clone()),
        })
    })?;
    let ca_data = cluster
        .master_auth
        .and_then(|auth| auth.cluster_ca_certificate)
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("GKE cluster '{cluster_name}' did not report certificate data"),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let token = gcp_config
        .get_bearer_token("https://container.googleapis.com/")
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to get GCP bearer token for GKE kubeconfig".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    let kubeconfig = json!({
        "apiVersion": "v1",
        "kind": "Config",
        "clusters": [{
            "name": cluster_name,
            "cluster": {
                "server": format!("https://{endpoint}"),
                "certificate-authority-data": ca_data,
            },
        }],
        "users": [{
            "name": "alien-setup",
            "user": { "token": token },
        }],
        "contexts": [{
            "name": "alien-setup",
            "context": {
                "cluster": cluster_name,
                "user": "alien-setup",
            },
        }],
        "current-context": "alien-setup",
    });
    write_json_kubeconfig(work_dir, config, kubeconfig)
}

async fn write_aks_setup_kubeconfig(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
) -> Result<PathBuf> {
    let azure_config = ctx.get_azure_config()?;
    let cluster_name = cluster_name_for_cloud_call(config, controller, ctx);
    let resource_group = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.resource_group.clone())
        .unwrap_or_else(|| format!("{}-k8s", ctx.resource_prefix));
    let client = ctx
        .service_provider
        .get_azure_managed_clusters_client(azure_config)?;
    let credentials = client
        .list_cluster_admin_credentials(&resource_group, &cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read AKS cluster '{cluster_name}' credentials"),
            resource_id: Some(config.id.clone()),
        })?;
    let encoded = credentials
        .kubeconfigs
        .first()
        .and_then(|credential| credential.value.as_deref())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("AKS cluster '{cluster_name}' returned no kubeconfig"),
                resource_id: Some(config.id.clone()),
            })
        })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to decode AKS cluster '{cluster_name}' kubeconfig"),
            resource_id: Some(config.id.clone()),
        })?;
    let path = work_dir.join("kubeconfig");
    std::fs::write(&path, bytes)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to write setup kubeconfig '{}'", path.display()),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(path)
}

fn write_json_kubeconfig(
    work_dir: &Path,
    config: &KubernetesCluster,
    kubeconfig: serde_json::Value,
) -> Result<PathBuf> {
    let path = work_dir.join("kubeconfig.json");
    let contents = serde_json::to_vec_pretty(&kubeconfig)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to serialize setup kubeconfig".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    std::fs::write(&path, contents)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to write setup kubeconfig '{}'", path.display()),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(path)
}

fn write_setup_agent_chart(
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
) -> Result<PathBuf> {
    let chart_dir = work_dir.join("chart");
    let registry = alien_helm::HelmRegistry::built_in();
    let mut stack_settings = ctx.deployment_config.stack_settings.clone();
    stack_settings.deployment_model = DeploymentModel::Pull;
    let chart = alien_helm::generate_helm_chart(
        ctx.desired_stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings,
            chart_name: sanitize_kubernetes_dns_label(
                ctx.deployment_config
                    .deployment_name
                    .as_deref()
                    .unwrap_or(ctx.resource_prefix),
            ),
        },
    )
    .map_err(|error| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!("Failed to generate Kubernetes Helm chart: {error}"),
        })
    })?;

    for (relative_path, contents) in chart.files {
        let path = chart_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).into_alien_error().context(
                ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create Helm chart directory '{}'",
                        parent.display()
                    ),
                    resource_id: Some(config.id.clone()),
                },
            )?;
        }
        std::fs::write(&path, contents).into_alien_error().context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to write Helm chart file '{}'", path.display()),
                resource_id: Some(config.id.clone()),
            },
        )?;
    }
    Ok(chart_dir)
}

fn write_setup_agent_values(
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    work_dir: &Path,
    manager_url: &str,
    deployment_token: &str,
    identity_values: SetupAgentIdentityValues,
) -> Result<PathBuf> {
    let mut stack_settings: StackSettings = ctx.deployment_config.stack_settings.clone();
    stack_settings.deployment_model = DeploymentModel::Pull;
    let deployment_name = ctx
        .deployment_config
        .deployment_name
        .clone()
        .unwrap_or_else(|| ctx.resource_prefix.to_string());
    let values = json!({
        "management": {
            "token": deployment_token,
            "name": deployment_name,
            "url": manager_url,
            "deploymentId": null,
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        },
        "runtime": {
            "image": {
                "repository": "ghcr.io/alienplatform/alien-agent",
                "tag": "latest",
                "pullPolicy": "IfNotPresent",
            },
            "encryption": {
                "key": generate_agent_encryption_key(),
            },
        },
        "stackSettings": stack_settings,
        "infrastructure": null,
        "basePlatform": ctx.deployment_config.base_platform.map(|platform| platform.as_str()),
        "serviceAccountPrefix": sanitize_kubernetes_dns_label(ctx.resource_prefix),
        "managerServiceAccount": identity_values.manager_service_account,
        "serviceAccounts": identity_values.service_accounts,
    });
    let path = work_dir.join("alien-agent-values.json");
    let contents = serde_json::to_vec_pretty(&values)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to serialize alien-agent Helm values".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    std::fs::write(&path, contents)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to write Helm values file '{}'", path.display()),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(path)
}

async fn run_helm_upgrade_install(
    chart_dir: &Path,
    values_file: &Path,
    release: &str,
    namespace: &str,
    kubeconfig: &Path,
) -> Result<()> {
    let output = tokio::process::Command::new("helm")
        .arg("upgrade")
        .arg("--install")
        .arg(release)
        .arg(chart_dir)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("-f")
        .arg(values_file)
        .arg("--wait")
        .arg("--timeout")
        .arg("300s")
        .env("KUBECONFIG", kubeconfig)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to execute helm upgrade --install for alien-agent".to_string(),
            resource_id: None,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "helm upgrade --install alien-agent failed with status {}: stdout: {}; stderr: {}",
                output.status,
                stdout.trim(),
                stderr.trim()
            ),
            resource_id: None,
        }));
    }

    Ok(())
}

fn generate_agent_encryption_key() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn sanitize_kubernetes_dns_label(input: &str) -> String {
    let mut label = String::with_capacity(input.len().min(63));
    let mut previous_dash = false;
    for ch in input.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if !previous_dash {
            Some('-')
        } else {
            None
        };
        if let Some(next) = next {
            label.push(next);
            previous_dash = next == '-';
        }
        if label.len() == 63 {
            break;
        }
    }
    let trimmed = label.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "alien".to_string()
    } else {
        trimmed
    }
}

async fn ensure_gke_autopilot_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<SetupClusterProgress> {
    let gcp_config = ctx.get_gcp_config()?;
    let cluster_name = setup_cluster_name(ctx);
    let location = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.region.clone())
        .unwrap_or_else(|| gcp_config.region.clone());
    let client = ctx.service_provider.get_gcp_container_client(gcp_config)?;

    if let Some(operation_id) = controller.cloud_operation_id.clone() {
        let operation = client
            .get_operation(&location, &operation_id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to poll GKE Autopilot cluster creation operation '{operation_id}'"
                ),
                resource_id: Some(config.id.clone()),
            })?;
        if operation.done != Some(true) {
            controller.cluster_name = Some(cluster_name);
            controller.cluster_id = controller.cluster_name.clone();
            controller.cloud_cluster_status = Some("PROVISIONING".to_string());
            controller.cloud_metadata_ready = Some(false);
            controller.status_message =
                Some("GKE Autopilot cluster creation is still in progress".to_string());
            return Ok(SetupClusterProgress::InProgress);
        }
        controller.cloud_operation_id = None;
    }

    if controller.cluster_name.as_deref() == Some(cluster_name.as_str()) {
        let cluster = client.get_cluster(&location, &cluster_name).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to read GKE Autopilot cluster '{cluster_name}'"),
                resource_id: Some(config.id.clone()),
            },
        )?;
        let status = cluster.status.unwrap_or_else(|| "UNKNOWN".to_string());
        controller.cluster_name = Some(cluster_name);
        controller.cluster_id = controller.cluster_name.clone();
        controller.cloud_cluster_status = Some(status.clone());
        controller.cloud_metadata_ready = Some(status == "RUNNING");
        return Ok(if status == "RUNNING" {
            SetupClusterProgress::ReadyForHandoff
        } else {
            SetupClusterProgress::InProgress
        });
    }

    let mut labels = HashMap::new();
    labels.insert("alien-resource".to_string(), config.id.clone());
    labels.insert(
        "alien-resource-prefix".to_string(),
        ctx.resource_prefix.to_string(),
    );

    let operation = client
        .create_cluster(
            &location,
            GkeCreateClusterRequest {
                cluster: Some(GkeCluster {
                    name: Some(cluster_name.clone()),
                    location: Some(location.clone()),
                    autopilot: Some(Autopilot {
                        enabled: Some(true),
                    }),
                    ip_allocation_policy: Some(IpAllocationPolicy {
                        use_ip_aliases: Some(true),
                        ..IpAllocationPolicy::default()
                    }),
                    release_channel: Some(ReleaseChannel {
                        channel: Some("REGULAR".to_string()),
                    }),
                    workload_identity_config: Some(WorkloadIdentityConfig {
                        workload_pool: Some(format!("{}.svc.id.goog", gcp_config.project_id)),
                    }),
                    resource_labels: Some(labels),
                    ..GkeCluster::default()
                }),
            },
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to start GKE Autopilot cluster '{cluster_name}' creation"),
            resource_id: Some(config.id.clone()),
        })?;

    controller.cluster_name = Some(cluster_name);
    controller.cluster_id = controller.cluster_name.clone();
    controller.cloud_operation_id = operation.name;
    controller.cloud_cluster_status = Some("PROVISIONING".to_string());
    controller.cloud_metadata_ready = Some(false);
    controller.status_message =
        Some("GKE Autopilot cluster creation has started; waiting for completion".to_string());
    Ok(SetupClusterProgress::InProgress)
}

async fn ensure_aks_base_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<SetupClusterProgress> {
    let azure_config = ctx.get_azure_config()?;
    let cluster_name = setup_cluster_name(ctx);
    let location = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.region.clone())
        .or_else(|| azure_config.region.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Managed AKS setup requires an Azure region".to_string(),
            })
        })?;
    let resource_group = config
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.resource_group.clone())
        .unwrap_or_else(|| format!("{}-k8s", ctx.resource_prefix));
    let resource_group_client = ctx
        .service_provider
        .get_azure_resources_client(azure_config)?;
    let client = ctx
        .service_provider
        .get_azure_managed_clusters_client(azure_config)?;

    let resource_group_payload: alien_azure_clients::models::resources::ResourceGroup =
        serde_json::from_value(json!({
            "location": location.clone(),
            "tags": {
                "alien-resource": config.id,
                "alien-resource-prefix": ctx.resource_prefix,
            },
        }))
        .into_alien_error()
        .context(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!("Failed to build Azure resource group request for '{resource_group}'"),
        })?;
    resource_group_client
        .create_or_update_resource_group(&resource_group, &resource_group_payload)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create or update Azure resource group '{resource_group}'"),
            resource_id: Some(config.id.clone()),
        })?;

    if controller.cluster_name.as_deref() == Some(cluster_name.as_str()) {
        let cluster = client
            .get_managed_cluster(&resource_group, &cluster_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to read AKS cluster '{cluster_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.cluster_id = cluster.id.clone().or_else(|| Some(cluster_name.clone()));
        controller.cluster_name = Some(cluster_name);
        controller.cloud_cluster_status = cluster
            .properties
            .as_ref()
            .and_then(|properties| properties.provisioning_state.clone())
            .or_else(|| Some("UNKNOWN".to_string()));
        controller.cloud_metadata_ready =
            Some(controller.cloud_cluster_status.as_deref() == Some("Succeeded"));
        return Ok(if controller.cloud_metadata_ready == Some(true) {
            SetupClusterProgress::ReadyForHandoff
        } else {
            SetupClusterProgress::InProgress
        });
    }

    let request = json!({
        "location": location,
        "identity": { "type": "SystemAssigned" },
        "sku": { "name": "Base", "tier": "Standard" },
        "tags": {
            "alien-resource": config.id,
            "alien-resource-prefix": ctx.resource_prefix,
        },
        "properties": {
            "dnsPrefix": format!("{}-k8s", ctx.resource_prefix),
            "aadProfile": {
                "managed": true,
                "enableAzureRBAC": true,
                "tenantID": azure_config.tenant_id,
            },
            "agentPoolProfiles": [{
                "name": "default",
                "count": 3,
                "vmSize": "Standard_D2s_v3",
                "mode": "System",
            }],
            "ingressProfile": {
                "webAppRouting": {
                    "enabled": true,
                },
            },
            "oidcIssuerProfile": {
                "enabled": true,
            },
            "securityProfile": {
                "workloadIdentity": {
                    "enabled": true,
                },
            },
        },
    });
    let managed_cluster = serde_json::from_value(request).into_alien_error().context(
        ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!("Failed to build AKS managed cluster request for '{cluster_name}'"),
        },
    )?;
    let operation = client
        .create_or_update_managed_cluster(&resource_group, &cluster_name, &managed_cluster)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to start AKS cluster '{cluster_name}' creation"),
            resource_id: Some(config.id.clone()),
        })?;

    controller.cluster_name = Some(cluster_name);
    controller.cluster_id = match operation {
        AzureOperationResult::Completed(cluster) => cluster.id,
        AzureOperationResult::LongRunning(_) => None,
    };
    if controller.cluster_id.is_none() {
        controller.cluster_id = controller.cluster_name.clone();
    }
    controller.cloud_cluster_status = Some("Creating".to_string());
    controller.cloud_metadata_ready = Some(false);
    controller.status_message =
        Some("AKS cluster creation has started; waiting for completion".to_string());
    Ok(SetupClusterProgress::InProgress)
}

async fn ensure_eks_auto_mode_cluster(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Result<SetupClusterProgress> {
    let aws_config = ctx.get_aws_config()?;
    let iam_client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
    let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
    let eks_client = ctx.service_provider.get_aws_eks_client(aws_config).await?;
    let cluster_name = setup_cluster_name(ctx);

    if controller.aws_cluster_role_arn.is_none() {
        let role_name = eks_cluster_role_name(ctx);
        let arn = ensure_aws_role(
            iam_client.as_ref(),
            config,
            &role_name,
            aws_service_trust_policy("eks.amazonaws.com"),
            "EKS Auto Mode cluster role",
            ctx,
        )
        .await?;
        controller.aws_cluster_role_name = Some(role_name);
        controller.aws_cluster_role_arn = Some(arn);
        controller.status_message = Some("Created EKS Auto Mode cluster IAM role".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_cluster_role_policies_attached != Some(true) {
        let role_name = require_field(
            &controller.aws_cluster_role_name,
            config,
            "EKS cluster role name",
        )?;
        for policy_arn in EKS_CLUSTER_POLICY_ARNS {
            iam_client
                .attach_role_policy(role_name, policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to attach EKS cluster policy '{policy_arn}' to '{role_name}'"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        controller.aws_cluster_role_policies_attached = Some(true);
        controller.status_message = Some("Attached EKS Auto Mode cluster IAM policies".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_node_role_arn.is_none() {
        let role_name = eks_node_role_name(ctx);
        let arn = ensure_aws_role(
            iam_client.as_ref(),
            config,
            &role_name,
            aws_service_trust_policy("ec2.amazonaws.com"),
            "EKS Auto Mode node role",
            ctx,
        )
        .await?;
        controller.aws_node_role_name = Some(role_name);
        controller.aws_node_role_arn = Some(arn);
        controller.status_message = Some("Created EKS Auto Mode node IAM role".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_node_role_policies_attached != Some(true) {
        let role_name =
            require_field(&controller.aws_node_role_name, config, "EKS node role name")?;
        for policy_arn in EKS_AUTO_NODE_POLICY_ARNS {
            iam_client
                .attach_role_policy(role_name, policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to attach EKS node policy '{policy_arn}' to '{role_name}'"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        controller.aws_node_role_policies_attached = Some(true);
        controller.status_message = Some("Attached EKS Auto Mode node IAM policies".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_managed_node_role_arn.is_none() {
        let role_name = eks_managed_node_role_name(ctx);
        let arn = ensure_aws_role(
            iam_client.as_ref(),
            config,
            &role_name,
            aws_service_trust_policy("ec2.amazonaws.com"),
            "EKS managed node group role",
            ctx,
        )
        .await?;
        controller.aws_managed_node_role_name = Some(role_name);
        controller.aws_managed_node_role_arn = Some(arn);
        controller.status_message = Some("Created EKS managed node group IAM role".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_managed_node_role_policies_attached != Some(true) {
        let role_name = require_field(
            &controller.aws_managed_node_role_name,
            config,
            "EKS managed node role name",
        )?;
        for policy_arn in EKS_MANAGED_NODE_POLICY_ARNS {
            iam_client
                .attach_role_policy(role_name, policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to attach EKS managed node policy '{policy_arn}' to '{role_name}'"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        controller.aws_managed_node_role_policies_attached = Some(true);
        controller.status_message =
            Some("Attached EKS managed node group IAM policies".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_vpc_id.is_none() {
        let response = ec2_client
            .create_vpc(CreateVpcRequest {
                cidr_block: EKS_VPC_CIDR.to_string(),
                instance_tenancy: None,
                amazon_provided_ipv6_cidr_block: None,
                tag_specifications: Some(vec![ec2_tag_specification("vpc", config, ctx)]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create EKS VPC".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_vpc_id = Some(required_value(
            response.vpc.and_then(|vpc| vpc.vpc_id),
            config,
            "EKS VPC id",
        )?);
        controller.status_message = Some("Created EKS VPC".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_vpc_dns_configured != Some(true) {
        let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
        for (enable_dns_support, enable_dns_hostnames) in [(Some(true), None), (None, Some(true))] {
            ec2_client
                .modify_vpc_attribute(ModifyVpcAttributeRequest {
                    vpc_id: vpc_id.clone(),
                    enable_dns_support,
                    enable_dns_hostnames,
                })
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to configure EKS VPC DNS attributes for '{vpc_id}'"),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        controller.aws_vpc_dns_configured = Some(true);
        controller.status_message = Some("Configured EKS VPC DNS support".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_internet_gateway_id.is_none() {
        let response = ec2_client
            .create_internet_gateway(CreateInternetGatewayRequest {
                tag_specifications: Some(vec![ec2_tag_specification(
                    "internet-gateway",
                    config,
                    ctx,
                )]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create EKS internet gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_internet_gateway_id = Some(required_value(
            response
                .internet_gateway
                .and_then(|gateway| gateway.internet_gateway_id),
            config,
            "EKS internet gateway id",
        )?);
        controller.status_message = Some("Created EKS internet gateway".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_public_subnet_ids.len() < 2 || controller.aws_private_subnet_ids.len() < 2 {
        ensure_next_eks_subnet(controller, config, ctx, ec2_client.as_ref()).await?;
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_public_route_table_id.is_none() {
        let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
        let response = ec2_client
            .create_route_table(CreateRouteTableRequest {
                vpc_id: vpc_id.clone(),
                tag_specifications: Some(vec![ec2_tag_specification("route-table", config, ctx)]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create EKS public route table for VPC '{vpc_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_public_route_table_id = Some(required_value(
            response.route_table.and_then(|table| table.route_table_id),
            config,
            "EKS public route table id",
        )?);
        controller.status_message = Some("Created EKS public route table".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_public_route_configured != Some(true) {
        let route_table_id = require_field(
            &controller.aws_public_route_table_id,
            config,
            "EKS public route table id",
        )?;
        let gateway_id = require_field(
            &controller.aws_internet_gateway_id,
            config,
            "EKS internet gateway id",
        )?;
        let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
        ec2_client
            .attach_internet_gateway(AttachInternetGatewayRequest {
                internet_gateway_id: gateway_id.clone(),
                vpc_id: vpc_id.clone(),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to attach EKS internet gateway '{gateway_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        create_route_ignore_duplicate(
            ec2_client.as_ref(),
            CreateRouteRequest {
                route_table_id: route_table_id.clone(),
                destination_cidr_block: "0.0.0.0/0".to_string(),
                gateway_id: Some(gateway_id.clone()),
                nat_gateway_id: None,
                instance_id: None,
                network_interface_id: None,
                vpc_peering_connection_id: None,
                transit_gateway_id: None,
            },
            config,
            "EKS public default route",
        )
        .await?;
        controller.aws_public_route_configured = Some(true);
        controller.status_message = Some("Configured EKS public routing".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_route_table_association_ids.len() < 2 {
        let route_table_id = require_field(
            &controller.aws_public_route_table_id,
            config,
            "EKS public route table id",
        )?;
        let subnet_id = controller.aws_public_subnet_ids
            [controller.aws_route_table_association_ids.len()]
        .clone();
        let response = ec2_client
            .associate_route_table(AssociateRouteTableRequest {
                route_table_id: route_table_id.clone(),
                subnet_id: subnet_id.clone(),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to associate EKS public subnet '{subnet_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        if let Some(association_id) = response.association_id {
            controller
                .aws_route_table_association_ids
                .push(association_id);
        }
        controller.status_message = Some("Associated EKS public subnet route table".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_nat_eip_allocation_id.is_none() {
        let response = ec2_client
            .allocate_address(AllocateAddressRequest {
                domain: Some("vpc".to_string()),
                tag_specifications: Some(vec![ec2_tag_specification("elastic-ip", config, ctx)]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to allocate EKS NAT gateway address".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_nat_eip_allocation_id = Some(required_value(
            response.allocation_id,
            config,
            "EKS NAT allocation id",
        )?);
        controller.status_message = Some("Allocated EKS NAT gateway address".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_nat_gateway_id.is_none() {
        let allocation_id = require_field(
            &controller.aws_nat_eip_allocation_id,
            config,
            "EKS NAT allocation id",
        )?;
        let subnet_id = required_value(
            controller.aws_public_subnet_ids.first().cloned(),
            config,
            "EKS public subnet id",
        )?;
        let response = ec2_client
            .create_nat_gateway(CreateNatGatewayRequest {
                subnet_id: subnet_id.clone(),
                allocation_id: Some(allocation_id.clone()),
                connectivity_type: Some("public".to_string()),
                private_ip_address: None,
                tag_specifications: Some(vec![ec2_tag_specification("natgateway", config, ctx)]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create EKS NAT gateway in subnet '{subnet_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_nat_gateway_id = Some(required_value(
            response
                .nat_gateway
                .and_then(|gateway| gateway.nat_gateway_id),
            config,
            "EKS NAT gateway id",
        )?);
        controller.status_message = Some("Created EKS NAT gateway".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if !is_nat_gateway_available(controller, config, ec2_client.as_ref()).await? {
        controller.status_message =
            Some("Waiting for EKS NAT gateway to become available".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_private_route_table_id.is_none() {
        let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
        let response = ec2_client
            .create_route_table(CreateRouteTableRequest {
                vpc_id: vpc_id.clone(),
                tag_specifications: Some(vec![ec2_tag_specification("route-table", config, ctx)]),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create EKS private route table for VPC '{vpc_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_private_route_table_id = Some(required_value(
            response.route_table.and_then(|table| table.route_table_id),
            config,
            "EKS private route table id",
        )?);
        controller.status_message = Some("Created EKS private route table".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_private_route_configured != Some(true) {
        let route_table_id = require_field(
            &controller.aws_private_route_table_id,
            config,
            "EKS private route table id",
        )?;
        let nat_gateway_id =
            require_field(&controller.aws_nat_gateway_id, config, "EKS NAT gateway id")?;
        create_route_ignore_duplicate(
            ec2_client.as_ref(),
            CreateRouteRequest {
                route_table_id: route_table_id.clone(),
                destination_cidr_block: "0.0.0.0/0".to_string(),
                gateway_id: None,
                nat_gateway_id: Some(nat_gateway_id.clone()),
                instance_id: None,
                network_interface_id: None,
                vpc_peering_connection_id: None,
                transit_gateway_id: None,
            },
            config,
            "EKS private default route",
        )
        .await?;
        controller.aws_private_route_configured = Some(true);
        controller.status_message = Some("Configured EKS private routing".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_route_table_association_ids.len() < 4 {
        let route_table_id = require_field(
            &controller.aws_private_route_table_id,
            config,
            "EKS private route table id",
        )?;
        let private_index = controller.aws_route_table_association_ids.len() - 2;
        let subnet_id = controller.aws_private_subnet_ids[private_index].clone();
        let response = ec2_client
            .associate_route_table(AssociateRouteTableRequest {
                route_table_id: route_table_id.clone(),
                subnet_id: subnet_id.clone(),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to associate EKS private subnet '{subnet_id}'"),
                resource_id: Some(config.id.clone()),
            })?;
        if let Some(association_id) = response.association_id {
            controller
                .aws_route_table_association_ids
                .push(association_id);
        }
        controller.status_message = Some("Associated EKS private subnet route table".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.cluster_name.as_deref() != Some(cluster_name.as_str()) {
        let cluster_role_arn = require_field(
            &controller.aws_cluster_role_arn,
            config,
            "EKS cluster role ARN",
        )?;
        let node_role_arn =
            require_field(&controller.aws_node_role_arn, config, "EKS node role ARN")?;
        let subnet_ids = controller
            .aws_public_subnet_ids
            .iter()
            .chain(controller.aws_private_subnet_ids.iter())
            .cloned()
            .collect();
        let response = eks_client
            .create_cluster(CreateClusterRequest {
                name: cluster_name.clone(),
                role_arn: cluster_role_arn.clone(),
                resources_vpc_config: VpcConfigRequest {
                    subnet_ids,
                    endpoint_private_access: Some(true),
                    endpoint_public_access: Some(true),
                    security_group_ids: None,
                },
                version: None,
                access_config: Some(CreateAccessConfigRequest {
                    authentication_mode: Some("API_AND_CONFIG_MAP".to_string()),
                    bootstrap_cluster_creator_admin_permissions: Some(true),
                }),
                bootstrap_self_managed_addons: Some(false),
                compute_config: Some(ComputeConfigRequest {
                    enabled: true,
                    node_pools: Some(vec!["general-purpose".to_string(), "system".to_string()]),
                    node_role_arn: Some(node_role_arn.clone()),
                }),
                kubernetes_network_config: Some(KubernetesNetworkConfigRequest {
                    elastic_load_balancing: Some(ElasticLoadBalancingRequest { enabled: true }),
                }),
                storage_config: Some(StorageConfigRequest {
                    block_storage: Some(BlockStorageRequest { enabled: true }),
                }),
                tags: Some(aws_tag_map(config, ctx)),
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to start EKS Auto Mode cluster '{cluster_name}' creation"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.cluster_name = Some(response.cluster.name);
        controller.cluster_id = response
            .cluster
            .arn
            .or_else(|| controller.cluster_name.clone());
        controller.cloud_cluster_status = response.cluster.status.or(Some("CREATING".to_string()));
        controller.cloud_metadata_ready = Some(false);
        controller.status_message =
            Some("EKS Auto Mode cluster creation has started; waiting for ACTIVE".to_string());
        return Ok(SetupClusterProgress::InProgress);
    }

    let cluster = eks_client
        .describe_cluster(&cluster_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read EKS Auto Mode cluster '{cluster_name}'"),
            resource_id: Some(config.id.clone()),
        })?
        .cluster;
    let status = cluster.status.unwrap_or_else(|| "UNKNOWN".to_string());
    controller.cluster_name = Some(cluster.name);
    controller.cluster_id = cluster.arn.or_else(|| controller.cluster_name.clone());
    controller.cloud_cluster_status = Some(status.clone());
    if status != "ACTIVE" {
        controller.cloud_metadata_ready = Some(false);
        controller.status_message = Some(format!(
            "Waiting for EKS cluster status ACTIVE; current status is {status}"
        ));
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_vpc_cni_addon_ready != Some(true) {
        ensure_eks_addon(
            controller,
            config,
            eks_client.as_ref(),
            &cluster_name,
            "vpc-cni",
        )
        .await?;
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_nodegroup_ready != Some(true) {
        ensure_eks_nodegroup(controller, config, eks_client.as_ref(), &cluster_name).await?;
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_kube_proxy_addon_ready != Some(true) {
        ensure_eks_addon(
            controller,
            config,
            eks_client.as_ref(),
            &cluster_name,
            "kube-proxy",
        )
        .await?;
        return Ok(SetupClusterProgress::InProgress);
    }

    if controller.aws_coredns_addon_ready != Some(true) {
        ensure_eks_addon(
            controller,
            config,
            eks_client.as_ref(),
            &cluster_name,
            "coredns",
        )
        .await?;
        return Ok(SetupClusterProgress::InProgress);
    }

    controller.cloud_metadata_ready = Some(true);
    controller.status_message =
        Some("EKS Auto Mode cluster is ready; waiting for alien-agent handoff".to_string());
    Ok(SetupClusterProgress::ReadyForHandoff)
}

async fn ensure_aws_role(
    client: &dyn IamApi,
    config: &KubernetesCluster,
    role_name: &str,
    assume_role_policy: String,
    description: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<String> {
    let request = CreateRoleRequest {
        role_name: role_name.to_string(),
        assume_role_policy_document: assume_role_policy,
        path: None,
        description: Some(description.to_string()),
        max_session_duration: None,
        tags: Some(aws_role_tags(config, ctx)),
    };

    match client.create_role(request).await {
        Ok(response) => Ok(response.create_role_result.role.arn),
        Err(error) if is_cloud_conflict(&error) => {
            let response =
                client
                    .get_role(role_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to read existing IAM role '{role_name}'"),
                        resource_id: Some(config.id.clone()),
                    })?;
            Ok(response.get_role_result.role.arn)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create IAM role '{role_name}'"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn ensure_next_eks_subnet(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
    client: &dyn Ec2Api,
) -> Result<()> {
    let vpc_id = require_field(&controller.aws_vpc_id, config, "EKS VPC id")?;
    let zones = client
        .describe_availability_zones(DescribeAvailabilityZonesRequest {
            zone_names: None,
            zone_ids: None,
            filters: None,
            all_availability_zones: None,
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to list AWS availability zones for EKS subnets".to_string(),
            resource_id: Some(config.id.clone()),
        })?
        .availability_zone_info
        .map(|set| {
            set.items
                .into_iter()
                .filter(|zone| zone.zone_state.as_deref() == Some("available"))
                .filter_map(|zone| zone.zone_name)
                .take(2)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if zones.len() < 2 {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: "Managed EKS setup requires at least two available AWS availability zones"
                .to_string(),
        }));
    }

    let (is_public, index, cidr_block) = if controller.aws_public_subnet_ids.len() < 2 {
        let index = controller.aws_public_subnet_ids.len();
        (true, index, format!("10.251.{index}.0/24"))
    } else {
        let index = controller.aws_private_subnet_ids.len();
        (false, index, format!("10.251.{}.0/24", index + 10))
    };

    let mut tags = aws_ec2_tags(config, ctx);
    tags.push(Tag {
        key: format!("kubernetes.io/cluster/{}", setup_cluster_name(ctx)),
        value: "shared".to_string(),
    });
    tags.push(Tag {
        key: if is_public {
            "kubernetes.io/role/elb".to_string()
        } else {
            "kubernetes.io/role/internal-elb".to_string()
        },
        value: "1".to_string(),
    });

    let response = client
        .create_subnet(CreateSubnetRequest {
            vpc_id: vpc_id.clone(),
            cidr_block: cidr_block.clone(),
            availability_zone: Some(zones[index].clone()),
            availability_zone_id: None,
            tag_specifications: Some(vec![TagSpecification {
                resource_type: "subnet".to_string(),
                tags,
            }]),
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create EKS subnet '{cidr_block}'"),
            resource_id: Some(config.id.clone()),
        })?;
    let subnet_id = required_value(
        response.subnet.and_then(|subnet| subnet.subnet_id),
        config,
        "EKS subnet id",
    )?;
    if is_public {
        controller.aws_public_subnet_ids.push(subnet_id);
        controller.status_message = Some("Created EKS public subnet".to_string());
    } else {
        controller.aws_private_subnet_ids.push(subnet_id);
        controller.status_message = Some("Created EKS private subnet".to_string());
    }
    Ok(())
}

async fn is_nat_gateway_available(
    controller: &KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn Ec2Api,
) -> Result<bool> {
    let nat_gateway_id =
        require_field(&controller.aws_nat_gateway_id, config, "EKS NAT gateway id")?;
    let response = client
        .describe_nat_gateways(DescribeNatGatewaysRequest {
            nat_gateway_ids: Some(vec![nat_gateway_id.clone()]),
            filters: None,
            max_results: None,
            next_token: None,
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS NAT gateway '{nat_gateway_id}'"),
            resource_id: Some(config.id.clone()),
        })?;
    let state = response
        .nat_gateway_set
        .and_then(|set| set.items.into_iter().next())
        .and_then(|gateway| gateway.state)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(state == "available")
}

async fn create_route_ignore_duplicate(
    client: &dyn Ec2Api,
    request: CreateRouteRequest,
    config: &KubernetesCluster,
    label: &str,
) -> Result<()> {
    match client.create_route(request).await {
        Ok(()) => Ok(()),
        Err(error) if is_cloud_conflict(&error) => Ok(()),
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create {label}"),
            resource_id: Some(config.id.clone()),
        })),
    }
}

async fn ensure_eks_addon(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn EksApi,
    cluster_name: &str,
    addon_name: &str,
) -> Result<()> {
    let ready_slot = match addon_name {
        "vpc-cni" => &mut controller.aws_vpc_cni_addon_ready,
        "kube-proxy" => &mut controller.aws_kube_proxy_addon_ready,
        "coredns" => &mut controller.aws_coredns_addon_ready,
        _ => {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: format!("Unsupported EKS add-on '{addon_name}'"),
            }));
        }
    };

    if ready_slot.is_none() {
        match client
            .create_addon(
                cluster_name,
                CreateAddonRequest {
                    addon_name: addon_name.to_string(),
                    addon_version: None,
                    resolve_conflicts: Some("OVERWRITE".to_string()),
                    service_account_role_arn: None,
                    tags: Some(aws_tag_map_for_resource(config)),
                },
            )
            .await
        {
            Ok(response) => {
                *ready_slot = Some(response.addon.status.as_deref() == Some("ACTIVE"));
            }
            Err(error) if is_cloud_conflict(&error) => {
                *ready_slot = Some(false);
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create EKS add-on '{addon_name}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }
        controller.status_message = Some(format!("Started EKS add-on '{addon_name}' setup"));
        return Ok(());
    }

    let addon = client
        .describe_addon(cluster_name, addon_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS add-on '{addon_name}'"),
            resource_id: Some(config.id.clone()),
        })?
        .addon;
    let status = addon.status.unwrap_or_else(|| "UNKNOWN".to_string());
    *ready_slot = Some(status == "ACTIVE");
    controller.status_message = Some(format!(
        "Waiting for EKS add-on '{addon_name}' status ACTIVE; current status is {status}"
    ));
    Ok(())
}

async fn ensure_eks_nodegroup(
    controller: &mut KubernetesClusterController,
    config: &KubernetesCluster,
    client: &dyn EksApi,
    cluster_name: &str,
) -> Result<()> {
    let nodegroup_name = controller
        .aws_nodegroup_name
        .clone()
        .unwrap_or_else(|| format!("{}-eks-mng", cluster_name));
    if controller.aws_nodegroup_name.is_none() {
        let node_role = require_field(
            &controller.aws_managed_node_role_arn,
            config,
            "EKS managed node role ARN",
        )?;
        client
            .create_nodegroup(
                cluster_name,
                CreateNodegroupRequest {
                    nodegroup_name: nodegroup_name.clone(),
                    node_role: node_role.clone(),
                    subnets: controller.aws_private_subnet_ids.clone(),
                    ami_type: Some("AL2023_ARM_64_STANDARD".to_string()),
                    capacity_type: Some("ON_DEMAND".to_string()),
                    disk_size: Some(20),
                    instance_types: Some(vec!["t4g.medium".to_string()]),
                    scaling_config: Some(NodegroupScalingConfig {
                        desired_size: Some(2),
                        max_size: Some(3),
                        min_size: Some(2),
                    }),
                    update_config: Some(NodegroupUpdateConfig {
                        max_unavailable: Some(1),
                        max_unavailable_percentage: None,
                    }),
                    tags: Some(aws_tag_map_for_resource(config)),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create EKS managed node group '{nodegroup_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        controller.aws_nodegroup_name = Some(nodegroup_name);
        controller.aws_nodegroup_ready = Some(false);
        controller.status_message = Some("Started EKS managed node group creation".to_string());
        return Ok(());
    }

    let nodegroup = client
        .describe_nodegroup(cluster_name, &nodegroup_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to describe EKS managed node group '{nodegroup_name}'"),
            resource_id: Some(config.id.clone()),
        })?
        .nodegroup;
    let status = nodegroup.status.unwrap_or_else(|| "UNKNOWN".to_string());
    controller.aws_nodegroup_ready = Some(status == "ACTIVE");
    controller.status_message = Some(format!(
        "Waiting for EKS managed node group status ACTIVE; current status is {status}"
    ));
    Ok(())
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
        "{} KubernetesCluster substrate",
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

    debug!(cluster_id = %config.id, "KubernetesCluster substrate ready");

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
        .get_kubernetes_deployment_client(kubernetes_config)
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

fn has_controller_setup_state(controller: &KubernetesClusterController) -> bool {
    has_controller_cluster_identity(controller)
        || controller.aws_vpc_id.is_some()
        || controller.aws_internet_gateway_id.is_some()
        || controller.aws_nat_eip_allocation_id.is_some()
        || controller.aws_nat_gateway_id.is_some()
        || !controller.aws_public_subnet_ids.is_empty()
        || !controller.aws_private_subnet_ids.is_empty()
        || controller.aws_public_route_table_id.is_some()
        || controller.aws_private_route_table_id.is_some()
        || !controller.aws_route_table_association_ids.is_empty()
        || controller.aws_cluster_role_name.is_some()
        || controller.aws_node_role_name.is_some()
        || controller.aws_managed_node_role_name.is_some()
        || controller.aws_oidc_provider_arn.is_some()
        || controller.aws_vpc_cni_addon_ready.is_some()
        || controller.aws_nodegroup_name.is_some()
        || controller.aws_kube_proxy_addon_ready.is_some()
        || controller.aws_coredns_addon_ready.is_some()
        || controller.azure_delete_operation.is_some()
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

fn required_value(
    value: Option<String>,
    config: &KubernetesCluster,
    field: &str,
) -> Result<String> {
    value.filter(|value| !value.is_empty()).ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message: format!("Cloud provider response did not include required {field}"),
        })
    })
}

fn require_field<'a>(
    value: &'a Option<String>,
    config: &KubernetesCluster,
    field: &str,
) -> Result<&'a String> {
    value
        .as_ref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: format!("KubernetesCluster controller state is missing required {field}"),
            })
        })
}

fn is_cloud_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

fn is_cloud_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

fn aws_service_trust_policy(service: &str) -> String {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": service },
            "Action": ["sts:AssumeRole", "sts:TagSession"],
        }]
    })
    .to_string()
}

fn aws_role_tags(
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> Vec<CreateRoleTag> {
    aws_tag_map(config, ctx)
        .into_iter()
        .map(|(key, value)| CreateRoleTag { key, value })
        .collect()
}

fn aws_tag_map(
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> HashMap<String, String> {
    let mut tags = aws_tag_map_for_resource(config);
    tags.insert(
        "alien-resource-prefix".to_string(),
        ctx.resource_prefix.to_string(),
    );
    tags
}

fn aws_tag_map_for_resource(config: &KubernetesCluster) -> HashMap<String, String> {
    HashMap::from([("alien-resource".to_string(), config.id.clone())])
}

fn aws_ec2_tags(config: &KubernetesCluster, ctx: &ResourceControllerContext<'_>) -> Vec<Tag> {
    aws_tag_map(config, ctx)
        .into_iter()
        .map(|(key, value)| Tag { key, value })
        .collect()
}

fn ec2_tag_specification(
    resource_type: &str,
    config: &KubernetesCluster,
    ctx: &ResourceControllerContext<'_>,
) -> TagSpecification {
    TagSpecification {
        resource_type: resource_type.to_string(),
        tags: aws_ec2_tags(config, ctx),
    }
}

fn eks_cluster_role_name(ctx: &ResourceControllerContext<'_>) -> String {
    format!("{}-eks-cluster", ctx.resource_prefix)
}

fn eks_node_role_name(ctx: &ResourceControllerContext<'_>) -> String {
    format!("{}-eks-node", ctx.resource_prefix)
}

fn eks_managed_node_role_name(ctx: &ResourceControllerContext<'_>) -> String {
    format!("{}-eks-mng-node", ctx.resource_prefix)
}

fn setup_cluster_name(ctx: &ResourceControllerContext<'_>) -> String {
    format!("{}-k8s", ctx.resource_prefix)
}

fn cluster_name_for_cloud_call(
    config: &KubernetesCluster,
    controller: &KubernetesClusterController,
    ctx: &ResourceControllerContext<'_>,
) -> String {
    controller
        .cluster_name
        .clone()
        .or_else(|| {
            config
                .cloud
                .as_ref()
                .and_then(|cloud| cloud.cluster_name.clone())
        })
        .unwrap_or_else(|| setup_cluster_name(ctx))
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

    #[test]
    fn eks_trust_policy_targets_exact_service_account_subject() {
        let policy = eks_web_identity_trust_policy(
            "arn:aws:iam::123456789012:oidc-provider/oidc.eks.us-east-1.amazonaws.com/id/abc",
            "oidc.eks.us-east-1.amazonaws.com/id/abc",
            "alien-system",
            "demo-execution-sa",
        )
        .expect("policy should serialize");
        let value: serde_json::Value =
            serde_json::from_str(&policy).expect("policy should be JSON");

        assert_eq!(
            value["Statement"][0]["Condition"]["StringEquals"]
                ["oidc.eks.us-east-1.amazonaws.com/id/abc:sub"],
            "system:serviceaccount:alien-system:demo-execution-sa"
        );
        assert_eq!(
            value["Statement"][0]["Condition"]["StringEquals"]
                ["oidc.eks.us-east-1.amazonaws.com/id/abc:aud"],
            "sts.amazonaws.com"
        );
    }

    #[test]
    fn service_account_values_include_annotations_and_labels() {
        let values = service_account_values_json(
            [("azure.workload.identity/client-id", "client-id")],
            [("azure.workload.identity/use", "true")],
        );

        assert_eq!(
            values["annotations"]["azure.workload.identity/client-id"],
            "client-id"
        );
        assert_eq!(values["labels"]["azure.workload.identity/use"], "true");
    }

    #[test]
    fn parses_azure_user_assigned_identity_resource_id() {
        let parsed = azure_identity_resource_group_and_name(
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/alien-agent",
        );

        assert_eq!(parsed, Some(("rg".to_string(), "alien-agent".to_string())));
    }
}
