//! GCP Agent Platform sandbox template controller.
//!
//! Reconciles the `SandboxEnvironmentTemplate` (T09): the Live, release-owned object that carries
//! the image digest, ceilings and egress and warms the session pool. The Agent Engine it hangs
//! under is Frozen setup and is not touched here — the controller is handed the engine and creates
//! templates beneath it.
//!
//! Template config is immutable: there is no update verb, so reconciliation is replace-not-update.
//! A changed image (or any field that lands in the template body) creates a new template, waits for
//! it to become `ACTIVE`, and only then reaps the old one — so a release never leaves a session
//! pointing at a template that has already been deleted.
//!
//! Unregistered like the provider it feeds (T05): the registered GCP sandbox backend is still Cloud
//! Run, so no declaration reaches this and it is proven by the controller tests below until the
//! cutover moves the registration.

use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Sandbox, SandboxCode, SandboxLimits};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::agent_platform::{
    ContainerResources, CustomContainerEnvironment, CustomContainerSpec, EgressControlConfig,
    SandboxEnvironmentTemplate,
};
use alien_gcp_clients::longrunning::OperationResult;
use alien_macros::controller;

/// Lifecycle state the API reports for a template that is ready to cut sessions from.
const TEMPLATE_ACTIVE: &str = "ACTIVE";

/// Last path segment of a resource name — the id the client interpolates back into its paths.
fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The fields of a template that, when changed, force a replace.
///
/// The template is immutable, so any of these differing between the desired and previous
/// declaration means the old template cannot be updated in place — it is torn down and rebuilt.
/// The image is the digest the spec names; the ceilings and egress are here because they are baked
/// into the same immutable body.
fn template_identity(sandbox: &Sandbox) -> Result<(String, SandboxLimits, bool)> {
    let image = match &sandbox.code {
        SandboxCode::Image { image } => image.clone(),
        SandboxCode::Source { .. } => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no sandbox backend builds an image from source; give code.image a \
                          prebuilt reference"
                    .to_string(),
                resource_id: Some(sandbox.id().to_string()),
            }));
        }
    };
    let internet_access = internet_access_or_refuse(sandbox)?;
    Ok((image, sandbox.resolved_limits(), internet_access))
}

/// The egress switch, or a refusal naming the sandbox and both accepted modes.
///
/// `AllowDomains` has no representation in the single internet-access switch, so it is refused
/// rather than approximated. The mode→switch mapping is `SandboxEgress::internet_access_switch`, so
/// this cannot disagree with the emitter or the provider on what a mode means.
fn internet_access_or_refuse(sandbox: &Sandbox) -> Result<bool> {
    sandbox.egress.internet_access_switch().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "sandbox '{}' asked for domain-scoped egress, which Agent Platform cannot \
                 express; it offers only 'allow' (open) and 'deny' (closed)",
                sandbox.id()
            ),
            resource_id: Some(sandbox.id().to_string()),
        })
    })
}

/// Builds the immutable template body from the declaration.
fn build_template_body(
    sandbox: &Sandbox,
    display_name: &str,
) -> Result<SandboxEnvironmentTemplate> {
    let (image, limits, internet_access) = template_identity(sandbox)?;

    // cpu and memory are the ceilings the API's resource map expresses; disk and max_processes have
    // no field on this template and are enforced by the runtime tier instead.
    let resources = ContainerResources {
        requests: None,
        limits: Some(HashMap::from([
            ("cpu".to_string(), limits.cpu),
            ("memory".to_string(), limits.memory),
        ])),
    };

    Ok(SandboxEnvironmentTemplate {
        name: None,
        display_name: Some(display_name.to_string()),
        custom_container_environment: Some(CustomContainerEnvironment {
            custom_container_spec: Some(CustomContainerSpec {
                image_uri: image,
                extra: Default::default(),
            }),
            resources: Some(resources),
            ports: vec![],
            extra: Default::default(),
        }),
        egress_control_config: Some(EgressControlConfig {
            internet_access: Some(internet_access),
            extra: Default::default(),
        }),
        state: None,
        extra: Default::default(),
    })
}

