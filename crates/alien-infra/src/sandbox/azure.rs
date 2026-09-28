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
//! group and heartbeats it. It cannot prove the linking resource's data-plane access by probing
//! with its own credential (which lacks Data Owner by design); the execute grant that opens the
//! data plane is authored on that resource's permission set by a preflight, not verified here.
//!
//! The one data-plane object it owns is the disk image a registry image is built into, and the one
//! a changed reference replaced, which it deletes. `sandbox/images` grants exactly those verbs.

use std::time::Duration;

use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::azure::sandbox_data_plane::{
    CreateDiskImage, DiskImage, SandboxDataPlaneApi,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    azure_disk_image_label, AzureSandboxImage, ResourceOutputs as CoreResourceOutputs,
    ResourceStatus, Sandbox, SandboxEgress, SandboxLimits, SandboxOutputs, AZURE_DISK_IMAGE_LABEL,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// A disk image builds in 10-30s. 60 polls at this interval is a 5-minute ceiling, an order of
/// magnitude past a healthy build, so reaching it means the build is wedged rather than slow.
const DISK_IMAGE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DISK_IMAGE_MAX_POLLS: u32 = 60;

/// Azure Sandbox controller.
#[controller]
pub struct AzureSandboxController {
    /// Sandbox group that scopes every sandbox created from this declaration.
    pub(crate) sandbox_group: Option<String>,
    /// Region the group lives in; the ADC endpoint is per-region.
    pub(crate) region: Option<String>,
    /// Resource group the sandbox group sits in.
    pub(crate) resource_group: Option<String>,
    /// Catalog name or registry image every sandbox is created from, taken from the declaration's
    /// `code`. A registry image is set only once its disk image is Ready, so the binding never
    /// names one sessions cannot start from yet.
    #[serde(default)]
    pub(crate) disk_image: Option<String>,
    /// Disk image built from a registry `disk_image`, which sessions start from.
    #[serde(default)]
    pub(crate) disk_image_id: Option<String>,
    /// Disk images a changed image replaced, deleted from `Ready`. One a stopped sandbox's
    /// snapshot still holds is refused with a 409 and stays here for the next tick.
    #[serde(default)]
    pub(crate) retired_disk_images: Vec<String>,
    /// Outbound policy every sandbox is created with, from the declaration.
    #[serde(default)]
    pub(crate) egress: Option<SandboxEgress>,
    /// Idle seconds after which a sandbox pauses, if the declaration asked for one.
    #[serde(default)]
    pub(crate) idle_pause_seconds: Option<u32>,
    /// Ceilings every sandbox is created with, from the declaration.
    #[serde(default)]
    pub(crate) limits: Option<SandboxLimits>,
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
        let (group, region, resource_group) =
            self.identity(&config.id)
                .context(ErrorData::CloudPlatformError {
                    message: "the Azure sandbox group is created by the setup package and arrives \
                              through its import; regenerate the package and rerun setup"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        self.capture_session_inputs(config)?;
        self.sandbox_group = Some(group.clone());
        self.region = Some(region);
        self.resource_group = Some(resource_group);
        info!(sandbox_id = %config.id, %group, "Azure sandbox group adopted");

        Ok(HandlerAction::Continue {
            state: if self.pending_image(config).is_some() {
                EnsureDiskImage
            } else {
                Ready
            },
            suggested_delay: None,
        })
    }

    #[handler(
        state = EnsureDiskImage,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn ensure_disk_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.build_disk_image(ctx).await
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
        // fail on, and must not flip a serving sandbox to a terminal RefreshFailed here. The
        // capture is all-or-nothing, so a refusal cannot pair a new policy or size with an
        // older image.
        match self.capture_session_inputs(&config) {
            // An imported group arrives here with nothing built for a registry image, and a
            // Frozen one whose image changed without an update flow arrives serving the old one.
            // The second builds as an update, so a failed build leaves the served binding alone.
            Ok(()) if self.pending_image(&config).is_some() => {
                return Ok(HandlerAction::Continue {
                    state: if self.disk_image.is_some() {
                        UpdatingDiskImage
                    } else {
                        EnsureDiskImage
                    },
                    suggested_delay: None,
                });
            }
            Ok(()) => {}
            Err(error) => debug!(
                sandbox_id = %config.id,
                %error,
                "the declaration did not capture, so the binding keeps the one it has"
            ),
        }
        self.reap_retired_disk_images(ctx, &config.id).await;

        // The data plane has no sandbox list operation, so the heartbeat carries the group's ARM
        // provisioning state rather than a sandbox count. A read failure here leaves the sandbox
        // serving: the heartbeat is an observation, not a health gate.
        if let Ok((group, _region, resource_group)) = self.identity(&config.id) {
            let mut provisioning_state = None;
            let mut status = alien_core::SandboxHeartbeatStatus::default();

            // sandboxGroups/read is granted, so a failure here is transient rather than a
            // permission gap. Report it as a partial collection instead of a default-Healthy
            // status that would claim a read happened.
            match ctx.get_azure_config() {
                Ok(azure_config) => match ctx
                    .service_provider
                    .get_azure_sandbox_groups_client(azure_config)
                {
                    Ok(arm) => match arm.get_sandbox_group(&resource_group, &group).await {
                        Ok(sandbox_group) => {
                            provisioning_state = sandbox_group
                                .properties
                                .and_then(|properties| properties.provisioning_state);
                        }
                        Err(error) => record_collection_issue(
                            &mut status,
                            format!("could not read the sandbox group's ARM state: {error}"),
                        ),
                    },
                    Err(error) => record_collection_issue(
                        &mut status,
                        format!("could not build the ARM client for the sandbox group: {error}"),
                    ),
                },
                Err(error) => record_collection_issue(
                    &mut status,
                    format!("could not read the Azure client config: {error}"),
                ),
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
            state: if self.pending_image(config).is_some() {
                UpdatingDiskImage
            } else {
                Ready
            },
            suggested_delay: None,
        })
    }

    /// The create flow's build, routed to `UpdateFailed`: a new image that fails to build leaves
    /// the sandbox updatable, with the previous image still published and serving.
    #[handler(
        state = UpdatingDiskImage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn updating_disk_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.build_disk_image(ctx).await
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
        let mut binding = SandboxBinding::azure(
            BindingValue::value(group.clone()),
            BindingValue::value(data_plane_endpoint(region)),
            BindingValue::value(region.clone()),
            BindingValue::value(resource_group.clone()),
            BindingValue::value(disk_image.clone()),
            egress.clone(),
            self.idle_pause_seconds,
        );

        let SandboxBinding::Azure(azure) = &mut binding else {
            unreachable!("SandboxBinding::azure builds the Azure variant");
        };
        // All three or none, as the declaration has them: the data plane reads a missing ceiling
        // as its own default, so filling one in alone would assert a size nobody declared.
        if let Some(limits) = self.limits.as_ref() {
            azure.cpu = Some(BindingValue::value(limits.cpu.clone()));
            azure.memory = Some(BindingValue::value(limits.memory.clone()));
            azure.disk = Some(BindingValue::value(limits.disk.clone()));
        }

        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize the sandbox binding".to_string(),
                },
            )?,
        ))
    }

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.sandbox_group.as_ref().map(|group| {
            CoreResourceOutputs::new(SandboxOutputs {
                parent_name: group.clone(),
                identifier: self.resource_group.clone(),
                endpoint: self.region.as_deref().map(data_plane_endpoint),
            })
        })
    }
}

