use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Firestore service configuration
#[derive(Debug)]
pub struct FirestoreServiceConfig;

impl GcpServiceConfig for FirestoreServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://firestore.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://firestore.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Firestore"
    }

    fn service_key(&self) -> &'static str {
        "firestore"
    }
}

// --- Database Management Data Structures ---

/// The type of the database.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseType {
    /// The default value. This value is used if the database type is omitted.
    DatabaseTypeUnspecified,
    /// Firestore Native Mode
    FirestoreNative,
    /// Firestore in Datastore Mode
    DatastoreMode,
}

/// The type of concurrency control mode for transactions.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConcurrencyMode {
    /// The default value. This value is used if the concurrency mode is omitted.
    ConcurrencyModeUnspecified,
    /// Use optimistic concurrency control by default.
    Optimistic,
    /// Use pessimistic concurrency control by default.
    Pessimistic,
    /// Use optimistic concurrency control with entity groups by default.
    OptimisticWithEntityGroups,
}

/// Point In Time Recovery feature enablement.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PointInTimeRecoveryEnablement {
    /// Not used.
    PointInTimeRecoveryEnablementUnspecified,
    /// Reads are supported on selected versions of the data from within the past 7 days.
    PointInTimeRecoveryEnabled,
    /// Reads are supported on any version of the data from within the past 1 hour.
    PointInTimeRecoveryDisabled,
}

/// The type of App Engine integration mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppEngineIntegrationMode {
    /// The default value. This value is used if the App Engine integration mode is omitted.
    AppEngineIntegrationModeUnspecified,
    /// If an App Engine application exists in the same region as this database, it will be enabled.
    Enabled,
    /// App Engine integration will never be enabled for this database.
    Disabled,
}

/// The delete protection state of the database.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeleteProtectionState {
    /// The default value. This value is used if the delete protection state is omitted.
    DeleteProtectionStateUnspecified,
    /// Delete protection is disabled.
    DeleteProtectionDisabled,
    /// Delete protection is enabled.
    DeleteProtectionEnabled,
}

/// The edition of the database.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseEdition {
    /// Not used.
    DatabaseEditionUnspecified,
    /// Standard edition.
    Standard,
    /// Enterprise edition.
    Enterprise,
}

/// The CMEK (Customer Managed Encryption Key) configuration for a Firestore database.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CmekConfig {
    /// Required. Only keys in the same location as this database are allowed to be used for encryption.
    pub kms_key_name: String,

    /// Output only. Currently in-use KMS key versions.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_key_version: Vec<String>,
}

/// The source from which this database is derived.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum SourceType {
    /// If set, this database was restored from the specified backup.
    Backup(BackupSource),
}

/// Information about a backup that was used to restore a database.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackupSource {
    /// The resource name of the backup that was used to restore this database.
    pub backup: String,
}

/// Information about the provenance of this database.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// The associated long-running operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,

    /// The source from which this database is derived.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceType>,
}

