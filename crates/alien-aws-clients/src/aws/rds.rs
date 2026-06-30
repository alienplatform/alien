//! RDS client scoped to Aurora Serverless v2 (PostgreSQL) — the AWS Postgres backend.
//!
//! RDS is an AWS query-protocol API (form-encoded request, XML response), so this mirrors
//! the EC2 client shape rather than the JSON DynamoDB one. Only the operations the Aurora
//! Postgres controller needs are implemented.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

const RDS_API_VERSION: &str = "2014-10-31";
pub const AURORA_POSTGRESQL_ENGINE: &str = "aurora-postgresql";
pub const SERVERLESS_INSTANCE_CLASS: &str = "db.serverless";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait RdsApi: Send + Sync + std::fmt::Debug {
    async fn create_db_subnet_group(&self, request: CreateDbSubnetGroupRequest) -> Result<()>;
    async fn delete_db_subnet_group(&self, name: &str) -> Result<()>;
    async fn create_db_cluster(&self, request: CreateDbClusterRequest) -> Result<DbCluster>;
    async fn modify_db_cluster(&self, request: ModifyDbClusterRequest) -> Result<()>;
    async fn delete_db_cluster(&self, request: DeleteDbClusterRequest) -> Result<()>;
    async fn describe_db_clusters(&self, identifier: &str) -> Result<Vec<DbCluster>>;
    async fn create_db_instance(&self, request: CreateDbInstanceRequest) -> Result<()>;
    async fn delete_db_instance(&self, request: DeleteDbInstanceRequest) -> Result<()>;
    async fn describe_db_instances(&self, cluster_identifier: &str) -> Result<Vec<DbInstance>>;
}

// ─────────────────────────── request types ───────────────────────────