impl AzureSandboxController {
    /// Captures the image, egress, idle-pause and ceilings the binding carries from the
    /// declaration.
    ///
    /// The data plane takes them only at sandbox-create and `get_binding_params` has no config to
    /// read, so they live in state; a change to any must reach the binding on the next reconcile.
    pub(crate) fn capture_session_inputs(&mut self, config: &Sandbox) -> Result<()> {
        // The image is resolved before anything is assigned: the executor persists this controller
        // and republishes its binding on the error branch too, so a capture that stopped halfway
        // would leave half a declaration applied on every retry.
        let image = config
            .azure_image()
            .context(ErrorData::CloudPlatformError {
                message: "sandbox code.image is neither an Azure catalog name nor a registry image"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        match image {
            AzureSandboxImage::Catalog(name) => {
                self.disk_image = Some(name.to_string());
                if let Some(replaced) = self.disk_image_id.take() {
                    self.retired_disk_images.push(replaced);
                }
            }
            // Published by the build once its disk image is Ready; until then the binding keeps
            // naming the image sessions can start from now.
            AzureSandboxImage::Registry(_) => {}
        }
        self.egress = Some(config.egress.clone());
        self.idle_pause_seconds = config.lifecycle.idle_pause_seconds;
        self.limits = config.limits.clone();
        Ok(())
    }

    /// The registry image the declaration names when no Ready disk image serves it yet.
    fn pending_image(&self, config: &Sandbox) -> Option<String> {
        let Ok(AzureSandboxImage::Registry(reference)) = config.azure_image() else {
            return None;
        };
        (self.disk_image.as_deref() != Some(reference) || self.disk_image_id.is_none())
            .then(|| reference.to_string())
    }

    /// Looked up by label before any create and on every poll: the id is server-minted, so a lost
    /// create response leaves an image only the label finds, and a second create would duplicate
    /// it. Extra images under the label are retired.
    async fn build_disk_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<AzureSandboxHandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let Some(reference) = self.pending_image(config) else {
            return Ok(AzureSandboxHandlerAction::Continue {
                state: AzureSandboxState::Ready,
                suggested_delay: None,
            });
        };
        let (group, client) = self.data_plane(ctx, &config.id)?;
        let label = azure_disk_image_label(&reference);

        let ours: Vec<DiskImage> = client
            .list_disk_images(&group)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to list the disk images of sandbox group '{group}'"),
                resource_id: Some(config.id.clone()),
            })?
            .into_iter()
            .filter(|image| image.labels.get(AZURE_DISK_IMAGE_LABEL) == Some(&label))
            .collect();