/// A Cloud Firestore Database.
/// Based on: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases#Database
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    /// The resource name of the Database.
    /// Format: `projects/{project}/databases/{database}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Output only. The system-generated UUID4 for this Database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Output only. The timestamp at which this database was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The timestamp at which this database was most recently updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// Output only. The timestamp at which this database was deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_time: Option<String>,

    /// The location of the database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,

    /// The type of the database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<DatabaseType>,

    /// The concurrency control mode to use for this database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_mode: Option<ConcurrencyMode>,

    /// Output only. The period during which past versions of data are retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_retention_period: Option<String>,

    /// Output only. The earliest timestamp at which older versions of the data can be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest_version_time: Option<String>,

    /// Whether to enable the PITR feature on this database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_in_time_recovery_enablement: Option<PointInTimeRecoveryEnablement>,

    /// The App Engine integration mode to use for this database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_engine_integration_mode: Option<AppEngineIntegrationMode>,

    /// State of delete protection for the database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_protection_state: Option<DeleteProtectionState>,

    /// Optional. Presence indicates CMEK is enabled for this database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmek_config: Option<CmekConfig>,

    /// The database edition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_edition: Option<DatabaseEdition>,

    /// Output only. Information about the provenance of this database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_info: Option<SourceInfo>,

    /// Output only. The system-generated etag for this Database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// The response for FirestoreAdmin.ListDatabases.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesResponse {
    /// The databases in the project.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<Database>,

    /// In the event that data is unavailable, the reason for this condition.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreachable: Vec<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait FirestoreApi: Send + Sync + Debug {
    /// Gets a single document.
    async fn get_document(
        &self,
        database_id: String,
        document_path: String,
        mask: Option<DocumentMask>,
        transaction: Option<String>,
        read_time: Option<String>,
    ) -> Result<Document>;

    /// Creates a new document.
    async fn create_document(
        &self,
        database_id: String,
        collection_id: String,
        document_id: Option<String>,
        document: Document,
        mask: Option<DocumentMask>,
    ) -> Result<Document>;

    /// Updates or inserts a document.
    async fn patch_document(
        &self,
        database_id: String,
        document_path: String,
        document: Document,
        update_mask: Option<DocumentMask>,
        mask: Option<DocumentMask>,
        current_document: Option<Precondition>,
    ) -> Result<Document>;

    /// Deletes a document.
    async fn delete_document(
        &self,
        database_id: String,
        document_path: String,
        current_document: Option<Precondition>,
    ) -> Result<()>;

    /// Gets multiple documents.
    /// Note: This returns a vector of responses as the API streams multiple response objects.
    async fn batch_get_documents(
        &self,
        database_id: String,
        request: BatchGetDocumentsRequest,
    ) -> Result<Vec<BatchGetDocumentsResponse>>;

    /// Commits a transaction, optionally updating documents.
    async fn commit(&self, database_id: String, request: CommitRequest) -> Result<CommitResponse>;

    /// Runs a query.
    /// Note: This returns a vector of responses as the API streams multiple response objects.
    async fn run_query(
        &self,
        database_id: String,
        request: RunQueryRequest,
    ) -> Result<Vec<RunQueryResponse>>;

    // Database management operations

    /// Creates a new database.
    async fn create_database(
        &self,
        database_id: String,
        database: Database,
    ) -> Result<crate::gcp::longrunning::Operation>;

    /// Gets information about a database.
    async fn get_database(&self, database_id: String) -> Result<Database>;

    /// Lists all databases in the project.
    async fn list_databases(&self) -> Result<ListDatabasesResponse>;

    /// Updates a database.
    async fn patch_database(
        &self,
        database_id: String,
        database: Database,
        update_mask: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation>;

    /// Deletes a database.
    async fn delete_database(
        &self,
        database_id: String,
        etag: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation>;

    /// Gets information about a long-running operation.
    async fn get_operation(
        &self,
        operation_name: String,
    ) -> Result<crate::gcp::longrunning::Operation>;

    // Index management operations

    /// Creates a composite index.
    async fn create_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index: Index,
    ) -> Result<crate::gcp::longrunning::Operation>;

    /// Gets an index.
    async fn get_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index_id: String,
    ) -> Result<Index>;

    /// Lists indexes.
    async fn list_indexes(
        &self,
        database_id: String,
        collection_group_id: String,
        filter: Option<String>,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListIndexesResponse>;

    /// Deletes an index.
    async fn delete_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index_id: String,
    ) -> Result<crate::gcp::longrunning::Operation>;

    // Field configuration operations

    /// Gets field configuration information.
    async fn get_field(
        &self,
        database_id: String,
        collection_group_id: String,
        field_id: String,
    ) -> Result<Field>;

    /// Lists field configurations.
    /// Note: filter is required. Use "indexConfig.usesAncestorConfig:false" or "ttlConfig:*"
    async fn list_fields(
        &self,
        database_id: String,
        collection_group_id: String,
        filter: String,
    ) -> Result<ListFieldsResponse>;

    /// Updates a field configuration.
    async fn patch_field(
        &self,
        database_id: String,
        collection_group_id: String,
        field_id: String,
        field: Field,
        update_mask: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation>;
}

