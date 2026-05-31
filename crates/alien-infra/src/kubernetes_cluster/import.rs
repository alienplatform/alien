use alien_core::{
    import::{data::KubernetesClusterImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::kubernetes_cluster::{KubernetesClusterController, KubernetesClusterState};

#[derive(Debug, Default)]
pub struct KubernetesClusterImporter;

impl ResourceImporter for KubernetesClusterImporter {
    type ImportData = KubernetesClusterImportData;

    fn import(
        &self,
        data: KubernetesClusterImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = KubernetesClusterController {
            state: KubernetesClusterState::Ready,
            provider: Some(data.provider),
            ownership: Some(data.ownership),
            namespace: Some(data.namespace),
            cluster_name: data.cluster_name,
            cluster_id: data.cluster_id,
            kubernetes_api_reachable: Some(true),
            namespace_ready: Some(true),
            rbac_ready: Some(true),
            agent_ready: Some(false),
            cloud_metadata_ready: data.cloud_metadata_ready,
            azure_application_gateway_for_containers: data.azure_application_gateway_for_containers,
            cloud_operation_id: None,
            cloud_cluster_status: None,
            azure_delete_operation: None,
            aws_vpc_id: None,
            aws_vpc_dns_configured: None,
            aws_internet_gateway_id: None,
            aws_internet_gateway_detached: None,
            aws_nat_eip_allocation_id: None,
            aws_nat_gateway_id: None,
            aws_public_subnet_ids: Vec::new(),
            aws_private_subnet_ids: Vec::new(),
            aws_public_route_table_id: None,
            aws_private_route_table_id: None,
            aws_public_route_configured: None,
            aws_private_route_configured: None,
            aws_route_table_association_ids: Vec::new(),
            aws_cluster_role_name: None,
            aws_cluster_role_arn: None,
            aws_node_role_name: None,
            aws_node_role_arn: None,
            aws_managed_node_role_name: None,
            aws_managed_node_role_arn: None,
            aws_cluster_role_policies_attached: None,
            aws_node_role_policies_attached: None,
            aws_managed_node_role_policies_attached: None,
            aws_oidc_provider_arn: None,
            aws_vpc_cni_addon_ready: None,
            aws_nodegroup_name: None,
            aws_nodegroup_ready: None,
            aws_kube_proxy_addon_ready: None,
            aws_coredns_addon_ready: None,
            agent_helm_installed: None,
            agent_helm_release: None,
            agent_helm_namespace: None,
            status_message: Some(
                "Kubernetes setup handoff imported; cluster bootstrap completed by setup"
                    .to_string(),
            ),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
