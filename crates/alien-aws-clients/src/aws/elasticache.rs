//! ElastiCache client scoped to Valkey replication groups (cluster mode disabled).
//!
//! ElastiCache is an AWS query-protocol API (form-encoded request, XML response), like RDS.
//! Only the operations the Redis controller needs are implemented.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, RequestBuilderExt, Result};
use alien_error::ContextError;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

const ELASTICACHE_API_VERSION: &str = "2015-02-02";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ElastiCacheApi: Send + Sync + std::fmt::Debug {
    async fn create_cache_parameter_group(
        &self,
        request: CreateCacheParameterGroupRequest,
    ) -> Result<()>;
    /// Sets parameters; dynamic ones such as `maxmemory-policy` apply without a reboot.
    async fn modify_cache_parameter_group(
        &self,
        name: &str,
        parameters: Vec<(String, String)>,
    ) -> Result<()>;
    async fn describe_cache_parameters(&self, group_name: &str) -> Result<Vec<CacheParameter>>;
    async fn delete_cache_parameter_group(&self, name: &str) -> Result<()>;
    async fn create_cache_subnet_group(&self, request: CreateCacheSubnetGroupRequest)
        -> Result<()>;
    async fn delete_cache_subnet_group(&self, name: &str) -> Result<()>;
    async fn create_replication_group(
        &self,
        request: CreateReplicationGroupRequest,
    ) -> Result<ReplicationGroup>;
    async fn modify_replication_group(&self, request: ModifyReplicationGroupRequest) -> Result<()>;
    async fn delete_replication_group(&self, id: &str) -> Result<()>;
    async fn describe_replication_groups(&self, id: &str) -> Result<Vec<ReplicationGroup>>;
    /// Tags on a resource, used to prove a same-named group is ours.
    async fn list_tags_for_resource(&self, arn: &str) -> Result<HashMap<String, String>>;
}

// ─────────────────────────── request types ───────────────────────────

#[derive(Debug, Clone)]
pub struct CreateCacheParameterGroupRequest {
    pub name: String,
    /// For example `valkey9`.
    pub family: String,
    pub description: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CreateCacheSubnetGroupRequest {
    pub name: String,
    pub description: String,
    pub subnet_ids: Vec<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Clone)]
pub struct CreateReplicationGroupRequest {
    pub id: String,
    pub description: String,
    /// For example `9.0`.
    pub engine_version: String,
    pub node_type: String,
    /// Primary plus replicas (1 or 2).
    pub num_cache_clusters: u8,
    pub multi_az: bool,
    pub parameter_group_name: String,
    pub subnet_group_name: String,
    pub security_group_ids: Vec<String>,
    /// AUTH token; never logged (see the manual `Debug`).
    pub auth_token: String,
    /// Daily snapshot retention in days; 0 turns snapshots off.
    pub snapshot_retention_days: u16,
    pub kms_key_id: Option<String>,
    pub tags: HashMap<String, String>,
}