/// Firestore client for managing documents and transactions
#[derive(Debug)]
pub struct FirestoreClient {
    base: GcpClientBase,
    project_id: String,
}

impl FirestoreClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(FirestoreServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl FirestoreApi for FirestoreClient {
    /// Gets a single document.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/get
    async fn get_document(
        &self,
        database_id: String,
        document_path: String,
        mask: Option<DocumentMask>,
        transaction: Option<String>,
        read_time: Option<String>,
    ) -> Result<Document> {
        let path = format!(
            "projects/{}/databases/{}/documents/{}",
            self.project_id, database_id, document_path
        );

        let mut query_params = Vec::new();
        if let Some(m) = &mask {
            for field_path in &m.field_paths {
                query_params.push(("mask.fieldPaths", field_path.clone()));
            }
        }
        if let Some(tx) = transaction {
            query_params.push(("transaction", tx));
        }
        if let Some(rt) = read_time {
            query_params.push(("readTime", rt));
        }

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &document_path,
            )
            .await
    }

    /// Creates a new document.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/createDocument
    async fn create_document(
        &self,
        database_id: String,
        collection_id: String,
        document_id: Option<String>,
        document: Document,
        mask: Option<DocumentMask>,
    ) -> Result<Document> {
        let path = format!(
            "projects/{}/databases/{}/documents/{}",
            self.project_id, database_id, collection_id
        );

        let mut query_params = Vec::new();
        if let Some(ref doc_id) = document_id {
            query_params.push(("documentId", doc_id.clone()));
        }
        if let Some(m) = &mask {
            for field_path in &m.field_paths {
                query_params.push(("mask.fieldPaths", field_path.clone()));
            }
        }

        let resource_name = document_id.unwrap_or_else(|| collection_id.clone());

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(document),
                &resource_name,
            )
            .await
    }

    /// Updates or inserts a document.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/patch
    async fn patch_document(
        &self,
        database_id: String,
        document_path: String,
        document: Document,
        update_mask: Option<DocumentMask>,
        mask: Option<DocumentMask>,
        current_document: Option<Precondition>,
    ) -> Result<Document> {
        let path = format!(
            "projects/{}/databases/{}/documents/{}",
            self.project_id, database_id, document_path
        );

        let mut query_params = Vec::new();
        if let Some(um) = &update_mask {
            for field_path in &um.field_paths {
                query_params.push(("updateMask.fieldPaths", field_path.clone()));
            }
        }
        if let Some(m) = &mask {
            for field_path in &m.field_paths {
                query_params.push(("mask.fieldPaths", field_path.clone()));
            }
        }
        if let Some(cd) = &current_document {
            match &cd.condition {
                PreconditionType::Exists(exists) => {
                    query_params.push(("currentDocument.exists", exists.to_string()));
                }
                PreconditionType::UpdateTime(update_time) => {
                    query_params.push(("currentDocument.updateTime", update_time.clone()));
                }
            }
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(document),
                &document_path,
            )
            .await
    }

    /// Deletes a document.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/delete
    async fn delete_document(
        &self,
        database_id: String,
        document_path: String,
        current_document: Option<Precondition>,
    ) -> Result<()> {
        let path = format!(
            "projects/{}/databases/{}/documents/{}",
            self.project_id, database_id, document_path
        );

        let mut query_params = Vec::new();
        if let Some(cd) = &current_document {
            match &cd.condition {
                PreconditionType::Exists(exists) => {
                    query_params.push(("currentDocument.exists", exists.to_string()));
                }
                PreconditionType::UpdateTime(update_time) => {
                    query_params.push(("currentDocument.updateTime", update_time.clone()));
                }
            }
        }

        self.base
            .execute_request_no_response(
                Method::DELETE,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &document_path,
            )
            .await
    }

    /// Gets multiple documents.
    /// Note: The Firestore API streams responses, but over HTTP REST this becomes an array of response objects.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/batchGet
    async fn batch_get_documents(
        &self,
        database_id: String,
        request: BatchGetDocumentsRequest,
    ) -> Result<Vec<BatchGetDocumentsResponse>> {
        let path = format!(
            "projects/{}/databases/{}/documents:batchGet",
            self.project_id, database_id
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &database_id)
            .await
    }

    /// Commits a transaction, optionally updating documents.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/commit
    async fn commit(&self, database_id: String, request: CommitRequest) -> Result<CommitResponse> {
        let path = format!(
            "projects/{}/databases/{}/documents:commit",
            self.project_id, database_id
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &database_id)
            .await
    }

    /// Runs a query.
    /// Note: The Firestore API streams responses, but over HTTP REST this becomes an array of response objects.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents/runQuery
    async fn run_query(
        &self,
        database_id: String,
        request: RunQueryRequest,
    ) -> Result<Vec<RunQueryResponse>> {
        let path = format!(
            "projects/{}/databases/{}/documents:runQuery",
            self.project_id, database_id
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &database_id)
            .await
    }

    // Database management operations

    /// Creates a new database.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases/create
    async fn create_database(
        &self,
        database_id: String,
        database: Database,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!("projects/{}/databases", self.project_id);

        let query_params = vec![("databaseId", database_id.clone())];

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(database),
                &database_id,
            )
            .await
    }

    /// Gets information about a database.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases/get
    async fn get_database(&self, database_id: String) -> Result<Database> {
        let path = format!("projects/{}/databases/{}", self.project_id, database_id);

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &database_id)
            .await
    }

    /// Lists all databases in the project.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases/list
    async fn list_databases(&self) -> Result<ListDatabasesResponse> {
        let path = format!("projects/{}/databases", self.project_id);

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &self.project_id,
            )
            .await
    }

    /// Updates a database.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases/patch
    async fn patch_database(
        &self,
        database_id: String,
        database: Database,
        update_mask: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!("projects/{}/databases/{}", self.project_id, database_id);

        let mut query_params = Vec::new();
        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(database),
                &database_id,
            )
            .await
    }

    /// Deletes a database.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases/delete
    async fn delete_database(
        &self,
        database_id: String,
        etag: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!("projects/{}/databases/{}", self.project_id, database_id);

        let mut query_params = Vec::new();
        if let Some(etag_val) = etag {
            query_params.push(("etag", etag_val));
        }

        self.base
            .execute_request(
                Method::DELETE,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &database_id,
            )
            .await
    }

    /// Gets information about a long-running operation.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/operations/get
    async fn get_operation(
        &self,
        operation_name: String,
    ) -> Result<crate::gcp::longrunning::Operation> {
        // Operation names from Firestore operations API should be used as-is
        // They typically come in format: "projects/{project}/databases/{database}/operations/{operation}"
        let path = if operation_name.starts_with("projects/") {
            operation_name.clone()
        } else {
            // For backward compatibility, assume it's a full operation name
            operation_name.clone()
        };

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    // Index management operations

    /// Creates a composite index.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.indexes/create
    async fn create_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index: Index,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/indexes",
            self.project_id, database_id, collection_group_id
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(index), &collection_group_id)
            .await
    }

    /// Gets an index.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.indexes/get
    async fn get_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index_id: String,
    ) -> Result<Index> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/indexes/{}",
            self.project_id, database_id, collection_group_id, index_id
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &index_id)
            .await
    }

    /// Lists indexes.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.indexes/list
    async fn list_indexes(
        &self,
        database_id: String,
        collection_group_id: String,
        filter: Option<String>,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListIndexesResponse> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/indexes",
            self.project_id, database_id, collection_group_id
        );

        let mut query_params = Vec::new();
        if let Some(f) = filter {
            query_params.push(("filter", f));
        }
        if let Some(ps) = page_size {
            query_params.push(("pageSize", ps.to_string()));
        }
        if let Some(pt) = page_token {
            query_params.push(("pageToken", pt));
        }

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &collection_group_id,
            )
            .await
    }

    /// Deletes an index.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.indexes/delete
    async fn delete_index(
        &self,
        database_id: String,
        collection_group_id: String,
        index_id: String,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/indexes/{}",
            self.project_id, database_id, collection_group_id, index_id
        );

        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &index_id)
            .await
    }

    // Field configuration operations

    /// Gets field configuration information.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.fields/get
    async fn get_field(
        &self,
        database_id: String,
        collection_group_id: String,
        field_id: String,
    ) -> Result<Field> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/fields/{}",
            self.project_id, database_id, collection_group_id, field_id
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &field_id)
            .await
    }

    /// Lists field configurations.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.fields/list
    /// Note: filter is required. Use "indexConfig.usesAncestorConfig:false" or "ttlConfig:*"
    async fn list_fields(
        &self,
        database_id: String,
        collection_group_id: String,
        filter: String,
    ) -> Result<ListFieldsResponse> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/fields",
            self.project_id, database_id, collection_group_id
        );

        let query_params = vec![("filter", filter)];

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params),
                Option::<()>::None,
                &collection_group_id,
            )
            .await
    }

    /// Updates a field configuration.
    /// See: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.fields/patch
    async fn patch_field(
        &self,
        database_id: String,
        collection_group_id: String,
        field_id: String,
        field: Field,
        update_mask: Option<String>,
    ) -> Result<crate::gcp::longrunning::Operation> {
        let path = format!(
            "projects/{}/databases/{}/collectionGroups/{}/fields/{}",
            self.project_id, database_id, collection_group_id, field_id
        );

        let mut query_params = Vec::new();
        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(field),
                &field_id,
            )
            .await
    }
}

