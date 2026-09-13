//! Azure Sandbox controller.
//!
//! Two planes. ARM creates the sandbox group; the ADC
//! data plane creates sandboxes inside it at runtime. The data plane is gated by `Container Apps
//! SandboxGroup Data Owner`, a role held by the sandbox's *own* execute identity — the boundary
//! `sandbox/execute` exists to hold — and deliberately withheld from this controller, whose
//! `sandbox/provision` grant could otherwise assign itself that role.
//!
//! The group itself is setup-owned: the package creates it, which is what lets a role assignment
//! name it at apply, and `terraform destroy` removes it. This controller adopts the imported
//! group and heartbeats it. It cannot prove the app's data-plane access by probing with its own
//! credential (which lacks Data Owner by design); the execute grant that opens the data plane is
//! authored on the linking resource's permission set by a preflight, not verified here.

use std::time::Duration;

use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox, SandboxEgress, SandboxOutputs,
};
use alien_error::{AlienError, Context};
use alien_macros::controller;

/// Azure Sandbox controller.
#[controller]
pub struct AzureSandboxController {
    /// Sandbox group that scopes every sandbox created from this declaration.
    pub(crate) sandbox_group: Option<String>,
    /// Region the group lives in; the ADC endpoint is per-region.
    pub(crate) region: Option<String>,
    /// Resource group the sandbox group sits in.
    pub(crate) resource_group: Option<String>,
    /// Catalog disk image every sandbox is created from, taken from the declaration's `code`.
    #[serde(default)]
    pub(crate) disk_image: Option<String>,
    /// Outbound policy every sandbox is created with, from the declaration.
    #[serde(default)]
    pub(crate) egress: Option<SandboxEgress>,
    /// Idle seconds after which a sandbox pauses, if the declaration asked for one.
    #[serde(default)]
    pub(crate) idle_pause_seconds: Option<u32>,
}

#[controller]
impl AzureSandboxController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = EnsureGroup,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn ensure_group(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        // Reached only when the import did not seed this controller at Ready, which means the
        // setup package predates the group emitter. Creating one here would take ownership of a
        // resource a regenerated package then creates under the same name.
        let (group, region, resource_group) = self.identity(&config.id).map_err(|_| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "the Azure sandbox group is created by the setup package and arrives \
                          through its import; regenerate the package and rerun setup"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        self.capture_session_inputs(config)?;
        self.sandbox_group = Some(group.clone());
        self.region = Some(region);
        self.resource_group = Some(resource_group);
        info!(sandbox_id = %config.id, %group, "Azure sandbox group adopted");

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
        // Refresh the binding inputs against the declaration, but do not gate the heartbeat on
        // them: a bad `code.image` is a declaration error the create and update paths already
        // fail on, and must not flip a serving sandbox to a terminal RefreshFailed here.
        self.egress = Some(config.egress.clone());
        self.idle_pause_seconds = config.lifecycle.idle_pause_seconds;
        if let Ok(image) = config.azure_catalog_image() {
            self.disk_image = Some(image.to_string());
        }

        // The data plane has no list operation, so the heartbeat carries the group's ARM
        // provisioning state rather than a sandbox count. A read failure here leaves the sandbox
        // serving: the heartbeat is an observation, not a health gate.
        if let Ok((group, _region, resource_group)) = self.identity(&config.id) {
            let mut provisioning_state = None;
            let mut status = alien_core::SandboxHeartbeatStatus::default();

            // sandboxGroups/read is granted, so a failure here is transient rather than a
            // permission gap. Report it as a partial collection instead of a default-Healthy
            // status that would claim a read happened.
            if let Ok(azure_config) = ctx.get_azure_config() {
                if let Ok(arm) = ctx
                    .service_provider
                    .get_azure_sandbox_groups_client(azure_config)
                {
                    match arm.get_sandbox_group(&resource_group, &group).await {
                        Ok(sandbox_group) => {
                            provisioning_state = sandbox_group
                                .properties
                                .and_then(|properties| properties.provisioning_state);
                        }
                        Err(error) => {
                            status.partial = true;
                            status.collection_issues.push(alien_core::HeartbeatCollectionIssue {
                                source: "provisioningState".to_string(),
                                reason: alien_core::HeartbeatCollectionIssueReason::CollectionFailed,
                                severity: alien_core::HeartbeatIssueSeverity::Warning,
                                message: format!(
                                    "could not read the sandbox group's ARM state: {error}"
                                ),
                            });
                        }
                    }
                }
            }

            ctx.emit_heartbeat(alien_core::ResourceHeartbeat {
                deployment_id: None,
                resource_id: config.id.clone(),
                resource_type: Sandbox::RESOURCE_TYPE,
                controller_platform: alien_core::Platform::Azure,
                backend: alien_core::HeartbeatBackend::Azure,
                observed_at: chrono::Utc::now(),
                data: alien_core::ResourceHeartbeatData::Sandbox(
                    alien_core::SandboxHeartbeatData::AzureSandboxGroup(
                        alien_core::AzureSandboxGroupHeartbeatData {
                            status,
                            sandbox_group: group,
                            provisioning_state,
                        },
                    ),
                ),
                raw: vec![],
            });
        }

        debug!(sandbox_id = %config.id, "Azure sandbox ready");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
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
        self.capture_session_inputs(config)?;
        info!(sandbox_id = %config.id, "Updated Azure sandbox configuration");

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
    async fn deleting(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // The group is the setup stack's to destroy, and destroying it takes its sandboxes with
        // it. A delete here would race `terraform destroy` and fail teardown on the 404 whichever
        // call lost; the runtime's own grant does not carry the delete in any case.
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

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, SandboxBinding};

        // disk_image and egress are non-optional on the binding, so a sandbox without them
        // publishes nothing yet rather than a sandbox with no image and open egress.
        let (Some(group), Some(region), Some(resource_group), Some(disk_image), Some(egress)) = (
            self.sandbox_group.as_ref(),
            self.region.as_ref(),
            self.resource_group.as_ref(),
            self.disk_image.as_ref(),
            self.egress.as_ref(),
        ) else {
            return Ok(None);
        };

        // The data plane is per-region and separate from ARM; a caller cannot derive it from
        // the Azure client config, so the binding carries it.
        let binding = SandboxBinding::azure(
            BindingValue::value(group.clone()),
            BindingValue::value(format!("https://management.{region}.azuredevcompute.io")),
            BindingValue::value(region.clone()),
            BindingValue::value(resource_group.clone()),
            BindingValue::value(disk_image.clone()),
            egress.clone(),
            self.idle_pause_seconds,
        );

        Ok(Some(serde_json::to_value(binding).map_err(|error| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("failed to serialize the sandbox binding: {error}"),
                resource_id: None,
            })
        })?))
    }

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.sandbox_group.as_ref().map(|group| {
            CoreResourceOutputs::new(SandboxOutputs {
                parent_name: group.clone(),
                identifier: self.resource_group.clone(),
                endpoint: self
                    .region
                    .as_ref()
                    .map(|region| format!("https://management.{region}.azuredevcompute.io")),
            })
        })
    }
}

