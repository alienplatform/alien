//! AWS EKS client.
//!
//! This module intentionally exposes the cluster, add-on, managed node group,
//! and access-entry operations needed by Alien's Kubernetes setup controller.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait EksApi: Send + Sync + Debug {
    async fn create_cluster(&self, request: CreateClusterRequest) -> Result<CreateClusterResponse>;
    async fn describe_cluster(&self, name: &str) -> Result<DescribeClusterResponse>;
    async fn delete_cluster(&self, name: &str) -> Result<DeleteClusterResponse>;

    async fn create_addon(
        &self,
        cluster_name: &str,
        request: CreateAddonRequest,
    ) -> Result<CreateAddonResponse>;
    async fn describe_addon(
        &self,
        cluster_name: &str,
        addon_name: &str,
    ) -> Result<DescribeAddonResponse>;
    async fn delete_addon(
        &self,
        cluster_name: &str,
        addon_name: &str,
    ) -> Result<DeleteAddonResponse>;

    async fn create_nodegroup(
        &self,
        cluster_name: &str,
        request: CreateNodegroupRequest,
    ) -> Result<CreateNodegroupResponse>;
    async fn describe_nodegroup(
        &self,
        cluster_name: &str,
        nodegroup_name: &str,
    ) -> Result<DescribeNodegroupResponse>;
    async fn delete_nodegroup(
        &self,
        cluster_name: &str,
        nodegroup_name: &str,
    ) -> Result<DeleteNodegroupResponse>;

    async fn create_access_entry(
        &self,
        cluster_name: &str,
        request: CreateAccessEntryRequest,
    ) -> Result<CreateAccessEntryResponse>;
    async fn delete_access_entry(&self, cluster_name: &str, principal_arn: &str) -> Result<()>;
    async fn associate_access_policy(
        &self,
        cluster_name: &str,
        principal_arn: &str,
        request: AssociateAccessPolicyRequest,
    ) -> Result<AssociateAccessPolicyResponse>;
}

#[derive(Debug, Clone)]
pub struct EksClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl EksClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "eks".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("eks") {
            override_url.to_string()
        } else {
            format!("https://eks.{}.amazonaws.com", self.credentials.region())
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
        operation: &str,
        resource_name: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let mut builder = self
            .client
            .request(method, &url)
            .host(&format!("eks.{}.amazonaws.com", self.credentials.region()));

        if let Some(body) = body {
            builder = builder
                .header("Content-Type", "application/json")
                .body(body);
        }

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
        Self::map_result(result, operation, resource_name)
    }

    async fn send_no_response(
        &self,
        method: Method,
        path: &str,
        operation: &str,
        resource_name: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let builder = self
            .client
            .request(method, &url)
            .host(&format!("eks.{}.amazonaws.com", self.credentials.region()));

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;
        Self::map_result(result, operation, resource_name)
    }

    fn map_result<T>(result: Result<T>, operation: &str, resource_name: &str) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorData::HttpResponseError { http_status, .. }) = &error.error {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    Err(error.context(status_to_client_error(
                        status,
                        "EKS resource",
                        resource_name,
                    )))
                } else {
                    Err(error.context(ErrorData::RemoteServiceUnavailable {
                        message: format!("EKS {operation} failed for {resource_name}"),
                    }))
                }
            }
        }
    }
}

