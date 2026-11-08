use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::bindings::RedisBinding;
use alien_core::{
    HeartbeatBackend, LocalRedisHeartbeatData, ObservedHealth, Platform, ProviderLifecycleState,
    Redis, RedisHeartbeatData, RedisHeartbeatStatus, RedisOutputs, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_local::{LocalRedisManager, RedisSpec, VALKEY_VERSION};
use alien_macros::controller;
use chrono::Utc;
use std::sync::Arc;

#[controller]
pub struct LocalRedisController {
    /// Port the local server listens on; tracked for outputs and heartbeats.
    pub(crate) port: Option<u16>,
    /// Resolved Local binding, held in memory only (`#[serde(skip)]`): it carries the
    /// generated password, which must stay out of persisted and synced state.
    /// `get_binding_params` strips the password; the bindings provider re-reads it from
    /// the manager's 0600 metadata.
    #[serde(skip)]
    pub(crate) binding: Option<RedisBinding>,
}

fn manager(ctx: &ResourceControllerContext<'_>) -> Result<Arc<LocalRedisManager>> {
    ctx.service_provider
        .get_local_redis_manager()
        .ok_or_else(|| {
            AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "redis_manager".to_string(),
            })
        })
}

/// What the local server runs with. Local has no replicas and no key management, so
/// `highAvailability` and `encryptionKey` have nothing to configure here.
fn spec_for(config: &Redis) -> Result<RedisSpec> {
    let max_memory_bytes =
        config
            .requested_memory_bytes()
            .context(ErrorData::ResourceConfigInvalid {
                message: format!("Redis '{}' has an invalid memory value", config.id),
                resource_id: Some(config.id.clone()),
            })?;
    if config.high_availability {
        warn!(redis_id = %config.id, "Local Redis has no replicas; highAvailability has no effect");
    }
    Ok(RedisSpec {
        persistence: config.persistence,
        eviction_policy: config.eviction_policy.as_str().to_string(),
        max_memory_bytes,
    })
}

fn port_of(id: &str, binding: &RedisBinding) -> Result<Option<u16>> {
    match binding {
        RedisBinding::Local(b) => Ok(Some(b.port.clone().into_value(id, "port").context(
            ErrorData::ResourceConfigInvalid {
                message: format!("local Redis '{id}' port is not a concrete binding value"),
                resource_id: Some(id.to_string()),
            },
        )?)),
        _ => Ok(None),
    }
}

#[controller]
impl LocalRedisController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Redis>()?;
        let manager = manager(ctx)?;

        info!(redis_id = %config.id, "Creating local Redis");
        manager
            .start_redis(&config.id, spec_for(&config)?)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to start local Redis for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        let binding = manager
            .get_binding(&config.id)
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to read binding for local Redis '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;
        self.port = port_of(&config.id, &binding)?;
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
        let config = ctx.desired_resource_config::<Redis>()?;
        let manager = manager(ctx)?;

        manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Redis health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // After a reload the skipped field is empty; re-resolve from the manager so
        // dependents can still read the connection details.
        if self.binding.is_none() {
            self.binding = Some(manager.get_binding(&config.id).context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to read binding for local Redis '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                },
            )?);
        }

        emit_local_redis_heartbeat(ctx, &config.id, self.port);
        debug!(redis_id = %config.id, "Redis health check passed");

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
        let config = ctx.desired_resource_config::<Redis>()?;
        let manager = manager(ctx)?;

        // The manager restarts the process only when the spec changed; with persistence
        // on, the append-only file keeps the data across that restart.
        info!(redis_id = %config.id, "Updating local Redis");
        manager
            .start_redis(&config.id, spec_for(&config)?)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply the new settings to local Redis '{}'",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            })?;

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
        let config = ctx.desired_resource_config::<Redis>()?;
        // A missing manager must fail loud, not mark Deleted: a running server would leak
        // its process and its on-disk password.
        let manager = manager(ctx)?;

        info!(redis_id = %config.id, "Deleting local Redis");
        manager
            .delete_redis(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete local Redis for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        self.port = None;
        self.binding = None;

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
        self.port.map(|port| {
            ResourceOutputs::new(RedisOutputs {
                endpoint: "127.0.0.1".to_string(),
                port,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Synced to the control plane: keep the coordinates, drop the password. Workloads
        // get the full binding re-resolved from the manager's 0600 metadata at start.
        match &self.binding {
            None => Ok(None),
            Some(binding) => {
                let mut value = serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "redis".to_string(),
                        message: "Failed to serialize Redis binding parameters".to_string(),
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

fn emit_local_redis_heartbeat(ctx: &ResourceControllerContext<'_>, id: &str, port: Option<u16>) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: id.to_string(),
        resource_type: Redis::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Redis(RedisHeartbeatData::Local(LocalRedisHeartbeatData {
            status: RedisHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: Some("Local Redis process is running".to_string()),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: id.to_string(),
            port,
            version: VALKEY_VERSION.to_string(),
            process_running: true,
        })),
        raw: vec![],
    });
}

impl LocalRedisController {
    /// A controller pinned to the Ready state with mock values.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(port: u16) -> Self {
        Self {
            state: LocalRedisState::Ready,
            port: Some(port),
            binding: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ResourceController;

    fn redis(memory: Option<&str>, persistence: bool, high_availability: bool) -> Redis {
        let mut redis = Redis::new("cache".to_string()).build();
        redis.memory = memory.map(str::to_string);
        redis.persistence = persistence;
        redis.high_availability = high_availability;
        redis
    }

    #[test]
    fn spec_maps_memory_policy_and_persistence() {
        let mut config = redis(Some("512Mi"), false, true);
        config.eviction_policy = alien_core::RedisEvictionPolicy::NoEviction;
        let spec = spec_for(&config).unwrap();
        assert_eq!(spec.max_memory_bytes, Some(512 * 1024 * 1024));
        assert_eq!(spec.eviction_policy, "noeviction");
        assert!(!spec.persistence);
    }

    #[test]
    fn spec_without_memory_leaves_the_server_unlimited() {
        assert_eq!(
            spec_for(&redis(None, true, false))
                .unwrap()
                .max_memory_bytes,
            None
        );
    }

    #[test]
    fn spec_rejects_an_invalid_memory_value() {
        let error = spec_for(&redis(Some("lots"), true, false)).unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    }

    #[test]
    fn binding_params_never_contain_the_password() {
        let controller = LocalRedisController {
            binding: Some(RedisBinding::local(
                "127.0.0.1",
                6380,
                "default",
                "s3cr3t-value",
            )),
            port: Some(6380),
            ..Default::default()
        };
        let params = controller.get_binding_params().unwrap().unwrap();
        assert!(!params.to_string().contains("s3cr3t-value"));
        assert_eq!(params["port"], 6380);
        // The persisted controller state must not hold it either.
        let state = serde_json::to_string(&controller).unwrap();
        assert!(!state.contains("s3cr3t-value"));
    }
}
