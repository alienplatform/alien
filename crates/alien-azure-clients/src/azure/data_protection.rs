//! Azure Data Protection client (Azure Backup's Backup vaults).
//!
//! Covers what Azure Disk Backup needs: a Backup vault with a system-assigned identity, backup
//! policies for `Microsoft.Compute/disks`, and backup instances that tie a managed disk to a
//! policy. Disk backups live only in the operational store: Azure Backup takes incremental
//! snapshots into a resource group of the caller's choosing and expires them per the policy.
//!
//! REST reference: https://learn.microsoft.com/en-us/rest/api/dataprotection/
//! Disk backup walkthrough:
//! https://learn.microsoft.com/en-us/azure/backup/backup-azure-dataprotection-use-rest-api-backup-disks

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{Error, ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// Datasource type of an Azure managed disk.
pub const DISK_DATASOURCE_TYPE: &str = "Microsoft.Compute/disks";

/// Built-in role the vault identity needs on each protected disk.
/// https://learn.microsoft.com/en-us/azure/role-based-access-control/built-in-roles/compute#disk-backup-reader
pub const DISK_BACKUP_READER_ROLE_ID: &str = "3e5e47e6-65f7-47ef-90b5-e5dd4d455f24";

/// Built-in role the vault identity needs on the resource group that holds the snapshots.
/// https://learn.microsoft.com/en-us/azure/role-based-access-control/built-in-roles/compute#disk-snapshot-contributor
pub const DISK_SNAPSHOT_CONTRIBUTOR_ROLE_ID: &str = "7efff54f-a5b4-42b5-a1c5-5411624893ce";

/// Error code Azure Backup returns while the vault identity lacks (or does not yet see) the role
/// assignments it needs. Role assignments take minutes to propagate.
const MISSING_PERMISSIONS_ERROR_CODE: &str = "UserErrorMissingRequiredPermissions";

/// True when Azure Backup rejected a request because the vault identity's role assignments are
/// missing or have not propagated yet. Callers requeue on this instead of failing.
pub fn is_missing_permissions_error(error: &Error) -> bool {
    serde_json::to_string(error)
        .map(|serialized| serialized.contains(MISSING_PERMISSIONS_ERROR_CODE))
        .unwrap_or(false)
}

// ─────────────────────────── vault models ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupVault {
    pub location: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<BackupVaultIdentity>,
    pub properties: BackupVaultProperties,
    /// ARM resource ID (response only).
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupVaultIdentity {
    /// "SystemAssigned" for the vault's own identity.
    #[serde(rename = "type")]
    pub type_: String,
    /// Object ID of the system-assigned identity (response only).
    #[serde(default, skip_serializing)]
    pub principal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupVaultProperties {
    /// Required by the API even for operational-store-only workloads such as disks.
    pub storage_settings: Vec<StorageSetting>,
    #[serde(default, skip_serializing)]
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StorageSetting {
    /// "VaultStore" (the only vault datastore type).
    pub datastore_type: String,
    /// "LocallyRedundant", "GeoRedundant" or "ZoneRedundant".
    #[serde(rename = "type")]
    pub type_: String,
}

// ─────────────────────────── policy models ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupPolicyResource {
    pub properties: BackupPolicy,
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupPolicy {
    /// Always "BackupPolicy".
    pub object_type: String,
    pub datasource_types: Vec<String>,
    pub policy_rules: Vec<PolicyRule>,
}

/// A backup policy rule, discriminated by `objectType`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "objectType")]
pub enum PolicyRule {
    /// When backups are taken and which datastore they go to.
    #[serde(rename = "AzureBackupRule", rename_all = "camelCase")]
    Backup {
        name: String,
        backup_parameters: BackupParameters,
        data_store: DataStoreInfo,
        trigger: ScheduleTrigger,
    },
    /// How long backups are kept.
    #[serde(rename = "AzureRetentionRule", rename_all = "camelCase")]
    Retention {
        name: String,
        is_default: bool,
        lifecycles: Vec<RetentionLifecycle>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupParameters {
    /// Always "AzureBackupParams".
    pub object_type: String,
    /// "Incremental" for disks.
    pub backup_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DataStoreInfo {
    /// "OperationalStore" for disks.
    pub data_store_type: String,
    /// Always "DataStoreInfoBase".
    pub object_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleTrigger {
    /// Always "ScheduleBasedTriggerContext".
    pub object_type: String,
    pub schedule: BackupSchedule,
    pub tagging_criteria: Vec<TaggingCriteria>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupSchedule {
    /// ISO 8601 repeating intervals, e.g. `R/2024-01-01T00:00:00+00:00/PT4H`.
    pub repeating_time_intervals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaggingCriteria {
    pub is_default: bool,
    pub tag_info: RetentionTag,
    pub tagging_priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetentionTag {
    pub tag_name: String,
    /// Response only; the service derives it from the tag name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetentionLifecycle {
    pub delete_after: DeleteOption,
    pub source_data_store: DataStoreInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOption {
    /// Always "AbsoluteDeleteOption".
    pub object_type: String,
    /// ISO 8601 duration, e.g. `P7D`.
    pub duration: String,
}

/// How often a disk backup policy takes snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskBackupFrequency {
    /// Every N hours. Azure Disk Backup accepts 1, 2, 4, 6, 8 and 12 for Standard HDD,
    /// Standard SSD and Premium SSD disks.
    Hours(u32),
    /// Once a day.
    Daily,
}

impl DiskBackupFrequency {
    fn iso_duration(self) -> String {
        match self {
            DiskBackupFrequency::Hours(hours) => format!("PT{hours}H"),
            DiskBackupFrequency::Daily => "P1D".to_string(),
        }
    }
}

impl BackupPolicy {
    /// An operational-store disk backup policy: one snapshot every `frequency` starting at
    /// `start` (RFC 3339 date-time; the service requires a full date-time, not a time of day),
    /// each kept for `retention_days`.
    ///
    /// Mirrors the request body in
    /// https://learn.microsoft.com/en-us/azure/backup/backup-azure-dataprotection-use-rest-api-create-update-disk-policy
    pub fn disk_operational(start: &str, frequency: DiskBackupFrequency, retention_days: u32) -> Self {
        let operational_store = || DataStoreInfo {
            data_store_type: "OperationalStore".to_string(),
            object_type: "DataStoreInfoBase".to_string(),
        };
        let rule_name = match frequency {
            DiskBackupFrequency::Hours(_) => "BackupHourly",
            DiskBackupFrequency::Daily => "BackupDaily",
        };
        BackupPolicy {
            object_type: "BackupPolicy".to_string(),
            datasource_types: vec![DISK_DATASOURCE_TYPE.to_string()],
            policy_rules: vec![
                PolicyRule::Backup {
                    name: rule_name.to_string(),
                    backup_parameters: BackupParameters {
                        object_type: "AzureBackupParams".to_string(),
                        backup_type: "Incremental".to_string(),
                    },
                    data_store: operational_store(),
                    trigger: ScheduleTrigger {
                        object_type: "ScheduleBasedTriggerContext".to_string(),
                        schedule: BackupSchedule {
                            repeating_time_intervals: vec![format!(
                                "R/{start}/{}",
                                frequency.iso_duration()
                            )],
                        },
                        tagging_criteria: vec![TaggingCriteria {
                            is_default: true,
                            tag_info: RetentionTag {
                                tag_name: "Default".to_string(),
                                id: None,
                            },
                            tagging_priority: 99,
                        }],
                    },
                },
                PolicyRule::Retention {
                    name: "Default".to_string(),
                    is_default: true,
                    lifecycles: vec![RetentionLifecycle {
                        delete_after: DeleteOption {
                            object_type: "AbsoluteDeleteOption".to_string(),
                            duration: format!("P{retention_days}D"),
                        },
                        source_data_store: operational_store(),
                    }],
                },
            ],
        }
    }
}

// ─────────────────────────── backup instance models ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInstanceResource {
    pub properties: BackupInstance,
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    #[serde(default, skip_serializing)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInstance {
    /// Always "BackupInstance".
    pub object_type: String,
    pub data_source_info: Datasource,
    pub policy_info: PolicyInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendly_name: Option<String>,
    /// e.g. "ConfiguringProtection", "ProtectionConfigured", "BackupsSuspended" (response only).
    #[serde(default, skip_serializing)]
    pub current_protection_state: Option<String>,
    /// e.g. "Provisioning", "Succeeded", "Failed" (response only).
    #[serde(default, skip_serializing)]
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Datasource {
    pub datasource_type: String,
    /// Always "Datasource".
    pub object_type: String,
    /// ARM ID of the protected disk. The API spells it `resourceID`.
    #[serde(rename = "resourceID")]
    pub resource_id: String,
    pub resource_location: String,
    pub resource_name: String,
    pub resource_type: String,
    #[serde(default)]
    pub resource_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PolicyInfo {
    pub policy_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_parameters: Option<PolicyParameters>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PolicyParameters {
    #[serde(default)]
    pub data_store_parameters_list: Vec<OperationalStoreParameters>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationalStoreParameters {
    /// Always "AzureOperationalStoreParameters".
    pub object_type: String,
    /// Always "OperationalStore".
    pub data_store_type: String,
    /// ARM ID of the resource group that receives the snapshots.
    pub resource_group_id: String,
}

impl BackupInstance {
    /// A backup instance protecting one managed disk with `policy_id`, storing its snapshots in
    /// `snapshot_resource_group_id`.
    pub fn for_disk(
        disk_id: &str,
        disk_name: &str,
        location: &str,
        policy_id: &str,
        snapshot_resource_group_id: &str,
    ) -> Self {
        BackupInstance {
            object_type: "BackupInstance".to_string(),
            data_source_info: Datasource {
                datasource_type: DISK_DATASOURCE_TYPE.to_string(),
                object_type: "Datasource".to_string(),
                resource_id: disk_id.to_string(),
                resource_location: location.to_string(),
                resource_name: disk_name.to_string(),
                resource_type: DISK_DATASOURCE_TYPE.to_string(),
                resource_uri: String::new(),
            },
            policy_info: PolicyInfo {
                policy_id: policy_id.to_string(),
                policy_parameters: Some(PolicyParameters {
                    data_store_parameters_list: vec![OperationalStoreParameters {
                        object_type: "AzureOperationalStoreParameters".to_string(),
                        data_store_type: "OperationalStore".to_string(),
                        resource_group_id: snapshot_resource_group_id.to_string(),
                    }],
                }),
            },
            friendly_name: None,
            current_protection_state: None,
            provisioning_state: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidateForBackupRequest<'a> {
    backup_instance: &'a BackupInstance,
}

// ─────────────────────────── trait + client ───────────────────────────

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait DataProtectionApi: Send + Sync + std::fmt::Debug {
    /// Creates or updates a Backup vault. Long-running.
    async fn create_or_update_backup_vault(
        &self,
        resource_group: &str,
        vault_name: &str,
        vault: &BackupVault,
    ) -> Result<OperationResult<BackupVault>>;

    async fn get_backup_vault(&self, resource_group: &str, vault_name: &str)
        -> Result<BackupVault>;

    /// Deletes a Backup vault. The vault must hold no active backup instances. Long-running.
    async fn delete_backup_vault(
        &self,
        resource_group: &str,
        vault_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Creates a backup policy. Azure Backup does not support changing an existing policy; create
    /// a new one and point the backup instances at it. Synchronous.
    async fn create_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
        policy: &BackupPolicy,
    ) -> Result<BackupPolicyResource>;

    async fn get_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
    ) -> Result<BackupPolicyResource>;

    /// Deletes a backup policy. It must not be in use by any backup instance. Synchronous.
    async fn delete_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
    ) -> Result<()>;

    /// Checks whether `backup_instance` could be configured, including the vault identity's
    /// permissions. Long-running; a missing-permission failure surfaces when the operation
    /// completes (see [`is_missing_permissions_error`]).
    async fn validate_for_backup(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance: &BackupInstance,
    ) -> Result<OperationResult<()>>;

    /// Creates a backup instance, or points an existing one at a different policy. Long-running.
    async fn create_or_update_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
        backup_instance: &BackupInstance,
    ) -> Result<OperationResult<BackupInstanceResource>>;

    async fn get_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<BackupInstanceResource>;

    /// Stops future backups of an instance and keeps its recovery points ("stop protection and
    /// retain data as per policy"; the latest one is kept until the instance is deleted).
    /// Long-running.
    async fn suspend_backups(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Resumes backups of an instance whose backups were suspended. Long-running.
    async fn resume_backups(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Stops protection and deletes the instance's backup data. From API version 2025-09-01
    /// the instance moves to soft delete; its operational snapshots stay until the soft-delete
    /// period ends, and are never cleaned up if the vault is soft-deleted too. Long-running.
    async fn delete_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzureDataProtectionClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureDataProtectionClient {
    /// Latest GA version of the Data Protection API.
    const API_VERSION: &'static str = "2026-07-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        let endpoint = token_cache.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    fn vault_path(&self, resource_group: &str, vault_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.DataProtection/backupVaults/{}",
            self.token_cache.config().subscription_id,
            resource_group,
            vault_name
        )
    }

    fn url(&self, path: &str) -> String {
        self.base.build_url(
            path,
            Some(vec![("api-version", Self::API_VERSION.to_string())]),
        )
    }

    async fn signed_request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
    ) -> Result<reqwest::Request> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let builder = match body {
            Some(body) => AzureRequestBuilder::new(method, url)
                .content_type_json()
                .content_length(&body)
                .body(body),
            None => AzureRequestBuilder::new(method, url).content_length(""),
        };
        self.base.sign_request(builder.build()?, &token).await
    }

    fn serialize<T: Serialize>(value: &T, what: &str) -> Result<String> {
        serde_json::to_string(value)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize {what}"),
            })
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str, op: &str, name: &str) -> Result<T> {
        let req = self.signed_request(Method::GET, self.url(path), None).await?;
        let resp = self.base.execute_request(req, op, name).await?;
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure {op}: failed to read body for {name}"),
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure {op}: JSON parse error for {name}"),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl DataProtectionApi for AzureDataProtectionClient {
    async fn create_or_update_backup_vault(
        &self,
        resource_group: &str,
        vault_name: &str,
        vault: &BackupVault,
    ) -> Result<OperationResult<BackupVault>> {
        let body = Self::serialize(vault, &format!("Backup vault {vault_name}"))?;
        let req = self
            .signed_request(
                Method::PUT,
                self.url(&self.vault_path(resource_group, vault_name)),
                Some(body),
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(req, "CreateOrUpdateBackupVault", vault_name)
            .await
    }

    async fn get_backup_vault(
        &self,
        resource_group: &str,
        vault_name: &str,
    ) -> Result<BackupVault> {
        self.get_json(
            &self.vault_path(resource_group, vault_name),
            "GetBackupVault",
            vault_name,
        )
        .await
    }

    async fn delete_backup_vault(
        &self,
        resource_group: &str,
        vault_name: &str,
    ) -> Result<OperationResult<()>> {
        let req = self
            .signed_request(
                Method::DELETE,
                self.url(&self.vault_path(resource_group, vault_name)),
                None,
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(req, "DeleteBackupVault", vault_name)
            .await
    }

    async fn create_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
        policy: &BackupPolicy,
    ) -> Result<BackupPolicyResource> {
        #[derive(Serialize)]
        struct Body<'a> {
            properties: &'a BackupPolicy,
        }
        let body = Self::serialize(
            &Body { properties: policy },
            &format!("backup policy {policy_name}"),
        )?;
        let path = format!(
            "{}/backupPolicies/{policy_name}",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::PUT, self.url(&path), Some(body))
            .await?;
        let resp = self
            .base
            .execute_request(req, "CreateBackupPolicy", policy_name)
            .await?;
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure CreateBackupPolicy: failed to read body for {policy_name}"),
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure CreateBackupPolicy: JSON parse error for {policy_name}"),
            })
    }

    async fn get_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
    ) -> Result<BackupPolicyResource> {
        self.get_json(
            &format!(
                "{}/backupPolicies/{policy_name}",
                self.vault_path(resource_group, vault_name)
            ),
            "GetBackupPolicy",
            policy_name,
        )
        .await
    }

    async fn delete_backup_policy(
        &self,
        resource_group: &str,
        vault_name: &str,
        policy_name: &str,
    ) -> Result<()> {
        let path = format!(
            "{}/backupPolicies/{policy_name}",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::DELETE, self.url(&path), None)
            .await?;
        self.base
            .execute_request(req, "DeleteBackupPolicy", policy_name)
            .await?;
        Ok(())
    }

    async fn validate_for_backup(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance: &BackupInstance,
    ) -> Result<OperationResult<()>> {
        let name = &backup_instance.data_source_info.resource_name;
        let body = Self::serialize(
            &ValidateForBackupRequest { backup_instance },
            &format!("validate-for-backup request for {name}"),
        )?;
        let path = format!(
            "{}/validateForBackup",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::POST, self.url(&path), Some(body))
            .await?;
        // A completed (200) validation returns an OperationJobExtendedInfo body the caller has no
        // use for; only success or failure matters.
        let result: OperationResult<serde_json::Value> = self
            .base
            .execute_request_with_long_running_support(req, "ValidateForBackup", name)
            .await?;
        Ok(match result {
            OperationResult::Completed(_) => OperationResult::Completed(()),
            OperationResult::LongRunning(operation) => OperationResult::LongRunning(operation),
        })
    }

    async fn create_or_update_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
        backup_instance: &BackupInstance,
    ) -> Result<OperationResult<BackupInstanceResource>> {
        #[derive(Serialize)]
        struct Body<'a> {
            properties: &'a BackupInstance,
        }
        let body = Self::serialize(
            &Body {
                properties: backup_instance,
            },
            &format!("backup instance {backup_instance_name}"),
        )?;
        let path = format!(
            "{}/backupInstances/{backup_instance_name}",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::PUT, self.url(&path), Some(body))
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "CreateOrUpdateBackupInstance",
                backup_instance_name,
            )
            .await
    }

    async fn get_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<BackupInstanceResource> {
        self.get_json(
            &format!(
                "{}/backupInstances/{backup_instance_name}",
                self.vault_path(resource_group, vault_name)
            ),
            "GetBackupInstance",
            backup_instance_name,
        )
        .await
    }

    async fn suspend_backups(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>> {
        let path = format!(
            "{}/backupInstances/{backup_instance_name}/suspendBackups",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::POST, self.url(&path), Some("{}".to_string()))
            .await?;
        let result: OperationResult<serde_json::Value> = self
            .base
            .execute_request_with_long_running_support(req, "SuspendBackups", backup_instance_name)
            .await?;
        Ok(match result {
            OperationResult::Completed(_) => OperationResult::Completed(()),
            OperationResult::LongRunning(operation) => OperationResult::LongRunning(operation),
        })
    }

    async fn resume_backups(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>> {
        let path = format!(
            "{}/backupInstances/{backup_instance_name}/resumeBackups",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::POST, self.url(&path), None)
            .await?;
        let result: OperationResult<serde_json::Value> = self
            .base
            .execute_request_with_long_running_support(req, "ResumeBackups", backup_instance_name)
            .await?;
        Ok(match result {
            OperationResult::Completed(_) => OperationResult::Completed(()),
            OperationResult::LongRunning(operation) => OperationResult::LongRunning(operation),
        })
    }

    async fn delete_backup_instance(
        &self,
        resource_group: &str,
        vault_name: &str,
        backup_instance_name: &str,
    ) -> Result<OperationResult<()>> {
        let path = format!(
            "{}/backupInstances/{backup_instance_name}",
            self.vault_path(resource_group, vault_name)
        );
        let req = self
            .signed_request(Method::DELETE, self.url(&path), None)
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "DeleteBackupInstance",
                backup_instance_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alien_error::AlienError;
    use httpmock::{Method::DELETE, Method::GET, Method::POST, Method::PUT, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    const VAULT_PATH: &str = "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg/providers/Microsoft.DataProtection/backupVaults/stack-db-backups";

    fn test_client(server: &MockServer) -> AzureDataProtectionClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureDataProtectionClient::new(Client::new(), AzureTokenCache::new(config))
    }

    #[test]
    fn hourly_policy_matches_documented_disk_policy_body() {
        // The documented example, minus the response-only tag id.
        let expected = json!({
            "objectType": "BackupPolicy",
            "datasourceTypes": ["Microsoft.Compute/disks"],
            "policyRules": [
                {
                    "objectType": "AzureBackupRule",
                    "name": "BackupHourly",
                    "backupParameters": {"objectType": "AzureBackupParams", "backupType": "Incremental"},
                    "dataStore": {"dataStoreType": "OperationalStore", "objectType": "DataStoreInfoBase"},
                    "trigger": {
                        "objectType": "ScheduleBasedTriggerContext",
                        "schedule": {"repeatingTimeIntervals": ["R/2020-04-05T13:00:00+00:00/PT4H"]},
                        "taggingCriteria": [{
                            "isDefault": true,
                            "tagInfo": {"tagName": "Default"},
                            "taggingPriority": 99
                        }]
                    }
                },
                {
                    "objectType": "AzureRetentionRule",
                    "name": "Default",
                    "isDefault": true,
                    "lifecycles": [{
                        "deleteAfter": {"objectType": "AbsoluteDeleteOption", "duration": "P7D"},
                        "sourceDataStore": {"dataStoreType": "OperationalStore", "objectType": "DataStoreInfoBase"}
                    }]
                }
            ]
        });
        let policy = BackupPolicy::disk_operational(
            "2020-04-05T13:00:00+00:00",
            DiskBackupFrequency::Hours(4),
            7,
        );
        assert_eq!(serde_json::to_value(&policy).unwrap(), expected);
    }

    #[test]
    fn daily_policy_uses_p1d_and_daily_rule() {
        let policy =
            BackupPolicy::disk_operational("2024-01-01T00:00:00+00:00", DiskBackupFrequency::Daily, 30);
        let json = serde_json::to_value(&policy).unwrap();
        assert_eq!(json["policyRules"][0]["name"], "BackupDaily");
        assert_eq!(
            json["policyRules"][0]["trigger"]["schedule"]["repeatingTimeIntervals"][0],
            "R/2024-01-01T00:00:00+00:00/P1D"
        );
        assert_eq!(
            json["policyRules"][1]["lifecycles"][0]["deleteAfter"]["duration"],
            "P30D"
        );
    }

    #[test]
    fn policy_response_round_trips() {
        // Response shape from the documentation (extra response fields are ignored).
        let body = json!({
            "id": "/subscriptions/s/resourceGroups/rg/providers/Microsoft.DataProtection/backupVaults/v/backupPolicies/p",
            "name": "p",
            "properties": {
                "policyRules": [
                    {
                        "backupParameters": {"backupType": "Incremental", "objectType": "AzureBackupParams"},
                        "trigger": {
                            "schedule": {"repeatingTimeIntervals": ["R/2021-07-01T19:00:00+00:00/P1D"]},
                            "taggingCriteria": [{"tagInfo": {"tagName": "Default", "id": "Default_"}, "taggingPriority": 99, "isDefault": true}],
                            "objectType": "ScheduleBasedTriggerContext"
                        },
                        "dataStore": {"dataStoreType": "OperationalStore", "objectType": "DataStoreInfoBase"},
                        "name": "BackupDaily",
                        "objectType": "AzureBackupRule"
                    },
                    {
                        "lifecycles": [{
                            "deleteAfter": {"objectType": "AbsoluteDeleteOption", "duration": "P7D"},
                            "targetDataStoreCopySettings": [],
                            "sourceDataStore": {"dataStoreType": "OperationalStore", "objectType": "DataStoreInfoBase"}
                        }],
                        "isDefault": true,
                        "name": "Default",
                        "objectType": "AzureRetentionRule"
                    }
                ],
                "datasourceTypes": ["Microsoft.Compute/disks"],
                "objectType": "BackupPolicy"
            }
        });
        let parsed: BackupPolicyResource = serde_json::from_value(body).unwrap();
        assert_eq!(
            parsed.properties,
            with_response_tag_id(BackupPolicy::disk_operational(
                "2021-07-01T19:00:00+00:00",
                DiskBackupFrequency::Daily,
                7
            ))
        );
    }

    /// The service echoes the derived tag id in responses.
    fn with_response_tag_id(mut policy: BackupPolicy) -> BackupPolicy {
        for rule in &mut policy.policy_rules {
            if let PolicyRule::Backup { trigger, .. } = rule {
                for criteria in &mut trigger.tagging_criteria {
                    criteria.tag_info.id = Some("Default_".to_string());
                }
            }
        }
        policy
    }

    #[tokio::test]
    async fn creates_vault_with_system_assigned_identity() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(VAULT_PATH)
                    .query_param("api-version", "2026-07-01")
                    .json_body(json!({
                        "location": "eastus",
                        "tags": {"alien-stack": "stack"},
                        "identity": {"type": "SystemAssigned"},
                        "properties": {
                            "storageSettings": [{"datastoreType": "VaultStore", "type": "LocallyRedundant"}]
                        }
                    }));
                then.status(200).json_body(json!({
                    "id": VAULT_PATH,
                    "location": "eastus",
                    "identity": {"type": "SystemAssigned", "principalId": "principal-1", "tenantId": "t"},
                    "properties": {
                        "provisioningState": "Succeeded",
                        "storageSettings": [{"datastoreType": "VaultStore", "type": "LocallyRedundant"}]
                    }
                }));
            })
            .await;

        let vault = BackupVault {
            location: "eastus".to_string(),
            tags: HashMap::from([("alien-stack".to_string(), "stack".to_string())]),
            identity: Some(BackupVaultIdentity {
                type_: "SystemAssigned".to_string(),
                principal_id: None,
            }),
            properties: BackupVaultProperties {
                storage_settings: vec![StorageSetting {
                    datastore_type: "VaultStore".to_string(),
                    type_: "LocallyRedundant".to_string(),
                }],
                provisioning_state: None,
            },
            id: None,
        };
        let result = test_client(&server)
            .create_or_update_backup_vault("rg", "stack-db-backups", &vault)
            .await
            .expect("vault create");
        mock.assert_async().await;
        let OperationResult::Completed(created) = result else {
            panic!("expected a synchronous completion");
        };
        assert_eq!(
            created.identity.and_then(|identity| identity.principal_id),
            Some("principal-1".to_string())
        );
        assert_eq!(created.id.as_deref(), Some(VAULT_PATH));
    }

    #[tokio::test]
    async fn backup_instance_put_uses_documented_body_and_returns_long_running_operation() {
        let server = MockServer::start_async().await;
        let disk_id = "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg/providers/Microsoft.Compute/disks/stack-db-disk-0";
        let policy_id = format!("{VAULT_PATH}/backupPolicies/backups-4h-7d");
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0"))
                    .query_param("api-version", "2026-07-01")
                    .json_body(json!({
                        "properties": {
                            "objectType": "BackupInstance",
                            "dataSourceInfo": {
                                "datasourceType": "Microsoft.Compute/disks",
                                "objectType": "Datasource",
                                "resourceID": disk_id,
                                "resourceLocation": "eastus",
                                "resourceName": "stack-db-disk-0",
                                "resourceType": "Microsoft.Compute/disks",
                                "resourceUri": ""
                            },
                            "policyInfo": {
                                "policyId": policy_id,
                                "policyParameters": {
                                    "dataStoreParametersList": [{
                                        "objectType": "AzureOperationalStoreParameters",
                                        "dataStoreType": "OperationalStore",
                                        "resourceGroupId": "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg"
                                    }]
                                }
                            }
                        }
                    }));
                then.status(201)
                    .header(
                        "Azure-AsyncOperation",
                        "https://management.azure.com/subscriptions/s/providers/Microsoft.DataProtection/locations/eastus/operationStatus/op1",
                    )
                    .json_body(json!({"properties": {"provisioningState": "Provisioning"}}));
            })
            .await;

        let instance = BackupInstance::for_disk(
            disk_id,
            "stack-db-disk-0",
            "eastus",
            &policy_id,
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg",
        );
        let result = test_client(&server)
            .create_or_update_backup_instance("rg", "stack-db-backups", "stack-db-disk-0", &instance)
            .await
            .expect("backup instance put");
        mock.assert_async().await;
        let OperationResult::LongRunning(operation) = result else {
            panic!("expected a long-running operation");
        };
        assert!(operation.url.ends_with("/operationStatus/op1"));
    }

    #[tokio::test]
    async fn validate_for_backup_wraps_instance_and_tracks_operation() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(format!("{VAULT_PATH}/validateForBackup"))
                    .query_param("api-version", "2026-07-01")
                    .json_body_partial(
                        r#"{"backupInstance": {"objectType": "BackupInstance", "dataSourceInfo": {"resourceName": "stack-db-disk-0"}}}"#,
                    );
                then.status(202)
                    .header("Retry-After", "10")
                    .header(
                        "Azure-AsyncOperation",
                        "https://management.azure.com/subscriptions/s/providers/Microsoft.DataProtection/locations/eastus/operationStatus/op2",
                    );
            })
            .await;
        let instance = BackupInstance::for_disk("disk-id", "stack-db-disk-0", "eastus", "policy-id", "rg-id");
        let result = test_client(&server)
            .validate_for_backup("rg", "stack-db-backups", &instance)
            .await
            .expect("validate");
        mock.assert_async().await;
        let OperationResult::LongRunning(operation) = result else {
            panic!("expected a long-running operation");
        };
        assert_eq!(operation.retry_after, Some(std::time::Duration::from_secs(10)));
    }

    #[tokio::test]
    async fn missing_permissions_rejection_is_recognized() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(PUT).path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0"));
                then.status(400).json_body(json!({
                    "error": {
                        "code": "UserErrorMissingRequiredPermissions",
                        "message": "Appropriate permissions to perform the operation is missing."
                    }
                }));
            })
            .await;
        let instance = BackupInstance::for_disk("disk-id", "stack-db-disk-0", "eastus", "policy-id", "rg-id");
        let error = test_client(&server)
            .create_or_update_backup_instance("rg", "stack-db-backups", "stack-db-disk-0", &instance)
            .await
            .expect_err("permissions are missing");
        assert!(is_missing_permissions_error(&error));

        let unrelated: Error = AlienError::new(ErrorData::GenericError {
            message: "Operation failed: UserErrorDiskNotFound".to_string(),
        });
        assert!(!is_missing_permissions_error(&unrelated));
    }

    #[tokio::test]
    async fn missing_backup_instance_maps_to_not_found() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0"));
                then.status(404).json_body(json!({
                    "error": {"code": "ResourceNotFound", "message": "not found"}
                }));
            })
            .await;
        let error = test_client(&server)
            .get_backup_instance("rg", "stack-db-backups", "stack-db-disk-0")
            .await
            .expect_err("instance is missing");
        assert!(matches!(
            error.error,
            Some(ErrorData::RemoteResourceNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn suspend_resume_and_delete_instance_hit_documented_routes() {
        let server = MockServer::start_async().await;
        let suspend = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0/suspendBackups"))
                    .query_param("api-version", "2026-07-01");
                then.status(202).header(
                    "Location",
                    "https://management.azure.com/subscriptions/s/providers/Microsoft.DataProtection/locations/eastus/operationResults/op3",
                );
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0"))
                    .query_param("api-version", "2026-07-01");
                then.status(204);
            })
            .await;
        let resume = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(format!("{VAULT_PATH}/backupInstances/stack-db-disk-0/resumeBackups"))
                    .query_param("api-version", "2026-07-01");
                then.status(200).json_body(json!({}));
            })
            .await;
        let client = test_client(&server);
        let resumed = client
            .resume_backups("rg", "stack-db-backups", "stack-db-disk-0")
            .await
            .expect("resume");
        assert!(matches!(resumed, OperationResult::Completed(())));
        resume.assert_async().await;
        let suspended = client
            .suspend_backups("rg", "stack-db-backups", "stack-db-disk-0")
            .await
            .expect("suspend");
        assert!(matches!(suspended, OperationResult::LongRunning(_)));
        let deleted = client
            .delete_backup_instance("rg", "stack-db-backups", "stack-db-disk-0")
            .await
            .expect("delete");
        assert!(matches!(deleted, OperationResult::Completed(())));
        suspend.assert_async().await;
        delete.assert_async().await;
    }
}