// --- Data Structures ---

/// A Firestore document.
/// Based on: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.documents#Document
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// The resource name of the document, for example
    /// `projects/{project_id}/databases/{database_id}/documents/{document_path}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The document's fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, Value>>,

    /// Output only. The time at which the document was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The time at which the document was last changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,
}

/// A message that can hold any of the supported value types.
/// Based on: https://cloud.google.com/firestore/docs/reference/rest/v1/Value
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    /// A null value.
    NullValue(NullValue),

    /// A boolean value.
    BooleanValue(bool),

    /// An integer value.
    IntegerValue(String),

    /// A double value.
    DoubleValue(f64),

    /// A timestamp value.
    TimestampValue(String),

    /// A string value.
    StringValue(String),

    /// A bytes value.
    BytesValue(String),

    /// A reference to a document.
    ReferenceValue(String),

    /// A geo point value representing a point on the surface of Earth.
    GeoPointValue(LatLng),

    /// An array value.
    ArrayValue(ArrayValue),

    /// A map value.
    MapValue(MapValue),
}

impl Default for Value {
    fn default() -> Self {
        Value::NullValue(NullValue)
    }
}

/// Represents a NULL value.
/// According to Firestore REST API, null values should serialize as JSON null.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct NullValue;

impl Default for NullValue {
    fn default() -> Self {
        NullValue
    }
}

