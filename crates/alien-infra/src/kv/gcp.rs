use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    GcpFirestoreKvHeartbeatData, HeartbeatBackend, Kv, KvHeartbeatData, KvHeartbeatStatus,
    KvOutputs, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use chrono::Utc;
use google_cloud_firestore_admin_v1::{
    client::FirestoreAdmin,
    model::{database, Database},
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_longrunning::model::Operation;

/// Generates the Firestore database name for the KV store.
///
/// Currently always returns "(default)" since Firestore Native mode supports
/// a single default database per project. Named databases could be supported
/// in the future if needed.
fn get_firestore_database_name(_prefix: &str, _name: &str) -> String {
    "(default)".to_string()
}

fn firestore_database_resource_name(project_id: &str, database_id: &str) -> String {
    format!("projects/{project_id}/databases/{database_id}")
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
        let client = ctx
            .service_provider
            .get_gcp_firestore_client(gcp_config)
            .await?;

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
        match get_firestore_database(&client, &gcp_config.project_id, &database_name).await {
            Ok(_) => {
                info!(database=%database_name, "Firestore database already exists");
                self.operation_name = None;
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(
                    database=%database_name,
                    region=%gcp_config.region,
                    "Firestore database does not exist, creating it"
                );

                // Enabling the Firestore API does NOT create a default database.
                // We must explicitly create it via the databases API.
                let database = Database::new()
                    .set_location_id(gcp_config.region.clone())
                    .set_type(database::DatabaseType::FirestoreNative);

                let operation = create_firestore_database(
                    &client,
                    &gcp_config.project_id,
                    &database_name,
                    database,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Firestore database '{}'", database_name),
                    resource_id: Some(config.id.clone()),
                })?;

                info!(
                    database=%database_name,
                    operation_name=%operation.name,
                    "Firestore database creation started"
                );

                self.operation_name = Some(operation.name);
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
        let client = ctx
            .service_provider
            .get_gcp_firestore_client(gcp_config)
            .await?;

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

        let operation = get_firestore_operation(&client, op_name).await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check Firestore database creation operation '{}'",
                    op_name
                ),
                resource_id: Some(config.id.clone()),
            },
        )?;

        if !operation.done {
            debug!(database=%database_name, operation=%op_name, "Database creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(10)),
            });
        }

        // Operation completed — check for errors
        if let Some(error) = operation.error() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Firestore database creation failed: {} (code: {})",
                    error.message, error.code
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Verify the database is now accessible
        get_firestore_database(&client, &gcp_config.project_id, database_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Firestore database '{}' not accessible after creation completed",
                    database_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

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
        let client = ctx
            .service_provider
            .get_gcp_firestore_client(gcp_config)
            .await?;

        let database_name = self.database_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Database name not set in state".to_string(),
            })
        })?;

        // Heartbeat: verify Firestore database is still accessible
        match get_firestore_database(&client, &gcp_config.project_id, database_name).await {
            Ok(database) => {
                emit_gcp_firestore_kv_heartbeat(
                    ctx,
                    &config.id,
                    database_name,
                    database,
                    &self.project_id,
                );
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: Some(Duration::from_secs(30)),
                })
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_core::Platform;
    use google_cloud_firestore_admin_v1::{
        client::FirestoreAdmin,
        model::{CreateDatabaseRequest, GetDatabaseRequest},
        stub::FirestoreAdmin as FirestoreAdminStub,
    };
    use google_cloud_gax::{
        error::{
            rpc::{Code, Status},
            Error as GaxError,
        },
        options::RequestOptions,
        response::Response,
    };
    use google_cloud_longrunning::model::GetOperationRequest;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    mockall::mock! {
        #[derive(Debug)]
        FirestoreAdmin {}

        impl FirestoreAdminStub for FirestoreAdmin {
            async fn create_database(
                &self,
                request: CreateDatabaseRequest,
                options: RequestOptions,
            ) -> google_cloud_firestore_admin_v1::Result<Response<Operation>>;

            async fn get_database(
                &self,
                request: GetDatabaseRequest,
                options: RequestOptions,
            ) -> google_cloud_firestore_admin_v1::Result<Response<Database>>;

            async fn get_operation(
                &self,
                request: GetOperationRequest,
                options: RequestOptions,
            ) -> google_cloud_firestore_admin_v1::Result<Response<Operation>>;
        }
    }

    fn kv_resource() -> Kv {
        Kv::new("settings".to_string()).build()
    }

    fn firestore_database() -> Database {
        Database::new()
            .set_name("projects/test-project/databases/(default)")
            .set_location_id("us-central1")
            .set_type(database::DatabaseType::FirestoreNative)
    }

    fn not_found_error() -> GaxError {
        GaxError::service(
            Status::default()
                .set_code(Code::NotFound)
                .set_message("database not found"),
        )
    }

    fn setup_mock_provider(firestore: FirestoreAdmin) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_gcp_firestore_client()
            .returning(move |_| Ok(firestore.clone()));
        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn create_flow_uses_sdk_native_firestore_admin_stub() {
        let database_name = "projects/test-project/databases/(default)".to_string();
        let operation_name = "operations/firestore-create".to_string();
        let get_database_calls = Arc::new(AtomicUsize::new(0));

        let mut stub = MockFirestoreAdmin::new();
        stub.expect_get_database()
            .withf({
                let database_name = database_name.clone();
                move |request, _| request.name == database_name
            })
            .times(2)
            .returning({
                let get_database_calls = Arc::clone(&get_database_calls);
                move |_, _| {
                    let previous = get_database_calls.fetch_add(1, Ordering::SeqCst);
                    if previous == 0 {
                        Err(not_found_error())
                    } else {
                        Ok(Response::from(firestore_database()))
                    }
                }
            });
        stub.expect_create_database()
            .withf(|request, _| {
                request.parent == "projects/test-project"
                    && request.database_id == "(default)"
                    && request
                        .database
                        .as_ref()
                        .is_some_and(|database| database.location_id == "us-central1")
            })
            .once()
            .returning({
                let operation_name = operation_name.clone();
                move |_, _| {
                    Ok(Response::from(
                        Operation::new().set_name(operation_name.clone()),
                    ))
                }
            });
        stub.expect_get_operation()
            .withf({
                let operation_name = operation_name.clone();
                move |request, _| request.name == operation_name
            })
            .once()
            .returning(|_, _| Ok(Response::from(Operation::new().set_done(true))));

        let firestore = FirestoreAdmin::from_stub(stub);
        let mock_provider = setup_mock_provider(firestore);

        let mut executor = SingleControllerExecutor::builder()
            .resource(kv_resource())
            .controller(GcpKvController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .build()
            .await
            .expect("executor should build");

        executor
            .run_until_terminal()
            .await
            .expect("Firestore KV create flow should reach ready");

        assert_eq!(executor.status(), ResourceStatus::Running);
        let outputs = executor.outputs().expect("outputs should be present");
        let outputs = outputs
            .downcast_ref::<KvOutputs>()
            .expect("outputs should be KV outputs");
        assert_eq!(outputs.store_name, "kv-test-settings");
        assert_eq!(
            outputs.identifier.as_deref(),
            Some("projects/test-project/databases/(default)/documents/kv-test-settings")
        );
    }
}