        // A Failed image is reported once, retired, and passed over from then on: the retry that
        // follows builds afresh instead of finding the same failure under the label again.
        let failed = |image: &&DiskImage| image.state() == Some("Failed");
        let image = match ours.iter().find(|image| image.state() == Some("Ready")) {
            Some(ready) => ready.clone(),
            None if ours.iter().any(|image| !failed(&image)) => {
                debug!(sandbox_id = %config.id, %reference, "disk image is still building");
                return Ok(AzureSandboxHandlerAction::Stay {
                    max_times: Some(DISK_IMAGE_MAX_POLLS),
                    suggested_delay: Some(DISK_IMAGE_POLL_INTERVAL),
                });
            }
            None => {
                let unreported: Vec<&DiskImage> = ours
                    .iter()
                    .filter(|image| !self.retired_disk_images.contains(&image.id))
                    .collect();
                if let Some(first) = unreported.first() {
                    let reason = first
                        .status
                        .as_ref()
                        .and_then(|status| status.error_message.clone())
                        .filter(|message| !message.is_empty())
                        .unwrap_or_else(|| "no reason given".to_string());
                    self.retired_disk_images
                        .extend(unreported.iter().map(|image| image.id.clone()));
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "the disk image built from '{reference}' failed: {reason}"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }

                let registry_credentials = registry_credentials(ctx, &reference, &config.id)?;
                let created = client
                    .create_disk_image(
                        &group,
                        CreateDiskImage {
                            base: reference.clone(),
                            labels: [(AZURE_DISK_IMAGE_LABEL.to_string(), label.clone())].into(),
                            registry_credentials,
                        },
                    )
                    .await
                    .map_err(|error| {
                        // Azure's reason (a missing tag, a denied pull, an arm64-only image) leads
                        // rather than sitting at the end of the chain.
                        let reason = error.to_string();
                        error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "the disk image build from '{reference}' failed: {reason}"
                            ),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                info!(sandbox_id = %config.id, %reference, image = %created.id, "disk image build started");
                if created.state() != Some("Ready") {
                    return Ok(AzureSandboxHandlerAction::Stay {
                        max_times: Some(DISK_IMAGE_MAX_POLLS),
                        suggested_delay: Some(DISK_IMAGE_POLL_INTERVAL),
                    });
                }
                created
            }
        };

        for other in ours.iter().filter(|other| other.id != image.id) {
            if !self.retired_disk_images.contains(&other.id) {
                self.retired_disk_images.push(other.id.clone());
            }
        }
        if let Some(replaced) = self.disk_image_id.replace(image.id.clone()) {
            if replaced != image.id {
                self.retired_disk_images.push(replaced);
            }
        }
        self.disk_image = Some(reference.clone());
        info!(sandbox_id = %config.id, %reference, image = %image.id, "disk image is Ready");

        Ok(AzureSandboxHandlerAction::Continue {
            state: AzureSandboxState::Ready,
            suggested_delay: None,
        })
    }