/// An array value.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ArrayValue {
    /// Values in the array.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Value>,
}

/// A map value.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MapValue {
    /// The map's fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, Value>>,
}

/// An object that represents a latitude/longitude pair.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LatLng {
    /// The latitude in degrees. It must be in the range [-90.0, +90.0].
    pub latitude: f64,

    /// The longitude in degrees. It must be in the range [-180.0, +180.0].
    pub longitude: f64,
}

/// A set of field paths on a document.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMask {
    /// The list of field paths in the mask.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_paths: Vec<String>,
}

/// A precondition on a document, used for conditional operations.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Precondition {
    /// The type of precondition.
    #[serde(flatten)]
    pub condition: PreconditionType,
}

/// The type of precondition on a document.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum PreconditionType {
    /// When set to `true`, the target document must exist.
    /// When set to `false`, the target document must not exist.
    Exists(bool),

    /// When set, the target document must exist and have been last updated at that time.
    UpdateTime(String),
}

/// The request for Firestore.BatchGetDocuments.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetDocumentsRequest {
    /// Required. The names of the documents to retrieve.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,

    /// The fields to return. If not set, returns all fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<DocumentMask>,

    /// The consistency mode for this transaction.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub consistency_selector: Option<ConsistencySelector>,
}

/// The response for Firestore.BatchGetDocuments.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetDocumentsResponse {
    /// A document that was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<Document>,

    /// A document name that was requested but does not exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,

    /// The transaction that was started as part of this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,

    /// The time at which the document was read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_time: Option<String>,
}

