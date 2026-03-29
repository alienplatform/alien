use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Kv, KvOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError};
use alien_gcp_clients::firestore::{Database, DatabaseType, FirestoreApi};
use alien_gcp_clients::longrunning::OperationResult;
use alien_macros::{controller, flow_entry, handler, terminal_state};

/// Generates the Firestore database name for the KV store.
///
/// Currently always returns "(default)" since Firestore Native mode supports
/// a single default database per project. Named databases could be supported
/// in the future if needed.
fn get_firestore_database_name(_prefix: &str, _name: &str) -> String {
    "(default)".to_string()
}

/// Generates the collection name for KV storage.
fn get_collection_name(prefix: &str, name: &str) -> String {
    format!("kv-{}-{}", prefix, name)
        .to_lowercase()
        .replace('_', "-")
}

#[controller]
pub struct GcpKvController {
    /// The Firestore database name (usually "(default)")
    pub(crate) database_name: Option<String>,
    /// The collection name for KV storage
    pub(crate) collection_name: Option<String>,
    /// The project ID where Firestore is located
    pub(crate) project_id: Option<String>,
    /// The name of the long-running operation for database creation (if any)
    pub(crate) operation_name: Option<String>,
}

#[controller]
impl GcpKvController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_firestore_client(gcp_config)?;

        let database_name = get_firestore_database_name(&ctx.resource_prefix, &config.id);
        let collection_name = get_collection_name(&ctx.resource_prefix, &config.id);

        info!(
            id=%config.id,
            database=%database_name,
            collection=%collection_name,
            "Setting up Firestore collection for KV store"
        );

        // Store the configuration early so it's available in subsequent states
        self.database_name = Some(database_name.clone());
        self.collection_name = Some(collection_name);
        self.project_id = Some(gcp_config.project_id.clone());

        // Check if Firestore database exists; create it if not
        match client.get_database(database_name.clone()).await {
            Ok(_) => {
                info!(database=%database_name, "Firestore database already exists");
                self.operation_name = None;
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(
                    database=%database_name,
                    region=%gcp_config.region,
                    "Firestore database does not exist, creating it"
                );

                // Enabling the Firestore API does NOT create a default database.
                // We must explicitly create it via the databases API.
                let database = Database::builder()
                    .location_id(gcp_config.region.clone())
                    .r#type(DatabaseType::FirestoreNative)
                    .build();

                let operation = client
                    .create_database(database_name.clone(), database)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create Firestore database '{}'", database_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(
                    database=%database_name,
                    operation_name=?operation.name,
                    "Firestore database creation started"
                );

                self.operation_name = operation.name;
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to check Firestore database '{}'", database_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDatabase,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDatabase,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_database(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_firestore_client(gcp_config)?;

        let database_name = self.database_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Database name not set in state".to_string(),
            })
        })?;

        // If no operation was started, the database already existed — proceed directly
        let Some(op_name) = self.operation_name.as_ref() else {
            debug!(database=%database_name, "No pending operation, database already existed");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        };

        // Poll the long-running database creation operation
        debug!(database=%database_name, operation=%op_name, "Checking database creation status");

        let operation = client.get_operation(op_name.to_string()).await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check Firestore database creation operation '{}'",
                    op_name
                ),
                resource_id: Some(config.id.clone()),
            },
        )?;

        if !operation.done.unwrap_or(false) {
            debug!(database=%database_name, operation=%op_name, "Database creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(10)),
            });
        }

        // Operation completed — check for errors
        if let Some(OperationResult::Error { error }) = &operation.result {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Firestore database creation failed: {} (code: {})",
                    error.message, error.code
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Verify the database is now accessible
        client.get_database(database_name.clone()).await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Firestore database '{}' not accessible after creation completed",
                    database_name
                ),
                resource_id: Some(config.id.clone()),
            },
        )?;

        let collection_name = self.collection_name.as_deref().unwrap_or("unknown");
        info!(
            database=%database_name,
            collection=%collection_name,
            "Firestore KV store ready (collections are created automatically on first write)"
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_firestore_client(gcp_config)?;

        let database_name = self.database_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Database name not set in state".to_string(),
            })
        })?;

        // Heartbeat: verify Firestore database is still accessible
        match client.get_database(database_name.clone()).await {
            Ok(_) => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            }),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: "Firestore database no longer exists".to_string(),
                }))
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check Firestore database '{}' during heartbeat",
                    database_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // KV has no mutable fields — update is a no-op that also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        info!(id=%config.id, "GCP KV update (no-op — no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;

        // Firestore collections auto-manage their lifecycle based on document presence.
        // We don't delete the collection to avoid accidental data loss in shared databases.
        info!(id=%config.id, "Firestore KV store cleanup complete (collections auto-manage lifecycle)");

        self.clear_state();

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let (collection_name, database_name, project_id) =
            match (&self.collection_name, &self.database_name, &self.project_id) {
                (Some(c), Some(d), Some(p)) => (c, d, p),
                _ => return None,
            };

        Some(ResourceOutputs::new(KvOutputs {
            store_name: collection_name.clone(),
            identifier: Some(format!(
                "projects/{}/databases/{}/documents/{}",
                project_id, database_name, collection_name
            )),
            endpoint: Some(format!(
                "https://firestore.googleapis.com/v1/projects/{}/databases/{}",
                project_id, database_name
            )),
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, KvBinding};

        let (collection_name, database_name, project_id) =
            match (&self.collection_name, &self.database_name, &self.project_id) {
                (Some(c), Some(d), Some(p)) => (c, d, p),
                _ => return Ok(None),
            };

        let binding = KvBinding::firestore(
            BindingValue::value(project_id.clone()),
            BindingValue::value(database_name.clone()),
            BindingValue::value(collection_name.clone()),
        );
        Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
    }
}

// Separate impl block for helper methods
impl GcpKvController {
    fn clear_state(&mut self) {
        self.database_name = None;
        self.collection_name = None;
        self.project_id = None;
        self.operation_name = None;
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(collection_name: &str, project_id: &str) -> Self {
        Self {
            state: GcpKvState::Ready,
            database_name: Some("(default)".to_string()),
            collection_name: Some(collection_name.to_string()),
            project_id: Some(project_id.to_string()),
            operation_name: None,
            _internal_stay_count: None,
        }
    }
}