#[controller]
pub struct GcpAgentPlatformTemplateController {
    /// Reasoning-engine id the template is created under, as the client's path interpolation wants
    /// it (a bare segment).
    pub(crate) engine: Option<String>,
    /// The `ACTIVE` template sessions are currently cut from (last path segment).
    pub(crate) template_id: Option<String>,
    /// The create long-running operation being polled to learn a new template's id.
    pub(crate) pending_operation: Option<String>,
    /// A template being brought to `ACTIVE` before it replaces `template_id`. During a replace the
    /// old template keeps serving until this one is live.
    pub(crate) pending_template_id: Option<String>,
    /// Project the engine lives in, kept for the binding the provider reads.
    pub(crate) project_id: Option<String>,
    /// Region selecting the regional endpoint, kept for the binding.
    pub(crate) region: Option<String>,
    /// Session lifetime from the declaration, carried into the binding.
    pub(crate) session_ttl_seconds: Option<u32>,
}

#[controller]
impl GcpAgentPlatformTemplateController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        // The concrete engine id is server-assigned at setup; until the cutover wires the real one
        // through, it is addressed by a stable per-sandbox convention. The controller is
        // unregistered, so nothing depends on this reaching a live engine yet.
        let engine = format!("{}-{}", ctx.resource_prefix, config.id);
        let display_name = format!("{}-{}", ctx.resource_prefix, config.id);
        let body = build_template_body(config, &display_name)?;

        self.engine = Some(engine.clone());
        self.project_id = Some(gcp_config.project_id.clone());
        self.region = Some(gcp_config.region.clone());
        self.session_ttl_seconds = config.session.max_lifetime_seconds;

        info!(id=%config.id, engine=%engine, "Creating sandbox environment template");
        let operation =
            client
                .create_template(&engine, body)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create sandbox template under engine '{engine}'"),
                    resource_id: Some(config.id.clone()),
                })?;

        self.pending_operation = Some(require_operation_name(operation.name, &config.id)?);
        Ok(HandlerAction::Continue {
            state: AwaitingTemplateOperation,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = AwaitingTemplateOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn awaiting_template_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        let op_name = self.pending_operation.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no pending template operation in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let operation =
            client
                .get_operation(&op_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to poll template operation '{op_name}'"),
                    resource_id: Some(config.id.clone()),
                })?;

        if operation.done != Some(true) {
            return Ok(HandlerAction::Stay {
                max_times: Some(150),
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let template = match operation.result {
            Some(OperationResult::Response { response }) => serde_json::from_value::<
                SandboxEnvironmentTemplate,
            >(response)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "template create operation returned an unreadable resource".to_string(),
                resource_id: Some(config.id.clone()),
            })?,
            Some(OperationResult::Error { error }) => {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "template create failed: {} (grpc {})",
                        error.message, error.code
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
            None => {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "template create operation reported done without a result".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let name = template.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "created template carried no resource name".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        self.pending_template_id = Some(last_segment(&name).to_string());
        self.pending_operation = None;

        Ok(HandlerAction::Continue {
            state: AwaitingTemplateActive,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = AwaitingTemplateActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn awaiting_template_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        let (engine, pending) = self.engine_and_pending(&config.id)?;

        let template = client.get_template(&engine, &pending).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to read template '{pending}' while waiting for ACTIVE"),
                resource_id: Some(config.id.clone()),
            },
        )?;

        if template.state.as_deref() != Some(TEMPLATE_ACTIVE) {
            return Ok(HandlerAction::Stay {
                max_times: Some(150),
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        // The new template is live; only now does it become the serving one, so the reap that
        // follows can delete the old without a window where sessions point at a deleted template.
        self.template_id = Some(pending);
        self.pending_template_id = None;

        Ok(HandlerAction::Continue {
            state: ReapingOldTemplates,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ReapingOldTemplates,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn reaping_old_templates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        let (engine, serving) = self.engine_and_template(&config.id)?;

        let templates =
            client
                .list_templates(&engine)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to list templates under engine '{engine}' to reap old ones"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

        for template in templates {
            let Some(name) = template.name.as_deref() else {
                continue;
            };
            let id = last_segment(name);
            if id == serving {
                continue;
            }
            // Best-effort: the new template already serves, so a straggler left by a transient
            // delete failure is cost, not a correctness break — the next reconcile reaps it. A
            // hard error here must not fail an update whose replacement is already live.
            if let Err(e) = client.delete_template(&engine, id).await {
                warn!(engine=%engine, template=%id, error=%e, "could not reap an old template, leaving it for the next reconcile");
            } else {
                info!(engine=%engine, template=%id, "reaped an old sandbox template");
            }
        }

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
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        let (engine, template_id) = self.engine_and_template(&config.id)?;

        // On this platform a resource's state is not a health signal for the sessions cut from it —
        // a session can be dead while everything here reads healthy. For the template itself the
        // lifecycle state is the only signal there is, so the heartbeat confirms exactly that and
        // claims nothing more.
        let template = client.get_template(&engine, &template_id).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to read template '{template_id}' during heartbeat"),
                resource_id: Some(config.id.clone()),
            },
        )?;

        if template.state.as_deref() != Some(TEMPLATE_ACTIVE) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: format!(
                    "template '{template_id}' is no longer ACTIVE (state '{}')",
                    template.state.as_deref().unwrap_or("<unset>")
                ),
            }));
        }

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
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let previous = ctx.previous_resource_config::<Sandbox>()?;

        // The template is immutable, so an unchanged body needs no work and a changed one is a
        // replace, never an in-place edit.
        if template_identity(config)? == template_identity(previous)? {
            info!(id=%config.id, "sandbox template unchanged; nothing to replace");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;
        let engine = self.engine.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no engine in state to replace the template under".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let display_name = format!("{}-{}", ctx.resource_prefix, config.id);
        let body = build_template_body(config, &display_name)?;

        info!(id=%config.id, "sandbox template body changed; creating a replacement");
        let operation =
            client
                .create_template(&engine, body)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create replacement template under engine '{engine}'"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

        // The old template stays in `template_id` and keeps serving; the reap after ACTIVE removes
        // it. Routing through the create flow's await states keeps one mutable op per state.
        self.pending_operation = Some(require_operation_name(operation.name, &config.id)?);
        Ok(HandlerAction::Continue {
            state: AwaitingTemplateOperation,
            suggested_delay: Some(Duration::from_secs(2)),
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
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let Some(engine) = self.engine.clone() else {
            // Nothing was ever created — a delete with no parent is already done.
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        // Best-effort and idempotent: delete_template treats a not-found as success, and both the
        // serving and any half-created template are torn down so a failed create leaves nothing.
        for template_id in [self.template_id.clone(), self.pending_template_id.clone()]
            .into_iter()
            .flatten()
        {
            if let Err(e) = client.delete_template(&engine, &template_id).await {
                warn!(engine=%engine, template=%template_id, error=%e, "could not delete a template during teardown, continuing");
            }
        }

        self.clear_state();
        info!(id=%config.id, "sandbox template teardown complete");
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
        None
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, SandboxBinding};

        let (Some(engine), Some(template_id), Some(project), Some(region)) = (
            &self.engine,
            &self.template_id,
            &self.project_id,
            &self.region,
        ) else {
            return Ok(None);
        };

        let engine_name =
            format!("projects/{project}/locations/{region}/reasoningEngines/{engine}");
        let template_name = format!("{engine_name}/sandboxEnvironmentTemplates/{template_id}");
        let binding = SandboxBinding::gcp_agent_platform(
            BindingValue::value(engine_name),
            BindingValue::value(template_name),
            BindingValue::value(region.clone()),
            self.session_ttl_seconds,
        );
        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize sandbox binding parameters".to_string(),
                },
            )?,
        ))
    }
}

/// Requires a long-running operation to carry a name to poll — a nameless one cannot be resumed.
fn require_operation_name(name: Option<String>, resource_id: &str) -> Result<String> {
    name.ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "template operation carried no name to poll".to_string(),
            resource_id: Some(resource_id.to_string()),
        })
    })
}

