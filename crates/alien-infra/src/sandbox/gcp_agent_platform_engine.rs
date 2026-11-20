//! GCP Agent Platform reasoning-engine controller.
//!
//! Reconciles the durable engine every sandbox template and session hangs under, one per sandbox.
//! A Live engine is created here and its server-assigned id recorded for the template controller
//! to read as a dependency; a Frozen one is created by the setup stack and imported Ready, and
//! this controller neither creates nor deletes it.
//!
//! Create-once: the id is persisted, so a later reconcile reuses it and never creates a second
//! engine. The provision permission set grants create and delete but no get/list, so readiness is
//! not re-read and reuse comes from state, never a lookup.

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

/// Whether the runtime created the engine and therefore owns its deletion.
///
/// A Frozen engine is removed by `terraform destroy`, and deleting it here would take the parent
/// of sessions the setup stack still owns. An absent lifecycle is refused rather than guessed:
/// unlike the AWS image there is no second signal to fall back to.
fn engine_deletion_is_runtime_owned(
    lifecycle: Option<alien_core::ResourceLifecycle>,
    resource_id: &str,
) -> Result<bool> {
    match lifecycle {
        Some(alien_core::ResourceLifecycle::Live) => Ok(true),
        Some(alien_core::ResourceLifecycle::Frozen) => Ok(false),
        None => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "teardown of reasoning engine '{resource_id}' refused: stack state records no \
                 lifecycle, so whether setup or the runtime owns it is unknown"
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

#[controller]
pub struct GcpAgentPlatformEngineController {
    /// Server-assigned engine id (last path segment), the contract the template controller reads.
    pub(crate) engine_id: Option<String>,
    /// The create long-running operation being polled to learn the engine's id.
    pub(crate) pending_operation: Option<String>,
    /// Set when this controller created the engine. A Frozen engine a setup package created is
    /// removed by that package's `terraform destroy`; one a direct setup created has no package,
    /// so its own teardown has to remove it or it is orphaned and bills.
    #[serde(default)]
    pub(crate) created_here: bool,
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
        let sandbox_id = config.id.strip_suffix("-engine").unwrap_or(&config.id);
        let operation_name = match crate::core::verifier::gcp_sandbox_operation(
            ctx,
            sandbox_id,
            crate::core::verifier::ResourceWrite::CreateSandboxEngine {
                display_name: display_name.clone(),
            },
        )
        .await?
        {
            Some(name) => Some(name),
            None => {
                client
                    .create_engine(
                        &display_name,
                        Some(engine_marker(
                            ctx.resource_prefix,
                            &config.id,
                            ctx.setup_owner_deployment_id()?,
                        )),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create reasoning engine '{display_name}'"),
                        resource_id: Some(config.id.clone()),
                    })?
                    .name
            }
        };

        self.pending_operation = Some(require_operation_name(operation_name, &config.id)?);
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
        self.created_here = true;
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

        // A destroy that arrives before the create operation was collected holds only the
        // operation name. This controller started that operation, so the engine it produces is
        // ours: wait for it, record the id, and let the delete below remove it. Skipping it
        // would report Deleted while the cloud finishes creating a billed engine.
        if self.engine_id.is_none() {
            if let Some(op_name) = self.pending_operation.clone() {
                let client = ctx
                    .service_provider
                    .get_gcp_agent_platform_client(ctx.get_gcp_config()?)?;
                let operation = client.get_operation(&op_name).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to poll engine operation '{op_name}'"),
                        resource_id: Some(config.id.clone()),
                    },
                )?;
                if operation.done != Some(true) {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(150),
                        suggested_delay: Some(Duration::from_secs(2)),
                    });
                }
                self.pending_operation = None;
                if let Some(OperationResult::Response { response }) = operation.result {
                    let engine = serde_json::from_value::<ReasoningEngine>(response)
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "engine create operation returned an unreadable resource"
                                .to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                    if let Some(name) = engine.name {
                        self.engine_id = Some(last_segment(&name).to_string());
                        self.created_here = true;
                    }
                }
                // A failed create produced no engine: nothing to remove.
            }
        }

        let owned = self.owns_engine_deletion(ctx, &config.id)?;
        // The ownership lookup runs only for a deployment installed by direct setup: a package
        // install's engine is never ours, and the management identity has no list permission, so
        // asking would turn a clean teardown into a 403.
        let direct = ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup;
        if !owned && !(direct && self.marked_as_ours(ctx, &config.id).await?) {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

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

/// Text in the engine's description that only a direct setup of this deployment writes. It
/// proves ownership when the controller state lost `created_here` (an older manager rewrote it).
// The marker names the deployment (`prefix/id@deployment id`), so a second deployment that
// reuses a prefix never matches it; it is compared exactly.
pub(crate) fn engine_marker(
    resource_prefix: &str,
    resource_id: &str,
    deployment_id: &str,
) -> String {
    crate::setup_owner::description_marker(&crate::setup_owner::owner_value(
        resource_prefix,
        resource_id,
        deployment_id,
    ))
}

impl GcpAgentPlatformEngineController {
    /// True only when the engine in state carries this deployment's marker in the cloud. An
    /// engine without the marker (Terraform-made, or another deployment's) is left alone.
    async fn marked_as_ours(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        let Some(engine) = self.engine_id.as_deref() else {
            return Ok(false);
        };
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(ctx.get_gcp_config()?)?;
        let owner = crate::setup_owner::owner_value(
            ctx.resource_prefix,
            resource_id,
            ctx.setup_owner_deployment_id()?,
        );
        let engines = client
            .list_engines()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list reasoning engines to check ownership".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;
        Ok(engines.iter().any(|candidate| {
            candidate.name.as_deref().map(last_segment) == Some(engine)
                && candidate
                    .description
                    .as_deref()
                    .is_some_and(|text| crate::setup_owner::description_names_owner(text, &owner))
        }))
    }

    fn owns_engine_deletion(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        let runtime_owned = engine_deletion_is_runtime_owned(
            ctx.state
                .resources
                .get(resource_id)
                .and_then(|resource| resource.lifecycle),
            resource_id,
        )?;
        Ok(runtime_owned || self.created_here)
    }

    /// Creates a controller already holding a ready engine id, for tests that seed it as a
    /// dependency of the template controller.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(engine_id: &str) -> Self {
        Self {
            state: GcpAgentPlatformEngineState::Ready,
            engine_id: Some(engine_id.to_string()),
            pending_operation: None,
            created_here: false,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::{Platform, ResourceLifecycle};
    use alien_gcp_clients::agent_platform::{MockAgentPlatformApi, ReasoningEngine};
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
        build_executor_with(provider, ResourceLifecycle::Live, Default::default()).await
    }

    async fn build_executor_with(
        provider: Arc<MockPlatformServiceProvider>,
        lifecycle: ResourceLifecycle,
        controller: GcpAgentPlatformEngineController,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(GcpAgentPlatformEngine::new("orders-engine".to_string()).build())
            .controller(controller)
            .platform(Platform::Gcp)
            .resource_lifecycle(lifecycle)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds")
    }

    #[tokio::test]
    async fn create_records_the_server_assigned_id_then_deletes_it() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine().returning(|_, _| Ok(pending_op()));
        m.expect_get_operation()
            .returning(|_| Ok(done_engine_op("eng-42")));
        m.expect_delete_engine().times(1).returning(|_| Ok(()));
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

    /// A controller must round-trip by tag: the executor persists and reloads state between
    /// reconciles, and a missing by-tag arm surfaces as an above-the-handler failure with no
    /// per-resource cause to read.
    #[test]
    fn controller_round_trips_by_tag() {
        use crate::core::{deserialize_controller, serialize_controller, ResourceController};

        let controller = GcpAgentPlatformEngineController {
            engine_id: Some("eng-42".to_string()),
            ..Default::default()
        };
        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "GcpAgentPlatformEngineController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    #[tokio::test]
    async fn a_create_failure_lands_in_provision_failed() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine().returning(|_, _| {
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

    fn listed(engine: &str, description: Option<&str>) -> ReasoningEngine {
        ReasoningEngine {
            name: Some(format!(
                "projects/p/locations/us-central1/reasoningEngines/{engine}"
            )),
            display_name: None,
            description: description.map(str::to_string),
            extra: Default::default(),
        }
    }

    /// A Frozen engine belongs to the setup stack unless the cloud object says this deployment
    /// made it. `terraform destroy` removes a package engine, and the controller deleting it
    /// first would take the parent of sessions the stack still owns, so an engine with no
    /// marker (or another deployment's) is never deleted, asserted on the client.
    #[tokio::test]
    async fn a_frozen_engine_without_this_deployments_marker_is_left_alone() {
        for description in [
            None,
            Some("Alien setup owner: other/orders-engine@dep_test."),
            // Same prefix and id, another deployment; and the earlier, deployment-less form.
            Some("Alien setup owner: test/orders-engine@dep_other."),
            Some("Alien setup owner: test/orders-engine."),
            Some("made by hand"),
        ] {
            let mut m = MockAgentPlatformApi::new();
            m.expect_list_engines()
                .returning(move || Ok(vec![listed("eng-42", description)]));
            m.expect_delete_engine().never();

            let mut executor = build_executor_with(
                provider_with(Arc::new(m)),
                ResourceLifecycle::Frozen,
                GcpAgentPlatformEngineController::mock_ready("eng-42"),
            )
            .await;

            executor.delete().expect("delete is accepted");
            executor
                .run_until_terminal()
                .await
                .expect("delete runs to terminal");
            assert_eq!(executor.status(), ResourceStatus::Deleted);
        }
    }

    /// A package-installed engine is never ours, and the management identity cannot list
    /// engines: teardown must make no provider call at all (the mock has no expectation).
    #[tokio::test]
    async fn a_package_installed_engine_is_left_alone_without_any_provider_call() {
        let m = MockAgentPlatformApi::new();
        let mut executor = SingleControllerExecutor::builder()
            .resource(GcpAgentPlatformEngine::new("orders-engine".to_string()).build())
            .controller(GcpAgentPlatformEngineController::mock_ready("eng-42"))
            .platform(Platform::Gcp)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
            .service_provider(provider_with(Arc::new(m)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.delete().expect("delete is accepted");
        executor.run_until_terminal().await.expect("terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// An older manager rewrites the controller state without `created_here`. The marker this
    /// setup wrote on the engine still proves the engine is its own, so teardown removes it.
    #[tokio::test]
    async fn a_frozen_engine_with_this_deployments_marker_is_deleted_when_the_state_lost_its_flag()
    {
        let mut m = MockAgentPlatformApi::new();
        m.expect_list_engines().returning(|| {
            Ok(vec![
                listed(
                    "eng-1",
                    Some("Alien setup owner: test/orders-engine@dep_test."),
                ),
                listed(
                    "eng-42",
                    Some("Alien setup owner: test/orders-engine@dep_test."),
                ),
            ])
        });
        m.expect_delete_engine()
            .withf(|engine| engine == "eng-42")
            .times(1)
            .returning(|_| Ok(()));

        let mut executor = build_executor_with(
            provider_with(Arc::new(m)),
            ResourceLifecycle::Frozen,
            GcpAgentPlatformEngineController::mock_ready("eng-42"),
        )
        .await;

        executor.delete().expect("delete is accepted");
        executor
            .run_until_terminal()
            .await
            .expect("delete runs to terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// The marker is written when the engine is created.
    #[tokio::test]
    async fn the_engine_is_created_with_this_deployments_marker() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine()
            .withf(|_, description| {
                description.as_deref() == Some("Alien setup owner: test/orders-engine@dep_test.")
            })
            .times(1)
            .returning(|_, _| Ok(pending_op()));
        m.expect_get_operation()
            .returning(|_| Ok(done_engine_op("eng-42")));
        let mut executor = build_executor_with(
            provider_with(Arc::new(m)),
            ResourceLifecycle::Frozen,
            Default::default(),
        )
        .await;
        executor.run_until_terminal().await.expect("create runs");
    }

    /// A direct setup has no package to run `terraform destroy`, so the engine its own controller
    /// created is removed by its own teardown, Frozen or not. An imported one (never created
    /// here) is still left to the package, which the test above pins.
    #[tokio::test]
    async fn a_frozen_engine_a_direct_setup_created_is_deleted_by_its_teardown() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_engine().returning(|_, _| Ok(pending_op()));
        m.expect_get_operation()
            .returning(|_| Ok(done_engine_op("eng-42")));
        m.expect_delete_engine()
            .withf(|engine| engine == "eng-42")
            .times(1)
            .returning(|_| Ok(()));

        let mut executor = build_executor_with(
            provider_with(Arc::new(m)),
            ResourceLifecycle::Frozen,
            Default::default(),
        )
        .await;
        executor.run_until_terminal().await.expect("create runs");
        let controller = executor
            .internal_state::<GcpAgentPlatformEngineController>()
            .expect("the controller downcasts");
        assert!(controller.created_here);

        executor.delete().expect("delete is accepted");
        executor
            .run_until_terminal()
            .await
            .expect("delete runs to terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Destroy before the create operation was collected: only the operation name is in state.
    /// The engine the operation produces is deleted, not orphaned.
    #[tokio::test]
    async fn delete_with_only_a_pending_create_operation_deletes_the_resulting_engine() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_get_operation()
            .returning(|_| Ok(done_engine_op("eng-77")));
        m.expect_delete_engine()
            .withf(|engine| engine == "eng-77")
            .times(1)
            .returning(|_| Ok(()));
        let controller = GcpAgentPlatformEngineController {
            pending_operation: Some("projects/p/locations/us-central1/operations/op1".into()),
            ..Default::default()
        };
        let mut executor = build_executor_with(
            provider_with(Arc::new(m)),
            ResourceLifecycle::Frozen,
            controller,
        )
        .await;
        executor.delete().expect("delete is accepted");
        executor.run_until_terminal().await.expect("terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Ownership is read from stack state and nowhere else. A row that records none is refused
    /// rather than guessed: guessing Live destroys a setup-owned engine, which no retry undoes.
    #[test]
    fn teardown_without_a_recorded_lifecycle_is_refused() {
        assert!(
            engine_deletion_is_runtime_owned(Some(ResourceLifecycle::Live), "orders-engine")
                .expect("a live engine is runtime-owned")
        );
        assert!(!engine_deletion_is_runtime_owned(
            Some(ResourceLifecycle::Frozen),
            "orders-engine"
        )
        .expect("a frozen engine is setup-owned"));

        let error = engine_deletion_is_runtime_owned(None, "orders-engine")
            .expect_err("an unrecorded lifecycle names no owner");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID", "{error}");
        assert!(
            error.to_string().contains("orders-engine"),
            "names the engine: {error}"
        );
    }
}
