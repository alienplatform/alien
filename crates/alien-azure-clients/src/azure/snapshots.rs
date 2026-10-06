//! Azure managed disk snapshots (`Microsoft.Compute/snapshots`).
//!
//! Incremental snapshots record only the changes since the previous snapshot of the same disk,
//! are billed for used size, and can be turned back into a full disk with
//! `creationData.createOption = Copy`. Snapshots of Standard HDD, Standard SSD and Premium SSD
//! disks can be used as soon as they are created.
//!
//! https://learn.microsoft.com/en-us/azure/virtual-machines/disks-incremental-snapshots
//! https://learn.microsoft.com/en-us/rest/api/compute/snapshots

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub location: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    pub properties: SnapshotProperties,
    /// ARM resource ID (response only).
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    /// Snapshot name (response only).
    #[serde(default, skip_serializing)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotProperties {
    pub creation_data: SnapshotCreationData,
    /// Request an incremental snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incremental: Option<bool>,
    /// Size of the source disk when the snapshot was taken (response only).
    #[serde(rename = "diskSizeGB", default, skip_serializing)]
    pub disk_size_gb: Option<i64>,
    /// When the snapshot was taken, RFC 3339 (response only).
    #[serde(default, skip_serializing)]
    pub time_created: Option<String>,
    /// "Creating", "Succeeded", "Failed", ... (response only).
    #[serde(default, skip_serializing)]
    pub provisioning_state: Option<String>,
    /// Background copy progress. Only Premium SSD v2 and Ultra Disk snapshots report less than
    /// 100 (response only).
    #[serde(default, skip_serializing)]
    pub completion_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotCreationData {
    /// "Copy" to snapshot an existing disk.
    pub create_option: String,
    /// ARM ID of the source disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_resource_id: Option<String>,
    /// Unique ID of the source disk (response only). Changes when a disk is deleted and
    /// recreated under the same name.
    #[serde(default, skip_serializing)]
    pub source_unique_id: Option<String>,
}

impl Snapshot {
    /// An incremental snapshot of `disk_id`.
    pub fn incremental_copy_of(
        disk_id: &str,
        location: &str,
        tags: HashMap<String, String>,
    ) -> Self {
        Snapshot {
            location: location.to_string(),
            tags,
            properties: SnapshotProperties {
                creation_data: SnapshotCreationData {
                    create_option: "Copy".to_string(),
                    source_resource_id: Some(disk_id.to_string()),
                    source_unique_id: None,
                },
                incremental: Some(true),
                disk_size_gb: None,
                time_created: None,
                provisioning_state: None,
                completion_percent: None,
            },
            id: None,
            name: None,
        }
    }