impl GcpAgentPlatformTemplateController {
    fn clear_state(&mut self) {
        self.engine = None;
        self.template_id = None;
        self.pending_operation = None;
        self.pending_template_id = None;
        self.project_id = None;
        self.region = None;
        self.session_ttl_seconds = None;
    }

    fn engine_and_template(&self, resource_id: &str) -> Result<(String, String)> {
        let engine = self
            .engine
            .clone()
            .ok_or_else(|| missing_state(resource_id, "engine"))?;
        let template_id = self
            .template_id
            .clone()
            .ok_or_else(|| missing_state(resource_id, "template id"))?;
        Ok((engine, template_id))
    }

    fn engine_and_pending(&self, resource_id: &str) -> Result<(String, String)> {
        let engine = self
            .engine
            .clone()
            .ok_or_else(|| missing_state(resource_id, "engine"))?;
        let pending = self
            .pending_template_id
            .clone()
            .ok_or_else(|| missing_state(resource_id, "pending template id"))?;
        Ok((engine, pending))
    }

    /// Creates a controller already serving an ACTIVE template, for update-flow tests.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(engine: &str, template_id: &str) -> Self {
        Self {
            state: GcpAgentPlatformTemplateState::Ready,
            engine: Some(engine.to_string()),
            template_id: Some(template_id.to_string()),
            pending_operation: None,
            pending_template_id: None,
            project_id: Some("test-project-123".to_string()),
            region: Some("us-central1".to_string()),
            session_ttl_seconds: None,
            _internal_stay_count: None,
        }
    }
}

