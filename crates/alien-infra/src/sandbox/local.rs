//! Local Sandbox controller.
//!
//! The Frozen parent on Local is manager state rather than a provider object: there is nothing
//! to provision until a session is created. What the controller owns is the guarantee that
//! Docker is reachable, and that sessions left behind by a previous run are reaped before
//! anything new starts.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox, SandboxOutputs};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

/// Local Sandbox controller.
#[controller]
pub struct LocalSandboxController {
    /// Sandbox this controller owns sessions for, and the enumeration scope for reaping.
    pub(crate) sandbox_name: Option<String>,
    /// Loopback route the workload's binding talks to.
    pub(crate) route_url: Option<String>,
    /// File the route's bearer token is written to. The binding carries the path, never the
    /// token, so no secret lands in deployment state.
    pub(crate) token_path: Option<String>,
}

#[controller]
impl LocalSandboxController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = EnsureRuntime,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn ensure_runtime(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let manager = sandbox_manager(ctx)?;

        // Reaping here rather than only at delete: a CLI restart leaves the previous run's
        // containers behind, and a session that outlives the process that owned it can never
        // be reached again.
        let reaped = manager
            .reap(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to reap stale sandbox sessions".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        if reaped > 0 {
            info!(sandbox_id = %config.id, reaped, "Reaped sandbox sessions from a previous run");
        }

        self.sandbox_name = Some(config.id.clone());

        // The session template is fixed here rather than accepted per create: a client-supplied
        // limit is a limit the client can decline to send, and this sandbox runs its code.
        let route = alien_local::SandboxRoute::ensure(manager, &config.id, session_template(&config)?)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to serve the local sandbox route".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.route_url = Some(route.base_url.clone());
        self.token_path = Some(route.token_path.display().to_string());

        info!(sandbox_id = %config.id, route = %route.base_url, "Local sandbox ready");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        // Re-ensured on every tick, not only at create: the route is a listener in this
        // process, so a manager restart leaves the persisted URL pointing at a dead port until
        // something binds it again. Idempotent by sandbox id.
        let manager = sandbox_manager(ctx)?;
        let route = alien_local::SandboxRoute::ensure(
            Arc::clone(&manager),
            &config.id,
            session_template(&config)?,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to serve the local sandbox route".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
        self.route_url = Some(route.base_url);
        self.token_path = Some(route.token_path.display().to_string());

        // "Healthy" on a platform with nothing durable means the runtime is still there to
        // create sessions in; the session count itself is not a health signal.
        let sessions = manager
            .list_sessions(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Docker sandbox health check failed".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        debug!(sandbox_id = %config.id, sessions = sessions.len(), "Sandbox health check passed");

        // Content-free by construction: a count and whether the route is bound. Nothing here
        // reaches inside a session, which is the whole reason the resource exists.
        ctx.emit_heartbeat(alien_core::ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Sandbox::RESOURCE_TYPE,
            controller_platform: alien_core::Platform::Local,
            backend: alien_core::HeartbeatBackend::Local,
            observed_at: chrono::Utc::now(),
            data: alien_core::ResourceHeartbeatData::Sandbox(
                alien_core::SandboxHeartbeatData::Local(alien_core::LocalSandboxHeartbeatData {
                    status: alien_core::SandboxHeartbeatStatus::default(),
                    active_sessions: sessions.len() as u32,
                    route_serving: self.route_url.is_some(),
                }),
            ),
            raw: vec![],
        });

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(15)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdatingSandbox,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn updating_sandbox(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        // Config changes apply to sessions created after them. Running sessions are not
        // restarted: a session is a unit of work someone is waiting on, not a replica. The
        // route keeps its address — the workload already holds that URL — and takes the new
        // template.
        let manager = sandbox_manager(ctx)?;
        alien_local::SandboxRoute::ensure(manager, &config.id, session_template(&config)?)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update the local sandbox route".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(sandbox_id = %config.id, "Updated local sandbox configuration");

        self.sandbox_name = Some(config.id.clone());

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let manager = sandbox_manager(ctx)?;
        let reaped = manager
            .reap(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to remove sandbox sessions".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // The containers are not the whole of it: a route left serving keeps accepting session
        // creates for a sandbox that no longer exists, and its token file stays on disk as a
        // live credential.
        alien_local::SandboxRoute::remove(&config.id).await;
        self.route_url = None;
        self.token_path = None;

        info!(sandbox_id = %config.id, reaped, "Removed local sandbox sessions and its route");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────────────────

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.sandbox_name.as_ref().map(|name| {
            CoreResourceOutputs::new(SandboxOutputs {
                parent_name: name.clone(),
                identifier: None,
                // Sessions are reached through the local manager's authenticated loopback
                // route, which the binding resolves; there is no provider endpoint to publish.
                endpoint: None,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, SandboxBinding};

        let (Some(route_url), Some(token_path), Some(sandbox_name)) = (
            self.route_url.as_ref(),
            self.token_path.as_ref(),
            self.sandbox_name.as_ref(),
        ) else {
            return Ok(None);
        };

        let binding = SandboxBinding::local(
            BindingValue::value(route_url.clone()),
            BindingValue::value(sandbox_name.clone()),
            BindingValue::value(token_path.clone()),
        );

        Ok(Some(serde_json::to_value(binding).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: "binding".to_string(),
                message: "Failed to serialize sandbox binding parameters".to_string(),
            },
        )?))
    }
}

/// Turns the declaration's ceilings into what Docker is given.
///
/// Every field is enforced. A limit this cannot express is an error rather than a default:
/// silently widening a ceiling on a sandbox is the failure this resource exists to prevent.
#[cfg(feature = "local")]
fn session_template(sandbox: &Sandbox) -> Result<alien_local::SandboxSessionConfig> {
    use alien_core::{SandboxCode, SandboxEgress};

    let SandboxCode::Image { image } = &sandbox.code else {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "Local sandboxes take a prebuilt image; building one from source is not \
                      supported on this platform"
                .to_string(),
            resource_id: Some(sandbox.id.clone()),
        }));
    };

    let egress = match &sandbox.egress {
        SandboxEgress::Deny => alien_local::SandboxEgressMode::Deny,
        SandboxEgress::Allow => alien_local::SandboxEgressMode::Allow,
        SandboxEgress::AllowDomains { .. } => {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "Local cannot restrict egress to a hostname list; only Azure can"
                    .to_string(),
                resource_id: Some(sandbox.id.clone()),
            }))
        }
    };

    let limits = sandbox.resolved_limits();

    Ok(alien_local::SandboxSessionConfig {
        image: image.clone(),
        cpu_cores: cpu_cores(&limits.cpu, &sandbox.id)?,
        memory_bytes: bytes(&limits.memory, &sandbox.id)? as i64,
        pids_limit: limits.max_processes.map(i64::from),
        scratch_bytes: bytes(&limits.disk, &sandbox.id)?,
        egress,
        preview_ports: sandbox.preview_ports.clone(),
        env: std::collections::HashMap::new(),
    })
}