async fn create_firestore_database(
    client: &FirestoreAdmin,
    project_id: &str,
    database_id: &str,
    database: Database,
) -> Result<Operation> {
    client
        .create_database()
        .set_parent(format!("projects/{project_id}"))
        .set_database_id(database_id.to_string())
        .set_database(database)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "FirestoreAdmin create_database request failed".to_string(),
            resource_id: None,
        })
}

async fn get_firestore_database(
    client: &FirestoreAdmin,
    project_id: &str,
    database_id: &str,
) -> Result<Database> {
    let resource_name = firestore_database_resource_name(project_id, database_id);
    match client
        .get_database()
        .set_name(resource_name.clone())
        .send()
        .await
    {
        Ok(database) => Ok(database),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Firestore database".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "FirestoreAdmin get_database request failed".to_string(),
                resource_id: None,
            })),
    }
}

async fn get_firestore_operation(
    client: &FirestoreAdmin,
    operation_name: &str,
) -> Result<Operation> {
    client
        .get_operation()
        .set_name(operation_name.to_string())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "FirestoreAdmin get_operation request failed".to_string(),
            resource_id: None,
        })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn emit_gcp_firestore_kv_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    configured_database_name: &str,
    database: Database,
    project_id: &Option<String>,
) {
    let database_name = if database.name.is_empty() {
        configured_database_name.to_string()
    } else {
        database.name.clone()
    };
    let lifecycle = if database.delete_time.is_some() {
        ProviderLifecycleState::Deleted
    } else {
        ProviderLifecycleState::Running
    };
    let health = if database.delete_time.is_some() {
        ObservedHealth::Unhealthy
    } else {
        ObservedHealth::Healthy
    };
    let message = Some(if database.location_id.is_empty() {
        format!(
            "Firestore database type is {}",
            serialize_enum(&database.r#type).unwrap_or_else(|| "unknown".to_string())
        )
    } else {
        format!(
            "Firestore database type is {} in {}",
            serialize_enum(&database.r#type).unwrap_or_else(|| "unknown".to_string()),
            database.location_id
        )
    });

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Kv::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Kv(KvHeartbeatData::GcpFirestore(
            GcpFirestoreKvHeartbeatData {
                status: KvHeartbeatStatus {
                    health,
                    lifecycle,
                    message,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                database_name,
                project_id: project_id.clone(),
                endpoint: project_id.as_ref().map(|project_id| {
                    format!(
                        "https://firestore.googleapis.com/v1/projects/{}/databases/{}",
                        project_id, configured_database_name
                    )
                }),
                location_id: if database.location_id.is_empty() {
                    None
                } else {
                    Some(database.location_id)
                },
                database_type: serialize_enum(&database.r#type),
                concurrency_mode: serialize_enum(&database.concurrency_mode),
                app_engine_integration_mode: serialize_enum(&database.app_engine_integration_mode),
                delete_protection_state: serialize_enum(&database.delete_protection_state),
                point_in_time_recovery_enablement: serialize_enum(
                    &database.point_in_time_recovery_enablement,
                ),
                version_retention_period: database
                    .version_retention_period
                    .as_ref()
                    .map(|value| format!("{value:?}")),
                earliest_version_time: database
                    .earliest_version_time
                    .as_ref()
                    .map(|value| format!("{value:?}")),
                create_time: database
                    .create_time
                    .as_ref()
                    .map(|value| format!("{value:?}")),
                update_time: database
                    .update_time
                    .as_ref()
                    .map(|value| format!("{value:?}")),
                delete_time: database
                    .delete_time
                    .as_ref()
                    .map(|value| format!("{value:?}")),
                database_edition: serialize_enum(&database.database_edition),
                cmek_enabled: database.cmek_config.is_some(),
                source_info_present: database.source_info.is_some(),
            },
        )),
        raw: vec![],
    });
}

fn serialize_enum<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
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