impl std::fmt::Debug for CreateReplicationGroupRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateReplicationGroupRequest")
            .field("id", &self.id)
            .field("node_type", &self.node_type)
            .field("auth_token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
pub struct ModifyReplicationGroupRequest {
    pub id: String,
    /// Replaces the AUTH token (strategy SET, applied immediately); never logged.
    pub auth_token: Option<String>,
    pub node_type: Option<String>,
    pub snapshot_retention_days: Option<u16>,
    pub multi_az: Option<bool>,
    pub automatic_failover: Option<bool>,
    pub engine_version: Option<String>,
}

// ─────────────────────────── response types (XML) ───────────────────────────

impl std::fmt::Debug for ModifyReplicationGroupRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModifyReplicationGroupRequest")
            .field("id", &self.id)
            .field("node_type", &self.node_type)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CacheParameter {
    pub parameter_name: String,
    #[serde(default)]
    pub parameter_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeGroup {
    #[serde(default)]
    pub primary_endpoint: Option<Endpoint>,
    #[serde(default)]
    pub node_group_members: NodeGroupMembers,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct NodeGroupMembers {
    #[serde(default, rename = "NodeGroupMember")]
    pub members: Vec<NodeGroupMember>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeGroupMember {
    #[serde(default)]
    pub cache_cluster_id: Option<String>,
    #[serde(default)]
    pub current_role: Option<String>,
    #[serde(default)]
    pub preferred_availability_zone: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct NodeGroups {
    #[serde(default, rename = "NodeGroup")]
    pub groups: Vec<NodeGroup>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReplicationGroup {
    #[serde(rename = "ReplicationGroupId")]
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub node_groups: NodeGroups,
    #[serde(default)]
    pub cache_node_type: Option<String>,
    #[serde(default)]
    pub snapshot_retention_limit: Option<u16>,
    #[serde(default)]
    pub transit_encryption_enabled: Option<bool>,
    #[serde(default)]
    pub at_rest_encryption_enabled: Option<bool>,
    #[serde(default)]
    pub auth_token_enabled: Option<bool>,
    #[serde(default, rename = "MultiAZ")]
    pub multi_az: Option<String>,
    #[serde(default)]
    pub automatic_failover: Option<String>,
    #[serde(default)]
    pub cluster_enabled: Option<bool>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default, rename = "KmsKeyId")]
    pub kms_key_id: Option<String>,
    #[serde(default, rename = "ARN")]
    pub arn: Option<String>,
}

impl ReplicationGroup {
    /// The primary endpoint of the single node group.
    pub fn primary_endpoint(&self) -> Option<&Endpoint> {
        self.node_groups
            .groups
            .first()
            .and_then(|g| g.primary_endpoint.as_ref())
    }

    /// Replicas behind the primary.
    pub fn replica_count(&self) -> usize {
        self.node_groups
            .groups
            .first()
            .map(|g| g.node_group_members.members.len().saturating_sub(1))
            .unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
struct CreateReplicationGroupEnvelope {
    #[serde(rename = "CreateReplicationGroupResult")]
    result: ReplicationGroupResult,
}
#[derive(Debug, Deserialize)]
struct ReplicationGroupResult {
    #[serde(rename = "ReplicationGroup")]
    group: ReplicationGroup,
}

#[derive(Debug, Deserialize)]
struct DescribeReplicationGroupsEnvelope {
    #[serde(rename = "DescribeReplicationGroupsResult")]
    result: DescribeReplicationGroupsResult,
}
#[derive(Debug, Deserialize)]
struct DescribeReplicationGroupsResult {
    #[serde(rename = "ReplicationGroups", default)]
    groups: ReplicationGroupList,
}
#[derive(Debug, Default, Deserialize)]
struct ReplicationGroupList {
    #[serde(rename = "ReplicationGroup", default)]
    members: Vec<ReplicationGroup>,
}

#[derive(Debug, Deserialize)]
struct ListTagsEnvelope {
    #[serde(rename = "ListTagsForResourceResult")]
    result: ListTagsResult,
}
#[derive(Debug, Deserialize)]
struct ListTagsResult {
    #[serde(rename = "TagList", default)]
    tags: TagList,
}
#[derive(Debug, Default, Deserialize)]
struct TagList {
    #[serde(rename = "Tag", default)]
    members: Vec<TagEntry>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TagEntry {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct DescribeCacheParametersEnvelope {
    #[serde(rename = "DescribeCacheParametersResult")]
    result: DescribeCacheParametersResult,
}
#[derive(Debug, Deserialize)]
struct DescribeCacheParametersResult {
    #[serde(rename = "Parameters", default)]
    parameters: CacheParameterList,
}
#[derive(Debug, Default, Deserialize)]
struct CacheParameterList {
    #[serde(rename = "Parameter", default)]
    members: Vec<CacheParameter>,
}

#[derive(Debug, Deserialize)]
struct ElastiCacheErrorEnvelope {
    #[serde(rename = "Error")]
    error: ElastiCacheError,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ElastiCacheError {
    code: String,
    message: String,
}

// ─────────────────────────── form builders (pure, unit-tested) ───────────────────────────

fn base_form(action: &str) -> HashMap<String, String> {
    HashMap::from([
        ("Action".to_string(), action.to_string()),
        ("Version".to_string(), ELASTICACHE_API_VERSION.to_string()),
    ])
}

fn tag_members(form: &mut HashMap<String, String>, tags: &HashMap<String, String>) {
    for (i, (key, value)) in tags.iter().enumerate() {
        form.insert(format!("Tags.Tag.{}.Key", i + 1), key.clone());
        form.insert(format!("Tags.Tag.{}.Value", i + 1), value.clone());
    }
}

fn create_replication_group_form(r: &CreateReplicationGroupRequest) -> HashMap<String, String> {
    let mut form = base_form("CreateReplicationGroup");
    form.insert("ReplicationGroupId".into(), r.id.clone());
    form.insert("ReplicationGroupDescription".into(), r.description.clone());
    form.insert("Engine".into(), "valkey".into());
    form.insert("EngineVersion".into(), r.engine_version.clone());
    form.insert("CacheNodeType".into(), r.node_type.clone());
    form.insert("NumCacheClusters".into(), r.num_cache_clusters.to_string());
    form.insert("MultiAZEnabled".into(), r.multi_az.to_string());
    form.insert(
        "AutomaticFailoverEnabled".into(),
        (r.num_cache_clusters > 1).to_string(),
    );
    form.insert(
        "CacheParameterGroupName".into(),
        r.parameter_group_name.clone(),
    );
    form.insert("CacheSubnetGroupName".into(), r.subnet_group_name.clone());
    for (i, id) in r.security_group_ids.iter().enumerate() {
        form.insert(
            format!("SecurityGroupIds.SecurityGroupId.{}", i + 1),
            id.clone(),
        );
    }
    // TLS and the AUTH token are never optional.
    form.insert("TransitEncryptionEnabled".into(), "true".into());
    form.insert("TransitEncryptionMode".into(), "required".into());
    form.insert("AtRestEncryptionEnabled".into(), "true".into());
    form.insert("AuthToken".into(), r.auth_token.clone());
    form.insert(
        "SnapshotRetentionLimit".into(),
        r.snapshot_retention_days.to_string(),
    );
    if let Some(key) = &r.kms_key_id {
        form.insert("KmsKeyId".into(), key.clone());
    }
    tag_members(&mut form, &r.tags);
    form
}

fn modify_replication_group_form(r: &ModifyReplicationGroupRequest) -> HashMap<String, String> {
    let mut form = base_form("ModifyReplicationGroup");
    form.insert("ReplicationGroupId".into(), r.id.clone());
    form.insert("ApplyImmediately".into(), "true".into());
    if let Some(v) = &r.node_type {
        form.insert("CacheNodeType".into(), v.clone());
    }
    if let Some(v) = r.snapshot_retention_days {
        form.insert("SnapshotRetentionLimit".into(), v.to_string());
    }
    if let Some(v) = r.multi_az {
        form.insert("MultiAZEnabled".into(), v.to_string());
    }
    if let Some(v) = r.automatic_failover {
        form.insert("AutomaticFailoverEnabled".into(), v.to_string());
    }
    if let Some(v) = &r.engine_version {
        form.insert("EngineVersion".into(), v.clone());
    }
    if let Some(v) = &r.auth_token {
        form.insert("AuthToken".into(), v.clone());
        form.insert("AuthTokenUpdateStrategy".into(), "SET".into());
    }
    form
}

// ─────────────────────────── client ───────────────────────────

#[derive(Debug, Clone)]
pub struct ElastiCacheClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl ElastiCacheClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "elasticache".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(url) = self.credentials.get_service_endpoint_option("elasticache") {
            url.to_string()
        } else {
            format!(
                "https://elasticache.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    fn get_host(&self) -> String {
        format!("elasticache.{}.amazonaws.com", self.credentials.region())
    }

    async fn send_form<T: DeserializeOwned + Send + 'static>(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();
        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&body)
            .body(body);
        // The controller owns retries and observes the result of an interrupted mutation.
        // In particular, never replay a failover or create before reading its remote state.
        let result = builder
            .sign_aws_request(&self.sign_config())?
            .send_xml::<T>()
            .await;
        Self::map_result(result, operation, resource)
    }

    async fn send_form_no_body(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();
        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&body)
            .body(body);
        let result =
            crate::aws::aws_request_utils::sign_send_no_response_once(builder, &self.sign_config())
                .await;
        Self::map_result(result, operation, resource)
    }

    fn map_result<T>(result: Result<T>, operation: &str, resource: &str) -> Result<T> {
        // The create form-body carries AuthToken; strip it from the whole chain BEFORE the
        // non-internal head wraps below, or it survives in the source chain (head-only sanitization)
        // and reaches durable state / external responses. See `redact_request_body`.
        let result = alien_client_core::redact_request_body(result);
        let Err(e) = result else {
            return result;
        };
        let Some(ErrorData::HttpResponseError {
            http_status,
            http_response_text: Some(text),
            ..
        }) = &e.error
        else {
            return Err(e);
        };
        let status =
            StatusCode::from_u16(*http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        match Self::map_elasticache_error(status, text, resource) {
            Some(mapped) => Err(e.context(mapped)),
            // No RDS-specific mapping: keep the original HttpResponseError as the source (its request
            // body was stripped above) and attach the non-sensitive `operation` for debuggability.
            // GenericError matches the current HttpResponseError metadata (internal=true,
            // retryable=true), so semantics are unchanged.
            None => Err(e.context(ErrorData::GenericError {
                message: format!("ElastiCache {operation} failed"),
            })),
        }
    }

    fn map_elasticache_error(status: StatusCode, body: &str, resource: &str) -> Option<ErrorData> {
        if matches!(status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) {
            return Some(ErrorData::RemoteAccessDenied {
                resource_type: "ElastiCache Resource".into(),
                resource_name: resource.into(),
            });
        }
        let parsed = quick_xml::de::from_str::<ElastiCacheErrorEnvelope>(body).ok()?;
        let code = parsed.error.code.as_str();
        if code.ends_with("NotFound") || code.ends_with("NotFoundFault") {
            return Some(ErrorData::RemoteResourceNotFound {
                resource_type: "ElastiCache Resource".into(),
                resource_name: resource.into(),
            });
        }
        // Wrong lifecycle state, still in use, or already exists: conflicts the controller's
        // re-entry logic keys off, not generic failures.
        if code.ends_with("StateFault")
            || code.ends_with("State")
            || code.ends_with("InUse")
            || code.ends_with("AlreadyExists")
            || code.ends_with("AlreadyExistsFault")
            || code == "InvalidParameterCombination" && parsed.error.message.contains("in use")
        {
            return Some(ErrorData::RemoteResourceConflict {
                resource_type: "ElastiCache Resource".into(),
                resource_name: resource.into(),
                message: format!("{}: {}", code, parsed.error.message),
            });
        }
        if matches!(
            code,
            "Throttling" | "ThrottlingException" | "RequestLimitExceeded"
        ) {
            return Some(ErrorData::RateLimitExceeded {
                message: format!("{}: {}", code, parsed.error.message),
            });
        }
        None
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ElastiCacheApi for ElastiCacheClient {
    async fn create_cache_parameter_group(
        &self,
        request: CreateCacheParameterGroupRequest,
    ) -> Result<()> {
        let mut form = base_form("CreateCacheParameterGroup");
        form.insert("CacheParameterGroupName".into(), request.name.clone());
        form.insert("CacheParameterGroupFamily".into(), request.family);
        form.insert("Description".into(), request.description);
        tag_members(&mut form, &request.tags);
        self.send_form_no_body(form, "CreateCacheParameterGroup", &request.name)
            .await
    }

    async fn modify_cache_parameter_group(
        &self,
        name: &str,
        parameters: Vec<(String, String)>,
    ) -> Result<()> {
        let mut form = base_form("ModifyCacheParameterGroup");
        form.insert("CacheParameterGroupName".into(), name.to_string());
        for (i, (key, value)) in parameters.iter().enumerate() {
            let n = i + 1;
            form.insert(
                format!("ParameterNameValues.ParameterNameValue.{n}.ParameterName"),
                key.clone(),
            );
            form.insert(
                format!("ParameterNameValues.ParameterNameValue.{n}.ParameterValue"),
                value.clone(),
            );
        }
        self.send_form_no_body(form, "ModifyCacheParameterGroup", name)
            .await
    }

    async fn describe_cache_parameters(&self, group_name: &str) -> Result<Vec<CacheParameter>> {
        let mut form = base_form("DescribeCacheParameters");
        form.insert("CacheParameterGroupName".into(), group_name.to_string());
        let envelope: DescribeCacheParametersEnvelope = self
            .send_form(form, "DescribeCacheParameters", group_name)
            .await?;
        Ok(envelope.result.parameters.members)
    }

    async fn delete_cache_parameter_group(&self, name: &str) -> Result<()> {
        let mut form = base_form("DeleteCacheParameterGroup");
        form.insert("CacheParameterGroupName".into(), name.to_string());
        self.send_form_no_body(form, "DeleteCacheParameterGroup", name)
            .await
    }

    async fn create_cache_subnet_group(
        &self,
        request: CreateCacheSubnetGroupRequest,
    ) -> Result<()> {
        let mut form = base_form("CreateCacheSubnetGroup");
        form.insert("CacheSubnetGroupName".into(), request.name.clone());
        form.insert("CacheSubnetGroupDescription".into(), request.description);
        for (i, id) in request.subnet_ids.iter().enumerate() {
            form.insert(format!("SubnetIds.SubnetIdentifier.{}", i + 1), id.clone());
        }
        tag_members(&mut form, &request.tags);
        self.send_form_no_body(form, "CreateCacheSubnetGroup", &request.name)
            .await
    }

    async fn delete_cache_subnet_group(&self, name: &str) -> Result<()> {
        let mut form = base_form("DeleteCacheSubnetGroup");
        form.insert("CacheSubnetGroupName".into(), name.to_string());
        self.send_form_no_body(form, "DeleteCacheSubnetGroup", name)
            .await
    }

    async fn create_replication_group(
        &self,
        request: CreateReplicationGroupRequest,
    ) -> Result<ReplicationGroup> {
        let form = create_replication_group_form(&request);
        let envelope: CreateReplicationGroupEnvelope = self
            .send_form(form, "CreateReplicationGroup", &request.id)
            .await?;
        Ok(envelope.result.group)
    }

    async fn modify_replication_group(&self, request: ModifyReplicationGroupRequest) -> Result<()> {
        let form = modify_replication_group_form(&request);
        self.send_form_no_body(form, "ModifyReplicationGroup", &request.id)
            .await
    }

    async fn delete_replication_group(&self, id: &str) -> Result<()> {
        let mut form = base_form("DeleteReplicationGroup");
        form.insert("ReplicationGroupId".into(), id.to_string());
        // No final snapshot: deleting a resource deletes its data.
        form.insert("RetainPrimaryCluster".into(), "false".into());
        self.send_form_no_body(form, "DeleteReplicationGroup", id)
            .await
    }

    async fn describe_replication_groups(&self, id: &str) -> Result<Vec<ReplicationGroup>> {
        let mut form = base_form("DescribeReplicationGroups");
        form.insert("ReplicationGroupId".into(), id.to_string());
        let envelope: DescribeReplicationGroupsEnvelope = self
            .send_form(form, "DescribeReplicationGroups", id)
            .await?;
        Ok(envelope.result.groups.members)
    }

    async fn list_tags_for_resource(&self, arn: &str) -> Result<HashMap<String, String>> {
        let mut form = base_form("ListTagsForResource");
        form.insert("ResourceName".into(), arn.to_string());
        let envelope: ListTagsEnvelope = self.send_form(form, "ListTagsForResource", arn).await?;
        Ok(envelope
            .result
            .tags
            .members
            .into_iter()
            .map(|t| (t.key, t.value))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CreateReplicationGroupRequest {
        CreateReplicationGroupRequest {
            id: "app-cache".into(),
            description: "d".into(),
            engine_version: "9.0".into(),
            node_type: "cache.t4g.micro".into(),
            num_cache_clusters: 2,
            multi_az: true,
            parameter_group_name: "pg".into(),
            subnet_group_name: "sng".into(),
            security_group_ids: vec!["sg-1".into(), "sg-2".into()],
            auth_token: "tok".into(),
            snapshot_retention_days: 1,
            kms_key_id: None,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn create_form_is_valkey_cluster_mode_disabled_with_tls_and_auth_always_on() {
        let form = create_replication_group_form(&request());
        assert_eq!(form["Engine"], "valkey");
        assert_eq!(form["TransitEncryptionEnabled"], "true");
        assert_eq!(form["TransitEncryptionMode"], "required");
        assert_eq!(form["AtRestEncryptionEnabled"], "true");
        assert_eq!(form["AuthToken"], "tok");
        // Cluster mode disabled: no node-group configuration keys.
        assert!(!form
            .keys()
            .any(|k| k.starts_with("NodeGroupConfiguration") || k == "NumNodeGroups"));
        assert_eq!(form["AutomaticFailoverEnabled"], "true");
        assert_eq!(form["SecurityGroupIds.SecurityGroupId.2"], "sg-2");
    }

    #[test]
    fn single_node_group_has_no_failover() {
        let mut r = request();
        r.num_cache_clusters = 1;
        r.multi_az = false;
        let form = create_replication_group_form(&r);
        assert_eq!(form["AutomaticFailoverEnabled"], "false");
        assert_eq!(form["MultiAZEnabled"], "false");
    }

    #[test]
    fn debug_never_prints_the_auth_token() {
        assert!(!format!("{:?}", request()).contains("tok\""));
    }

    #[test]
    fn modify_form_sends_only_the_changed_fields() {
        let form = modify_replication_group_form(&ModifyReplicationGroupRequest {
            id: "g".into(),
            node_type: Some("cache.t4g.small".into()),
            ..Default::default()
        });
        assert_eq!(form["CacheNodeType"], "cache.t4g.small");
        assert_eq!(form["ApplyImmediately"], "true");
        assert!(!form.contains_key("SnapshotRetentionLimit"));
        assert!(!form.contains_key("MultiAZEnabled"));
    }

    #[test]
    fn describe_response_parses_endpoint_members_and_flags() {
        let xml = r#"<DescribeReplicationGroupsResponse xmlns="http://elasticache.amazonaws.com/doc/2015-02-02/">
  <DescribeReplicationGroupsResult><ReplicationGroups><ReplicationGroup>
    <ReplicationGroupId>r1</ReplicationGroupId><Status>available</Status>
    <NodeGroups><NodeGroup>
      <PrimaryEndpoint><Address>master.r1.x.cache.amazonaws.com</Address><Port>6379</Port></PrimaryEndpoint>
      <NodeGroupMembers>
        <NodeGroupMember><CacheClusterId>r1-001</CacheClusterId><CurrentRole>primary</CurrentRole></NodeGroupMember>
        <NodeGroupMember><CacheClusterId>r1-002</CacheClusterId><CurrentRole>replica</CurrentRole></NodeGroupMember>
      </NodeGroupMembers>
    </NodeGroup></NodeGroups>
    <CacheNodeType>cache.t4g.micro</CacheNodeType><SnapshotRetentionLimit>1</SnapshotRetentionLimit>
    <TransitEncryptionEnabled>true</TransitEncryptionEnabled><AuthTokenEnabled>true</AuthTokenEnabled>
    <MultiAZ>enabled</MultiAZ><ClusterEnabled>false</ClusterEnabled><Engine>valkey</Engine>
  </ReplicationGroup></ReplicationGroups></DescribeReplicationGroupsResult>
</DescribeReplicationGroupsResponse>"#;
        let parsed: DescribeReplicationGroupsEnvelope = quick_xml::de::from_str(xml).unwrap();
        let group = &parsed.result.groups.members[0];
        assert_eq!(group.status, "available");
        let endpoint = group.primary_endpoint().unwrap();
        assert_eq!(
            (endpoint.address.as_str(), endpoint.port),
            ("master.r1.x.cache.amazonaws.com", 6379)
        );
        assert_eq!(group.replica_count(), 1);
        assert_eq!(group.cluster_enabled, Some(false));
        assert_eq!(group.auth_token_enabled, Some(true));
        assert_eq!(group.cache_node_type.as_deref(), Some("cache.t4g.micro"));
    }

    // The exact error body ElastiCache returned for a missing group in the test account.
    const NOT_FOUND: &str = r#"<ErrorResponse xmlns="http://elasticache.amazonaws.com/doc/2015-02-02/">
  <Error><Type>Sender</Type><Code>ReplicationGroupNotFoundFault</Code><Message>ReplicationGroup redis-nope not found.</Message></Error>
  <RequestId>cdf1d3a6-c259-4677-8c99-5bccb80cd8a8</RequestId></ErrorResponse>"#;

    #[test]
    fn a_missing_group_maps_to_not_found_and_access_denied_to_denied() {
        let mapped =
            ElastiCacheClient::map_elasticache_error(StatusCode::NOT_FOUND, NOT_FOUND, "g");
        assert!(matches!(
            mapped,
            Some(ErrorData::RemoteResourceNotFound { .. })
        ));
        let denied = ElastiCacheClient::map_elasticache_error(StatusCode::FORBIDDEN, "", "g");
        assert!(matches!(denied, Some(ErrorData::RemoteAccessDenied { .. })));
    }

    #[test]
    fn lifecycle_conflicts_are_conflicts() {
        let body = r#"<ErrorResponse><Error><Type>Sender</Type><Code>InvalidReplicationGroupState</Code><Message>not available</Message></Error></ErrorResponse>"#;
        let mapped = ElastiCacheClient::map_elasticache_error(StatusCode::BAD_REQUEST, body, "g");
        assert!(matches!(
            mapped,
            Some(ErrorData::RemoteResourceConflict { .. })
        ));
    }

    #[test]
    fn modify_with_a_token_sets_it_immediately() {
        let form = modify_replication_group_form(&ModifyReplicationGroupRequest {
            id: "g".into(),
            auth_token: Some("fresh".into()),
            ..Default::default()
        });
        assert_eq!(form["AuthToken"], "fresh");
        assert_eq!(form["AuthTokenUpdateStrategy"], "SET");
        let plain = modify_replication_group_form(&ModifyReplicationGroupRequest {
            id: "g".into(),
            ..Default::default()
        });
        assert!(!plain.contains_key("AuthToken"));
        assert!(!plain.contains_key("AuthTokenUpdateStrategy"));
    }

    fn mock_client(server: &httpmock::MockServer) -> ElastiCacheClient {
        use crate::ServiceOverrides;
        use alien_core::{AwsClientConfig, AwsCredentials};
        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("elasticache".to_string(), server.base_url())]),
            }),
        };
        ElastiCacheClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        )
    }

    const SECRET: &str = "s3cr3t-token-do-not-leak-0123456789";

    /// A failed call that carried a token must not leave the token anywhere in the error,
    /// including its serialized form (that is what reaches durable state).
    #[tokio::test]
    async fn failed_create_and_modify_errors_never_contain_the_token() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).body_contains(SECRET);
                then.status(400).body(
                    "<ErrorResponse><Error><Type>Sender</Type><Code>ReplicationGroupAlreadyExists</Code>\
                     <Message>already exists</Message></Error><RequestId>r</RequestId></ErrorResponse>",
                );
            })
            .await;
        let client = mock_client(&server);

        let mut create = request();
        create.auth_token = SECRET.into();
        let err = client.create_replication_group(create).await.unwrap_err();
        assert!(
            err.error
                .as_ref()
                .is_some_and(|e| matches!(e, ErrorData::RemoteResourceConflict { .. })),
            "{err:?}"
        );
        let all = format!("{err:?} {}", serde_json::to_string(&err).unwrap());
        assert!(
            !all.contains(SECRET),
            "create error leaked the token: {all}"
        );

        let err = client
            .modify_replication_group(ModifyReplicationGroupRequest {
                id: "app-cache".into(),
                auth_token: Some(SECRET.into()),
                ..Default::default()
            })
            .await
            .unwrap_err();
        let all = format!("{err:?} {}", serde_json::to_string(&err).unwrap());
        assert!(
            !all.contains(SECRET),
            "modify error leaked the token: {all}"
        );
    }

    #[tokio::test]
    async fn list_tags_parses_the_tag_list() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .body_contains("Action=ListTagsForResource");
                then.status(200).body(
                    "<ListTagsForResourceResponse><ListTagsForResourceResult><TagList>\
                     <Tag><Key>alien:stack</Key><Value>s1</Value></Tag>\
                     <Tag><Key>Name</Key><Value>x</Value></Tag>\
                     </TagList></ListTagsForResourceResult></ListTagsForResourceResponse>",
                );
            })
            .await;
        let tags = mock_client(&server)
            .list_tags_for_resource("arn:aws:elasticache:us-east-2:1:replicationgroup:g")
            .await
            .unwrap();
        assert_eq!(tags["alien:stack"], "s1");
        assert_eq!(tags.len(), 2);
    }
}
