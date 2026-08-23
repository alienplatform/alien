//! GCP Agent Platform reasoning-engine controller.
//!
//! Creates the durable engine every sandbox template and session hangs under, one per sandbox, and
//! records its server-assigned id in state for the template controller to read as a dependency.
//! Vertex exposes no Terraform resource for the engine, so this API-call controller is its only
//! creator.
//!
//! Create-once: the id is persisted, so a later reconcile reuses it and never creates a second
//! engine. The provision permission set grants create and delete but no get/list, so readiness is
//! not re-read and reuse comes from state, never a lookup.
//!
//! Unregistered until the cutover, like the template controller (T09) it feeds: the registered GCP
//! sandbox backend is still Cloud Run, so nothing reaches this yet and it is proven by its tests.

use std::time::Duration;
use tracing::info;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{GcpAgentPlatformEngine, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::agent_platform::ReasoningEngine;
use alien_gcp_clients::longrunning::OperationResult;
use alien_macros::controller;

/// Last path segment of a resource name — the bare id the client interpolates back into its paths.
fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Requires a long-running operation to carry a name to poll — a nameless one cannot be resumed.
fn require_operation_name(name: Option<String>, resource_id: &str) -> Result<String> {
    name.ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "engine operation carried no name to poll".to_string(),
            resource_id: Some(resource_id.to_string()),
        })
    })
}

#[controller]
pub struct GcpAgentPlatformEngineController {
    /// Server-assigned engine id (last path segment), the contract the template controller reads.
    pub(crate) engine_id: Option<String>,
    /// The create long-running operation being polled to learn the engine's id.
    pub(crate) pending_operation: Option<String>,
}

#[controller]
impl GcpAgentPlatformEngineController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<GcpAgentPlatformEngine>()?;

        // A persisted id means the engine already exists; create is never retried.
        if self.engine_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        let display_name = format!("{}-{}", ctx.resource_prefix, config.id);
        info!(id=%config.id, "Creating Agent Platform reasoning engine");
        let operation =
            client
                .create_engine(&display_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create reasoning engine '{display_name}'"),
                    resource_id: Some(config.id.clone()),
                })?;

        self.pending_operation = Some(require_operation_name(operation.name, &config.id)?);
        Ok(HandlerAction::Continue {
            state: AwaitingEngineOperation,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = AwaitingEngineOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn awaiting_engine_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<GcpAgentPlatformEngine>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        let op_name = self.pending_operation.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no pending engine operation in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let operation =
            client
                .get_operation(&op_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to poll engine operation '{op_name}'"),
                    resource_id: Some(config.id.clone()),
                })?;

        if operation.done != Some(true) {
            return Ok(HandlerAction::Stay {
                max_times: Some(150),
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let engine = match operation.result {
            Some(OperationResult::Response { response }) => {
                serde_json::from_value::<ReasoningEngine>(response)
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "engine create operation returned an unreadable resource"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    })?
            }
            Some(OperationResult::Error { error }) => {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "engine create failed: {} (grpc {})",
                        error.message, error.code
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
            None => {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "engine create operation reported done without a result".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let name = engine.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "created engine carried no resource name".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        self.engine_id = Some(last_segment(&name).to_string());
        self.pending_operation = None;
        info!(id=%config.id, engine=%last_segment(&name), "reasoning engine ready");

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
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // No `get_engine` on the client and no per-session health to read here; the engine is
        // create-once, so Ready idles and re-reads nothing.
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
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
        let config = ctx.desired_resource_config::<GcpAgentPlatformEngine>()?;

        let Some(engine) = self.engine_id.clone() else {
            // Nothing was ever created — a delete with no engine is already done.
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        // An orphaned engine bills, so a genuine delete failure surfaces rather than being
        // swallowed; the client already maps not-found to success.
        client
            .delete_engine(&engine)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete reasoning engine '{engine}'"),
                resource_id: Some(config.id.clone()),
            })?;

        self.engine_id = None;
        info!(id=%config.id, "reasoning engine teardown complete");
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
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        None
    }
}

impl GcpAgentPlatformEngineController {
    /// Creates a controller already holding a ready engine id, for tests that seed it as a
    /// dependency of the template controller.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(engine_id: &str) -> Self {
        Self {
            state: GcpAgentPlatformEngineState::Ready,
            engine_id: Some(engine_id.to_string()),
            pending_operation: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use alien_gcp_clients::agent_platform::MockAgentPlatformApi;
    use alien_gcp_clients::longrunning::Operation;
    use std::sync::Arc;

    fn provider_with(client: Arc<MockAgentPlatformApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_agent_platform_client()
            .returning(move |_| Ok(client.clone()));
        Arc::new(provider)
    }

    fn pending_op() -> Operation {
        Operation {
            name: Some("projects/p/locations/us-central1/operations/op1".to_string()),
            metadata: None,
            done: Some(false),
            result: None,
        }
    }

    /// A completed create operation whose response carries the engine's full resource name.
    fn done_engine_op(engine_id: &str) -> Operation {
        Operation {
            name: Some("projects/p/locations/us-central1/operations/op1".to_string()),
            metadata: None,
            done: Some(true),
            result: Some(OperationResult::Response {
                response: serde_json::json!({
                    "name": format!(
                        "projects/p/locations/us-central1/reasoningEngines/{engine_id}"
                    )
                }),
            }),
        }
    }

    async fn build_executor(
        provider: Arc<MockPlatformServiceProvider>,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(GcpAgentPlatformEngine::new("orders-engine".to_string()).build())
            .controller(GcpAgentPlatformEngineController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds")
    }

    #[tokio::test]
    async fn create_records_the_server_assigned_id_then_deletes_it() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine().returning(|_| Ok(pending_op()));
        m.expect_get_operation()
            .returning(|_| Ok(done_engine_op("eng-42")));
        m.expect_delete_engine().returning(|_| Ok(()));
        let provider = provider_with(Arc::new(m));

        let mut executor = build_executor(provider).await;
        executor
            .run_until_terminal()
            .await
            .expect("create runs to a steady state");
        assert_eq!(executor.status(), ResourceStatus::Running);

        let controller = executor
            .internal_state::<GcpAgentPlatformEngineController>()
            .expect("the controller downcasts");
        assert_eq!(
            controller.engine_id.as_deref(),
            Some("eng-42"),
            "the server-assigned engine id is recorded, not fabricated"
        );

        executor.delete().expect("delete is accepted");
        executor
            .run_until_terminal()
            .await
            .expect("delete runs to terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn a_create_failure_lands_in_provision_failed() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine().returning(|_| {
            Err(AlienError::new(
                alien_gcp_clients::agent_platform::AgentPlatformErrorData::RequestFailed {
                    operation: "create engine".to_string(),
                    message: "quota exceeded".to_string(),
                },
            ))
        });
        let provider = provider_with(Arc::new(m));

        let mut executor = build_executor(provider).await;

        // The failure must surface as an error the executor routes to CreateFailed, not a silent
        // retry. Bounded so a poll-forever regression fails instead of hanging.
        let mut surfaced = false;
        for _ in 0..3 {
            if executor.step().await.is_err() {
                surfaced = true;
                break;
            }
        }
        assert!(
            surfaced,
            "a create-engine failure surfaces rather than being swallowed"
        );
    }
}