    /// Best-effort, and never fails the heartbeat: a lingering old image does not make a serving
    /// sandbox unhealthy. A 409 (a stopped sandbox's snapshot holds it) or any other failure keeps
    /// the id for the next tick.
    async fn reap_retired_disk_images(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) {
        if self.retired_disk_images.is_empty() {
            return;
        }
        let (group, client) = match self.data_plane(ctx, resource_id) {
            Ok(plane) => plane,
            Err(error) => {
                warn!(sandbox_id = %resource_id, %error, "retired disk images kept for the next tick");
                return;
            }
        };

        let mut kept = Vec::new();
        for image_id in std::mem::take(&mut self.retired_disk_images) {
            if self.disk_image_id.as_deref() == Some(image_id.as_str()) || kept.contains(&image_id)
            {
                continue;
            }
            match client.delete_disk_image(&group, &image_id).await {
                Ok(()) => {
                    debug!(sandbox_id = %resource_id, image = %image_id, "retired disk image deleted")
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceConflict { .. })
                    ) =>
                {
                    debug!(sandbox_id = %resource_id, image = %image_id, "retired disk image is still held by a snapshot");
                    kept.push(image_id);
                }
                Err(error) => {
                    warn!(sandbox_id = %resource_id, image = %image_id, %error, "retired disk image kept for the next tick");
                    kept.push(image_id);
                }
            }
        }
        self.retired_disk_images = kept;
    }

    fn data_plane(
        &self,
        ctx: &ResourceControllerContext<'_>,
        sandbox_id: &str,
    ) -> Result<(String, std::sync::Arc<dyn SandboxDataPlaneApi>)> {
        let (group, region, resource_group) = self.identity(sandbox_id)?;
        let client = ctx.service_provider.get_azure_sandbox_data_plane_client(
            ctx.get_azure_config()?,
            &region,
            &resource_group,
        )?;
        Ok((group, client))
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
            _ => {
                let mut missing = Vec::new();
                if self.sandbox_group.is_none() {
                    missing.push("sandbox group");
                }
                if self.region.is_none() {
                    missing.push("region");
                }
                if self.resource_group.is_none() {
                    missing.push("resource group");
                }
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "no complete sandbox group recorded: {} missing, and all three are \
                         needed before sandboxes can be created",
                        missing.join(", ")
                    ),
                    resource_id: Some(sandbox_id.to_string()),
                }))
            }
        }
    }
}

/// The proxy's `deployment` Basic credentials, only for a reference on the proxy's own host: a
/// sandbox image may be public elsewhere, and that registry must not see the deployment token.
fn registry_credentials(
    ctx: &ResourceControllerContext<'_>,
    reference: &str,
    resource_id: &str,
) -> Result<Option<(String, String)>> {
    let Some(manager_url) = ctx.deployment_config.manager_url.as_deref() else {
        return Ok(None);
    };
    let proxy_host = alien_core::image_rewrite::strip_url_scheme(manager_url);
    let proxied = reference
        .split_once('/')
        .is_some_and(|(host, _)| host.eq_ignore_ascii_case(proxy_host));
    if !proxied {
        return Ok(None);
    }
    let token = ctx
        .deployment_config
        .deployment_token
        .clone()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "deployment_token is required for Azure to pull a sandbox image from \
                          the registry proxy"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;
    Ok(Some(("deployment".to_string(), token)))
}

/// ADC data-plane host for a region. The plane is per-region, so the region selects it.
fn data_plane_endpoint(region: &str) -> String {
    format!("https://management.{region}.azuredevcompute.io")
}