/// Parses a CPU ceiling in cores or millicores.
#[cfg(feature = "local")]
fn cpu_cores(value: &str, sandbox_id: &str) -> Result<f64> {
    let trimmed = value.trim();
    let parsed = match trimmed.strip_suffix('m') {
        Some(millis) => millis.parse::<f64>().ok().map(|value| value / 1000.0),
        None => trimmed.parse::<f64>().ok(),
    };

    parsed.filter(|cores| *cores > 0.0).ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("'{value}' is not a CPU ceiling in cores or millicores"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })
}

/// Parses a byte quantity written the way Kubernetes writes one.
#[cfg(feature = "local")]
fn bytes(value: &str, sandbox_id: &str) -> Result<u64> {
    const SUFFIXES: &[(&str, u64)] = &[
        ("Ki", 1024),
        ("Mi", 1024 * 1024),
        ("Gi", 1024 * 1024 * 1024),
        ("Ti", 1024 * 1024 * 1024 * 1024),
        ("K", 1000),
        ("M", 1000 * 1000),
        ("G", 1000 * 1000 * 1000),
        ("T", 1000u64.pow(4)),
    ];

    let trimmed = value.trim();
    let parsed = SUFFIXES
        .iter()
        .find_map(|(suffix, scale)| {
            trimmed
                .strip_suffix(suffix)
                .and_then(|number| number.trim().parse::<f64>().ok())
                .map(|number| (number * *scale as f64) as u64)
        })
        .or_else(|| trimmed.parse::<u64>().ok());

    parsed.filter(|bytes| *bytes > 0).ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("'{value}' is not a byte quantity"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })
}

