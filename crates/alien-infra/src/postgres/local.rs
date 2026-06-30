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
    /// The major engine version the embedded server was created with; `update_start` rejects a
    /// day-2 change against it (the reason lives there).
    pub(crate) version: Option<String>,
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
        self.version = Some(config.version.clone());

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

        // Report the version the server was actually created with, not the desired config — keeps the
        // heartbeat honest even if a day-2 version change was requested (and rejected by update_start).
        let reported_version = self.version.as_deref().unwrap_or(&config.version);
        emit_local_postgres_heartbeat(ctx, &config.id, reported_version, self.port);
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

        // Local pins the engine version at create: the embedded server keeps its data dir, and an
        // in-place major upgrade would need `pg_upgrade`, which isn't wired here. So a day-2 version
        // change must fail loud rather than falsely claim an upgrade that never happened. `cpu`/`memory`
        // genuinely have no meaning for the embedded server, so a change to those alone is a true no-op.
        // When the version isn't recorded (state created before this field existed), fall back to the
        // previously applied config so the rejection still fires.
        let recorded_version = self.version.clone().or_else(|| {
            ctx.previous_resource_config::<Postgres>()
                .ok()
                .map(|prev| prev.version.clone())
        });
        if let Some(current) = recorded_version {
            if current != config.version {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "local Postgres '{}' cannot change engine version in place ({} -> {}): the \
                         embedded server has no upgrade path. Recreate the resource to move to a new \
                         version (its data is not preserved).",
                        config.id, current, config.version
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        info!(
            postgres_id = %config.id,
            "Updating local Postgres (no-op; cpu/memory have no effect on the embedded server)"
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
        // emitted copy so it never reaches synced/persisted state. In-process dependents resolve the
        // password from the manager's 0600 metadata (`LocalBindingsProvider::load_postgres`). A linked
        // out-of-process workload (worker/daemon) gets the full binding re-resolved live from
        // that same 0600 metadata at process start — never from this synced copy.
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
            version: Some("17".to_string()),
            binding: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Local Postgres day-2 update behavior. `update_start` is the only handler that doesn't bind
    //! the concrete `LocalPostgresManager`, so it is the unit-testable surface here; create /
    //! readiness / delete drive a real embedded server and are covered by the end-to-end tests.

    use std::sync::Arc;

    use alien_core::{Platform, Postgres};

    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

    use super::{LocalPostgresController, LocalPostgresState};

    fn local_postgres(version: &str) -> Postgres {
        Postgres::new("db".to_string()).version(version.to_string()).build()
    }

    /// A Ready Local Postgres recorded at version 17 — the starting point for the day-2 update
    /// tests. `update_start` never touches the local manager, so an empty provider suffices.
    async fn ready_executor() -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(local_postgres("17"))
            .controller(LocalPostgresController::mock_ready("db", 5432))
            .platform(Platform::Local)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build")
    }

    #[tokio::test]
    async fn update_rejects_version_change() {
        let mut executor = ready_executor().await;
        executor
            .update(local_postgres("16"))
            .expect("transition to update");
        let error = executor.step().await.expect_err(
            "a Local version change must fail loud (no pg_upgrade), not be reported as applied",
        );
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    }

    #[tokio::test]
    async fn update_is_noop_on_cpu_memory_change() {
        let mut executor = ready_executor().await;
        let mut resized = local_postgres("17");
        resized.cpu = Some("4".to_string());
        resized.memory = Some("8Gi".to_string());
        executor.update(resized).expect("transition to update");
        executor
            .step()
            .await
            .expect("a cpu/memory-only change must be a clean no-op on Local");
        assert_eq!(
            executor
                .internal_state::<LocalPostgresController>()
                .expect("controller should be LocalPostgresController")
                .state,
            LocalPostgresState::Ready,
        );
    }
}