/// Options for creating a new transaction.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TransactionOptions {
    /// The maximum amount of time the transaction can run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempt_time: Option<String>,
}

/// Explain options for queries.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ExplainOptions {
    /// Whether to analyze the query for performance statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyze: Option<bool>,
}

/// Query performance metrics.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ExplainMetrics {
    /// Planning phase information for the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_summary: Option<PlanSummary>,

    /// Execution statistics for the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_stats: Option<ExecutionStats>,
}

/// Summary of query planning.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    /// The indexes used by the query.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes_used: Vec<HashMap<String, serde_json::Value>>,
}

/// Execution statistics for the query.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStats {
    /// Total number of results returned by the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results_returned: Option<i64>,

    /// Total time spent executing the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_duration: Option<String>,

    /// Total billable read operations consumed by the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_operations: Option<i64>,
}

/// The consistency mode for reads.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ConsistencySelector {
    /// Reads documents in a transaction.
    Transaction(String),

    /// Starts a new transaction and reads documents in it.
    NewTransaction(TransactionOptions),

    /// Reads documents at the given time.
    ReadTime(String),
}

/// The request for Firestore.Commit.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CommitRequest {
    /// If set, applies all writes in this transaction, and commits it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,

    /// The writes to apply.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<Write>,
}

/// The response for Firestore.Commit.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CommitResponse {
    /// The write results. This may be empty if only TransformWrites were performed.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_results: Vec<WriteResult>,

    /// The time at which the commit occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_time: Option<String>,
}

/// A write on a document.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Write {
    /// The operation to execute.
    #[serde(flatten)]
    pub operation: WriteOperation,

    /// The fields to update in this write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mask: Option<DocumentMask>,

    /// An optional precondition on the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_document: Option<Precondition>,
}

/// The operation to execute in a write.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum WriteOperation {
    /// A document to write.
    Update(Document),

    /// A document name to delete.
    Delete(String),

    /// Applies a transformation to a document.
    Transform(DocumentTransform),
}

/// A transformation of a document.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTransform {
    /// The name of the document to transform.
    pub document: String,

    /// The list of transformations to apply to the fields of the document.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_transforms: Vec<FieldTransform>,
}

/// A transformation of a field of the document.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FieldTransform {
    /// The path of the field.
    pub field_path: String,

    /// The transformation to apply on the field.
    #[serde(flatten)]
    pub transform_type: TransformType,
}

/// The type of transformation to apply to a field.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum TransformType {
    /// Sets the field to the given server value.
    SetToServerValue(ServerValue),

    /// Adds the given value to the field's current value.
    Increment(Value),

    /// Sets the field to the maximum of its current value and the given value.
    Maximum(Value),

    /// Sets the field to the minimum of its current value and the given value.
    Minimum(Value),

    /// Append the given elements in order if they are not already present.
    AppendMissingElements(ArrayValue),

    /// Remove all of the given elements from the array.
    RemoveAllFromArray(ArrayValue),
}