impl AzureSandboxController {
    /// Captures the image, egress and idle-pause the binding carries from the declaration.
    ///
    /// The data plane takes them only at sandbox-create and `get_binding_params` has no config to
    /// read, so they live in state; a change to any must reach the binding on the next reconcile.
    pub(crate) fn capture_session_inputs(&mut self, config: &Sandbox) -> Result<()> {
        // Egress and idle-pause are set before the fallible image lookup so a partial capture on
        // a bad declaration errs tight rather than leaving a stale, looser egress behind.
        self.egress = Some(config.egress.clone());
        self.idle_pause_seconds = config.lifecycle.idle_pause_seconds;
        self.disk_image = Some(
            config
                .azure_catalog_image()
                .context(ErrorData::CloudPlatformError {
                    message: "sandbox code.image is not a valid Azure catalog image".to_string(),
                    resource_id: Some(config.id.clone()),
                })?
                .to_string(),
        );
        Ok(())
    }

    fn identity(&self, sandbox_id: &str) -> Result<(String, String, String)> {
        match (
            self.sandbox_group.clone(),
            self.region.clone(),
            self.resource_group.clone(),
        ) {
            (Some(group), Some(region), Some(resource_group)) => {
                Ok((group, region, resource_group))
            }
            // A half import is refused rather than filled in: deriving the missing parts would
            // address a different group from the one setup created, and both would provision.
            _ => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "no complete sandbox group recorded: group, region and resource group \
                          are all needed before sandboxes can be created"
                    .to_string(),
                resource_id: Some(sandbox_id.to_string()),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{
        deserialize_controller, serialize_controller, MockPlatformServiceProvider,
        ResourceController, ResourceRegistry,
    };
    use alien_core::{Platform, ResourceLifecycle, SandboxCode, SandboxLifecyclePolicy};
    use std::sync::Arc;

    /// The ADC data plane is per-region and separate from ARM, so a caller cannot derive the
    /// endpoint from its Azure client config — the binding has to carry it.
    #[test]
    fn the_binding_carries_the_regional_data_plane_endpoint() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("westus2".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            egress: Some(SandboxEgress::Deny),
            idle_pause_seconds: Some(300),
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a fully imported sandbox group publishes a binding");

        assert_eq!(
            params["dataPlaneEndpoint"],
            "https://management.westus2.azuredevcompute.io"
        );
        assert_eq!(params["resourceGroup"], "rg");
        assert_eq!(params["diskImage"], "ubuntu");
        assert_eq!(params["idlePauseSeconds"], 300);
        // The service tag and egress shape are what the runtime provider dispatches and reads on,
        // so they are pinned here rather than trusted to round-trip silently.
        assert_eq!(params["service"], "sandbox-azure");
        assert_eq!(params["egress"]["mode"], "deny");
    }

    /// Teardown must not touch the group: the setup stack destroys it, and a delete here would
    /// race that. The provider carries no client expectation, so any ARM lookup fails the test.
    #[tokio::test]
    async fn teardown_leaves_the_setup_owned_group_alone() {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("westus2".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            egress: Some(SandboxEgress::Allow),
            idle_pause_seconds: None,
            _internal_stay_count: None,
        };

        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox)
            .controller(controller)
            .platform(Platform::Azure)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .await
            .expect("executor should build");

        executor.delete().expect("transition to delete");
        while executor.status() != ResourceStatus::Deleted {
            executor.step().await.expect("teardown makes no cloud call");
        }
    }

    #[test]
    fn controller_round_trips_by_tag() {
        let controller = AzureSandboxController {
            sandbox_group: Some("sbg1".to_string()),
            region: Some("westus2".to_string()),
            resource_group: Some("rg".to_string()),
            ..Default::default()
        };

        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "AzureSandboxController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    /// A saved row is the only input this controller ever resumes from, and serde names it by
    /// field. Building the JSON by hand is the point: a serialize-then-deserialize pass agrees
    /// with itself through a rename, and the field would land as `None` with nothing to read.
    #[test]
    fn a_persisted_row_loads_every_field() {
        let restored: AzureSandboxController = serde_json::from_value(serde_json::json!({
            "state": "ready",
            "sandboxGroup": "sbg",
            "region": "westus2",
            "resourceGroup": "rg",
            "diskImage": "ubuntu",
            "egress": { "mode": "allowDomains", "domains": ["api.example.com"] },
            "idlePauseSeconds": 300,
        }))
        .expect("a persisted sandbox row deserializes");

        assert!(matches!(restored.state, AzureSandboxState::Ready));
        assert_eq!(restored.sandbox_group.as_deref(), Some("sbg"));
        assert_eq!(restored.region.as_deref(), Some("westus2"));
        assert_eq!(restored.resource_group.as_deref(), Some("rg"));
        assert_eq!(restored.disk_image.as_deref(), Some("ubuntu"));
        assert_eq!(
            restored.egress,
            Some(SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            })
        );
        assert_eq!(restored.idle_pause_seconds, Some(300));
    }

    /// Resolving a controller for a new deployment is a different path from deserializing
    /// saved state, so registering one does not imply the other.
    #[test]
    fn the_registry_resolves_an_azure_sandbox_controller() {
        let registry = ResourceRegistry::with_built_ins();

        let controller = registry
            .get_controller(Sandbox::RESOURCE_TYPE, Platform::Azure)
            .expect("Azure must have a registered Sandbox controller");
        assert_eq!(controller.controller_type(), "AzureSandboxController");
    }

    /// A partially-imported parent is refused rather than half-used: the ADC endpoint is
    /// per-region, so a group without its region cannot be addressed at all.
    #[test]
    fn an_incomplete_import_is_refused_with_a_message_naming_what_is_missing() {
        let controller = AzureSandboxController {
            sandbox_group: Some("sbg1".to_string()),
            region: None,
            resource_group: Some("rg".to_string()),
            ..Default::default()
        };

        let error = controller
            .identity("agent")
            .expect_err("a group without its region cannot be addressed");
        assert!(
            error.to_string().contains("region"),
            "the refusal must name what is missing: {error}"
        );
    }

    /// `AllowDomains` is the one egress variant carrying a payload, and Azure is the backend that
    /// expresses it, so the tagged shape and the domain list are pinned rather than trusted.
    #[test]
    fn the_binding_carries_the_allow_domains_egress_shape() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("westus2".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            egress: Some(SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            }),
            idle_pause_seconds: None,
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a fully imported sandbox group publishes a binding");

        assert_eq!(params["egress"]["mode"], "allowDomains");
        assert_eq!(params["egress"]["domains"][0], "api.example.com");
    }

    /// A row without the image, egress and idle fields loads, and publishes no binding until a
    /// reconcile captures them — the fields are not optional on the binding.
    #[test]
    fn state_without_the_session_fields_loads_and_publishes_no_binding() {
        let mut value = serde_json::to_value(AzureSandboxController {
            sandbox_group: Some("sbg".to_string()),
            region: Some("westus2".to_string()),
            resource_group: Some("rg".to_string()),
            ..Default::default()
        })
        .expect("serializes");
        let object = value.as_object_mut().expect("controller is an object");
        object.remove("diskImage");
        object.remove("egress");
        object.remove("idlePauseSeconds");

        let restored: AzureSandboxController =
            serde_json::from_value(value).expect("state without the fields deserializes");
        assert_eq!(restored.disk_image, None);
        assert_eq!(restored.egress, None);
        assert!(
            restored
                .get_binding_params()
                .expect("binding params")
                .is_none(),
            "no binding is published until the sandbox fields are captured"
        );
    }
}
