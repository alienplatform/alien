//! Cloud SQL Admin client scoped to the GCP Postgres backend (Cloud SQL for PostgreSQL,
//! Enterprise edition). REST/JSON, mirroring the other GCP clients. Only the operations the
//! Cloud SQL Postgres controller needs are implemented.

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct CloudSqlServiceConfig;

impl GcpServiceConfig for CloudSqlServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://sqladmin.googleapis.com/sql/v1beta4"
    }
    fn default_audience(&self) -> &'static str {
        "https://sqladmin.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Cloud SQL"
    }
    fn service_key(&self) -> &'static str {
        "cloudsql"
    }
}

/// Maps an Alien major version ("15"|"16"|"17") to a Cloud SQL `databaseVersion`.
pub fn postgres_database_version(major: &str) -> String {
    format!("POSTGRES_{major}")
}

// ─────────────────────────── data shapes (camelCase JSON) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInstance {
    pub name: String,
    pub database_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub settings: InstanceSettings,
    /// Master password — request-only; never returned by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_password: Option<String>,
    /// Lifecycle state (response only): PENDING_CREATE / RUNNABLE / FAILED ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// PSC service-attachment URI for the regional consumer `ForwardingRule` to target (response
    /// only). Server-assigned, so it can't be derived from the instance name; present only once
    /// the instance is RUNNABLE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psc_service_attachment_link: Option<String>,
    /// IP mappings the instance reports (response only). The PRIVATE_SERVICE_CONNECT entry is a
    /// fallback attachment surface if a future API revision drops `pscServiceAttachmentLink`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_addresses: Vec<IpMapping>,
}

/// One entry of a Cloud SQL instance's reported IP addresses. `type` distinguishes
/// PRIMARY / PRIVATE / PRIVATE_SERVICE_CONNECT surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpMapping {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ip_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettings {
    pub tier: String,
    pub ip_configuration: IpConfiguration,
    pub backup_configuration: BackupConfiguration,
    /// REGIONAL for highAvailability(), else ZONAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_type: Option<String>,
    /// Edition: ENTERPRISE (the default backend).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
}

/// Partial body for `instances.patch` — only the fields a day-2 resize changes. Cloud SQL merges
/// the provided settings, so a tier-only patch preserves networking, backups, the database, and the
/// master password (the full instance body, with its `root_password`, is never re-sent).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatch {
    pub settings: InstanceSettingsPatch,
}

/// The patchable subset of `InstanceSettings`. Only `tier` today (cpu/memory map to it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettingsPatch {
    pub tier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpConfiguration {
    /// Always false — Postgres is private-only on every platform.
    pub ipv4_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psc_config: Option<PscConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PscConfig {
    pub psc_enabled: bool,
    /// Auto-register a PSC DNS name (Service Directory) for the endpoint. Kept `false`: consumers
    /// connect by the reserved endpoint IP, and auto-DNS would force a `servicedirectory.services.create`
    /// permission on the installer for a name nothing resolves.
    #[serde(default)]
    pub psc_auto_dns_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_consumer_projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfiguration {
    pub enabled: bool,
    pub point_in_time_recovery_enabled: bool,
}

/// A Cloud SQL long-running operation handle.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SqlOperation {
    pub name: String,
    /// PENDING, RUNNING, or DONE.
    #[serde(default)]
    pub status: Option<SqlOperationStatus>,
    /// Set when a DONE operation failed; absent on success. Without it a failed op deserializes
    /// byte-identically to success, so a poller would declare the instance created. Mirrors
    /// `compute.rs` `Operation::error`.
    #[serde(default)]
    pub error: Option<SqlOperationError>,
}

impl SqlOperation {
    pub fn is_done(&self) -> bool {
        matches!(self.status, Some(SqlOperationStatus::Done))
    }

    pub fn has_error(&self) -> bool {
        self.error.as_ref().is_some_and(|e| !e.errors.is_empty())
    }
}

/// Status of a Cloud SQL operation.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SqlOperationStatus {
    SqlOperationStatusUnspecified,
    Pending,
    Running,
    Done,
}