/// A server value that can be set to a field.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerValue {
    /// Unspecified. This value must not be used.
    ServerValueUnspecified,

    /// The time at which the server processed the request.
    RequestTime,
}

/// The result of applying a write.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// The last update time of the document after applying the write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// The results of applying each DocumentTransform.FieldTransform.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transform_results: Vec<Value>,
}

/// The request for Firestore.RunQuery.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RunQueryRequest {
    /// The parent resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,

    /// The query to run.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub query_type: Option<QueryType>,

    /// The consistency mode for the query.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub consistency_selector: Option<ConsistencySelector>,

    /// Optional. Explain options for the query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_options: Option<ExplainOptions>,
}

/// The type of query to run.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum QueryType {
    /// A structured query.
    StructuredQuery(StructuredQuery),
}

/// A Firestore query.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct StructuredQuery {
    /// The projection to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub select: Option<Projection>,

    /// The collections to query.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<CollectionSelector>,

    /// The filter to apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#where: Option<Filter>,

    /// The order to apply to the query results.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<Order>,

    /// A starting point for the query results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_at: Option<Cursor>,

    /// A end point for the query results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_at: Option<Cursor>,

    /// The number of results to skip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,

    /// The maximum number of results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

/// The projection of document's fields to return.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    /// The fields to return.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldReference>,
}

/// A reference to a field.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FieldReference {
    /// The field path.
    pub field_path: String,
}

/// A selection of a collection.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSelector {
    /// The collection ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<String>,

    /// When false, selects only collections that are immediate children of the parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_descendants: Option<bool>,
}

/// A filter on the documents returned by a query.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    /// A composite filter.
    CompositeFilter(CompositeFilter),

    /// A filter on a document field.
    FieldFilter(FieldFilter),

    /// A filter that takes exactly one argument.
    UnaryFilter(UnaryFilter),
}

/// A filter that merges multiple other filters.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CompositeFilter {
    /// The operator for combining multiple filters.
    pub op: CompositeFilterOperator,

    /// The list of filters to combine.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

/// A composite filter operator.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompositeFilterOperator {
    #[default]
    /// Unspecified. This value must not be used.
    OperatorUnspecified,

    /// The results are required to satisfy each of the combined filters.
    And,

    /// Documents are required to satisfy at least one of the combined filters.
    Or,
}

/// A filter on a specific field.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FieldFilter {
    /// The field to filter by.
    pub field: FieldReference,

    /// The operator to filter by.
    pub op: FieldFilterOperator,

    /// The value to compare to.
    pub value: Value,
}

/// A field filter operator.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FieldFilterOperator {
    #[default]
    /// Unspecified. This value must not be used.
    OperatorUnspecified,

    /// The given field is less than the given value.
    LessThan,

    /// The given field is less than or equal to the given value.
    LessThanOrEqual,

    /// The given field is greater than the given value.
    GreaterThan,

    /// The given field is greater than or equal to the given value.
    GreaterThanOrEqual,

    /// The given field is equal to the given value.
    Equal,

    /// The given field is not equal to the given value.
    NotEqual,

    /// The given field is an array that contains the given value.
    ArrayContains,

    /// The given field is equal to at least one value in the given array.
    In,

    /// The given field is an array that contains any of the given values.
    ArrayContainsAny,

    /// The value of the field is not in the given array.
    NotIn,
}

/// A filter with a single operand.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UnaryFilter {
    /// The unary operator to apply.
    pub op: UnaryFilterOperator,

    /// The field to which to apply the operator.
    pub field: FieldReference,
}