fn record_collection_issue(status: &mut alien_core::SandboxHeartbeatStatus, message: String) {
    status.partial = true;
    status
        .collection_issues
        .push(alien_core::HeartbeatCollectionIssue {
            source: "provisioningState".to_string(),
            reason: alien_core::HeartbeatCollectionIssueReason::CollectionFailed,
            severity: alien_core::HeartbeatIssueSeverity::Warning,
            message,
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{
        deserialize_controller, serialize_controller, MockPlatformServiceProvider,
        ResourceController, ResourceRegistry,
    };
    use alien_core::{
        Platform, ResourceLifecycle, SandboxCode, SandboxLifecyclePolicy, ToolchainConfig,
    };
    use std::sync::Arc;

    /// The ADC data plane is per-region and separate from ARM, so a caller cannot derive the
    /// endpoint from its Azure client config — the binding has to carry it.
    #[test]
    fn the_binding_carries_the_regional_data_plane_endpoint() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Deny),
            idle_pause_seconds: Some(300),
            limits: None,
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a fully imported sandbox group publishes a binding");

        assert_eq!(
            params["dataPlaneEndpoint"],
            "https://management.swedencentral.azuredevcompute.io"
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
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Allow),
            idle_pause_seconds: None,
            limits: None,
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
            region: Some("swedencentral".to_string()),
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
            "region": "swedencentral",
            "resourceGroup": "rg",
            "diskImage": "ubuntu",
            "egress": { "mode": "allowDomains", "domains": ["api.example.com"] },
            "idlePauseSeconds": 300,
            "limits": { "cpu": "4000m", "memory": "8192Mi", "disk": "40960Mi" },
        }))
        .expect("a persisted sandbox row deserializes");

        assert!(matches!(restored.state, AzureSandboxState::Ready));
        assert_eq!(restored.sandbox_group.as_deref(), Some("sbg"));
        assert_eq!(restored.region.as_deref(), Some("swedencentral"));
        assert_eq!(restored.resource_group.as_deref(), Some("rg"));
        assert_eq!(restored.disk_image.as_deref(), Some("ubuntu"));
        assert_eq!(
            restored.egress,
            Some(SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            })
        );
        assert_eq!(restored.idle_pause_seconds, Some(300));
        assert_eq!(
            restored.limits,
            Some(SandboxLimits {
                cpu: "4000m".to_string(),
                memory: "8192Mi".to_string(),
                disk: "40960Mi".to_string(),
                max_processes: None,
            })
        );
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
        let message = error.to_string();
        assert!(
            message.contains("region"),
            "the refusal must name what is missing: {message}"
        );
        assert!(
            !message.contains("resource group"),
            "the refusal must not name a field that is present: {message}"
        );
    }

    /// The executor persists this controller and republishes its binding on the error branch too,
    /// so a capture that gave up halfway would serve half a declaration on every retry.
    #[test]
    fn a_failed_capture_leaves_the_declaration_whole() {
        let mut controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Deny),
            idle_pause_seconds: None,
            limits: None,
            _internal_stay_count: None,
        };
        let config = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Source {
                src: "./sandbox".to_string(),
                toolchain: ToolchainConfig::Docker {
                    dockerfile: None,
                    build_args: None,
                    target: None,
                },
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: Some(300),
            })
            .build();

        controller
            .capture_session_inputs(&config)
            .expect_err("a source-built sandbox has no Azure catalog image");

        assert_eq!(controller.egress, Some(SandboxEgress::Deny));
        assert_eq!(controller.idle_pause_seconds, None);
        assert_eq!(controller.disk_image.as_deref(), Some("ubuntu"));
    }

    /// The same refusal in the tightening direction, which is the one worth stating outright: a
    /// declaration that narrows egress does not take effect either, because a capture that applied
    /// half of it would publish a policy paired with an image nobody declared.
    #[test]
    fn a_failed_capture_does_not_apply_a_tightened_egress_either() {
        let mut controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Allow),
            idle_pause_seconds: None,
            limits: None,
            _internal_stay_count: None,
        };
        let config = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Source {
                src: "./sandbox".to_string(),
                toolchain: ToolchainConfig::Docker {
                    dockerfile: None,
                    build_args: None,
                    target: None,
                },
            })
            .egress(SandboxEgress::Deny)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();

        controller
            .capture_session_inputs(&config)
            .expect_err("a source-built sandbox has no Azure catalog image");

        assert_eq!(
            controller.egress,
            Some(SandboxEgress::Allow),
            "the refused declaration leaves the served policy alone in both directions"
        );
        assert_eq!(controller.disk_image.as_deref(), Some("ubuntu"));
    }

    /// Preflight validates the declared ceilings against Azure's own steps and then has nothing
    /// more to do with them: this binding is the only channel to the data plane, which takes them
    /// at sandbox-create and nowhere else.
    #[test]
    fn the_binding_carries_the_declared_ceilings() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Deny),
            idle_pause_seconds: Some(300),
            limits: Some(SandboxLimits {
                cpu: "4000m".to_string(),
                memory: "8192Mi".to_string(),
                disk: "40960Mi".to_string(),
                max_processes: None,
            }),
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a fully imported sandbox group publishes a binding");

        assert_eq!(params["cpu"], "4000m");
        assert_eq!(params["memory"], "8192Mi");
        assert_eq!(params["disk"], "40960Mi");
    }

    /// A declaration naming no ceilings must not have any invented for it: the data plane reads an
    /// absent field as its own default, which is not the same as a size the binding asserts.
    #[test]
    fn a_sandbox_without_declared_ceilings_publishes_none() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::Deny),
            idle_pause_seconds: None,
            limits: None,
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a fully imported sandbox group publishes a binding");

        assert!(params.get("cpu").is_none());
        assert!(params.get("memory").is_none());
        assert!(params.get("disk").is_none());
    }

    /// `AllowDomains` is the one egress variant carrying a payload, and Azure is the backend that
    /// expresses it, so the tagged shape and the domain list are pinned rather than trusted.
    #[test]
    fn the_binding_carries_the_allow_domains_egress_shape() {
        let controller = AzureSandboxController {
            state: AzureSandboxState::Ready,
            sandbox_group: Some("sbg".to_string()),
            region: Some("swedencentral".to_string()),
            resource_group: Some("rg".to_string()),
            disk_image: Some("ubuntu".to_string()),
            disk_image_id: None,
            retired_disk_images: Vec::new(),
            egress: Some(SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            }),
            idle_pause_seconds: None,
            limits: None,
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
            region: Some("swedencentral".to_string()),
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

    mod disk_images {
        use super::*;
        use alien_azure_clients::azure::sandbox_data_plane::{
            DiskImageStatus, MockSandboxDataPlaneApi,
        };
        use alien_azure_clients::azure::sandbox_groups::MockSandboxGroupsApi;
        use std::sync::atomic::{AtomicUsize, Ordering};

        const PYTHON: &str = "docker.io/library/python:3.14-slim";
        const NODE: &str = "docker.io/library/node:22";

        fn sandbox(image: &str) -> Sandbox {
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        }

        fn adopted(
            disk_image: Option<&str>,
            disk_image_id: Option<&str>,
        ) -> AzureSandboxController {
            AzureSandboxController {
                state: AzureSandboxState::Ready,
                sandbox_group: Some("sbg".to_string()),
                region: Some("westus2".to_string()),
                resource_group: Some("rg".to_string()),
                disk_image: disk_image.map(str::to_string),
                disk_image_id: disk_image_id.map(str::to_string),
                retired_disk_images: Vec::new(),
                egress: Some(SandboxEgress::Allow),
                idle_pause_seconds: None,
                limits: None,
                _internal_stay_count: None,
            }
        }

        fn image(id: &str, reference: &str, state: &str) -> DiskImage {
            DiskImage {
                id: id.to_string(),
                labels: [(
                    AZURE_DISK_IMAGE_LABEL.to_string(),
                    azure_disk_image_label(reference),
                )]
                .into(),
                status: Some(DiskImageStatus {
                    state: Some(state.to_string()),
                    error_message: None,
                }),
            }
        }

        /// A provider handing out `client` for the data plane, and an ARM client whose read fails,
        /// which the heartbeat reports as a partial collection rather than an error.
        fn provider_with(client: MockSandboxDataPlaneApi) -> Arc<MockPlatformServiceProvider> {
            let client: Arc<dyn SandboxDataPlaneApi> = Arc::new(client);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_azure_sandbox_data_plane_client()
                .returning(move |_, _, _| Ok(client.clone()));
            provider
                .expect_get_azure_sandbox_groups_client()
                .returning(|_| {
                    let mut arm = MockSandboxGroupsApi::new();
                    arm.expect_get_sandbox_group().returning(|_, name| {
                        Err(AlienError::new(
                            alien_client_core::ErrorData::RemoteResourceNotFound {
                                resource_type: "SandboxGroup".to_string(),
                                resource_name: name.to_string(),
                            },
                        ))
                    });
                    Ok(Arc::new(arm))
                });
            Arc::new(provider)
        }

        async fn executor(
            declared: Sandbox,
            controller: AzureSandboxController,
            client: MockSandboxDataPlaneApi,
        ) -> SingleControllerExecutor {
            SingleControllerExecutor::builder()
                .resource(declared)
                .controller(controller)
                .platform(Platform::Azure)
                .resource_lifecycle(ResourceLifecycle::Frozen)
                .service_provider(provider_with(client))
                .build()
                .await
                .expect("executor should build")
        }

        fn state_of(executor: &SingleControllerExecutor) -> &AzureSandboxController {
            executor
                .internal_state::<AzureSandboxController>()
                .expect("an Azure sandbox controller")
        }

        /// An imported group reaches `Ready` with nothing built for its registry image, so the
        /// first tick builds it. The binding names the image only once the build is Ready, and
        /// the build carries the label the provider finds it by and no registry credential.
        #[tokio::test]
        async fn an_imported_registry_image_is_built_before_its_binding_is_published() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .times(1)
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .withf(|group, request| {
                    group == "sbg"
                        && request.base == PYTHON
                        && request.labels.get(AZURE_DISK_IMAGE_LABEL)
                            == Some(&azure_disk_image_label(PYTHON))
                        && request.registry_credentials.is_none()
                })
                .times(1)
                .returning(|_, _| Ok(image("img-1", PYTHON, "Ready")));
            let mut executor = executor(sandbox(PYTHON), adopted(None, None), client).await;
            assert!(
                state_of(&executor).get_binding_params().unwrap().is_none(),
                "no binding names an image nothing has built"
            );

            executor.step().await.expect("the tick routes to the build");
            executor.step().await.expect("the build succeeds");

            let controller = state_of(&executor);
            assert!(matches!(controller.state, AzureSandboxState::Ready));
            assert_eq!(controller.disk_image_id.as_deref(), Some("img-1"));
            let params = controller.get_binding_params().unwrap().expect("a binding");
            assert_eq!(params["diskImage"], PYTHON);
        }

        /// A create whose response was lost left an image under the label. The retry finds and
        /// adopts it rather than building a second one, and a duplicate from an earlier loss is
        /// retired.
        #[tokio::test]
        async fn a_retried_build_reuses_the_image_already_built() {
            let mut client = MockSandboxDataPlaneApi::new();
            client.expect_list_disk_images().returning(|_| {
                Ok(vec![
                    image("other", NODE, "Ready"),
                    image("img-1", PYTHON, "Ready"),
                    image("dup", PYTHON, "Ready"),
                ])
            });
            client.expect_create_disk_image().times(0);
            let mut executor = executor(sandbox(PYTHON), adopted(None, None), client).await;

            executor.step().await.expect("the tick routes to the build");
            executor
                .step()
                .await
                .expect("the build adopts what is there");

            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("img-1"));
            assert_eq!(controller.retired_disk_images, vec!["dup".to_string()]);
        }

        /// A build that is not Ready yet is polled on the same label rather than re-created.
        #[tokio::test]
        async fn a_building_image_is_polled_until_ready() {
            let polls = Arc::new(AtomicUsize::new(0));
            let seen = polls.clone();
            let mut client = MockSandboxDataPlaneApi::new();
            client.expect_list_disk_images().returning(move |_| {
                let state = if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    "Building"
                } else {
                    "Ready"
                };
                Ok(vec![image("img-1", PYTHON, state)])
            });
            client.expect_create_disk_image().times(0);
            let mut executor = executor(sandbox(PYTHON), adopted(None, None), client).await;

            executor.step().await.expect("the tick routes to the build");
            executor.step().await.expect("the build is still running");
            assert!(state_of(&executor).disk_image_id.is_none());
            executor.step().await.expect("the build is Ready");

            assert_eq!(state_of(&executor).disk_image_id.as_deref(), Some("img-1"));
            assert_eq!(polls.load(Ordering::SeqCst), 2);
        }

        /// A changed reference builds a new image, then retires the old one. The delete a stopped
        /// sandbox's snapshot refuses with a 409 is kept and retried on a later tick, not lost.
        #[tokio::test]
        async fn a_changed_image_is_rebuilt_and_the_old_one_retired_through_a_409() {
            let deletes = Arc::new(AtomicUsize::new(0));
            let seen = deletes.clone();
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("old", PYTHON, "Ready")]));
            client
                .expect_create_disk_image()
                .withf(|_, request| request.base == NODE)
                .times(1)
                .returning(|_, _| Ok(image("new", NODE, "Ready")));
            client
                .expect_delete_disk_image()
                .withf(|group, id| group == "sbg" && id == "old")
                .times(2)
                .returning(move |_, _| {
                    if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(AlienError::new(
                            alien_client_core::ErrorData::RemoteResourceConflict {
                                message: "DiskImageHasDependents".to_string(),
                                resource_type: "Resource".to_string(),
                                resource_name: "old".to_string(),
                            },
                        ))
                    } else {
                        Ok(())
                    }
                });
            let mut executor =
                executor(sandbox(PYTHON), adopted(Some(PYTHON), Some("old")), client).await;

            executor
                .update(sandbox(NODE))
                .expect("transition to update");
            executor
                .step()
                .await
                .expect("the update routes to the build");
            executor.step().await.expect("the new image builds");
            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("new"));
            assert_eq!(controller.retired_disk_images, vec!["old".to_string()]);
            assert_eq!(
                controller.get_binding_params().unwrap().unwrap()["diskImage"],
                NODE
            );

            executor
                .step()
                .await
                .expect("a held image does not fail the tick");
            assert_eq!(
                state_of(&executor).retired_disk_images,
                vec!["old".to_string()]
            );

            executor.step().await.expect("the next tick deletes it");
            assert!(state_of(&executor).retired_disk_images.is_empty());
            assert_eq!(deletes.load(Ordering::SeqCst), 2);
        }

        /// A Frozen sandbox whose image changed without an update flow reaches `Ready` serving the
        /// old image, and builds the new one as an update, so a failed build leaves the served
        /// binding alone rather than failing the sandbox's provisioning.
        #[tokio::test]
        async fn an_image_changed_under_a_serving_sandbox_builds_as_an_update() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .times(1)
                .returning(|_, _| Ok(image("new", NODE, "Ready")));
            let mut executor =
                executor(sandbox(NODE), adopted(Some(PYTHON), Some("old")), client).await;

            executor.step().await.expect("the tick routes to the build");
            assert!(
                matches!(
                    state_of(&executor).state,
                    AzureSandboxState::UpdatingDiskImage
                ),
                "{:?}",
                state_of(&executor).state
            );
            assert_eq!(executor.status(), ResourceStatus::Updating);

            executor.step().await.expect("the new image builds");
            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("new"));
            assert_eq!(controller.retired_disk_images, vec!["old".to_string()]);
        }

        /// An image on the manager's own host is pulled through its registry proxy, which takes
        /// the deployment's Basic credentials as an Azure Worker's pull does.
        #[tokio::test]
        async fn a_proxied_image_builds_with_the_deployment_credentials() {
            const PROXIED: &str = "test-manager.alien.dev/artifacts/prj_test:sandbox-v1";
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .withf(|_, request| {
                    request.registry_credentials
                        == Some((
                            "deployment".to_string(),
                            "test-deployment-token".to_string(),
                        ))
                })
                .times(1)
                .returning(|_, _| Ok(image("img-1", PROXIED, "Ready")));
            let mut executor = executor(sandbox(PROXIED), adopted(None, None), client).await;

            executor.step().await.expect("the tick routes to the build");
            executor.step().await.expect("the build succeeds");
            assert_eq!(state_of(&executor).disk_image_id.as_deref(), Some("img-1"));
        }

        /// An arm64-only image is refused by Azure at create, and its reason is what the failure
        /// leads with rather than a generic build error.
        #[tokio::test]
        async fn a_refused_build_fails_with_azures_reason() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .times(1)
                .returning(|_, _| {
                    Err(AlienError::new(
                        alien_client_core::ErrorData::InvalidInput {
                            message: "Bad request for Resource 'sbg': The container image \
                              'docker.io/arm64v8/alpine:3.19' does not provide a linux/amd64 \
                              variant. Only linux/amd64 images are supported."
                                .to_string(),
                            field_name: None,
                        },
                    ))
                });
            let mut executor = executor(
                sandbox("docker.io/arm64v8/alpine:3.19"),
                adopted(None, None),
                client,
            )
            .await;

            executor.step().await.expect("the tick routes to the build");
            let error = executor
                .step()
                .await
                .expect_err("an arm64-only image is refused");

            assert!(error.to_string().contains("linux/amd64"), "{error}");
            assert!(state_of(&executor).get_binding_params().unwrap().is_none());
        }

        /// Switching back to a catalog name publishes it at once and retires the built image.
        #[test]
        fn a_catalog_name_retires_the_image_a_registry_reference_built() {
            let mut controller = adopted(Some(PYTHON), Some("img-1"));

            controller
                .capture_session_inputs(&sandbox("ubuntu"))
                .expect("a catalog name captures");

            assert_eq!(controller.disk_image.as_deref(), Some("ubuntu"));
            assert_eq!(controller.disk_image_id, None);
            assert_eq!(controller.retired_disk_images, vec!["img-1".to_string()]);
        }
    }
}