/// Error payload on a failed Cloud SQL operation (`error.errors[]`).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SqlOperationError {
    #[serde(default)]
    pub errors: Vec<SqlOperationErrorItem>,
}

/// One item in a Cloud SQL operation error.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SqlOperationErrorItem {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    pub name: String,
    pub instance: String,
}

// ─────────────────────────── trait + client ───────────────────────────

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudSqlApi: Send + Sync + Debug {
    async fn create_instance(&self, instance: DatabaseInstance) -> Result<SqlOperation>;
    /// Patches an instance in place (currently the machine `tier` for a day-2 resize), returning the
    /// long-running operation to poll.
    async fn patch_instance(&self, instance: &str, patch: InstancePatch) -> Result<SqlOperation>;
    async fn get_instance(&self, instance: &str) -> Result<DatabaseInstance>;
    async fn delete_instance(&self, instance: &str) -> Result<()>;
    async fn create_database(&self, instance: &str, database: &str) -> Result<SqlOperation>;
    /// Fetches a long-running operation so a poller can read its `status`/`error`.
    async fn get_operation(&self, operation: &str) -> Result<SqlOperation>;
}

#[derive(Debug)]
pub struct CloudSqlClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudSqlClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudSqlServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudSqlApi for CloudSqlClient {
    async fn create_instance(&self, instance: DatabaseInstance) -> Result<SqlOperation> {
        let path = format!("projects/{}/instances", self.project_id);
        let name = instance.name.clone();
        // The request body carries `root_password`; strip the captured request body from any error
        // so the password can never reach logs or an external response.
        alien_client_core::redact_request_body(
            self.base
                .execute_request(Method::POST, &path, None, Some(instance), &name)
                .await,
        )
    }

    async fn patch_instance(&self, instance: &str, patch: InstancePatch) -> Result<SqlOperation> {
        // The patch body carries only the machine tier — no password — so no redaction is needed.
        let path = format!("projects/{}/instances/{}", self.project_id, instance);
        self.base
            .execute_request(Method::PATCH, &path, None, Some(patch), instance)
            .await
    }

