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
//! It owns one data-plane object: the disk image built from a registry image, deleted once a
//! changed reference replaces it; `sandbox/images` grants exactly those verbs. One build per
//! reference string, so a tag pushed again is not rebuilt: a changed image needs a new tag or digest.

use std::time::Duration;

use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::azure::sandbox_data_plane::{
    CreateDiskImage, DiskImage, SandboxDataPlaneApi,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    azure_disk_image_label, classify_azure_sandbox_image, AzureSandboxImage,
    ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox, SandboxEgress, SandboxLimits,
    SandboxOutputs, AZURE_DISK_IMAGE_LABEL,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// A disk image builds in 10-30s, so a build still running after 5 minutes is wedged. Measured in
/// time rather than polls: the executor steps every resource whenever any one asks, so a poll
/// count can run out while a healthy build is still under way.
const DISK_IMAGE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DISK_IMAGE_BUILD_TIMEOUT: chrono::Duration = chrono::Duration::minutes(5);

/// How long a disk image a binding named outlives its replacement. Azure caps no session's life,
/// so this borrows AWS's MicroVM ceiling: long past the rollout that moves every consumer of the
/// binding to the new image.
const RETIRED_DISK_IMAGE_RETENTION_SECONDS: i64 = 28_800;

/// A disk image this controller built and no longer serves.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetiredDiskImage {
    pub(crate) id: String,
    /// At once for one no binding named (a duplicate, a failed or abandoned build); after the
    /// retention window for one a consumer may still start sandboxes from.
    pub(crate) delete_after: chrono::DateTime<chrono::Utc>,
}

