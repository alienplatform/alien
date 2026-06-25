use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::bindings::PostgresBinding;
use alien_core::{
    HeartbeatBackend, LocalPostgresHeartbeatData, ObservedHealth, Platform, Postgres,
    PostgresHeartbeatData, PostgresHeartbeatStatus, PostgresOutputs, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct LocalPostgresController {
    /// Database id created on the local server.
    pub(crate) database: Option<String>,
    /// Port the local server listens on; tracked for outputs and heartbeats.
    pub(crate) port: Option<u16>,
    /// Resolved Local binding, held in memory only (`#[serde(skip)]`) — it inlines the
    /// runtime-generated password, which must stay out of durable, persisted, and control-plane-synced
    /// state. `get_binding_params` strips the password before emitting the connection coordinates;
    /// `LocalBindingsProvider::load_postgres` re-reads the password in-process from the manager's 0600
    /// metadata, the source of truth.
    #[serde(skip)]
    pub(crate) binding: Option<PostgresBinding>,
}

#[controller]
impl LocalPostgresController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Postgres>()?;

        let manager = ctx
            .service_provider
            .get_local_postgres_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "postgres_manager".to_string(),
                })
            })?;

        info!(postgres_id = %config.id, "Creating local Postgres");
        manager
            .start_postgres(&config.id, &config.version)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to start local Postgres for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        let binding =
            manager
                .get_binding(&config.id)
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read binding for local Postgres '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;
        self.port = match &binding {
            PostgresBinding::Local(b) => Some(
                b.port
                    .clone()
                    .into_value(&config.id, "port")
                    .context(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "local Postgres '{}' port is not a concrete binding value",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    })?,
            ),
            _ => None,
        };
        self.database = Some(config.id.clone());
        self.binding = Some(binding);

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
        let config = ctx.desired_resource_config::<Postgres>()?;

        let manager = ctx
            .service_provider
            .get_local_postgres_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "postgres_manager".to_string(),
                })
            })?;

        // Watch the manager's process status (it checks liveness without speaking SQL).
        manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Postgres health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Re-resolve from the manager after a reload (the field is `#[serde(skip)]`, so empty), so
        // dependents can still read the connection details via `get_binding_params`.
        if self.binding.is_none() {
            self.binding = Some(manager.get_binding(&config.id).context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to read binding for local Postgres '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                },
            )?);
        }

        emit_local_postgres_heartbeat(ctx, &config.id, &config.version, self.port);
        debug!(postgres_id = %config.id, "Postgres health check passed");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Postgres>()?;

        // Local pins the version at create: the embedded server keeps its data dir, and an in-place
        // major upgrade would need pg_upgrade; cpu/memory have no meaning for it either. So an update
        // is a deliberate no-op — to change the version, recreate the resource. (Cloud controllers
        // honor cpu/memory/version changes; Local does not.)
        info!(
            postgres_id = %config.id,
            "Updating local Postgres (no-op; version/cpu/memory are pinned at create)"
        );

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
        let config = ctx.desired_resource_config::<Postgres>()?;

        info!(postgres_id = %config.id, "Deleting local Postgres");

        // A missing manager must fail loud, not mark Deleted: a running local Postgres would
        // leak its process and on-disk password. Mirror the create/ready handlers.
        let manager = ctx
            .service_provider
            .get_local_postgres_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "postgres_manager".to_string(),
                })
            })?;

        // Best-effort: the manager's delete succeeds even if the database is already gone.
        manager
            .delete_postgres(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete local Postgres for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        self.database = None;
        self.port = None;

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
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.database.as_ref().map(|database| {
            ResourceOutputs::new(PostgresOutputs {
                endpoint: "127.0.0.1".to_string(),
                database: database.clone(),
                port: self.port,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // The executor persists this into the deployment's `remote_binding_params`, which is synced
        // to the control plane. Unlike cloud variants (which carry a secret-store locator), a Local
        // binding inlines its runtime-generated password because the in-process resolver connects
        // directly. Keep the password in the in-memory binding for that path, but strip it from this
        // emitted copy so it never reaches synced/persisted state — `LocalBindingsProvider::load_postgres`
        // reads it from the manager's 0600 metadata, so the synced coordinates don't need it. (An
        // out-of-process SDK resolver reading that metadata is a follow-up; it doesn't work today.)
        match &self.binding {
            None => Ok(None),
            Some(binding) => {
                let mut value = serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: self
                            .database
                            .clone()
                            .unwrap_or_else(|| "postgres".to_string()),
                        message: "Failed to serialize Postgres binding parameters".to_string(),
                    },
                )?;
                if let Some(obj) = value.as_object_mut() {
                    obj.remove("password");
                }
                Ok(Some(value))
            }
        }
    }
}

fn emit_local_postgres_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    version: &str,
    port: Option<u16>,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Postgres::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Postgres(PostgresHeartbeatData::Local(
            LocalPostgresHeartbeatData {
                status: PostgresHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some("Local Postgres process is running".to_string()),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: resource_id.to_string(),
                port,
                version: version.to_string(),
                process_running: true,
            },
        )),
        raw: vec![],
    });
}

impl LocalPostgresController {
    /// A controller pinned to the Ready state with mock values.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(database: &str, port: u16) -> Self {
        Self {
            state: LocalPostgresState::Ready,
            database: Some(database.to_string()),
            port: Some(port),
            binding: None,
            _internal_stay_count: None,
        }
    }
}
