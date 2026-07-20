use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ServiceAccountHeartbeatData {
    AwsIamRole(AwsIamRoleServiceAccountHeartbeatData),
    GcpServiceAccount(GcpServiceAccountHeartbeatData),
    AzureManagedIdentity(AzureManagedIdentityServiceAccountHeartbeatData),
    Local(LocalServiceAccountHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsIamRoleServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub role_name: String,
    pub role_arn: String,
    pub role_id: String,
    pub path: String,
    pub create_date: String,
    pub description: Option<String>,
    pub max_session_duration: Option<i32>,
    pub assume_role_policy_present: bool,
    pub permissions_boundary_type: Option<String>,
    pub permissions_boundary_arn: Option<String>,
    pub tag_count: u32,
    pub managed_tag_count: u32,
    pub attached_policy_count: u32,
    pub attached_policy_names: Vec<String>,
    pub inline_policy_count: u32,
    pub inline_policy_names: Vec<String>,
    pub stack_permissions_applied: bool,
    pub last_used_date: Option<String>,
    pub last_used_region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub unique_id: Option<String>,
    pub email: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub oauth2_client_id: Option<String>,
    pub disabled: Option<bool>,
    pub etag: Option<String>,
    pub project_binding_count: u32,
    pub project_roles: Vec<String>,
    pub service_account_binding_count: u32,
    pub service_account_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureManagedIdentityServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub name: String,
    pub resource_id: String,
    pub resource_group: String,
    pub location: String,
    pub type_: Option<String>,
    pub client_id: Option<String>,
    pub principal_id: Option<String>,
    pub tenant_id: Option<String>,
    pub isolation_scope: Option<String>,
    pub managed_tag_count: u32,
    pub role_assignment_count: u32,
    pub role_assignment_ids: Vec<String>,
    pub custom_role_definition_count: u32,
    pub custom_role_definition_ids: Vec<String>,
    pub stack_permissions_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub identity: String,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum NetworkHeartbeatData {
    AwsVpc(AwsVpcNetworkHeartbeatData),
    GcpVpc(GcpVpcNetworkHeartbeatData),
    AzureVnet(AzureVnetNetworkHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct NetworkHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsVpcNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub vpc_id: Option<String>,
    pub vpc_state: Option<String>,
    pub cidr_block: Option<String>,
    pub public_subnet_ids: Vec<String>,
    pub private_subnet_ids: Vec<String>,
    pub availability_zones: Vec<String>,
    pub internet_gateway_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub route_table_count: u32,
    pub security_group_id: Option<String>,
    pub is_byo_vpc: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpVpcNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub network_name: Option<String>,
    pub network_self_link: Option<String>,
    pub subnetwork_name: Option<String>,
    pub subnetwork_self_link: Option<String>,
    pub region: Option<String>,
    pub cidr_block: Option<String>,
    pub router_name: Option<String>,
    pub cloud_nat_name: Option<String>,
    pub firewall_name: Option<String>,
    pub is_byo_vpc: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureVnetNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub vnet_name: Option<String>,
    pub vnet_resource_id: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub cidr_block: Option<String>,
    pub public_subnet_name: Option<String>,
    pub private_subnet_name: Option<String>,
    pub application_gateway_subnet_name: Option<String>,
    pub private_endpoint_subnet_name: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub public_ip_id: Option<String>,
    pub nsg_id: Option<String>,
    pub is_byo_vnet: bool,
    pub last_byo_vnet_verification_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum RemoteStackManagementHeartbeatData {
    AwsIamRole(AwsRemoteStackManagementHeartbeatData),
    GcpServiceAccount(GcpRemoteStackManagementHeartbeatData),
    AzureManagedIdentity(AzureRemoteStackManagementHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RemoteStackManagementHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub role_name: Option<String>,
    pub role_arn: Option<String>,
    pub management_permissions_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub service_account_email: Option<String>,
    pub service_account_unique_id: Option<String>,
    pub role_bound: bool,
    pub impersonation_granted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub uami_resource_id: Option<String>,
    pub uami_client_id: Option<String>,
    pub uami_principal_id: Option<String>,
    pub tenant_id: Option<String>,
    pub fic_name: Option<String>,
    pub role_definition_id: Option<String>,
    pub role_assignment_ids: Vec<String>,
}