/// A build Azure accepted but has not finished, kept so a poll never depends on the list alone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingDiskImage {
    pub(crate) reference: String,
    pub(crate) id: String,
    /// When the build was started or first seen, which the wedge timeout counts from.
    #[serde(default = "chrono::Utc::now")]
    pub(crate) started_at: chrono::DateTime<chrono::Utc>,
}

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
    /// Disk images deleted from `Ready` once due. One a stopped sandbox's snapshot still holds is
    /// refused with a 409 and stays here for the next tick.
    #[serde(default)]
    pub(crate) retired_disk_images: Vec<RetiredDiskImage>,
    /// The build in flight, if any.
    #[serde(default)]
    pub(crate) pending_disk_image: Option<PendingDiskImage>,
    /// Set once a build has run, so `Ready` sweeps the group's labelled images; a sandbox that
    /// only ever served catalog names never lists them.
    #[serde(default)]
    pub(crate) built_disk_images: bool,
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
        // fail on, and must not flip a serving sandbox to a terminal RefreshFailed here. A
        // refused capture changes nothing; a registry image reaches the binding only once built.
        match self.capture_session_inputs(&config) {
            // A Frozen sandbox whose image changed without an update flow arrives serving the old
            // one, and builds as an update so a failed build leaves the served binding alone. One
            // serving nothing yet builds through the create flow.
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
        // A build the declaration no longer names (a switch to a catalog name, or a revert to
        // the served image) is retired here; the transitions into Ready leave it for this tick.
        if self.pending_image(&config).is_none() {
            self.abandon_pending_build(None);
        }
        self.sweep_disk_images(ctx, &config.id).await;
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

    // `EnsureDiskImage` lets a failed first build be repaired by a changed image: without it the
    // update restarts the create, whose fresh controller has lost the imported group.
    #[flow_entry(Update, from = [Ready, RefreshFailed, EnsureDiskImage])]
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
    /// the sandbox updatable, still serving the previous image if it had one.
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
        // The group is the setup stack's to destroy, and destroying it takes its sandboxes and
        // disk images with it. A delete here would race `terraform destroy` and fail teardown on
        // the 404 whichever call lost; the runtime's own grant does not carry the delete anyway.
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
                    self.retire(replaced, true);
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

    /// Looked up by label before any create and on every poll: the id is server-minted, so only the
    /// label finds an image whose create response was lost. A returned id is also read directly,
    /// since the list can lag it. Extra images under the label are retired.
    async fn build_disk_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<AzureSandboxHandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        // Nothing to build: `Ready` abandons any build left in flight.
        let Some(reference) = self.pending_image(config) else {
            return Ok(AzureSandboxHandlerAction::Continue {
                state: AzureSandboxState::Ready,
                suggested_delay: None,
            });
        };
        self.abandon_pending_build(Some(&reference));
        let (group, client) = self.data_plane(ctx, &config.id)?;
        self.built_disk_images = true;
        let label = azure_disk_image_label(&reference);

        let mut ours: Vec<DiskImage> = client
            .list_disk_images(&group)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to list the disk images of sandbox group '{group}'"),
                resource_id: Some(config.id.clone()),
            })?
            .into_iter()
            .filter(|image| image.labels.get(AZURE_DISK_IMAGE_LABEL) == Some(&label))
            .collect();
        if let Some(pending) = self.pending_disk_image.clone() {
            if !ours.iter().any(|image| image.id == pending.id) {
                match client.get_disk_image(&group, &pending.id).await {
                    Ok(image) => ours.push(image),
                    // Retired rather than forgotten: a read that lags the create too must not
                    // leave the build untracked once it appears.
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        self.pending_disk_image = None;
                        self.retire(pending.id, false);
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to read disk image '{}' of sandbox group '{group}'",
                                pending.id
                            ),
                            resource_id: Some(config.id.clone()),
                        }))
                    }
                }
            }
        }

        // A Failed image is reported once, retired, and passed over from then on. The error is
        // not retryable, so the rebuild comes from the next update or a retry of the failed
        // resource, which finds the failure retired and builds afresh.
        let failed = |image: &&DiskImage| image.state() == Some("Failed");
        let image = match ours.iter().find(|image| image.state() == Some("Ready")) {
            Some(ready) => ready.clone(),
            None if ours
                .iter()
                .any(|image| !failed(&image) && !self.is_retired(&image.id)) =>
            {
                debug!(sandbox_id = %config.id, %reference, "disk image is still building");
                let building: Vec<String> = ours
                    .iter()
                    .filter(|image| !failed(image) && !self.is_retired(&image.id))
                    .map(|image| image.id.clone())
                    .collect();
                // One found by label after a lost create response is tracked too, so a change of
                // declaration mid-build still retires it; a tracked build that ended is replaced
                // by one still running, whose timeout starts now.
                let tracked = match self.pending_disk_image.take() {
                    Some(pending) if building.contains(&pending.id) => pending,
                    ended => {
                        if let Some(ended) = ended {
                            self.retire(ended.id, false);
                        }
                        PendingDiskImage {
                            reference: reference.clone(),
                            id: building[0].clone(),
                            started_at: chrono::Utc::now(),
                        }
                    }
                };
                let started_at = tracked.started_at;
                self.pending_disk_image = Some(tracked);
                return self.poll_build(building, started_at, &reference, &config.id);
            }
            None => {
                self.pending_disk_image = None;
                let unreported: Vec<&DiskImage> = ours
                    .iter()
                    .filter(|image| !self.is_retired(&image.id))
                    .collect();
                if let Some(first) = unreported.first() {
                    let reason = first
                        .status
                        .as_ref()
                        .and_then(|status| status.error_message.clone())
                        .filter(|message| !message.is_empty())
                        .unwrap_or_else(|| "no reason given".to_string());
                    for image in &unreported {
                        self.retire(image.id.clone(), false);
                    }
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
                        let reason = error.message.clone();
                        error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "the disk image build from '{reference}' failed: {reason}"
                            ),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                info!(sandbox_id = %config.id, %reference, image = %created.id, "disk image build started");
                if created.state() != Some("Ready") {
                    self.pending_disk_image = Some(PendingDiskImage {
                        reference: reference.clone(),
                        id: created.id.clone(),
                        started_at: chrono::Utc::now(),
                    });
                    return Ok(AzureSandboxHandlerAction::Stay {
                        max_times: None,
                        suggested_delay: Some(DISK_IMAGE_POLL_INTERVAL),
                    });
                }
                created
            }
        };

        // An abandoned build adopted after all serves now, so it leaves the deletion queue.
        self.retired_disk_images
            .retain(|retired| retired.id != image.id);
        // A Ready duplicate may be the one a provider found first, so it keeps the window.
        for other in ours.iter().filter(|other| other.id != image.id) {
            self.retire(other.id.clone(), other.state() == Some("Ready"));
        }
        // The pending build is in `ours` by now (listed, or read by id), so the loop covered it.
        self.pending_disk_image = None;
        if let Some(replaced) = self.disk_image_id.replace(image.id.clone()) {
            if replaced != image.id {
                self.retire(replaced, true);
            }
        }
        self.disk_image = Some(reference.clone());
        info!(sandbox_id = %config.id, %reference, image = %image.id, "disk image is Ready");

        Ok(AzureSandboxHandlerAction::Continue {
            state: AzureSandboxState::Ready,
            suggested_delay: None,
        })
    }

    /// Queues a disk image for deletion from `Ready`; `served` holds it for the retention window.
    fn retire(&mut self, id: String, served: bool) {
        if self.is_retired(&id) {
            return;
        }
        let now = chrono::Utc::now();
        let delete_after = if served {
            now + chrono::Duration::seconds(RETIRED_DISK_IMAGE_RETENTION_SECONDS)
        } else {
            now
        };
        self.retired_disk_images
            .push(RetiredDiskImage { id, delete_after });
    }

    fn is_retired(&self, id: &str) -> bool {
        self.retired_disk_images
            .iter()
            .any(|retired| retired.id == id)
    }

    /// Retires a build in flight for anything but `reference`: the declaration moved on, and
    /// nothing else remembers its id once its label is no longer the one looked up.
    fn abandon_pending_build(&mut self, reference: Option<&str>) {
        if let Some(pending) = self
            .pending_disk_image
            .take_if(|pending| Some(pending.reference.as_str()) != reference)
        {
            self.retire(pending.id, false);
        }
    }

    /// Polls a build, or past the timeout (the `Stay`'s only ceiling) retires it and fails. A retry
    /// adopts it if Ready by then, since retired images are deleted only from the `Ready` state;
    /// else it builds afresh.
    fn poll_build(
        &mut self,
        building: Vec<String>,
        started_at: chrono::DateTime<chrono::Utc>,
        reference: &str,
        resource_id: &str,
    ) -> Result<AzureSandboxHandlerAction> {
        if chrono::Utc::now() - started_at < DISK_IMAGE_BUILD_TIMEOUT {
            return Ok(AzureSandboxHandlerAction::Stay {
                max_times: None,
                suggested_delay: Some(DISK_IMAGE_POLL_INTERVAL),
            });
        }
        self.pending_disk_image = None;
        for id in &building {
            self.retire(id.clone(), false);
        }
        Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "the disk image build from '{reference}' ({}) did not finish within {} minutes; \
                 a retry uses it if it is Ready by then, or builds it again",
                building.join(", "),
                DISK_IMAGE_BUILD_TIMEOUT.num_minutes()
            ),
            resource_id: Some(resource_id.to_string()),
        }))
    }

    /// Best-effort, like the reap: retires every labelled image in this sandbox's group but the
    /// served one (a lost create response, or one a lagging list hid until another was adopted).
    /// Unlabelled images, a session's commits, are never touched.
    async fn sweep_disk_images(&mut self, ctx: &ResourceControllerContext<'_>, resource_id: &str) {
        if !self.built_disk_images {
            return;
        }
        let (group, client) = match self.data_plane(ctx, resource_id) {
            Ok(plane) => plane,
            Err(error) => {
                warn!(sandbox_id = %resource_id, %error, "disk images not swept this tick");
                return;
            }
        };
        let images = match client.list_disk_images(&group).await {
            Ok(images) => images,
            Err(error) => {
                warn!(sandbox_id = %resource_id, %error, "disk images not swept this tick");
                return;
            }
        };
        let mut labelled = false;
        for image in images {
            if !image.labels.contains_key(AZURE_DISK_IMAGE_LABEL) {
                continue;
            }
            labelled = true;
            if self.disk_image_id.as_deref() == Some(image.id.as_str()) {
                continue;
            }
            // Any Ready one may be what a consumer on this or the previous binding cached.
            let ready = image.state() == Some("Ready");
            self.retire(image.id, ready);
        }
        // Back on a catalog name with nothing left to delete: stop listing the group.
        if !labelled && self.disk_image_id.is_none() && self.retired_disk_images.is_empty() {
            self.built_disk_images = false;
        }
    }

    /// Best-effort, and never fails the heartbeat: a lingering old image does not make a serving
    /// sandbox unhealthy. One not yet due, refused with a 409 (a stopped sandbox's snapshot holds
    /// it) or failing otherwise is kept for a later tick.
    async fn reap_retired_disk_images(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) {
        let now = chrono::Utc::now();
        if !self
            .retired_disk_images
            .iter()
            .any(|retired| retired.delete_after <= now)
        {
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
        for retired in std::mem::take(&mut self.retired_disk_images) {
            if self.disk_image_id.as_deref() == Some(retired.id.as_str()) {
                continue;
            }
            if retired.delete_after > now {
                kept.push(retired);
                continue;
            }
            match client.delete_disk_image(&group, &retired.id).await {
                Ok(()) => {
                    debug!(sandbox_id = %resource_id, image = %retired.id, "retired disk image deleted")
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
                    debug!(sandbox_id = %resource_id, image = %retired.id, "retired disk image is still held by a snapshot");
                    kept.push(retired);
                }
                Err(error) => {
                    warn!(sandbox_id = %resource_id, image = %retired.id, %error, "retired disk image kept for the next tick");
                    kept.push(retired);
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
            pending_disk_image: None,
            built_disk_images: false,
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
            pending_disk_image: None,
            built_disk_images: false,
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
            "diskImageId": "img-1",
            "retiredDiskImages": [{ "id": "img-0", "deleteAfter": "2026-01-01T00:00:00Z" }],
            "pendingDiskImage": {
                "reference": "docker.io/library/python:3.14-slim",
                "id": "img-2",
                "startedAt": "2026-01-01T00:00:00Z",
            },
            "builtDiskImages": true,
        }))
        .expect("a persisted sandbox row deserializes");
        let epoch = "2026-01-01T00:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();

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
        assert_eq!(restored.disk_image_id.as_deref(), Some("img-1"));
        assert_eq!(
            restored.retired_disk_images,
            vec![RetiredDiskImage {
                id: "img-0".to_string(),
                delete_after: epoch,
            }]
        );
        assert_eq!(
            restored.pending_disk_image,
            Some(PendingDiskImage {
                reference: "docker.io/library/python:3.14-slim".to_string(),
                id: "img-2".to_string(),
                started_at: epoch,
            })
        );
        assert!(restored.built_disk_images);
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
            pending_disk_image: None,
            built_disk_images: false,
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
            pending_disk_image: None,
            built_disk_images: false,
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
            pending_disk_image: None,
            built_disk_images: false,
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
            pending_disk_image: None,
            built_disk_images: false,
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
            pending_disk_image: None,
            built_disk_images: false,
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
        use crate::core::{StackExecutor, StackResourceStateExt};
        use alien_azure_clients::azure::sandbox_data_plane::{
            DiskImageStatus, MockSandboxDataPlaneApi,
        };
        use alien_azure_clients::azure::sandbox_groups::MockSandboxGroupsApi;
        use alien_azure_clients::AzureClientConfigExt as _;
        use alien_core::{ClientConfig, StackResourceState, StackState};
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
                pending_disk_image: None,
                built_disk_images: false,
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

        fn retired_ids(controller: &AzureSandboxController) -> Vec<&str> {
            controller
                .retired_disk_images
                .iter()
                .map(|retired| retired.id.as_str())
                .collect()
        }

        /// Whether `id` is held for the retention window rather than due now.
        fn held(controller: &AzureSandboxController, id: &str) -> bool {
            controller.retired_disk_images.iter().any(|retired| {
                retired.id == id
                    && retired.delete_after > chrono::Utc::now() + chrono::Duration::seconds(3600)
            })
        }

        fn state_of(executor: &SingleControllerExecutor) -> &AzureSandboxController {
            executor
                .internal_state::<AzureSandboxController>()
                .expect("an Azure sandbox controller")
        }

        /// A sandbox at `Ready` with nothing built for its registry image builds on its first
        /// tick. The binding names the image only once the build is Ready, and the build carries
        /// the label the provider finds it by and no registry credential.
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
            assert_eq!(retired_ids(controller), vec!["dup"]);
            assert!(
                held(controller, "dup"),
                "a Ready duplicate may be what a provider cached"
            );
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
            assert_eq!(
                state_of(&executor)
                    .pending_disk_image
                    .as_ref()
                    .map(|pending| pending.id.as_str()),
                Some("img-1"),
                "a build found by label is tracked like one this controller started"
            );
            executor.step().await.expect("the build is Ready");

            assert_eq!(state_of(&executor).disk_image_id.as_deref(), Some("img-1"));
            assert_eq!(polls.load(Ordering::SeqCst), 2);
        }

        /// A changed reference builds a new image and retires the old one, which stays for the
        /// retention window: a consumer still holding the previous binding starts from it.
        #[tokio::test]
        async fn a_changed_image_is_rebuilt_and_the_old_one_kept_for_the_window() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("old", PYTHON, "Ready")]));
            client
                .expect_create_disk_image()
                .withf(|_, request| request.base == NODE)
                .times(1)
                .returning(|_, _| Ok(image("new", NODE, "Ready")));
            client.expect_delete_disk_image().times(0);
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
            assert!(held(controller, "old"));
            assert_eq!(
                controller.get_binding_params().unwrap().unwrap()["diskImage"],
                NODE
            );

            executor
                .step()
                .await
                .expect("the Ready tick keeps an image not yet due");
            assert_eq!(retired_ids(state_of(&executor)), vec!["old"]);
        }

        /// A due image a stopped sandbox's snapshot still holds is refused with a 409. It is kept
        /// and deleted on a later tick, not lost.
        #[tokio::test]
        async fn a_due_image_held_by_a_snapshot_is_deleted_on_a_later_tick() {
            let deletes = Arc::new(AtomicUsize::new(0));
            let seen = deletes.clone();
            let mut client = MockSandboxDataPlaneApi::new();
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
            let mut controller = adopted(Some(NODE), Some("new"));
            controller.retired_disk_images = vec![RetiredDiskImage {
                id: "old".to_string(),
                delete_after: chrono::Utc::now() - chrono::Duration::seconds(1),
            }];
            let mut executor = executor(sandbox(NODE), controller, client).await;

            executor
                .step()
                .await
                .expect("a held image does not fail the tick");
            assert_eq!(retired_ids(state_of(&executor)), vec!["old"]);

            executor.step().await.expect("the next tick deletes it");
            assert!(state_of(&executor).retired_disk_images.is_empty());
            assert_eq!(deletes.load(Ordering::SeqCst), 2);
        }

        /// A create Azure accepted but has not finished is polled by its id when the list lags it,
        /// rather than built a second time.
        #[tokio::test]
        async fn a_build_the_list_does_not_show_yet_is_read_by_id() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .times(1)
                .returning(|_, _| Ok(image("img-1", PYTHON, "Building")));
            client
                .expect_get_disk_image()
                .withf(|group, id| group == "sbg" && id == "img-1")
                .times(1)
                .returning(|_, _| Ok(image("img-1", PYTHON, "Ready")));
            let mut executor = executor(sandbox(PYTHON), adopted(None, None), client).await;

            executor.step().await.expect("the tick routes to the build");
            executor.step().await.expect("the build is accepted");
            assert_eq!(
                state_of(&executor)
                    .pending_disk_image
                    .as_ref()
                    .map(|pending| pending.id.as_str()),
                Some("img-1")
            );
            executor
                .step()
                .await
                .expect("the poll reads the build by id");

            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("img-1"));
            assert!(controller.pending_disk_image.is_none());
        }

        /// A build still running when the declaration moves to another image is retired for
        /// deletion at once: its label is no longer looked up, so nothing else would find it.
        #[tokio::test]
        async fn a_build_the_declaration_moved_past_is_retired() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .withf(|_, request| request.base == NODE)
                .times(1)
                .returning(|_, _| Ok(image("node-1", NODE, "Ready")));
            let mut controller = adopted(None, None);
            controller.state = AzureSandboxState::EnsureDiskImage;
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: PYTHON.to_string(),
                id: "python-1".to_string(),
                started_at: chrono::Utc::now(),
            });
            let mut executor = executor(sandbox(NODE), controller, client).await;

            executor
                .step()
                .await
                .expect("the abandoned build is retired and the new image builds");

            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("node-1"));
            assert_eq!(retired_ids(controller), vec!["python-1"]);
            assert!(!held(controller, "python-1"), "no binding ever named it");
        }

        /// A build still running past the timeout is retired and the step fails. If it is still
        /// not Ready at the retry of the failed resource, the retry starts a new build instead of
        /// polling the wedged one again.
        #[tokio::test]
        async fn a_wedged_build_is_given_up_and_the_retry_builds_afresh() {
            let creates = Arc::new(AtomicUsize::new(0));
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("stuck", PYTHON, "Building")]));
            client.expect_create_disk_image().times(0);
            let mut controller = adopted(None, None);
            controller.state = AzureSandboxState::EnsureDiskImage;
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: PYTHON.to_string(),
                id: "stuck".to_string(),
                started_at: chrono::Utc::now() - DISK_IMAGE_BUILD_TIMEOUT,
            });
            let mut executor = executor(sandbox(PYTHON), controller, client).await;

            let error = executor
                .step()
                .await
                .expect_err("the timed-out poll gives up");
            assert!(error.to_string().contains("stuck"), "{error}");
            assert!(
                !error.retryable,
                "the executor must not rebuild on its own: {error}"
            );
            assert_eq!(retired_ids(state_of(&executor)), vec!["stuck"]);

            let resumed = state_of(&executor).clone();
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("stuck", PYTHON, "Building")]));
            let counter = creates.clone();
            client
                .expect_create_disk_image()
                .times(1)
                .returning(move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(image("fresh", PYTHON, "Ready"))
                });
            let mut executor = self::executor(sandbox(PYTHON), resumed, client).await;
            executor.step().await.expect("the retry builds afresh");

            assert_eq!(creates.load(Ordering::SeqCst), 1);
            assert_eq!(state_of(&executor).disk_image_id.as_deref(), Some("fresh"));
        }

        /// A build given up at the timeout that is Ready by the retry serves: the provider finds
        /// it by label anyway, so building another would leave two Ready images under the label,
        /// one queued for deletion. It leaves the deletion queue and the Ready tick keeps it.
        #[tokio::test]
        async fn a_timed_out_build_ready_by_the_retry_is_adopted_not_rebuilt() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("late", PYTHON, "Building")]));
            let mut controller = adopted(None, None);
            controller.state = AzureSandboxState::EnsureDiskImage;
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: PYTHON.to_string(),
                id: "late".to_string(),
                started_at: chrono::Utc::now() - DISK_IMAGE_BUILD_TIMEOUT,
            });
            let mut executor = executor(sandbox(PYTHON), controller, client).await;
            executor
                .step()
                .await
                .expect_err("the timed-out poll gives up");
            assert_eq!(retired_ids(state_of(&executor)), vec!["late"]);

            let resumed = state_of(&executor).clone();
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(vec![image("late", PYTHON, "Ready")]));
            client.expect_create_disk_image().times(0);
            client.expect_delete_disk_image().times(0);
            let mut executor = self::executor(sandbox(PYTHON), resumed, client).await;
            executor.step().await.expect("the retry adopts the build");

            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("late"));
            assert!(controller.retired_disk_images.is_empty());

            executor
                .step()
                .await
                .expect("the Ready tick keeps the served image");
            assert!(state_of(&executor).retired_disk_images.is_empty());
        }

        /// A build a failed update left running is retired once the declaration reverts to the
        /// image already served, since no later build under its label would find it.
        #[tokio::test]
        async fn a_build_left_by_a_reverted_update_is_retired_from_ready() {
            let mut client = MockSandboxDataPlaneApi::new();
            client.expect_list_disk_images().times(0);
            client
                .expect_delete_disk_image()
                .withf(|_, id| id == "node-1")
                .times(1)
                .returning(|_, _| Ok(()));
            let mut controller = adopted(Some(PYTHON), Some("python-1"));
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: NODE.to_string(),
                id: "node-1".to_string(),
                started_at: chrono::Utc::now(),
            });
            let mut executor = executor(sandbox(PYTHON), controller, client).await;

            executor.step().await.expect("the Ready tick");

            let controller = state_of(&executor);
            assert!(controller.pending_disk_image.is_none());
            assert!(controller.retired_disk_images.is_empty());
            assert_eq!(controller.disk_image_id.as_deref(), Some("python-1"));
        }

        /// `Ready` retires the labelled images it does not serve: one that is not Ready goes at
        /// once, a Ready one keeps the window since a consumer may have cached it, and an
        /// unlabelled image (a session's commit) is left alone.
        #[tokio::test]
        async fn ready_sweeps_labelled_images_it_does_not_serve() {
            let mut client = MockSandboxDataPlaneApi::new();
            client.expect_list_disk_images().returning(|_| {
                let mut committed = image("committed", PYTHON, "Ready");
                committed.labels.clear();
                Ok(vec![
                    image("img-1", PYTHON, "Ready"),
                    image("orphan", NODE, "Failed"),
                    image("dup", PYTHON, "Ready"),
                    image("stale", NODE, "Ready"),
                    committed,
                ])
            });
            client
                .expect_delete_disk_image()
                .withf(|_, id| id == "orphan")
                .times(1)
                .returning(|_, _| Ok(()));
            let mut controller = adopted(Some(PYTHON), Some("img-1"));
            controller.built_disk_images = true;
            let mut executor = executor(sandbox(PYTHON), controller, client).await;

            executor.step().await.expect("the Ready tick");

            let controller = state_of(&executor);
            assert_eq!(retired_ids(controller), vec!["dup", "stale"]);
            assert!(held(controller, "dup") && held(controller, "stale"));
            assert_eq!(controller.disk_image_id.as_deref(), Some("img-1"));
            assert!(controller.built_disk_images);
        }

        /// A tracked build that ended while a duplicate still runs is retired, and the timeout
        /// follows the running one from when it was first seen, not from the ended build's start.
        #[tokio::test]
        async fn a_tracked_build_that_ended_hands_the_timeout_to_one_still_running() {
            let mut client = MockSandboxDataPlaneApi::new();
            client.expect_list_disk_images().returning(|_| {
                Ok(vec![
                    image("first", PYTHON, "Failed"),
                    image("second", PYTHON, "Building"),
                ])
            });
            client.expect_create_disk_image().times(0);
            let mut controller = adopted(None, None);
            controller.state = AzureSandboxState::EnsureDiskImage;
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: PYTHON.to_string(),
                id: "first".to_string(),
                started_at: chrono::Utc::now() - DISK_IMAGE_BUILD_TIMEOUT,
            });
            let mut executor = executor(sandbox(PYTHON), controller, client).await;

            executor.step().await.expect("the running build is polled");

            let controller = state_of(&executor);
            let pending = controller
                .pending_disk_image
                .as_ref()
                .expect("still tracked");
            assert_eq!(pending.id, "second");
            assert!(chrono::Utc::now() - pending.started_at < chrono::Duration::minutes(1));
            assert_eq!(retired_ids(controller), vec!["first"]);
        }

        /// A sandbox back on a catalog name stops listing the group once no labelled image is
        /// left in it.
        #[tokio::test]
        async fn the_sweep_stops_once_a_catalog_sandbox_has_nothing_left() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .times(1)
                .returning(|_| Ok(Vec::new()));
            let mut controller = adopted(Some("ubuntu"), None);
            controller.built_disk_images = true;
            let mut executor = executor(sandbox("ubuntu"), controller, client).await;

            executor.step().await.expect("the Ready tick lists once");
            assert!(!state_of(&executor).built_disk_images);
            executor.step().await.expect("the next tick does not list");
        }

        /// A pending build its id no longer finds is retired rather than forgotten, so if it
        /// appears later it is still deleted, and the build starts again.
        #[tokio::test]
        async fn a_pending_build_not_found_by_id_is_retired() {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client.expect_get_disk_image().times(1).returning(|_, id| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceNotFound {
                        resource_type: "DiskImage".to_string(),
                        resource_name: id.to_string(),
                    },
                ))
            });
            client
                .expect_create_disk_image()
                .times(1)
                .returning(|_, _| Ok(image("img-2", PYTHON, "Ready")));
            let mut controller = adopted(None, None);
            controller.state = AzureSandboxState::EnsureDiskImage;
            controller.pending_disk_image = Some(PendingDiskImage {
                reference: PYTHON.to_string(),
                id: "img-1".to_string(),
                started_at: chrono::Utc::now(),
            });
            let mut executor = executor(sandbox(PYTHON), controller, client).await;

            executor.step().await.expect("the build starts again");

            let controller = state_of(&executor);
            assert_eq!(controller.disk_image_id.as_deref(), Some("img-2"));
            assert_eq!(retired_ids(controller), vec!["img-1"]);
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
            assert!(held(controller, "old"));
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

            let rendered = error.to_string();
            assert!(rendered.contains("linux/amd64"), "{rendered}");
            assert_eq!(rendered.matches("INVALID_INPUT").count(), 1, "{rendered}");
            assert!(state_of(&executor).get_binding_params().unwrap().is_none());
        }

        /// A first build that failed is repaired by declaring another image: the update resumes
        /// from the build's checkpoint, keeping the imported group, rather than restarting the
        /// create, which cannot recover the group.
        #[tokio::test]
        async fn a_failed_first_build_is_repaired_by_a_changed_image() {
            let mut failed = StackResourceState::new_pending(
                Sandbox::RESOURCE_TYPE.to_string(),
                alien_core::Resource::new(sandbox(PYTHON)),
                Some(ResourceLifecycle::Frozen),
                vec![],
            );
            failed.status = ResourceStatus::ProvisionFailed;
            let mut checkpoint = adopted(None, None);
            checkpoint.state = AzureSandboxState::EnsureDiskImage;
            failed
                .set_last_failed_controller(Some(Box::new(checkpoint)))
                .unwrap();
            let mut state = StackState::new(Platform::Azure);
            state.resources.insert("agents".to_string(), failed);

            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_list_disk_images()
                .returning(|_| Ok(Vec::new()));
            client
                .expect_create_disk_image()
                .withf(|group, request| group == "sbg" && request.base == NODE)
                .times(1)
                .returning(|_, _| Ok(image("node-1", NODE, "Ready")));
            let stack = alien_core::Stack::new("repair".to_string())
                .add(sandbox(NODE), ResourceLifecycle::Frozen)
                .build();
            let deployment_config = alien_core::DeploymentConfig::builder()
                .stack_settings(alien_core::StackSettings::default())
                .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(alien_core::ExternalBindings::default())
                .allow_frozen_changes(false)
                .build();
            let executor = StackExecutor::builder(
                &stack,
                ClientConfig::Azure(Box::new(alien_azure_clients::AzureClientConfig::mock())),
            )
            .deployment_config(&deployment_config)
            .service_provider(provider_with(client))
            .build()
            .unwrap();

            let plan = executor.plan(&state).unwrap();
            assert!(plan.updates.contains_key("agents"), "{plan:?}");
            assert!(!plan.creates.contains(&"agents".to_string()), "{plan:?}");

            for _ in 0..3 {
                state = executor.step(state).await.unwrap().next_state;
            }
            let repaired = &state.resources["agents"];
            assert_eq!(repaired.status, ResourceStatus::Running, "{repaired:?}");
            let controller = repaired
                .get_internal_controller_typed::<AzureSandboxController>()
                .unwrap();
            assert_eq!(controller.sandbox_group.as_deref(), Some("sbg"));
            assert_eq!(controller.disk_image_id.as_deref(), Some("node-1"));
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
            assert!(held(&controller, "img-1"));
        }
    }
}