#[cfg(feature = "local")]
fn sandbox_manager(
    ctx: &ResourceControllerContext<'_>,
) -> Result<std::sync::Arc<alien_local::LocalSandboxManager>> {
    ctx.service_provider
        .get_local_sandbox_manager()
        .ok_or_else(|| {
            AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "LocalSandboxManager".to_string(),
            })
        })
}

#[cfg(test)]
mod tests {
    /// A ceiling that cannot be expressed must fail rather than default. Silently widening a
    /// limit is the failure this resource exists to prevent.
    #[test]
    fn limits_parse_or_fail_loudly() {
        assert_eq!(cpu_cores("500m", "sbx").expect("millicores"), 0.5);
        assert_eq!(cpu_cores("2", "sbx").expect("cores"), 2.0);
        cpu_cores("half", "sbx").expect_err("an unparseable CPU ceiling is an error");
        cpu_cores("0", "sbx").expect_err("a zero CPU ceiling is not a ceiling");

        assert_eq!(bytes("512Mi", "sbx").expect("mebibytes"), 536_870_912);
        assert_eq!(bytes("1Gi", "sbx").expect("gibibytes"), 1_073_741_824);
        assert_eq!(bytes("1M", "sbx").expect("megabytes"), 1_000_000);
        assert_eq!(bytes("4096", "sbx").expect("plain bytes"), 4096);
        bytes("plenty", "sbx").expect_err("an unparseable size is an error");
    }

    /// Local has no hostname allowlist, and accepting one would run untrusted code with wider
    /// egress than the declaration asked for.
    #[test]
    fn a_hostname_allowlist_is_refused_rather_than_widened() {
        use alien_core::{SandboxCode, SandboxEgress, SandboxLimits, SandboxSessionPolicy};

        let sandbox = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Image {
                image: "alpine:3.20".to_string(),
            })
            .limits(SandboxLimits {
                cpu: "500m".to_string(),
                memory: "512Mi".to_string(),
                disk: "1Gi".to_string(),
                max_processes: Some(64),
            })
            .egress(SandboxEgress::AllowDomains {
                domains: vec!["example.com".to_string()],
            })
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        session_template(&sandbox).expect_err("Local cannot honour a hostname allowlist");
    }

    use super::*;
    use crate::core::{deserialize_controller, serialize_controller, ResourceController};

    /// A controller must round-trip by tag. Miss the by-tag arm and the executor cannot resolve
    /// it, which surfaces as InitialSetupFailed with no per-resource error to read — it fails
    /// above the handler layer, so nothing logs a cause.
    #[test]
    fn controller_round_trips_by_tag() {
        let controller = LocalSandboxController {
            sandbox_name: Some("agent".to_string()),
            ..Default::default()
        };

        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "LocalSandboxController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    /// Resolving a controller for a new deployment is a different path from deserializing saved
    /// state, so registering one does not imply the other. Both are needed and both are tested.
    #[test]
    fn the_registry_resolves_a_local_sandbox_controller() {
        let registry = crate::core::ResourceRegistry::with_built_ins();

        let controller = registry
            .get_controller(alien_core::Sandbox::RESOURCE_TYPE, alien_core::Platform::Local)
            .expect("Local must have a registered Sandbox controller");
        assert_eq!(controller.controller_type(), "LocalSandboxController");
    }
}