#[async_trait]
impl EksApi for EksClient {
    async fn create_cluster(&self, request: CreateClusterRequest) -> Result<CreateClusterResponse> {
        let resource_name = request.name.clone();
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EKS cluster '{resource_name}'"),
            },
        )?;
        self.send_json(
            Method::POST,
            "/clusters",
            Some(body),
            "CreateCluster",
            &resource_name,
        )
        .await
    }

    async fn describe_cluster(&self, name: &str) -> Result<DescribeClusterResponse> {
        self.send_json(
            Method::GET,
            &format!("/clusters/{}", urlencoding::encode(name)),
            None,
            "DescribeCluster",
            name,
        )
        .await
    }

    async fn delete_cluster(&self, name: &str) -> Result<DeleteClusterResponse> {
        self.send_json(
            Method::DELETE,
            &format!("/clusters/{}", urlencoding::encode(name)),
            None,
            "DeleteCluster",
            name,
        )
        .await
    }

    async fn create_addon(
        &self,
        cluster_name: &str,
        request: CreateAddonRequest,
    ) -> Result<CreateAddonResponse> {
        let resource_name = format!("{cluster_name}/{}", request.addon_name);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EKS add-on '{resource_name}'"),
            },
        )?;
        self.send_json(
            Method::POST,
            &format!("/clusters/{}/addons", urlencoding::encode(cluster_name)),
            Some(body),
            "CreateAddon",
            &resource_name,
        )
        .await
    }

    async fn describe_addon(
        &self,
        cluster_name: &str,
        addon_name: &str,
    ) -> Result<DescribeAddonResponse> {
        let resource_name = format!("{cluster_name}/{addon_name}");
        self.send_json(
            Method::GET,
            &format!(
                "/clusters/{}/addons/{}",
                urlencoding::encode(cluster_name),
                urlencoding::encode(addon_name)
            ),
            None,
            "DescribeAddon",
            &resource_name,
        )
        .await
    }

    async fn delete_addon(
        &self,
        cluster_name: &str,
        addon_name: &str,
    ) -> Result<DeleteAddonResponse> {
        let resource_name = format!("{cluster_name}/{addon_name}");
        self.send_json(
            Method::DELETE,
            &format!(
                "/clusters/{}/addons/{}",
                urlencoding::encode(cluster_name),
                urlencoding::encode(addon_name)
            ),
            None,
            "DeleteAddon",
            &resource_name,
        )
        .await
    }

    async fn create_nodegroup(
        &self,
        cluster_name: &str,
        request: CreateNodegroupRequest,
    ) -> Result<CreateNodegroupResponse> {
        let resource_name = format!("{cluster_name}/{}", request.nodegroup_name);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EKS node group '{resource_name}'"),
            },
        )?;
        self.send_json(
            Method::POST,
            &format!(
                "/clusters/{}/node-groups",
                urlencoding::encode(cluster_name)
            ),
            Some(body),
            "CreateNodegroup",
            &resource_name,
        )
        .await
    }

    async fn describe_nodegroup(
        &self,
        cluster_name: &str,
        nodegroup_name: &str,
    ) -> Result<DescribeNodegroupResponse> {
        let resource_name = format!("{cluster_name}/{nodegroup_name}");
        self.send_json(
            Method::GET,
            &format!(
                "/clusters/{}/node-groups/{}",
                urlencoding::encode(cluster_name),
                urlencoding::encode(nodegroup_name)
            ),
            None,
            "DescribeNodegroup",
            &resource_name,
        )
        .await
    }

    async fn delete_nodegroup(
        &self,
        cluster_name: &str,
        nodegroup_name: &str,
    ) -> Result<DeleteNodegroupResponse> {
        let resource_name = format!("{cluster_name}/{nodegroup_name}");
        self.send_json(
            Method::DELETE,
            &format!(
                "/clusters/{}/node-groups/{}",
                urlencoding::encode(cluster_name),
                urlencoding::encode(nodegroup_name)
            ),
            None,
            "DeleteNodegroup",
            &resource_name,
        )
        .await
    }

    async fn create_access_entry(
        &self,
        cluster_name: &str,
        request: CreateAccessEntryRequest,
    ) -> Result<CreateAccessEntryResponse> {
        let resource_name = format!("{cluster_name}/{}", request.principal_arn);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EKS access entry '{resource_name}'"),
            },
        )?;
        self.send_json(
            Method::POST,
            &format!(
                "/clusters/{}/access-entries",
                urlencoding::encode(cluster_name)
            ),
            Some(body),
            "CreateAccessEntry",
            &resource_name,
        )
        .await
    }

    async fn delete_access_entry(&self, cluster_name: &str, principal_arn: &str) -> Result<()> {
        let resource_name = format!("{cluster_name}/{principal_arn}");
        self.send_no_response(
            Method::DELETE,
            &format!(
                "/clusters/{}/access-entries/{}",
                urlencoding::encode(cluster_name),
                urlencoding::encode(principal_arn)
            ),
            "DeleteAccessEntry",
            &resource_name,
        )
        .await
    }

    async fn associate_access_policy(
        &self,
        cluster_name: &str,
        principal_arn: &str,
        request: AssociateAccessPolicyRequest,
    ) -> Result<AssociateAccessPolicyResponse> {
        let resource_name = format!("{cluster_name}/{principal_arn}/{}", request.policy_arn);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EKS access policy '{resource_name}'"),
            },
        )?;
        self.send_json(
            Method::POST,
            &format!(
                "/clusters/{}/access-entries/{}/access-policies",
                urlencoding::encode(cluster_name),
                urlencoding::encode(principal_arn)
            ),
            Some(body),
            "AssociateAccessPolicy",
            &resource_name,
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateClusterRequest {
    pub name: String,
    pub role_arn: String,
    pub resources_vpc_config: VpcConfigRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_config: Option<CreateAccessConfigRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_self_managed_addons: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_config: Option<ComputeConfigRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes_network_config: Option<KubernetesNetworkConfigRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_config: Option<StorageConfigRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VpcConfigRequest {
    pub subnet_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_private_access: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_public_access: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccessConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_cluster_creator_admin_permissions: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ComputeConfigRequest {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_pools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_role_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNetworkConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elastic_load_balancing: Option<ElasticLoadBalancingRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ElasticLoadBalancingRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_storage: Option<BlockStorageRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BlockStorageRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateAddonRequest {
    pub addon_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addon_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolve_conflicts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_role_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateNodegroupRequest {
    pub nodegroup_name: String,
    pub node_role: String,
    pub subnets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ami_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling_config: Option<NodegroupScalingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_config: Option<NodegroupUpdateConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodegroupScalingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodegroupUpdateConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_unavailable: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_unavailable_percentage: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccessEntryRequest {
    pub principal_arn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes_groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AssociateAccessPolicyRequest {
    pub policy_arn: String,
    pub access_scope: AccessScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AccessScope {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespaces: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClusterResponse {
    pub cluster: EksCluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeClusterResponse {
    pub cluster: EksCluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteClusterResponse {
    pub cluster: EksCluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EksCluster {
    pub name: String,
    #[serde(default)]
    pub arn: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub certificate_authority: Option<CertificateAuthority>,
    #[serde(default)]
    pub identity: Option<EksClusterIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateAuthority {
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EksClusterIdentity {
    #[serde(default)]
    pub oidc: Option<EksClusterOidcIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EksClusterOidcIdentity {
    #[serde(default)]
    pub issuer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAddonResponse {
    pub addon: Addon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeAddonResponse {
    pub addon: Addon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAddonResponse {
    pub addon: Addon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Addon {
    pub addon_name: String,
    pub cluster_name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub addon_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNodegroupResponse {
    pub nodegroup: Nodegroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeNodegroupResponse {
    pub nodegroup: Nodegroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteNodegroupResponse {
    pub nodegroup: Nodegroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nodegroup {
    pub nodegroup_name: String,
    pub cluster_name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub nodegroup_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccessEntryResponse {
    pub access_entry: AccessEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociateAccessPolicyResponse {
    pub cluster_name: String,
    pub principal_arn: String,
    pub associated_access_policy: AssociatedAccessPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    pub cluster_name: String,
    pub principal_arn: String,
    #[serde(default)]
    pub access_entry_arn: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociatedAccessPolicy {
    pub policy_arn: String,
    pub access_scope: AccessScope,
}

fn status_to_client_error(
    status: StatusCode,
    resource_type: &str,
    resource_name: &str,
) -> ErrorData {
    match status {
        StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        },
        StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
            message: format!("{resource_type} '{resource_name}' already exists or is busy"),
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        },
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        },
        StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
            message: format!("EKS request throttled for {resource_type} '{resource_name}'"),
        },
        _ => ErrorData::RemoteServiceUnavailable {
            message: format!("EKS request failed for {resource_type} '{resource_name}'"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_eks_auto_mode_cluster_shape() {
        let request = CreateClusterRequest::builder()
            .name("alien-e2e".to_string())
            .role_arn("arn:aws:iam::123456789012:role/eks-cluster".to_string())
            .resources_vpc_config(
                VpcConfigRequest::builder()
                    .subnet_ids(vec!["subnet-a".to_string(), "subnet-b".to_string()])
                    .endpoint_public_access(true)
                    .endpoint_private_access(true)
                    .build(),
            )
            .bootstrap_self_managed_addons(false)
            .access_config(
                CreateAccessConfigRequest::builder()
                    .authentication_mode("API_AND_CONFIG_MAP".to_string())
                    .bootstrap_cluster_creator_admin_permissions(true)
                    .build(),
            )
            .compute_config(
                ComputeConfigRequest::builder()
                    .enabled(true)
                    .node_pools(vec!["general-purpose".to_string(), "system".to_string()])
                    .node_role_arn("arn:aws:iam::123456789012:role/eks-node".to_string())
                    .build(),
            )
            .kubernetes_network_config(
                KubernetesNetworkConfigRequest::builder()
                    .elastic_load_balancing(
                        ElasticLoadBalancingRequest::builder().enabled(true).build(),
                    )
                    .build(),
            )
            .storage_config(
                StorageConfigRequest::builder()
                    .block_storage(BlockStorageRequest::builder().enabled(true).build())
                    .build(),
            )
            .build();

        let value = serde_json::to_value(request).expect("request should serialize");
        assert_eq!(value["name"], "alien-e2e");
        assert_eq!(value["bootstrapSelfManagedAddons"], false);
        assert_eq!(value["computeConfig"]["enabled"], true);
        assert_eq!(value["computeConfig"]["nodePools"][0], "general-purpose");
        assert_eq!(
            value["kubernetesNetworkConfig"]["elasticLoadBalancing"]["enabled"],
            true
        );
        assert_eq!(value["storageConfig"]["blockStorage"]["enabled"], true);
    }
}