fn missing_state(resource_id: &str, field: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!("controller state is missing the {field}"),
        resource_id: Some(resource_id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::Platform;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::{SandboxEgress, SandboxSessionPolicy};
    use alien_gcp_clients::agent_platform::MockAgentPlatformApi;
    use alien_gcp_clients::longrunning::{Operation, OperationResult};
    use std::sync::{Arc, Mutex};

    fn sandbox_with(
        egress: SandboxEgress,
        image: &str,
        ttl: Option<u32>,
        limits: Option<SandboxLimits>,
    ) -> Sandbox {
        let builder = Sandbox::new("agent-sbx".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: ttl,
                idle_suspend_seconds: None,
            });
        match limits {
            Some(limits) => builder.limits(limits).build(),
            None => builder.build(),
        }
    }

    /// A create long-running operation, still pending — the controller only reads its name here.
    fn pending_op() -> Operation {
        Operation {
            name: Some("projects/p/locations/us-central1/operations/op1".to_string()),
            metadata: None,
            done: Some(false),
            result: None,
        }
    }

    /// A completed create operation whose response carries the new template's resource name.
    fn done_op(template_id: &str) -> Operation {
        Operation {
            name: Some("projects/p/locations/us-central1/operations/op1".to_string()),
            metadata: None,
            done: Some(true),
            result: Some(OperationResult::Response {
                response: serde_json::json!({
                    "name": format!(
                        "projects/p/locations/us-central1/reasoningEngines/eng/sandboxEnvironmentTemplates/{template_id}"
                    ),
                    "state": "CREATING"
                }),
            }),
        }
    }

    fn active_template(template_id: &str) -> SandboxEnvironmentTemplate {
        SandboxEnvironmentTemplate {
            name: Some(format!(
                "projects/p/locations/us-central1/reasoningEngines/eng/sandboxEnvironmentTemplates/{template_id}"
            )),
            display_name: None,
            custom_container_environment: None,
            egress_control_config: None,
            state: Some(TEMPLATE_ACTIVE.to_string()),
            extra: Default::default(),
        }
    }

    /// A mock that carries one sandbox from create through a heartbeat and a clean delete.
    fn happy_client() -> Arc<MockAgentPlatformApi> {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_template()
            .returning(|_, _| Ok(pending_op()));
        m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
        m.expect_get_template()
            .returning(|_, id| Ok(active_template(id)));
        m.expect_list_templates()
            .returning(|_| Ok(vec![active_template("tpl1")]));
        m.expect_delete_template().returning(|_, _| Ok(()));
        Arc::new(m)
    }

    fn provider_with(client: Arc<MockAgentPlatformApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_agent_platform_client()
            .returning(move |_| Ok(client.clone()));
        Arc::new(provider)
    }

    async fn build_executor(
        resource: Sandbox,
        provider: Arc<MockPlatformServiceProvider>,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(resource)
            .controller(GcpAgentPlatformTemplateController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds")
    }

    // ---- 1. Create and delete flow, across config variants. -----------------------------------

    async fn create_then_delete(resource: Sandbox) {
        let provider = provider_with(happy_client());
        let mut executor = build_executor(resource, provider).await;

        executor
            .run_until_terminal()
            .await
            .expect("create runs to a steady state");
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "an ACTIVE template leaves the controller Running"
        );

        let controller = executor
            .internal_state::<GcpAgentPlatformTemplateController>()
            .expect("the controller downcasts");
        assert_eq!(
            controller.template_id.as_deref(),
            Some("tpl1"),
            "the ACTIVE template id is the serving one"
        );

        executor.delete().expect("delete is accepted");
        executor
            .run_until_terminal()
            .await
            .expect("delete runs to terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn create_delete_deny_egress_default_limits() {
        create_then_delete(sandbox_with(
            SandboxEgress::Deny,
            "ubuntu:24.04",
            None,
            None,
        ))
        .await;
    }

    #[tokio::test]
    async fn create_delete_allow_egress() {
        create_then_delete(sandbox_with(
            SandboxEgress::Allow,
            "ubuntu:24.04",
            None,
            None,
        ))
        .await;
    }

    #[tokio::test]
    async fn create_delete_with_session_ttl() {
        create_then_delete(sandbox_with(
            SandboxEgress::Deny,
            "ubuntu:24.04",
            Some(3600),
            None,
        ))
        .await;
    }

    #[tokio::test]
    async fn create_delete_with_explicit_limits() {
        let limits = SandboxLimits {
            cpu: "2".to_string(),
            memory: "4Gi".to_string(),
            disk: "20Gi".to_string(),
            max_processes: None,
        };
        create_then_delete(sandbox_with(
            SandboxEgress::Allow,
            "ghcr.io/org/sbx:v1",
            Some(1800),
            Some(limits),
        ))
        .await;
    }

    // ---- 1b. The binding the provider will read. ----------------------------------------------

    #[tokio::test]
    async fn binding_params_carry_the_active_template_region_and_ttl() {
        let provider = provider_with(happy_client());
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::Deny, "ubuntu:24.04", Some(3600), None),
            provider,
        )
        .await;
        executor.run_until_terminal().await.expect("create runs");

        use crate::core::ResourceController;
        let params = executor
            .internal_state::<GcpAgentPlatformTemplateController>()
            .expect("downcasts")
            .get_binding_params()
            .expect("binding serializes")
            .expect("a running template has a binding");
        let binding: alien_core::bindings::SandboxBinding =
            serde_json::from_value(params).expect("binding parses back to the T07 type");

        match binding {
            alien_core::bindings::SandboxBinding::GcpAgentPlatform(b) => {
                let region = b
                    .region
                    .into_value("gcp-agent-platform", "region")
                    .expect("region is a literal in a test");
                assert_eq!(region, "us-central1");
                assert_eq!(b.session_ttl_seconds, Some(3600));
                let template = b
                    .template
                    .into_value("gcp-agent-platform", "template")
                    .expect("template is a literal in a test");
                assert!(
                    template.ends_with("/sandboxEnvironmentTemplates/tpl1"),
                    "the binding points at the ACTIVE template: {template}"
                );
            }
            other => panic!("expected a GCP Agent Platform binding, got {other:?}"),
        }
    }

    // ---- 2. Update flow: no-op when the body is unchanged. -------------------------------------

    #[tokio::test]
    async fn update_with_unchanged_body_creates_no_template() {
        let mut m = MockAgentPlatformApi::new();
        // The heartbeat still reads the template; a replace would create, and it must not.
        m.expect_get_template()
            .returning(|_, id| Ok(active_template(id)));
        m.expect_create_template().never();
        let provider = provider_with(Arc::new(m));

        let resource = sandbox_with(SandboxEgress::Deny, "ubuntu:24.04", None, None);
        let mut executor = SingleControllerExecutor::builder()
            .resource(resource.clone())
            .controller(GcpAgentPlatformTemplateController::mock_ready(
                "eng", "tpl1",
            ))
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");

        executor.update(resource).expect("update accepted");
        executor.run_until_terminal().await.expect("update runs");
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            executor
                .internal_state::<GcpAgentPlatformTemplateController>()
                .expect("downcasts")
                .template_id
                .as_deref(),
            Some("tpl1"),
            "an unchanged body keeps the original template"
        );
    }

    // ---- 3. Replace on image change: new template ACTIVE before the old is reaped. -------------

    #[tokio::test]
    async fn image_change_replaces_the_template_reaping_the_old_after_active() {
        // Records call order so the ordering guard is checked, not just the end state.
        let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let mut m = MockAgentPlatformApi::new();
        m.expect_create_template()
            .returning(|_, _| Ok(pending_op()));
        m.expect_get_operation().returning(|_| Ok(done_op("tpl2")));
        {
            let calls = calls.clone();
            m.expect_get_template().returning(move |_, id| {
                if id == "tpl2" {
                    calls.lock().unwrap().push("active:tpl2".to_string());
                }
                Ok(active_template(id))
            });
        }
        m.expect_list_templates()
            .returning(|_| Ok(vec![active_template("tpl1"), active_template("tpl2")]));
        {
            let calls = calls.clone();
            m.expect_delete_template().returning(move |_, id| {
                calls.lock().unwrap().push(format!("delete:{id}"));
                Ok(())
            });
        }
        let provider = provider_with(Arc::new(m));

        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with(SandboxEgress::Deny, "old:v1", None, None))
            .controller(GcpAgentPlatformTemplateController::mock_ready(
                "eng", "tpl1",
            ))
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");

        executor
            .update(sandbox_with(SandboxEgress::Deny, "new:v2", None, None))
            .expect("update accepted");
        executor.run_until_terminal().await.expect("replace runs");
        assert_eq!(executor.status(), ResourceStatus::Running);

        assert_eq!(
            executor
                .internal_state::<GcpAgentPlatformTemplateController>()
                .expect("downcasts")
                .template_id
                .as_deref(),
            Some("tpl2"),
            "the new template is now the serving one"
        );

        let calls = calls.lock().unwrap();
        let active_at = calls
            .iter()
            .position(|c| c == "active:tpl2")
            .expect("the new template was confirmed ACTIVE");
        let delete_at = calls
            .iter()
            .position(|c| c == "delete:tpl1")
            .expect("the old template was reaped");
        assert!(
            active_at < delete_at,
            "the old template must be reaped only AFTER the new one is ACTIVE; order was {calls:?}"
        );
    }

    // ---- 4. Best-effort deletion: teardown succeeds even when a delete errors. -----------------

    #[tokio::test]
    async fn delete_is_best_effort_when_the_api_errors() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_get_template()
            .returning(|_, id| Ok(active_template(id)));
        m.expect_delete_template().returning(|_, _| {
            Err(AlienError::new(
                alien_gcp_clients::agent_platform::AgentPlatformErrorData::RequestFailed {
                    operation: "delete template".to_string(),
                    message: "persistent failure".to_string(),
                },
            ))
        });
        let provider = provider_with(Arc::new(m));

        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with(
                SandboxEgress::Deny,
                "ubuntu:24.04",
                None,
                None,
            ))
            .controller(GcpAgentPlatformTemplateController::mock_ready(
                "eng", "tpl1",
            ))
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");

        executor.delete().expect("delete accepted");
        executor
            .run_until_terminal()
            .await
            .expect("a failing template delete does not fail teardown");
        assert_eq!(
            executor.status(),
            ResourceStatus::Deleted,
            "deletion is best-effort: a straggler is left for a sweep, teardown still completes"
        );
    }

    #[tokio::test]
    async fn delete_before_anything_created_is_already_done() {
        let m = MockAgentPlatformApi::new();
        let provider = provider_with(Arc::new(m));
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::Deny, "ubuntu:24.04", None, None),
            provider,
        )
        .await;

        executor.delete().expect("delete accepted");
        executor
            .run_until_terminal()
            .await
            .expect("deleting a never-created template is a no-op");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    // ---- 5. Validation: the body carried, and the egress refusal. ------------------------------

    #[tokio::test]
    async fn create_carries_the_declared_image_limits_and_egress() {
        let mut m = MockAgentPlatformApi::new();
        m.expect_create_template()
            .withf(|_engine, template| {
                let env = template
                    .custom_container_environment
                    .as_ref()
                    .expect("template carries a container environment");
                let image = env
                    .custom_container_spec
                    .as_ref()
                    .expect("a container spec")
                    .image_uri
                    .as_str();
                let limits = env
                    .resources
                    .as_ref()
                    .and_then(|r| r.limits.as_ref())
                    .expect("cpu/memory limits");
                let internet = template
                    .egress_control_config
                    .as_ref()
                    .and_then(|e| e.internet_access);
                image == "ghcr.io/org/sbx:v9"
                    && limits.get("cpu").map(String::as_str) == Some("2")
                    && limits.get("memory").map(String::as_str) == Some("4Gi")
                    && internet == Some(true)
            })
            .returning(|_, _| Ok(pending_op()));
        m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
        m.expect_get_template()
            .returning(|_, id| Ok(active_template(id)));
        m.expect_list_templates()
            .returning(|_| Ok(vec![active_template("tpl1")]));
        let provider = provider_with(Arc::new(m));

        let limits = SandboxLimits {
            cpu: "2".to_string(),
            memory: "4Gi".to_string(),
            disk: "20Gi".to_string(),
            max_processes: None,
        };
        let mut executor = build_executor(
            sandbox_with(
                SandboxEgress::Allow,
                "ghcr.io/org/sbx:v9",
                None,
                Some(limits),
            ),
            provider,
        )
        .await;
        executor
            .run_until_terminal()
            .await
            .expect("create runs with the asserted body");
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Domain-scoped egress has no representation in the single switch, so the template body build
    /// refuses it naming the sandbox and both accepted modes — it is never approximated.
    #[test]
    fn build_template_body_refuses_domain_egress_naming_the_sandbox_and_modes() {
        let sandbox = sandbox_with(
            SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            },
            "ubuntu:24.04",
            None,
            None,
        );
        let error = build_template_body(&sandbox, "agent-sbx")
            .expect_err("a hostname list has no representation on Agent Platform");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID", "{error}");
        let rendered = error.to_string();
        assert!(
            rendered.contains("agent-sbx"),
            "names the sandbox: {rendered}"
        );
        assert!(
            rendered.contains("allow") && rendered.contains("deny"),
            "names both accepted modes: {rendered}"
        );
    }
}