#[derive(Debug, Clone)]
pub struct CreateDbSubnetGroupRequest {
    pub name: String,
    pub description: String,
    pub subnet_ids: Vec<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CreateDbClusterRequest {
    pub identifier: String,
    pub engine_version: String,
    pub master_username: String,
    pub master_user_password: String,
    pub database_name: String,
    pub db_subnet_group_name: String,
    pub vpc_security_group_ids: Vec<String>,
    /// Aurora Serverless v2 max ACU ceiling (1 ACU ≈ 2 GiB).
    pub max_capacity: f64,
    pub backup_retention_days: u16,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ModifyDbClusterRequest {
    pub identifier: String,
    /// Only set for an in-place major upgrade.
    pub engine_version: Option<String>,
    /// Only set for an in-place ACU (memory) resize; mirrors the create scaling ceiling.
    pub max_capacity: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct DeleteDbClusterRequest {
    pub identifier: String,
}

#[derive(Debug, Clone)]
pub struct CreateDbInstanceRequest {
    pub identifier: String,
    pub cluster_identifier: String,
    pub engine_version: String,
}

#[derive(Debug, Clone)]
pub struct DeleteDbInstanceRequest {
    pub identifier: String,
}

// ─────────────────────────── response types (XML) ───────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DbCluster {
    #[serde(rename = "DBClusterIdentifier")]
    pub identifier: String,
    pub status: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub reader_endpoint: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub engine_version: Option<String>,
    /// The serverless v2 scaling window the cluster reports — drives in-place ACU resize detection.
    #[serde(default, rename = "ServerlessV2ScalingConfiguration")]
    pub serverless_v2_scaling_configuration: Option<ServerlessV2ScalingConfiguration>,
}

/// The cluster's Serverless v2 ACU scaling window, as reported by DescribeDBClusters.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ServerlessV2ScalingConfiguration {
    /// The ACU ceiling (`memory` maps to this; the floor stays 0 for auto-pause).
    pub max_capacity: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DbInstance {
    #[serde(rename = "DBInstanceIdentifier")]
    pub identifier: String,
    #[serde(rename = "DBInstanceStatus")]
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateDbClusterEnvelope {
    #[serde(rename = "CreateDBClusterResult")]
    result: CreateDbClusterResult,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateDbClusterResult {
    #[serde(rename = "DBCluster")]
    db_cluster: DbCluster,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeDbClustersEnvelope {
    #[serde(rename = "DescribeDBClustersResult")]
    result: DescribeDbClustersResult,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeDbClustersResult {
    #[serde(rename = "DBClusters", default)]
    db_clusters: DbClusterList,
}
#[derive(Debug, Default, Deserialize)]
struct DbClusterList {
    #[serde(rename = "DBCluster", default)]
    members: Vec<DbCluster>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeDbInstancesEnvelope {
    #[serde(rename = "DescribeDBInstancesResult")]
    result: DescribeDbInstancesResult,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeDbInstancesResult {
    #[serde(rename = "DBInstances", default)]
    db_instances: DbInstanceList,
}
#[derive(Debug, Default, Deserialize)]
struct DbInstanceList {
    #[serde(rename = "DBInstance", default)]
    members: Vec<DbInstance>,
}

#[derive(Debug, Deserialize)]
struct RdsErrorEnvelope {
    #[serde(rename = "Error")]
    error: RdsError,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RdsError {
    code: String,
    message: String,
}

// ─────────────────────────── form builders (pure, unit-tested) ───────────────────────────

fn indexed_members(form: &mut HashMap<String, String>, prefix: &str, member: &str, values: &[String]) {
    for (i, value) in values.iter().enumerate() {
        form.insert(format!("{prefix}.{member}.{}", i + 1), value.clone());
    }
}

fn tag_members(form: &mut HashMap<String, String>, tags: &HashMap<String, String>) {
    for (i, (key, value)) in tags.iter().enumerate() {
        form.insert(format!("Tags.Tag.{}.Key", i + 1), key.clone());
        form.insert(format!("Tags.Tag.{}.Value", i + 1), value.clone());
    }
}

fn base_form(action: &str) -> HashMap<String, String> {
    HashMap::from([
        ("Action".to_string(), action.to_string()),
        ("Version".to_string(), RDS_API_VERSION.to_string()),
    ])
}

fn create_db_subnet_group_form(r: &CreateDbSubnetGroupRequest) -> HashMap<String, String> {
    let mut form = base_form("CreateDBSubnetGroup");
    form.insert("DBSubnetGroupName".into(), r.name.clone());
    form.insert("DBSubnetGroupDescription".into(), r.description.clone());
    indexed_members(&mut form, "SubnetIds", "SubnetIdentifier", &r.subnet_ids);
    tag_members(&mut form, &r.tags);
    form
}

fn create_db_cluster_form(r: &CreateDbClusterRequest) -> HashMap<String, String> {
    let mut form = base_form("CreateDBCluster");
    form.insert("DBClusterIdentifier".into(), r.identifier.clone());
    form.insert("Engine".into(), AURORA_POSTGRESQL_ENGINE.to_string());
    form.insert("EngineVersion".into(), r.engine_version.clone());
    form.insert("MasterUsername".into(), r.master_username.clone());
    form.insert("MasterUserPassword".into(), r.master_user_password.clone());
    form.insert("DatabaseName".into(), r.database_name.clone());
    form.insert("DBSubnetGroupName".into(), r.db_subnet_group_name.clone());
    indexed_members(
        &mut form,
        "VpcSecurityGroupIds",
        "VpcSecurityGroupId",
        &r.vpc_security_group_ids,
    );
    // minCapacity is always 0 — auto-pause is the point of the AWS backend, not a knob.
    form.insert("ServerlessV2ScalingConfiguration.MinCapacity".into(), "0".into());
    form.insert(
        "ServerlessV2ScalingConfiguration.MaxCapacity".into(),
        format!("{}", r.max_capacity),
    );
    form.insert("BackupRetentionPeriod".into(), r.backup_retention_days.to_string());
    form.insert("StorageEncrypted".into(), "true".into());
    tag_members(&mut form, &r.tags);
    form
}

fn modify_db_cluster_form(r: &ModifyDbClusterRequest) -> HashMap<String, String> {
    let mut form = base_form("ModifyDBCluster");
    form.insert("DBClusterIdentifier".into(), r.identifier.clone());
    form.insert("ApplyImmediately".into(), "true".into());
    if let Some(version) = &r.engine_version {
        form.insert("EngineVersion".into(), version.clone());
        form.insert("AllowMajorVersionUpgrade".into(), "true".into());
    }
    if let Some(max_capacity) = r.max_capacity {
        // Mirror create: the floor stays 0 (auto-pause); only the ACU ceiling moves.
        form.insert("ServerlessV2ScalingConfiguration.MinCapacity".into(), "0".into());
        form.insert(
            "ServerlessV2ScalingConfiguration.MaxCapacity".into(),
            format!("{max_capacity}"),
        );
    }
    form
}

fn create_db_instance_form(r: &CreateDbInstanceRequest) -> HashMap<String, String> {
    let mut form = base_form("CreateDBInstance");
    form.insert("DBInstanceIdentifier".into(), r.identifier.clone());
    form.insert("DBClusterIdentifier".into(), r.cluster_identifier.clone());
    form.insert("Engine".into(), AURORA_POSTGRESQL_ENGINE.to_string());
    form.insert("EngineVersion".into(), r.engine_version.clone());
    form.insert("DBInstanceClass".into(), SERVERLESS_INSTANCE_CLASS.to_string());
    // Hard constraint: Postgres is never public. Pin it on the instance (the cluster has no such
    // flag) so the guarantee holds regardless of the subnet group, matching GCP (`ipv4Enabled=false`)
    // and Azure (`publicNetworkAccess=Disabled`) rather than relying on the subnet group's default.
    form.insert("PubliclyAccessible".into(), "false".into());
    form
}

fn delete_db_cluster_form(r: &DeleteDbClusterRequest) -> HashMap<String, String> {
    let mut form = base_form("DeleteDBCluster");
    form.insert("DBClusterIdentifier".into(), r.identifier.clone());
    // No final snapshot — consistent with every other Alien resource and required for
    // E2E teardown to actually tear down.
    form.insert("SkipFinalSnapshot".into(), "true".into());
    form
}

fn delete_db_instance_form(r: &DeleteDbInstanceRequest) -> HashMap<String, String> {
    let mut form = base_form("DeleteDBInstance");
    form.insert("DBInstanceIdentifier".into(), r.identifier.clone());
    form.insert("SkipFinalSnapshot".into(), "true".into());
    form
}

// ─────────────────────────── client ───────────────────────────

#[derive(Debug, Clone)]
pub struct RdsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl RdsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "rds".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(url) = self.credentials.get_service_endpoint_option("rds") {
            url.to_string()
        } else {
            format!("https://rds.{}.amazonaws.com", self.credentials.region())
        }
    }

    fn get_host(&self) -> String {
        format!("rds.{}.amazonaws.com", self.credentials.region())
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
        let result = crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;
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
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config()).await;
        Self::map_result(result, operation, resource)
    }

    fn map_result<T>(result: Result<T>, operation: &str, resource: &str) -> Result<T> {
        // The create form-body carries MasterUserPassword; strip it from the whole chain BEFORE the
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
        let status = StatusCode::from_u16(*http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        match Self::map_rds_error(status, text, resource) {
            Some(mapped) => Err(e.context(mapped)),
            // No RDS-specific mapping: keep the original HttpResponseError as the source (its request
            // body was stripped above) and attach the non-sensitive `operation` for debuggability.
            // GenericError matches the current HttpResponseError metadata (internal=true,
            // retryable=true), so semantics are unchanged.
            None => Err(e.context(ErrorData::GenericError {
                message: format!("RDS {operation} failed"),
            })),
        }
    }

    fn map_rds_error(status: StatusCode, body: &str, resource: &str) -> Option<ErrorData> {
        // RDS reports "already gone" as 404 with DBClusterNotFoundFault / DBInstanceNotFoundFault;
        // surfacing it as RemoteResourceNotFound lets the controller's delete be best-effort.
        if let Ok(parsed) = quick_xml::de::from_str::<RdsErrorEnvelope>(body) {
            let code = parsed.error.code.as_str();
            if code.ends_with("NotFoundFault") {
                return Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "RDS Resource".into(),
                    resource_name: resource.into(),
                });
            }
            // "still in use / wrong lifecycle state" and "already exists" are conflicts, not
            // generic failures — the controller's create/delete flows key their re-entry retry
            // (`is_conflict_or_exists`) off RemoteResourceConflict (e.g. a subnet group still
            // attached to a deleting cluster: `InvalidDBSubnetGroupStateFault`).
            if code.ends_with("StateFault")
                || code.ends_with("InUseFault")
                || code.ends_with("AlreadyExistsFault")
            {
                return Some(ErrorData::RemoteResourceConflict {
                    resource_type: "RDS Resource".into(),
                    resource_name: resource.into(),
                    message: format!("{}: {}", code, parsed.error.message),
                });
            }
            // Surface throttling/rate-limit faults as RateLimitExceeded — a non-internal,
            // rate-limit-shaped error the executor backs off on — rather than letting them fall
            // through to the internal-flagged GenericError catch-all below.
            if matches!(
                code,
                "Throttling"
                    | "ThrottlingException"
                    | "RequestLimitExceeded"
                    | "ProvisionedThroughputExceededException"
            ) || code.contains("Throttl")
                || code.ends_with("LimitExceeded")
            {
                return Some(ErrorData::RateLimitExceeded {
                    message: format!("{}: {}", code, parsed.error.message),
                });
            }
            // AWS query-protocol client faults — bad parameters, malformed requests, validation —
            // are deterministic: retrying the same call cannot succeed. Map them to a non-retryable
            // InvalidInput so the executor fails fast instead of retrying until timeout. (StateFault
            // / InUseFault / NotFoundFault / AlreadyExistsFault / throttles already returned above as
            // their own retry-appropriate kinds.)
            if code.starts_with("Invalid")
                || code.contains("Validation")
                || code.contains("MalformedQuery")
            {
                return Some(ErrorData::InvalidInput {
                    message: format!("{}: {}", code, parsed.error.message),
                    field_name: None,
                });
            }
            // Quota / capacity faults are operator-actionable (raise the limit) or transient
            // (capacity frees up), not opaque internal failures: map them to non-internal kinds so
            // the message reaches the user (QuotaExceeded) or the executor backs off
            // (RemoteServiceUnavailable), instead of the internal-flagged GenericError below.
            if code.ends_with("QuotaExceededFault") {
                return Some(ErrorData::QuotaExceeded {
                    message: format!("{}: {}", code, parsed.error.message),
                });
            }
            if code.starts_with("Insufficient") || code.ends_with("CapacityFault") {
                return Some(ErrorData::RemoteServiceUnavailable {
                    message: format!("{}: {}", code, parsed.error.message),
                });
            }
            return Some(ErrorData::GenericError {
                message: format!("{}: {}", code, parsed.error.message),
            });
        }
        match status {
            StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                resource_type: "RDS Resource".into(),
                resource_name: resource.into(),
            }),
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => Some(ErrorData::RemoteAccessDenied {
                resource_type: "RDS Resource".into(),
                resource_name: resource.into(),
            }),
            _ => None,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl RdsApi for RdsClient {
    async fn create_db_subnet_group(&self, request: CreateDbSubnetGroupRequest) -> Result<()> {
        let name = request.name.clone();
        self.send_form_no_body(create_db_subnet_group_form(&request), "CreateDBSubnetGroup", &name)
            .await
    }

    async fn delete_db_subnet_group(&self, name: &str) -> Result<()> {
        let mut form = base_form("DeleteDBSubnetGroup");
        form.insert("DBSubnetGroupName".into(), name.to_string());
        self.send_form_no_body(form, "DeleteDBSubnetGroup", name).await
    }

    async fn create_db_cluster(&self, request: CreateDbClusterRequest) -> Result<DbCluster> {
        let id = request.identifier.clone();
        let envelope: CreateDbClusterEnvelope = self
            .send_form(create_db_cluster_form(&request), "CreateDBCluster", &id)
            .await?;
        Ok(envelope.result.db_cluster)
    }

    async fn modify_db_cluster(&self, request: ModifyDbClusterRequest) -> Result<()> {
        self.send_form_no_body(
            modify_db_cluster_form(&request),
            "ModifyDBCluster",
            &request.identifier,
        )
        .await
    }

    async fn delete_db_cluster(&self, request: DeleteDbClusterRequest) -> Result<()> {
        let id = request.identifier.clone();
        self.send_form_no_body(delete_db_cluster_form(&request), "DeleteDBCluster", &id).await
    }

    async fn describe_db_clusters(&self, identifier: &str) -> Result<Vec<DbCluster>> {
        let mut form = base_form("DescribeDBClusters");
        form.insert("DBClusterIdentifier".into(), identifier.to_string());
        let envelope: DescribeDbClustersEnvelope = self
            .send_form(form, "DescribeDBClusters", identifier)
            .await?;
        Ok(envelope.result.db_clusters.members)
    }

    async fn create_db_instance(&self, request: CreateDbInstanceRequest) -> Result<()> {
        let id = request.identifier.clone();
        self.send_form_no_body(create_db_instance_form(&request), "CreateDBInstance", &id).await
    }

    async fn delete_db_instance(&self, request: DeleteDbInstanceRequest) -> Result<()> {
        let id = request.identifier.clone();
        self.send_form_no_body(delete_db_instance_form(&request), "DeleteDBInstance", &id).await
    }

    async fn describe_db_instances(&self, cluster_identifier: &str) -> Result<Vec<DbInstance>> {
        let mut form = base_form("DescribeDBInstances");
        form.insert("Filters.Filter.1.Name".into(), "db-cluster-id".into());
        form.insert("Filters.Filter.1.Values.Value.1".into(), cluster_identifier.to_string());
        let envelope: DescribeDbInstancesEnvelope = self
            .send_form(form, "DescribeDBInstances", cluster_identifier)
            .await?;
        Ok(envelope.result.db_instances.members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster_request() -> CreateDbClusterRequest {
        CreateDbClusterRequest {
            identifier: "stack-db".into(),
            engine_version: "17.4".into(),
            master_username: "alien".into(),
            master_user_password: "secret".into(),
            database_name: "db".into(),
            db_subnet_group_name: "stack-subnets".into(),
            vpc_security_group_ids: vec!["sg-1".into(), "sg-2".into()],
            max_capacity: 4.0,
            backup_retention_days: 7,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn create_cluster_form_pins_min_capacity_zero_and_engine() {
        let form = create_db_cluster_form(&cluster_request());
        assert_eq!(form["Action"], "CreateDBCluster");
        assert_eq!(form["Engine"], "aurora-postgresql");
        assert_eq!(form["ServerlessV2ScalingConfiguration.MinCapacity"], "0");
        assert_eq!(form["ServerlessV2ScalingConfiguration.MaxCapacity"], "4");
        assert_eq!(form["BackupRetentionPeriod"], "7");
        assert_eq!(form["VpcSecurityGroupIds.VpcSecurityGroupId.1"], "sg-1");
        assert_eq!(form["VpcSecurityGroupIds.VpcSecurityGroupId.2"], "sg-2");
    }

    #[test]
    fn modify_cluster_form_moves_acu_ceiling_and_pins_min_zero() {
        let form = modify_db_cluster_form(&ModifyDbClusterRequest {
            identifier: "stack-db".into(),
            engine_version: None,
            max_capacity: Some(8.0),
        });
        assert_eq!(form["Action"], "ModifyDBCluster");
        assert_eq!(form["ApplyImmediately"], "true");
        assert_eq!(form["ServerlessV2ScalingConfiguration.MinCapacity"], "0");
        assert_eq!(form["ServerlessV2ScalingConfiguration.MaxCapacity"], "8");
        // A memory-only resize must not touch the engine version.
        assert!(!form.contains_key("EngineVersion"));
    }

    #[test]
    fn modify_cluster_form_omits_scaling_when_no_memory_change() {
        let form = modify_db_cluster_form(&ModifyDbClusterRequest {
            identifier: "stack-db".into(),
            engine_version: None,
            max_capacity: None,
        });
        assert!(!form.contains_key("ServerlessV2ScalingConfiguration.MaxCapacity"));
    }

    #[test]
    fn delete_cluster_skips_final_snapshot() {
        let form = delete_db_cluster_form(&DeleteDbClusterRequest {
            identifier: "stack-db".into(),
        });
        assert_eq!(form["Action"], "DeleteDBCluster");
        assert_eq!(form["SkipFinalSnapshot"], "true");
    }

    #[test]
    fn create_instance_uses_serverless_class_and_is_never_public() {
        let form = create_db_instance_form(&CreateDbInstanceRequest {
            identifier: "stack-db-1".into(),
            cluster_identifier: "stack-db".into(),
            engine_version: "17.4".into(),
        });
        assert_eq!(form["DBInstanceClass"], "db.serverless");
        assert_eq!(form["Engine"], "aurora-postgresql");
        // pinned on the instance — the cluster has no such flag.
        assert_eq!(form["PubliclyAccessible"], "false");
    }

    #[test]
    fn map_result_strips_master_password_from_error_chain() {
        // The create form-body carries MasterUserPassword and the transport captures it into the
        // HttpResponseError; map_result must strip it before the non-internal head wraps the error,
        // or it survives into durable state / status responses. (The chain-walk is covered in
        // alien-client-core; this pins the redaction ordering at the AWS call site.)
        use alien_error::AlienError;
        const PW: &str = "Sup3rSecret-MasterPassword!";
        let raw = AlienError::new(ErrorData::HttpResponseError {
            message: "Request failed with HTTP 400".into(),
            url: "https://rds.us-east-1.amazonaws.com/".into(),
            http_status: 400,
            http_request_text: Some(format!("Action=CreateDBCluster&MasterUserPassword={PW}")),
            http_response_text: Some("<Error><Code>InvalidParameterValue</Code></Error>".into()),
        });
        let mapped = RdsClient::map_result::<()>(Err(raw), "CreateDBCluster", "stack-db")
            .expect_err("an error result stays an error");
        let json = serde_json::to_string(&mapped).expect("serialize");
        assert!(!json.contains(PW), "master password leaked through map_result: {json}");
    }

    #[test]
    fn describe_clusters_parses_xml() {
        let xml = r#"<DescribeDBClustersResponse><DescribeDBClustersResult><DBClusters>
            <DBCluster><DBClusterIdentifier>stack-db</DBClusterIdentifier><Status>available</Status>
            <Endpoint>stack-db.cluster-x.us-east-1.rds.amazonaws.com</Endpoint><Port>5432</Port>
            <EngineVersion>17.4</EngineVersion>
            <ServerlessV2ScalingConfiguration><MinCapacity>0</MinCapacity><MaxCapacity>8</MaxCapacity></ServerlessV2ScalingConfiguration></DBCluster>
        </DBClusters></DescribeDBClustersResult></DescribeDBClustersResponse>"#;
        let env: DescribeDbClustersEnvelope = quick_xml::de::from_str(xml).expect("parses");
        let clusters = env.result.db_clusters.members;
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].identifier, "stack-db");
        assert_eq!(clusters[0].status, "available");
        assert_eq!(clusters[0].port, Some(5432));
        // The serverless scaling ceiling must parse: `ready`/refresh read it to detect a day-2 memory
        // change, and an imported cluster (max_capacity None) relies on this XML to learn its ceiling.
        assert_eq!(
            clusters[0]
                .serverless_v2_scaling_configuration
                .as_ref()
                .map(|s| s.max_capacity),
            Some(8.0)
        );
    }
}