/// A unary filter operator.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnaryFilterOperator {
    #[default]
    /// Unspecified. This value must not be used.
    OperatorUnspecified,

    /// The given field is equal to `NaN`.
    IsNan,

    /// The given field is equal to `NULL`.
    IsNull,

    /// The given field is not equal to `NaN`.
    IsNotNan,

    /// The given field is not equal to `NULL`.
    IsNotNull,
}

/// An order on a field.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    /// The field to order by.
    pub field: FieldReference,

    /// The direction to order by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
}

/// A sort direction.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Direction {
    /// Unspecified. This value must not be used.
    DirectionUnspecified,

    /// Ascending.
    Ascending,

    /// Descending.
    Descending,
}

/// A position in a query result set.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    /// The values that represent a position.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Value>,

    /// Whether to start just before or just after the given values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<bool>,
}

/// The response for Firestore.RunQuery.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RunQueryResponse {
    /// A query result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,

    /// The transaction that was started as part of this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,

    /// The time at which the document was read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_time: Option<String>,

    /// The number of results that have been skipped due to an offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_results: Option<i32>,

    /// Query explain metrics (only present when explain options were provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_metrics: Option<ExplainMetrics>,

    /// If present, Firestore has completely finished the request and no more documents will be returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
}

// --- Index Management Data Structures ---

/// Cloud Firestore index configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    /// Output only. A server defined name for this index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The collection ID to which this index applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<String>,

    /// The fields supported by this index.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<IndexField>,

    /// Output only. The serving state of the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<IndexState>,
}

/// A field in an index.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IndexField {
    /// The field path to index.
    pub field_path: String,

    /// The field's mode in the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<IndexFieldMode>,
}

/// The mode of a field in an index.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IndexFieldMode {
    /// The mode is unspecified.
    ModeUnspecified,
    /// The field's values are indexed so as to support sequencing in ascending order.
    Ascending,
    /// The field's values are indexed so as to support sequencing in descending order.
    Descending,
    /// The field's array values are indexed so as to support membership using ARRAY_CONTAINS queries.
    ArrayContains,
}

/// The state of an index.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IndexState {
    /// The state is unspecified.
    StateUnspecified,
    /// The index is being created.
    Creating,
    /// The index is ready to be used.
    Ready,
    /// The index is being deleted.
    NeedsRepair,
}

/// The response for FirestoreAdmin.ListIndexes.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListIndexesResponse {
    /// The indexes.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<Index>,

    /// A page token that may be used to request another page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// --- Field Configuration Data Structures ---

/// Represents a single field in the database.
/// Based on: https://cloud.google.com/firestore/docs/reference/rest/v1/projects.databases.collectionGroups.fields#Field
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Required. A field name of the form:
    /// `projects/{projectId}/databases/{databaseId}/collectionGroups/{collectionId}/fields/{fieldPath}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The index configuration for this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_config: Option<IndexConfig>,

    /// The TTL configuration for this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_config: Option<TtlConfig>,
}

/// The index configuration for a field.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IndexConfig {
    /// The indexes supported for this field.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<Index>,

    /// Output only. When true, the `Field` path is the default field path for the database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses_ancestor_config: Option<bool>,

    /// Output only. The ID of the ancestor field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ancestor_field: Option<String>,

    /// Output only. The current state of the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverting: Option<bool>,
}

/// The TTL (time-to-live) configuration for a field.
/// This enables automatic deletion of documents based on a timestamp field.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TtlConfig {
    /// Output only. The current state of the TTL configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<TtlState>,
}

/// The state of a TTL configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TtlState {
    /// The state is unspecified or not set.
    StateUnspecified,
    /// The TTL is being created.
    Creating,
    /// The TTL is active and functioning.
    Active,
    /// The TTL needs repair.
    NeedsRepair,
}

/// The response for FirestoreAdmin.ListFields.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListFieldsResponse {
    /// The fields.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,

    /// A page token that may be used to request another page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}