    /// Whether the snapshot finished provisioning and its data copy is complete, so a disk can
    /// be created from it and its source disk can go away.
    pub fn is_ready(&self) -> bool {
        self.properties.provisioning_state.as_deref() == Some("Succeeded")
            && self
                .properties
                .completion_percent
                .is_none_or(|percent| percent >= 100.0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotList {
    #[serde(default)]
    value: Vec<Snapshot>,
    #[serde(default)]
    next_link: Option<String>,
}

pub type SnapshotOperationResult = OperationResult<Snapshot>;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait SnapshotsApi: Send + Sync + std::fmt::Debug {
    /// Creates or updates a snapshot. Long-running.
    async fn create_or_update_snapshot(
        &self,
        resource_group: &str,
        snapshot_name: &str,
        snapshot: &Snapshot,
    ) -> Result<SnapshotOperationResult>;

    async fn get_snapshot(&self, resource_group: &str, snapshot_name: &str) -> Result<Snapshot>;

    /// Lists every snapshot in a resource group, following `nextLink` pages.
    async fn list_snapshots(&self, resource_group: &str) -> Result<Vec<Snapshot>>;

    /// Deletes a snapshot. Long-running.
    async fn delete_snapshot(
        &self,
        resource_group: &str,
        snapshot_name: &str,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzureSnapshotsClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureSnapshotsClient {
    /// Same Compute disk resource provider version as the managed disks client.
    const API_VERSION: &'static str = "2024-03-02";

    /// Follows a `nextLink` only on the management endpoint, so the bearer token is never sent
    /// to another host.
    fn same_origin_next_link(&self, link: String, resource_group: &str) -> Result<String> {
        let endpoint = url::Url::parse(&self.base.build_url("", None))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: "Azure management endpoint is not a valid URL".to_string(),
                errors: None,
            })?;
        let next = url::Url::parse(&link)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                message: format!(
                    "Azure ListSnapshots for {resource_group} returned an invalid nextLink"
                ),
                field_name: Some("nextLink".to_string()),
            })?;
        if next.origin() != endpoint.origin() {
            return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                message: format!(
                    "Azure ListSnapshots for {resource_group} returned a nextLink on another host: {}",
                    next.origin().ascii_serialization()
                ),
                field_name: Some("nextLink".to_string()),
            }));
        }
        Ok(link)
    }

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

    fn snapshots_path(&self, resource_group: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/snapshots",
            self.token_cache.config().subscription_id,
            resource_group
        )
    }

    fn snapshot_url(&self, resource_group: &str, snapshot_name: &str) -> String {
        self.base.build_url(
            &format!("{}/{}", self.snapshots_path(resource_group), snapshot_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        )
    }

    async fn read_body(resp: reqwest::Response, op: &str, name: &str) -> Result<String> {
        resp.text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure {op}: failed to read body for {name}"),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl SnapshotsApi for AzureSnapshotsClient {
    async fn create_or_update_snapshot(
        &self,
        resource_group: &str,
        snapshot_name: &str,
        snapshot: &Snapshot,
    ) -> Result<SnapshotOperationResult> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let body = serde_json::to_string(snapshot).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize snapshot {snapshot_name}"),
            },
        )?;
        let req = AzureRequestBuilder::new(
            Method::PUT,
            self.snapshot_url(resource_group, snapshot_name),
        )
        .content_type_json()
        .content_length(&body)
        .body(body)
        .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateSnapshot",
                snapshot_name,
            )
            .await
    }

    async fn get_snapshot(&self, resource_group: &str, snapshot_name: &str) -> Result<Snapshot> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let req = AzureRequestBuilder::new(
            Method::GET,
            self.snapshot_url(resource_group, snapshot_name),
        )
        .content_length("")
        .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetSnapshot", snapshot_name)
            .await?;
        let body = Self::read_body(resp, "GetSnapshot", snapshot_name).await?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure GetSnapshot: JSON parse error for {snapshot_name}"),
            })
    }

    async fn list_snapshots(&self, resource_group: &str) -> Result<Vec<Snapshot>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let mut snapshots = Vec::new();
        let mut next_url = Some(self.base.build_url(
            &self.snapshots_path(resource_group),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        ));
        while let Some(url) = next_url.take() {
            let req = AzureRequestBuilder::new(Method::GET, url)
                .content_length("")
                .build()?;
            let signed = self.base.sign_request(req, &token).await?;
            let resp = self
                .base
                .execute_request(signed, "ListSnapshots", resource_group)
                .await?;
            let body = Self::read_body(resp, "ListSnapshots", resource_group).await?;
            let page: SnapshotList = serde_json::from_str(&body).into_alien_error().context(
                ErrorData::SerializationError {
                    message: format!("Azure ListSnapshots: JSON parse error for {resource_group}"),
                },
            )?;
            snapshots.extend(page.value);
            next_url = page
                .next_link
                .map(|link| self.same_origin_next_link(link, resource_group))
                .transpose()?;
        }
        Ok(snapshots)
    }

    async fn delete_snapshot(
        &self,
        resource_group: &str,
        snapshot_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let req = AzureRequestBuilder::new(
            Method::DELETE,
            self.snapshot_url(resource_group, snapshot_name),
        )
        .content_length("")
        .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "DeleteSnapshot", snapshot_name)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use httpmock::{Method::GET, Method::PUT, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    const SNAPSHOTS_PATH: &str = "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg/providers/Microsoft.Compute/snapshots";

    fn test_client(server: &MockServer) -> AzureSnapshotsClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureSnapshotsClient::new(Client::new(), AzureTokenCache::new(config))
    }

    #[tokio::test]
    async fn creates_incremental_copy_with_tags() {
        let server = MockServer::start_async().await;
        let disk_id = "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/rg/providers/Microsoft.Compute/disks/stack-db-disk-0";
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{SNAPSHOTS_PATH}/stack-db-disk-0-final"))
                    .query_param("api-version", "2024-03-02")
                    .json_body(json!({
                        "location": "eastus",
                        "tags": {"alien-snapshot-kind": "final"},
                        "properties": {
                            "creationData": {"createOption": "Copy", "sourceResourceId": disk_id},
                            "incremental": true
                        }
                    }));
                then.status(202).header(
                    "Azure-AsyncOperation",
                    "https://management.azure.com/subscriptions/s/providers/Microsoft.Compute/locations/eastus/operations/op1",
                );
            })
            .await;
        let snapshot = Snapshot::incremental_copy_of(
            disk_id,
            "eastus",
            HashMap::from([("alien-snapshot-kind".to_string(), "final".to_string())]),
        );
        let result = test_client(&server)
            .create_or_update_snapshot("rg", "stack-db-disk-0-final", &snapshot)
            .await
            .expect("snapshot create");
        mock.assert_async().await;
        assert!(matches!(result, OperationResult::LongRunning(_)));
    }

    #[tokio::test]
    async fn list_follows_next_link_pages() {
        let server = MockServer::start_async().await;
        let next_link = format!(
            "{}{SNAPSHOTS_PATH}?api-version=2024-03-02&$skiptoken=page2",
            server.base_url()
        );
        // Registered first: httpmock serves the first matching mock, and only the second page
        // request carries the skip token.
        let second = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(SNAPSHOTS_PATH)
                    .query_param("$skiptoken", "page2");
                then.status(200).json_body(json!({
                    "value": [{
                        "name": "b",
                        "location": "eastus",
                        "properties": {
                            "creationData": {"createOption": "Copy", "sourceResourceId": "disk-b"},
                            "provisioningState": "Creating"
                        }
                    }]
                }));
            })
            .await;

        let first = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(SNAPSHOTS_PATH)
                    .query_param("api-version", "2024-03-02");
                then.status(200).json_body(json!({
                    "value": [{
                        "name": "a",
                        "location": "eastus",
                        "properties": {
                            "creationData": {"createOption": "Copy", "sourceResourceId": "disk-a"},
                            "provisioningState": "Succeeded",
                            "timeCreated": "2026-10-01T00:00:00Z",
                            "diskSizeGB": 10
                        }
                    }],
                    "nextLink": next_link
                }));
            })
            .await;
        let snapshots = test_client(&server)
            .list_snapshots("rg")
            .await
            .expect("list");
        first.assert_async().await;
        second.assert_async().await;
        let names: Vec<_> = snapshots.iter().filter_map(|s| s.name.as_deref()).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(snapshots[0].is_ready());
        assert_eq!(snapshots[0].properties.disk_size_gb, Some(10));
        assert!(!snapshots[1].is_ready());
    }

    #[tokio::test]
    async fn list_refuses_a_next_link_on_another_host() {
        let server = MockServer::start_async().await;
        let other = MockServer::start_async().await;
        let leaked = other
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).json_body(json!({ "value": [] }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(SNAPSHOTS_PATH);
                then.status(200).json_body(json!({
                    "value": [],
                    "nextLink": format!("{}{SNAPSHOTS_PATH}?$skiptoken=page2", other.base_url())
                }));
            })
            .await;

        let error = test_client(&server)
            .list_snapshots("rg")
            .await
            .expect_err("a nextLink on another host must not be followed");
        assert!(error.to_string().contains("another host"), "{error}");
        leaked.assert_hits_async(0).await;
    }

    #[test]
    fn snapshot_is_not_ready_until_background_copy_completes() {
        let mut snapshot = Snapshot::incremental_copy_of("disk", "eastus", HashMap::new());
        snapshot.properties.provisioning_state = Some("Succeeded".to_string());
        snapshot.properties.completion_percent = Some(42.0);
        assert!(!snapshot.is_ready());
        snapshot.properties.completion_percent = Some(100.0);
        assert!(snapshot.is_ready());
    }
}