    async fn get_instance(&self, instance: &str) -> Result<DatabaseInstance> {
        let path = format!("projects/{}/instances/{}", self.project_id, instance);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, instance)
            .await
    }

    async fn delete_instance(&self, instance: &str) -> Result<()> {
        let path = format!("projects/{}/instances/{}", self.project_id, instance);
        self.base
            .execute_request_no_response(Method::DELETE, &path, None, Option::<()>::None, instance)
            .await
    }

    async fn create_database(&self, instance: &str, database: &str) -> Result<SqlOperation> {
        let path = format!("projects/{}/instances/{}/databases", self.project_id, instance);
        let body = Database {
            name: database.to_string(),
            instance: instance.to_string(),
        };
        self.base
            .execute_request(Method::POST, &path, None, Some(body), database)
            .await
    }

    async fn get_operation(&self, operation: &str) -> Result<SqlOperation> {
        let path = format!("projects/{}/operations/{}", self.project_id, operation);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, operation)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_instance() -> DatabaseInstance {
        DatabaseInstance {
            name: "stack-db".into(),
            database_version: postgres_database_version("17"),
            region: Some("us-east1".into()),
            settings: InstanceSettings {
                tier: "db-g1-small".into(),
                ip_configuration: IpConfiguration {
                    ipv4_enabled: false,
                    psc_config: Some(PscConfig {
                        psc_enabled: true,
                        psc_auto_dns_enabled: false,
                        allowed_consumer_projects: vec!["my-project".into()],
                    }),
                },
                backup_configuration: BackupConfiguration {
                    enabled: true,
                    point_in_time_recovery_enabled: true,
                },
                availability_type: Some("REGIONAL".into()),
                edition: Some("ENTERPRISE".into()),
            },
            root_password: Some("secret".into()),
            state: None,
            psc_service_attachment_link: None,
            ip_addresses: vec![],
        }
    }

    #[test]
    fn database_version_maps_major() {
        assert_eq!(postgres_database_version("17"), "POSTGRES_17");
        assert_eq!(postgres_database_version("15"), "POSTGRES_15");
    }

    #[test]
    fn instance_serializes_private_psc_config() {
        let json = serde_json::to_value(private_instance()).unwrap();
        assert_eq!(json["databaseVersion"], "POSTGRES_17");
        // private-only: ipv4Enabled must be false and PSC enabled.
        assert_eq!(json["settings"]["ipConfiguration"]["ipv4Enabled"], false);
        assert_eq!(json["settings"]["ipConfiguration"]["pscConfig"]["pscEnabled"], true);
        assert_eq!(json["settings"]["backupConfiguration"]["enabled"], true);
        assert_eq!(json["settings"]["availabilityType"], "REGIONAL");
        // password is request-only; state absent on the way out.
        assert_eq!(json["rootPassword"], "secret");
        assert!(json.get("state").is_none());
    }

    #[test]
    fn instance_patch_serializes_tier_only() {
        // The patch body must carry only the tier — never the password or any other setting.
        let patch = InstancePatch {
            settings: InstanceSettingsPatch {
                tier: "db-custom-2-7680".into(),
            },
        };
        let json = serde_json::to_value(&patch).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "settings": { "tier": "db-custom-2-7680" } })
        );
    }

    #[test]
    fn instance_deserializes_get_response() {
        // A RUNNABLE PSC instance reports the generated service-attachment link and a
        // PRIVATE_SERVICE_CONNECT IP mapping — the controller reads the link, not a fabricated one.
        let body = r#"{"name":"stack-db","databaseVersion":"POSTGRES_17",
            "settings":{"tier":"db-g1-small",
            "ipConfiguration":{"ipv4Enabled":false},
            "backupConfiguration":{"enabled":true,"pointInTimeRecoveryEnabled":true}},
            "state":"RUNNABLE",
            "pscServiceAttachmentLink":"projects/p/regions/us-east1/serviceAttachments/a-abc123",
            "ipAddresses":[{"type":"PRIVATE_SERVICE_CONNECT","ipAddress":"10.9.8.7"}]}"#;
        let instance: DatabaseInstance = serde_json::from_str(body).unwrap();
        assert_eq!(instance.state.as_deref(), Some("RUNNABLE"));
        assert!(!instance.settings.ip_configuration.ipv4_enabled);
        assert_eq!(
            instance.psc_service_attachment_link.as_deref(),
            Some("projects/p/regions/us-east1/serviceAttachments/a-abc123")
        );
        assert_eq!(
            instance.ip_addresses.first().and_then(|m| m.ip_type.as_deref()),
            Some("PRIVATE_SERVICE_CONNECT")
        );
    }

    #[test]
    fn operation_surfaces_done_failure() {
        // A DONE op that failed must not read as success: error.errors stays visible.
        let body = r#"{"name":"op-1","status":"DONE",
            "error":{"errors":[{"code":"INVALID_DATABASE_VERSION","message":"unsupported version"}]}}"#;
        let op: SqlOperation = serde_json::from_str(body).unwrap();
        assert!(op.is_done());
        assert!(op.has_error(), "a DONE op with error.errors must report has_error");
        assert_eq!(
            op.error.unwrap().errors[0].code.as_deref(),
            Some("INVALID_DATABASE_VERSION")
        );
    }

    #[test]
    fn operation_success_has_no_error() {
        let op: SqlOperation =
            serde_json::from_str(r#"{"name":"op-2","status":"DONE"}"#).unwrap();
        assert!(op.is_done());
        assert!(!op.has_error());
    }
}
