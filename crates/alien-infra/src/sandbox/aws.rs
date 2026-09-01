//! AWS Sandbox controller.
//!
//! The durable parent is a Lambda MicroVM image. A Live sandbox's image is built here, after
//! the deployment registers: setup installs the build role and egress connector and registers
//! the bundle, and only at runtime does a customer account exist as a principal Alien's
//! registry can open to. A release that changes the bundle re-enters that flow against an image
//! that already exists and rolls a new version onto it. A Frozen sandbox arrives through the
//! importer with its image already built by stack creation, and this controller only watches it.
//!
//! Sessions are MicroVMs started from the image at runtime. `RunMicrovm` has no `tags`, so
//! image plus version *is* the session identity; `lambda:ListMicrovms` is account-wide and
//! granted to no sandbox set, so sessions cannot be enumerated here and self-reap at their
//! declared lifetime ceiling.

use std::collections::BTreeMap;
use std::time::Duration;

use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::lambda_microvms::{
    CreateMicrovmImageRequest, MicrovmCodeArtifact, MicrovmCpuConfiguration, MicrovmImageBuild,
    MicrovmImageBuildHooks, MicrovmImageHooks, MicrovmImageLogging, MicrovmImageResources,
    MicrovmLifecycleHooks, UpdateMicrovmImageRequest,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    parse_bundle_uri, standard_resource_tags, BundleUri, ResourceOutputs as CoreResourceOutputs,
    ResourceStatus, Sandbox, SandboxCode, SandboxOutputs,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use sha2::{Digest, Sha256};

/// Port the in-image agent serves, both its own protocol and the lifecycle hooks.
const AGENT_PORT: u16 = 8971;

/// Unprivileged identity commands run as inside the sandbox, never the agent's own.
const EXEC_UID: &str = "60000";

/// The only architecture MicroVM images accept.
const ARCHITECTURE: &str = "ARM_64";

/// A build takes ~110-160s, gated on the in-image `Ready` hook (itself capped at 120s).
/// 90 polls at this interval is a 15-minute ceiling — an order of magnitude past a healthy
/// build, so hitting it means the image is wedged, not slow.
const BUILD_POLL_INTERVAL: Duration = Duration::from_secs(10);
const BUILD_MAX_POLLS: u32 = 90;

/// AWS's own ceiling on a MicroVM's life, used as the retention window when a declaration sets
/// none: a session cannot outlive it, so nothing can still be running on a version older than
/// this.
const MAX_SESSION_LIFETIME_SECONDS: i64 = 28_800;

/// AWS Sandbox controller.
#[controller]
pub struct AwsSandboxController {
    /// MicroVM image backing this sandbox's sessions; the ARN, which every call accepts.
    pub(crate) image_identifier: Option<String>,
    /// Image ARN, published in outputs so the binding can address it.
    pub(crate) image_arn: Option<String>,
    /// Version sessions run on and the binding names. `1.0` for a fresh image, not `1`.
    ///
    /// The alias reads state written before a sandbox's image was ever rebuilt, where this was
    /// the only version field.
    #[serde(default, alias = "imageVersion")]
    pub(crate) active_version: Option<String>,
    /// Version currently building. Never published: a binding is a promise sessions can be
    /// created now, and a building version cannot serve one.
    #[serde(default)]
    pub(crate) pending_version: Option<String>,
    /// Bundle `pending_version` is being built from, promoted with it so a failed build leaves
    /// no claim that the new release is serving.
    #[serde(default)]
    pub(crate) pending_bundle_uri: Option<String>,
    /// Version a roll replaced, retained while sessions started from it may still be running.
    #[serde(default)]
    pub(crate) previous_version: Option<String>,
    /// When `previous_version` stopped serving, which starts its retention window.
    #[serde(default)]
    pub(crate) retired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Region the MicroVMs run in. Held here because the binding is built without a context.
    pub(crate) region: Option<String>,
    /// Role passed to `CreateMicrovmImage`. Setup owns it because `sandbox/provision` grants
    /// `iam:PassRole` and no `iam:CreateRole`; the registration hands it over.
    #[serde(default)]
    pub(crate) build_role_arn: Option<String>,
    /// Bundle the image is built from, handed over by the registration.
    #[serde(default)]
    pub(crate) bundle_uri: Option<String>,
    /// Connectors every session is started with; empty is `allow`, readable only because
    /// `allow_egress` travels with it.
    #[serde(default)]
    pub(crate) egress_connector_arns: Vec<String>,
    /// Whether the declaration asked for open egress.
    #[serde(default)]
    pub(crate) allow_egress: bool,
    /// Ports a preview token may be minted for; the mint is bounded by this list alone.
    #[serde(default)]
    pub(crate) preview_ports: Vec<u16>,
    /// Idle-suspend seconds for the binding, captured from the declaration.
    #[serde(default)]
    pub(crate) idle_suspend_seconds: Option<u32>,
    /// Session lifetime ceiling for the binding, captured from the declaration.
    #[serde(default)]
    pub(crate) max_lifetime_seconds: Option<u32>,
}

#[controller]
impl AwsSandboxController {
    // ─────────────── CREATE FLOW ──────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingImage,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn creating_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let build_role_arn = self.build_role_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no build role was registered for this sandbox; setup must install \
                          one before the image can be built"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let bundle_uri = self.bundle_uri.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no bundle was registered for this sandbox; setup must publish one \
                          before the image can be built"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let image_name = format!("{}-{}", ctx.resource_prefix, config.id);
        let tier = config
            .microvm_tier()
            .context(ErrorData::ResourceConfigInvalid {
                message: "declared limits do not fit any MicroVM tier".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        self.idle_suspend_seconds = config.session.idle_suspend_seconds;
        self.max_lifetime_seconds = config.session.max_lifetime_seconds;

        // The vendor's base image travels inside the bundle's Dockerfile, already resolved to
        // a registry the build role can reach; the only image this call names is AWS's managed
        // base, and the build's connector is AWS's own — it must reach a registry, which the
        // session-time deny connector cannot.
        let inputs = ImageBuildInputs {
            description: format!("Sandbox {}", config.id),
            base_image_arn: managed_base_image_arn(&aws_config.region),
            build_role_arn,
            code_artifact: MicrovmCodeArtifact {
                uri: bundle_uri.clone(),
            },
            egress_network_connectors: vec![internet_egress_connector_arn(&aws_config.region)],
            resources: vec![MicrovmImageResources {
                minimum_memory_in_mib: tier.baseline_memory_mib,
            }],
            // Keyed on the bundle, not just the resource: a retry of the same release replays
            // the prior success, while a new release's changed bundle asks for a real build
            // rather than replaying the image built from the previous one.
            client_token: build_client_token(&image_name, &bundle_uri),
        };

        // A create whose response never reached state leaves an image under this
        // account-unique name, and a second create would collide with it. Read first and adopt
        // what is there; a bundle that changed later is rolled by the update flow, not here.
        let existing = match client.get_microvm_image(&image_name).await {
            Ok(image) => Some(image),
            Err(error) if is_remote_resource_absent(&error) => None,
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to read MicroVM image '{image_name}' before building it"
                    ),
                    resource_id: Some(config.id.clone()),
                });
            }
        };

        let (operation, image_arn, image_version) = match existing {
            None => {
                let created = client
                    .create_microvm_image(
                        inputs.create_request(
                            image_name.clone(),
                            standard_resource_tags(ctx.resource_prefix, &config.id)
                                .into_iter()
                                .collect(),
                        ),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create MicroVM image '{image_name}'"),
                        resource_id: Some(config.id.clone()),
                    })?;
                (
                    "CreateMicrovmImage",
                    created.image_arn,
                    created.image_version,
                )
            }
            // Adopting costs no mutation: the build this image is already running is the one
            // this handler would have asked for.
            Some(image) => {
                drop(inputs);
                ("GetMicrovmImage", image.image_arn, image.image_version)
            }
        };

        let image_arn = image_arn.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation} for '{image_name}' returned no image ARN"),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let image_version = image_version.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation} for '{image_name}' returned no version"),
                resource_id: Some(config.id.clone()),
            })
        })?;

        self.image_identifier = Some(image_arn.clone());
        self.image_arn = Some(image_arn);
        self.pending_version = Some(image_version);
        self.pending_bundle_uri = Some(bundle_uri);
        self.region = Some(aws_config.region.clone());

        info!(sandbox_id = %config.id, image = %image_name, "MicroVM image build started");

        Ok(HandlerAction::Continue {
            state: WaitingForImageActive,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForImageActive,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn waiting_for_image_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let (image_identifier, image_version) = self.require_image(&config.id)?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let version = client
            .get_microvm_image_version(&image_identifier, &image_version)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read MicroVM image '{image_identifier}' version '{image_version}'"
                ),
                resource_id: Some(config.id.clone()),
            })?;

        match version_readiness(version.state.as_deref(), version.status.as_deref()) {
            VersionReadiness::Building => {
                debug!(
                    sandbox_id = %config.id,
                    state = version.state.as_deref().unwrap_or("unknown"),
                    "MicroVM image is still building"
                );
                Ok(HandlerAction::Stay {
                    max_times: Some(BUILD_MAX_POLLS),
                    suggested_delay: Some(BUILD_POLL_INTERVAL),
                })
            }
            VersionReadiness::Failed => {
                // The version's own reason plus each failed build's — the build reason is the
                // one that says what actually broke (a bad FROM, a denied pull). A generic
                // error after 160 seconds is the worst debugging experience this resource can
                // produce, so an unreadable build log is reported inside the failure rather
                // than allowed to replace it.
                let build_detail = match client
                    .list_microvm_image_builds(&image_identifier, &image_version)
                    .await
                {
                    Ok(builds) => failed_build_reasons(&builds),
                    Err(error) => format!("build details unreadable: {error}"),
                };
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "MicroVM image '{image_identifier}' version '{image_version}' failed to \
                         build: {}; {build_detail}",
                        version.state_reason.as_deref().unwrap_or("no reason given"),
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
            VersionReadiness::Ready => {
                self.promote_pending_version();
                info!(sandbox_id = %config.id, version = %image_version, "MicroVM image is active");
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
        }
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        if let Some(image_identifier) = self.image_identifier.clone() {
            let aws_config = ctx.get_aws_config()?;
            let client = ctx
                .service_provider
                .get_aws_microvms_client(aws_config)
                .await?;

            let image = client.get_microvm_image(&image_identifier).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to read MicroVM image '{image_identifier}'"),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if image.image_arn.is_some() {
                self.image_arn = image.image_arn.clone();
            }
            // The served version is this controller's own record: it is promoted only when a
            // build is observed ACTIVE. Taking it from the image read would adopt a version
            // some other writer rolled and point the binding at a bundle nothing here built.
            if self.active_version.is_none() && image.image_version.is_some() {
                self.active_version = image.image_version.clone();
            }
            self.region = Some(aws_config.region.clone());

            self.reap_retired_version(&client, &config.id).await?;

            // Session counts require `lambda:ListMicrovms`, which AWS authorizes against no
            // resource type — no sandbox permission set grants it, so the count travels as
            // uncollectable rather than as a genuine zero.
            let status = alien_core::SandboxHeartbeatStatus {
                partial: true,
                collection_issues: vec![alien_core::HeartbeatCollectionIssue {
                    source: "sessions".to_string(),
                    reason: alien_core::HeartbeatCollectionIssueReason::Forbidden,
                    severity: alien_core::HeartbeatIssueSeverity::Warning,
                    message: "session counts need lambda:ListMicrovms, which is account-wide \
                              and granted to no sandbox set"
                        .to_string(),
                }],
                ..Default::default()
            };

            ctx.emit_heartbeat(alien_core::ResourceHeartbeat {
                deployment_id: None,
                resource_id: config.id.clone(),
                resource_type: Sandbox::RESOURCE_TYPE,
                controller_platform: alien_core::Platform::Aws,
                backend: alien_core::HeartbeatBackend::Aws,
                observed_at: chrono::Utc::now(),
                data: alien_core::ResourceHeartbeatData::Sandbox(
                    alien_core::SandboxHeartbeatData::AwsMicrovm(
                        alien_core::AwsMicrovmSandboxHeartbeatData {
                            status,
                            image_identifier: image_identifier.clone(),
                            // Where AWS surfaces base-image deprecation.
                            image_state: image.state.clone(),
                        },
                    ),
                ),
                raw: vec![],
            });
        }

        debug!(sandbox_id = %config.id, "Sandbox ready");

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

        // The session-policy ceilings feed the binding and cost nothing to refresh.
        self.idle_suspend_seconds = config.session.idle_suspend_seconds;
        self.max_lifetime_seconds = config.session.max_lifetime_seconds;

        let aws_config = ctx.get_aws_config()?;
        let desired_bundle = desired_bundle_uri(&config, &aws_config.region)?;

        if self.bundle_uri.as_deref() == Some(desired_bundle.as_str()) {
            info!(sandbox_id = %config.id, "Updated AWS sandbox configuration");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // A Frozen sandbox's image is built and owned by stack creation. Rebuilding it here
        // would act with credentials that were never granted it, so the mismatch is surfaced
        // rather than acted on.
        if !self.owns_image_builds(ctx, &config.id) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "sandbox '{}' declares bundle '{desired_bundle}' but its image is owned by \
                     stack creation; redeploy the setup package to change it",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        info!(sandbox_id = %config.id, bundle = %desired_bundle, "rolling MicroVM image onto a new bundle");

        Ok(HandlerAction::Continue {
            state: UpdatingImage,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingImage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn updating_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let build_role_arn = self.build_role_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no build role was registered for this sandbox; setup must install \
                          one before the image can be rebuilt"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let image_identifier = self.image_identifier.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no MicroVM image is recorded for this sandbox; there is nothing to \
                          roll a new version onto"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let desired_bundle = desired_bundle_uri(&config, &aws_config.region)?;
        let tier = config
            .microvm_tier()
            .context(ErrorData::ResourceConfigInvalid {
                message: "declared limits do not fit any MicroVM tier".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let rolled = client
            .update_microvm_image(
                &image_identifier,
                image_build_inputs(
                    &aws_config.region,
                    &config.id,
                    &build_role_arn,
                    &desired_bundle,
                    tier.baseline_memory_mib,
                )
                .update_request(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to roll MicroVM image '{image_identifier}'"),
                resource_id: Some(config.id.clone()),
            })?;

        let image_version = rolled.image_version.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("UpdateMicrovmImage for '{image_identifier}' returned no version"),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // The version that is serving stays in `active_version` until this build is observed
        // ACTIVE, so the binding keeps naming the bundle sessions can actually start from.
        self.pending_version = Some(image_version);
        self.pending_bundle_uri = Some(desired_bundle);
        if let Some(arn) = rolled.image_arn {
            self.image_arn = Some(arn);
        }

        info!(sandbox_id = %config.id, image = %image_identifier, "MicroVM image roll started");

        Ok(HandlerAction::Continue {
            state: WaitingForRolledImageActive,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    /// The create flow's poll, routed to `UpdateFailed`: a roll that fails must leave the
    /// sandbox updatable rather than provision-failed, with the previous version still serving.
    #[handler(
        state = WaitingForRolledImageActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn waiting_for_rolled_image_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_image_active(ctx).await
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    // Versions first, then the image: the versions hold the image, and the API accepts a
    // delete on an image with versions present while removing nothing. Sessions cannot be
    // enumerated (see the module doc) and self-reap; a version delete refused while they run
    // fails loudly here and the executor retries — nothing is fired and forgotten.

    #[flow_entry(Delete)]
    #[handler(
        state = DeletingImageVersions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting_image_versions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        // The binding is a promise sessions can be created now; withdraw it before the first
        // delete rather than after the last one.
        self.active_version = None;

        // A setup-baked image belongs to the setup stack and is removed by stack teardown;
        // deleting it here would act with credentials that were never granted it — and would
        // destroy a resource this controller does not own if they ever were.
        if !self.owns_image_deletion(ctx, &config.id) {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        // Deterministic fallback: a create can succeed without its ARN ever being recorded
        // (a crash between the call and the state write), and walking past it here would leak
        // a live image nothing ever removes. The name is derived, so sweep by it; an image
        // that never existed answers NotFound, which the loop below already treats as done.
        let image_identifier = self
            .image_identifier
            .clone()
            .unwrap_or_else(|| format!("{}-{}", ctx.resource_prefix, config.id));

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let versions = match client.list_microvm_image_versions(&image_identifier).await {
            Ok(versions) => versions,
            Err(error) if is_remote_resource_absent(&error) => Vec::new(),
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to list versions of MicroVM image '{image_identifier}'"
                    ),
                    resource_id: Some(config.id.clone()),
                });
            }
        };

        // Deleting a version that is still building fails and rides the executor's retry budget
        // (~17 min ceiling against a ~160s build) rather than a dedicated wait state.
        for version in versions {
            // A versionless entry cannot be deleted, and skipping it would let the image
            // delete below no-op while a version survives — Deleted without deleting.
            let Some(image_version) = version.image_version else {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "MicroVM image '{image_identifier}' listed a version record with no \
                         version identifier; refusing to delete around it"
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            };
            match client
                .delete_microvm_image_version(&image_identifier, &image_version)
                .await
            {
                Ok(()) => {}
                Err(error) if is_remote_resource_absent(&error) => {}
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete MicroVM image '{image_identifier}' version \
                             '{image_version}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    });
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingImage,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingImage,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        if !self.owns_image_deletion(ctx, &config.id) {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }
        let image_identifier = self
            .image_identifier
            .clone()
            .unwrap_or_else(|| format!("{}-{}", ctx.resource_prefix, config.id));

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        match client.delete_microvm_image(&image_identifier).await {
            Ok(()) => {}
            Err(error) if is_remote_resource_absent(&error) => {
                debug!(sandbox_id = %config.id, image = %image_identifier, "MicroVM image already gone");
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete MicroVM image '{image_identifier}'"),
                    resource_id: Some(config.id.clone()),
                });
            }
        }

        self.active_version = None;
        info!(sandbox_id = %config.id, "AWS sandbox image deleted; sessions expire on their own");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{AwsSandboxBinding, BindingValue, SandboxBinding};

        // Only the active version is publishable. It is set when a build is observed ACTIVE,
        // ~160s after the create call returns every other field below — and during a roll it
        // still names the version that is serving, so an update never withdraws a working
        // binding to advertise one that is still building.
        let (Some(image_arn), Some(image_version), Some(region)) = (
            self.image_arn.as_ref(),
            self.active_version.as_ref(),
            self.region.as_ref(),
        ) else {
            return Ok(None);
        };

        // The connectors decide what a session can reach, and the preview ports bound what a
        // token can be minted for; neither can be inferred from the image, and a session
        // started without the connectors reaches the public internet.
        let binding = SandboxBinding::Aws(AwsSandboxBinding {
            image_arn: BindingValue::value(image_arn.clone()),
            image_version: BindingValue::value(image_version.clone()),
            region: BindingValue::value(region.clone()),
            execution_role_arn: None,
            egress_connector_arns: self
                .egress_connector_arns
                .iter()
                .map(|arn| BindingValue::value(arn.clone()))
                .collect(),
            preview_ports: self.preview_ports.clone(),
            idle_suspend_seconds: self.idle_suspend_seconds,
            max_lifetime_seconds: self.max_lifetime_seconds,
            allow_egress: self.allow_egress,
        });

        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize sandbox binding parameters".to_string(),
                },
            )?,
        ))
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
        self.image_identifier.as_ref().map(|identifier| {
            CoreResourceOutputs::new(SandboxOutputs {
                parent_name: identifier.clone(),
                identifier: self.image_arn.clone(),
                // Each MicroVM has its own endpoint, minted per session; there is no parent
                // one.
                endpoint: None,
            })
        })
    }
}

impl AwsSandboxController {
    /// Whether this controller built the image and therefore owns its deletion.
    ///
    /// The lifecycle in stack state is the honest source. A state that carries none falls
    /// back to the registration's build inputs, which only a runtime-provisioned sandbox has
    /// — and errs toward not deleting, because destroying a setup-owned image is the failure
    /// that cannot be retried.
    fn owns_image_deletion(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        match ctx
            .state
            .resources
            .get(resource_id)
            .and_then(|resource| resource.lifecycle)
        {
            Some(alien_core::ResourceLifecycle::Live) => true,
            Some(alien_core::ResourceLifecycle::Frozen) => false,
            None => self.build_role_arn.is_some(),
        }
    }

    fn require_image(&self, resource_id: &str) -> Result<(String, String)> {
        match (self.image_identifier.clone(), self.pending_version.clone()) {
            (Some(identifier), Some(version)) => Ok((identifier, version)),
            _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "waiting on a MicroVM image build with no image identifier or pending \
                          version recorded"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }

    /// Moves the built version into service and starts the retention clock on the one it
    /// replaced. Both halves happen together: a gap would either unpublish a working binding or
    /// leave a retired version with no window.
    fn promote_pending_version(&mut self) {
        let Some(pending) = self.pending_version.take() else {
            return;
        };
        if let Some(replaced) = self.active_version.replace(pending) {
            self.previous_version = Some(replaced);
            self.retired_at = Some(chrono::Utc::now());
        }
        if let Some(bundle) = self.pending_bundle_uri.take() {
            self.bundle_uri = Some(bundle);
        }
    }

    /// Deletes the version a roll replaced, once nothing started from it can still be running.
    ///
    /// Sessions cannot be enumerated (see the module doc), so the only sound signal is time: a
    /// MicroVM cannot outlive the declared ceiling, and AWS's own 8-hour maximum stands in when
    /// the declaration sets none.
    async fn reap_retired_version(
        &mut self,
        client: &std::sync::Arc<dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi>,
        resource_id: &str,
    ) -> Result<()> {
        let (Some(version), Some(retired_at), Some(identifier)) = (
            self.previous_version.clone(),
            self.retired_at,
            self.image_identifier.clone(),
        ) else {
            return Ok(());
        };

        let window = self
            .max_lifetime_seconds
            .map(i64::from)
            .unwrap_or(MAX_SESSION_LIFETIME_SECONDS);
        if (chrono::Utc::now() - retired_at).num_seconds() < window {
            return Ok(());
        }

        match client
            .delete_microvm_image_version(&identifier, &version)
            .await
        {
            Ok(()) => {}
            Err(error) if is_remote_resource_absent(&error) => {}
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete retired MicroVM image '{identifier}' version '{version}'"
                    ),
                    resource_id: Some(resource_id.to_string()),
                });
            }
        }

        self.previous_version = None;
        self.retired_at = None;
        debug!(sandbox_id = %resource_id, version = %version, "retired MicroVM image version deleted");
        Ok(())
    }

    /// Whether this controller built the image and may therefore rebuild it. A Frozen sandbox's
    /// image belongs to the setup stack, which owns its bundle too.
    fn owns_image_builds(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        self.owns_image_deletion(ctx, resource_id)
    }
}

/// Assembles the build inputs from the declaration, so a create and a roll of the same sandbox
/// differ only in the bundle.
fn image_build_inputs(
    region: &str,
    resource_id: &str,
    build_role_arn: &str,
    bundle_uri: &str,
    baseline_memory_mib: i64,
) -> ImageBuildInputs {
    ImageBuildInputs {
        description: format!("Sandbox {resource_id}"),
        base_image_arn: managed_base_image_arn(region),
        build_role_arn: build_role_arn.to_string(),
        code_artifact: MicrovmCodeArtifact {
            uri: bundle_uri.to_string(),
        },
        egress_network_connectors: vec![internet_egress_connector_arn(region)],
        resources: vec![MicrovmImageResources {
            minimum_memory_in_mib: baseline_memory_mib,
        }],
        client_token: build_client_token(resource_id, bundle_uri),
    }
}

/// The bundle the declaration asks for, with the region token resolved.
///
/// AWS builds a MicroVM image only from a bucket in the image's own region, so a vendor's one
/// stored URI resolves per region here exactly as the emitters resolve it.
fn desired_bundle_uri(config: &Sandbox, region: &str) -> Result<String> {
    let SandboxCode::Image { image } = &config.code else {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "an AWS sandbox is built from a prebuilt s3:// bundle, not from source"
                .to_string(),
            resource_id: Some(config.id.clone()),
        }));
    };

    match parse_bundle_uri(image).map_err(|reason| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: reason,
            resource_id: Some(config.id.clone()),
        })
    })? {
        BundleUri::Literal(uri) => Ok(uri.to_string()),
        BundleUri::Regional { before, after } => Ok(format!("{before}{region}{after}")),
    }
}

/// The build inputs a create and a roll must state identically.
///
/// `UpdateMicrovmImage` has PUT semantics — a field left out of the roll is dropped from the
/// new version — so the two requests are built from one value rather than assembled twice.
struct ImageBuildInputs {
    description: String,
    base_image_arn: String,
    build_role_arn: String,
    code_artifact: MicrovmCodeArtifact,
    egress_network_connectors: Vec<String>,
    resources: Vec<MicrovmImageResources>,
    client_token: String,
}

impl ImageBuildInputs {
    fn create_request(
        self,
        name: String,
        tags: BTreeMap<String, String>,
    ) -> CreateMicrovmImageRequest {
        CreateMicrovmImageRequest::builder()
            .name(name)
            .description(self.description)
            .base_image_arn(self.base_image_arn)
            .base_image_version("1")
            .build_role_arn(self.build_role_arn)
            .code_artifact(self.code_artifact)
            // The switch behind "control plane never sees sandbox contents".
            .logging(MicrovmImageLogging::Disabled {})
            .egress_network_connectors(self.egress_network_connectors)
            .cpu_configurations(vec![MicrovmCpuConfiguration {
                architecture: ARCHITECTURE.to_string(),
            }])
            .resources(self.resources)
            .hooks(agent_hooks())
            .environment_variables(agent_environment())
            // The deployed `sandbox/provision` grant conditions `CreateMicrovmImage` on the
            // stack tag and `managed-by: runtime` arriving as request tags; without them the
            // call is denied by a policy that is already installed.
            .tags(tags)
            .client_token(self.client_token)
            .build()
    }

    /// The roll carries no tags: the API rejects them here, and the image keeps the ones its
    /// create set — which is what every other sandbox statement scopes against.
    fn update_request(self) -> UpdateMicrovmImageRequest {
        UpdateMicrovmImageRequest::builder()
            .description(self.description)
            .base_image_arn(self.base_image_arn)
            .base_image_version("1")
            .build_role_arn(self.build_role_arn)
            .code_artifact(self.code_artifact)
            .logging(MicrovmImageLogging::Disabled {})
            .egress_network_connectors(self.egress_network_connectors)
            .cpu_configurations(vec![MicrovmCpuConfiguration {
                architecture: ARCHITECTURE.to_string(),
            }])
            .resources(self.resources)
            .hooks(agent_hooks())
            .environment_variables(agent_environment())
            .client_token(self.client_token)
            .build()
    }
}

/// A build idempotency token that changes with the bundle. Both the create and the roll replay a
/// prior success for a repeated token, so folding the content-addressed bundle in keeps a retry
/// idempotent while a genuinely new release asks for a real build.
fn build_client_token(image_name: &str, bundle_uri: &str) -> String {
    let digest = Sha256::digest(bundle_uri.as_bytes());
    // Sixteen hex chars of the bundle digest is plenty to separate one release from the next,
    // and keeps the token well under the API's length ceiling.
    format!("{image_name}-{:x}", digest)
        .chars()
        .take(image_name.len() + 1 + 16)
        .collect()
}

/// The Lambda-managed base image the build runs on. The vendor's own base is inside the
/// bundle's Dockerfile, not here.
fn managed_base_image_arn(region: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:aws:microvm-image:al2023-1",
        aws_partition(region)
    )
}

/// AWS's own internet connector, which the image build routes through to reach a registry.
fn internet_egress_connector_arn(region: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:aws:network-connector:aws-network-connector:INTERNET_EGRESS",
        aws_partition(region)
    )
}

/// Partition for ARNs the controller mints itself, where no CloudFormation pseudo-parameter
/// can resolve it.
fn aws_partition(region: &str) -> &'static str {
    if region.starts_with("us-gov-") {
        "aws-us-gov"
    } else if region.starts_with("cn-") {
        "aws-cn"
    } else {
        "aws"
    }
}

/// Lifecycle hooks, served by the agent on its own port; mirrors what the setup emitters bake
/// into a Frozen image.
///
/// `Ready` is not optional — AWS rejects an image enabling any MicroVM hook without it, and it
/// is what defers the snapshot until the agent is serving. `Run` and `Resume` reseed the
/// CSPRNG after each start, because every MicroVM shares the state resident at capture.
fn agent_hooks() -> MicrovmImageHooks {
    MicrovmImageHooks::builder()
        .port(AGENT_PORT)
        .microvm_image_hooks(MicrovmImageBuildHooks {
            ready: "ENABLED".to_string(),
            ready_timeout_in_seconds: 120,
        })
        .microvm_hooks(MicrovmLifecycleHooks {
            run: "ENABLED".to_string(),
            run_timeout_in_seconds: 30,
            resume: "ENABLED".to_string(),
            resume_timeout_in_seconds: 30,
        })
        .build()
}

/// The agent's configuration contract, identical to the setup emitters' rendering.
fn agent_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ALIEN_SANDBOX_ROOT".to_string(), "/sandbox".to_string()),
        ("ALIEN_SANDBOX_PORT".to_string(), AGENT_PORT.to_string()),
        (
            "ALIEN_SANDBOX_AUTHORIZATION".to_string(),
            "transport".to_string(),
        ),
        ("ALIEN_SANDBOX_EXEC_UID".to_string(), EXEC_UID.to_string()),
        ("ALIEN_SANDBOX_EXEC_GID".to_string(), EXEC_UID.to_string()),
    ])
}

/// How far along a MicroVM image version's build is.
enum VersionReadiness {
    /// Still building; waiting is recoverable and the poll ceiling bounds it.
    Building,
    /// Built and runnable.
    Ready,
    /// Terminal failure; the version will never become runnable.
    Failed,
}

/// Classifies a version's build state and availability.
///
/// Ready needs both halves: `SUCCESSFUL` says the build finished, and only `ACTIVE` versions
/// can be run — reporting ready on either alone publishes a binding sessions cannot start
/// from. An unknown state waits, bounded by the poll ceiling, because binding to a half-built
/// image is not recoverable.
fn version_readiness(state: Option<&str>, status: Option<&str>) -> VersionReadiness {
    match state {
        Some(state) if state.eq_ignore_ascii_case("FAILED") => VersionReadiness::Failed,
        Some(state) if state.eq_ignore_ascii_case("SUCCESSFUL") => {
            if status.is_some_and(|status| status.eq_ignore_ascii_case("ACTIVE")) {
                VersionReadiness::Ready
            } else {
                VersionReadiness::Building
            }
        }
        _ => VersionReadiness::Building,
    }
}

/// Renders the failed builds' own reasons for the surfaced error.
fn failed_build_reasons(builds: &[MicrovmImageBuild]) -> String {
    let reasons: Vec<String> = builds
        .iter()
        .filter(|build| {
            build
                .build_state
                .as_deref()
                .is_some_and(|state| state.eq_ignore_ascii_case("FAILED"))
        })
        .map(|build| {
            format!(
                "build {}: {}",
                build.build_id.as_deref().unwrap_or("unknown"),
                build.state_reason.as_deref().unwrap_or("no reason given"),
            )
        })
        .collect();

    if reasons.is_empty() {
        "no failed build carried a reason".to_string()
    } else {
        reasons.join("; ")
    }
}

/// Whether a MicroVMs API error means the resource is already gone, which deletion treats as
/// success.
fn is_remote_resource_absent(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{
        deserialize_controller, serialize_controller, MockPlatformServiceProvider,
        ResourceController,
    };
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageResponse, MicrovmImage, MicrovmImageVersion, MockLambdaMicrovmsApi,
        UpdateMicrovmImageResponse,
    };
    use alien_core::{Platform, SandboxCode, SandboxEgress, SandboxSessionPolicy};
    use std::sync::Arc;

    const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";
    const BUILD_ROLE_ARN: &str = "arn:aws:iam::123456789012:role/test-agents-build";
    const BUNDLE_URI: &str = "s3://alien-bundles-test/sandbox/bundle.zip";
    const DENY_CONNECTOR_ARN: &str =
        "arn:aws:lambda:us-east-1:123456789012:network-connector:nc-deny123";

    fn sandbox() -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: BUNDLE_URI.to_string(),
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: Some(1800),
                idle_suspend_seconds: Some(600),
            })
            .build()
    }

    /// The controller as the importer seeds it for a Live sandbox: build inputs present, no
    /// image yet.
    /// The same sandbox declaring a different bundle, which is what a new release looks like.
    fn sandbox_with_bundle(bundle_uri: &str) -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: bundle_uri.to_string(),
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: Some(1800),
                idle_suspend_seconds: Some(600),
            })
            .build()
    }

    fn runtime_seeded_controller() -> AwsSandboxController {
        AwsSandboxController {
            build_role_arn: Some(BUILD_ROLE_ARN.to_string()),
            bundle_uri: Some(BUNDLE_URI.to_string()),
            egress_connector_arns: vec![DENY_CONNECTOR_ARN.to_string()],
            preview_ports: vec![8080],
            ..Default::default()
        }
    }

    fn ready_controller() -> AwsSandboxController {
        AwsSandboxController {
            state: AwsSandboxState::Ready,
            image_identifier: Some(IMAGE_ARN.to_string()),
            image_arn: Some(IMAGE_ARN.to_string()),
            active_version: Some("1.0".to_string()),
            region: Some("us-east-1".to_string()),
            ..runtime_seeded_controller()
        }
    }

    fn provider(client: MockLambdaMicrovmsApi) -> Arc<MockPlatformServiceProvider> {
        let client = Arc::new(client);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(client.clone()));
        Arc::new(provider)
    }

    async fn executor(
        controller: AwsSandboxController,
        client: MockLambdaMicrovmsApi,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build")
    }

    fn active_version() -> MicrovmImageVersion {
        MicrovmImageVersion {
            image_arn: Some(IMAGE_ARN.to_string()),
            image_version: Some("1.0".to_string()),
            state: Some("SUCCESSFUL".to_string()),
            status: Some("ACTIVE".to_string()),
            state_reason: None,
        }
    }

    fn created_response() -> CreateMicrovmImageResponse {
        CreateMicrovmImageResponse {
            image_arn: Some(IMAGE_ARN.to_string()),
            name: Some("test-agents".to_string()),
            state: Some("CREATING".to_string()),
            image_version: Some("1.0".to_string()),
        }
    }

    fn not_found() -> alien_error::AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Microvm".to_string(),
            resource_name: "probe".to_string(),
        })
    }

    // ─────────────── CREATE FLOW ──────────────────────────────────────────

    /// A retry of the same release must replay, a new release must not: the token separates
    /// two bundles and stays put for one.
    #[test]
    fn the_client_token_tracks_the_bundle_not_just_the_resource() {
        let same_a = build_client_token("p-agents", "s3://b/sandbox-bundle/aaaa/bundle.zip");
        let same_b = build_client_token("p-agents", "s3://b/sandbox-bundle/aaaa/bundle.zip");
        let other = build_client_token("p-agents", "s3://b/sandbox-bundle/bbbb/bundle.zip");
        assert_eq!(
            same_a, same_b,
            "one bundle keeps one token so a retry replays"
        );
        assert_ne!(same_a, other, "a new bundle must not replay the old build");
        assert!(same_a.starts_with("p-agents-"));
    }

    /// The full runtime build: the create call must satisfy the already-deployed
    /// `sandbox/provision` grant — the `deployment` and `managed-by: runtime` request tags
    /// are what the policy conditions on, the build role is what it authorizes as a pass —
    /// and the recorded version must be the string AWS mints, `1.0`.
    #[tokio::test]
    async fn a_live_sandbox_builds_its_image_and_reaches_running() {
        let mut client = MockLambdaMicrovmsApi::new();
        // The pre-create probe: nothing under the derived name, so this is a real create.
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == "test-agents")
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .withf(|request| {
                request.name == "test-agents"
                    // Token carries the bundle digest so a new release is a new logical create.
                    && request.client_token.starts_with("test-agents-")
                    && request.client_token != "test-agents"
                    && request.build_role_arn == BUILD_ROLE_ARN
                    && request.code_artifact.uri == BUNDLE_URI
                    && request.base_image_arn
                        == "arn:aws:lambda:us-east-1:aws:microvm-image:al2023-1"
                    && request.base_image_version.as_deref() == Some("1")
                    && request.tags.get("deployment").map(String::as_str) == Some("test")
                    && request.tags.get("resource").map(String::as_str) == Some("agents")
                    && request.tags.get("managed-by").map(String::as_str) == Some("runtime")
                    && request.egress_network_connectors
                        == vec![
                            "arn:aws:lambda:us-east-1:aws:network-connector:aws-network-connector:INTERNET_EGRESS"
                                .to_string()
                        ]
                    && request.resources
                        == vec![MicrovmImageResources {
                            minimum_memory_in_mib: 2048,
                        }]
                    && request.logging == Some(MicrovmImageLogging::Disabled {})
                    && request
                        .hooks
                        .as_ref()
                        .is_some_and(|hooks| hooks.port == AGENT_PORT)
                    && request
                        .environment_variables
                        .get("ALIEN_SANDBOX_PORT")
                        .map(String::as_str)
                        == Some("8971")
            })
            .times(1)
            .returning(|_| Ok(created_response()));
        client
            .expect_get_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "1.0")
            .times(1)
            .returning(|_, _| Ok(active_version()));
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .returning(|_| {
                Ok(MicrovmImage {
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("CREATED".to_string()),
                })
            });

        let mut executor = executor(runtime_seeded_controller(), client).await;
        executor
            .run_until_terminal()
            .await
            .expect("create flow should succeed");

        assert_eq!(executor.status(), ResourceStatus::Running);

        let outputs = executor.outputs().expect("outputs after create");
        let outputs = outputs
            .downcast_ref::<SandboxOutputs>()
            .expect("sandbox outputs");
        assert_eq!(outputs.identifier.as_deref(), Some(IMAGE_ARN));

        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.active_version.as_deref(), Some("1.0"));
        assert!(
            controller.pending_version.is_none(),
            "the built version is promoted, not left pending"
        );

        let binding = controller
            .get_binding_params()
            .expect("binding params")
            .expect("an active image publishes a binding");
        assert_eq!(binding["imageArn"], IMAGE_ARN);
        assert_eq!(binding["imageVersion"], "1.0");
        assert_eq!(binding["region"], "us-east-1");
        assert_eq!(binding["egressConnectorArns"][0], DENY_CONNECTOR_ARN);
        assert_eq!(binding["previewPorts"][0], 8080);
        assert_eq!(binding["idleSuspendSeconds"], 600);
        assert_eq!(binding["maxLifetimeSeconds"], 1800);
    }

    /// A re-imported release lands here with fresh state while the image from the previous
    /// release still exists under the account-unique name. The probe must adopt that image and
    /// roll the new bundle onto it as a new version — a second create under the same name is
    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

    /// A new release's bundle rolls onto the existing image as a further version, and the
    /// binding keeps naming the version that is serving until the new one is ACTIVE. Publishing
    /// the pending version early would hand an application an image no session can start from.
    #[tokio::test]
    async fn a_changed_bundle_rolls_a_version_and_switches_the_binding_only_when_active() {
        const NEXT_BUNDLE: &str = "s3://alien-bundles-test/sandbox/bundle-v2.zip";

        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .withf(|identifier, request| {
                identifier == IMAGE_ARN
                    && request.code_artifact.uri == NEXT_BUNDLE
                    && request.build_role_arn == BUILD_ROLE_ARN
                    && request.base_image_arn
                        == "arn:aws:lambda:us-east-1:aws:microvm-image:al2023-1"
                    && request.resources
                        == vec![MicrovmImageResources {
                            minimum_memory_in_mib: 2048,
                        }]
                    && request.client_token.starts_with("agents-")
            })
            .times(1)
            .returning(|_, _| {
                Ok(UpdateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: Some("test-agents".to_string()),
                    state: Some("UPDATING".to_string()),
                    image_version: Some("2.0".to_string()),
                })
            });
        // Still building on the first poll, ACTIVE on the second: the binding must not move
        // until the second.
        let mut sequence = mockall::Sequence::new();
        client
            .expect_get_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "2.0")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_version: Some("2.0".to_string()),
                    state: Some("IN_PROGRESS".to_string()),
                    status: Some("INACTIVE".to_string()),
                    ..active_version()
                })
            });
        client
            .expect_get_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "2.0")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_version: Some("2.0".to_string()),
                    ..active_version()
                })
            });

        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(NEXT_BUNDLE))
            .expect("transition to update");
        executor.step().await.expect("updating_sandbox");
        executor.step().await.expect("updating_image");

        let binding = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller")
            .get_binding_params()
            .expect("binding params")
            .expect("the serving version keeps its binding while the roll builds");
        assert_eq!(
            binding["imageVersion"], "1.0",
            "the binding must name the version sessions can actually start from"
        );

        executor.step().await.expect("first poll, still building");
        let binding = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller")
            .get_binding_params()
            .expect("binding params")
            .expect("a building roll does not withdraw the binding");
        assert_eq!(binding["imageVersion"], "1.0", "still building, still 1.0");

        executor.step().await.expect("second poll, ACTIVE");

        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.active_version.as_deref(), Some("2.0"));
        assert!(controller.pending_version.is_none());
        assert_eq!(
            controller.previous_version.as_deref(),
            Some("1.0"),
            "the replaced version is retained until its sessions can no longer be running"
        );
        assert!(
            controller.retired_at.is_some(),
            "retention starts at the switch"
        );
        assert_eq!(
            controller.bundle_uri.as_deref(),
            Some(NEXT_BUNDLE),
            "the promoted bundle is what the sandbox now serves"
        );
        let binding = controller
            .get_binding_params()
            .expect("binding params")
            .expect("an active version publishes a binding");
        assert_eq!(binding["imageVersion"], "2.0");
    }

    /// A roll whose build fails must leave the previous version serving and the sandbox
    /// updatable — not provision-failed with no binding.
    #[tokio::test]
    async fn a_failed_roll_keeps_the_previous_version_serving() {
        const NEXT_BUNDLE: &str = "s3://alien-bundles-test/sandbox/bundle-v2.zip";

        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(|_, _| {
                Ok(UpdateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: Some("test-agents".to_string()),
                    state: Some("UPDATING".to_string()),
                    image_version: Some("2.0".to_string()),
                })
            });
        client
            .expect_get_microvm_image_version()
            .times(1)
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_version: Some("2.0".to_string()),
                    state: Some("FAILED".to_string()),
                    status: Some("INACTIVE".to_string()),
                    state_reason: Some("One or more builds failed".to_string()),
                    ..active_version()
                })
            });
        client
            .expect_list_microvm_image_builds()
            .times(1)
            .returning(|_, _| {
                Ok(vec![MicrovmImageBuild {
                    build_id: Some("build-9".to_string()),
                    build_state: Some("FAILED".to_string()),
                    state_reason: Some("bundle setup command exited non-zero".to_string()),
                }])
            });

        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(NEXT_BUNDLE))
            .expect("transition to update");
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a failed roll must fail the update flow");

        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("bundle setup command exited non-zero"),
            "the failed build's own reason must survive into the error: {rendered}"
        );
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            controller.active_version.as_deref(),
            Some("1.0"),
            "a failed roll must not disturb the version that is serving"
        );
        let binding = controller
            .get_binding_params()
            .expect("binding params")
            .expect("the previous version keeps its binding");
        assert_eq!(binding["imageVersion"], "1.0");
    }

    /// An update that changes nothing about the bundle must not rebuild: an image build is
    /// ~160s and a new billed version. The expectation-free mock panics on any call.
    #[tokio::test]
    async fn an_unchanged_bundle_rebuilds_nothing() {
        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        let mut executor = executor(controller, MockLambdaMicrovmsApi::new()).await;

        executor.update(sandbox()).expect("transition to update");
        executor.step().await.expect("updating_sandbox");

        assert_eq!(
            executor
                .internal_state::<AwsSandboxController>()
                .expect("typed controller")
                .active_version
                .as_deref(),
            Some("1.0")
        );
    }

    /// A Frozen sandbox's image belongs to the setup stack. Rebuilding it at runtime would use
    /// credentials that were never granted it, so a changed bundle is refused rather than built.
    #[tokio::test]
    async fn a_frozen_sandbox_refuses_to_rebuild_its_image() {
        let controller = AwsSandboxController {
            state: AwsSandboxState::Ready,
            image_identifier: Some(IMAGE_ARN.to_string()),
            image_arn: Some(IMAGE_ARN.to_string()),
            active_version: Some("1.0".to_string()),
            region: Some("us-east-1".to_string()),
            bundle_uri: Some(BUNDLE_URI.to_string()),
            // The importer's Frozen arm leaves the build role empty, and that absence is what
            // says setup owns the image.
            ..Default::default()
        };
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .service_provider(provider(MockLambdaMicrovmsApi::new()))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(
                "s3://alien-bundles-test/sandbox/bundle-v2.zip",
            ))
            .expect("transition to update");
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a Frozen sandbox must refuse the rebuild");
        assert!(
            error.to_string().contains("owned by stack creation"),
            "the refusal names its cause: {error}"
        );
    }

    /// The retired version is the only cleanup scope for sessions that cannot be enumerated, so
    /// it must survive its whole retention window and then actually be deleted.
    #[tokio::test]
    async fn a_retired_version_is_deleted_only_after_its_sessions_can_no_longer_run() {
        // Inside the window: nothing may be deleted. Any delete call panics the mock.
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("2.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        controller.max_lifetime_seconds = Some(1800);
        controller.previous_version = Some("1.0".to_string());
        controller.retired_at = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
        let mut inside_window = executor(controller, client).await;
        inside_window
            .step()
            .await
            .expect("ready tick inside the window");
        assert_eq!(
            inside_window
                .internal_state::<AwsSandboxController>()
                .expect("typed controller")
                .previous_version
                .as_deref(),
            Some("1.0"),
            "a version whose sessions may still be running must not be deleted"
        );

        // Past the declared 1800s ceiling: the version is swept and the fields cleared.
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("2.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        client
            .expect_delete_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "1.0")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        controller.max_lifetime_seconds = Some(1800);
        controller.previous_version = Some("1.0".to_string());
        controller.retired_at = Some(chrono::Utc::now() - chrono::Duration::seconds(3600));
        let mut executor = executor(controller, client).await;
        executor.step().await.expect("ready tick past the window");

        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert!(controller.previous_version.is_none());
        assert!(controller.retired_at.is_none());
    }

    /// State written before a sandbox's image could be rebuilt carried one version field and no
    /// notion of an active one. It must re-hydrate with that version serving: reading it as "no
    /// active version" would withdraw the binding of a deployment that never changed.
    #[test]
    fn state_written_before_versions_were_tracked_keeps_its_binding() {
        let controller: AwsSandboxController = serde_json::from_value(serde_json::json!({
            "_controllerStateVersion": 1,
            "allowEgress": false,
            "egressConnectorArns": ["arn:aws:lambda:us-east-2:111122223333:network-connector:deny"],
            "imageArn": "arn:aws:lambda:us-east-2:111122223333:microvm-image:sbx-image",
            "imageIdentifier": "sbx-image",
            "imageVersion": "1.0",
            "internalStayCount": null,
            "previewPorts": [8080],
            "region": "us-east-2",
            "state": "ready",
            "type": "AwsSandboxController"
        }))
        .expect("a settled record must re-hydrate");

        assert_eq!(controller.active_version.as_deref(), Some("1.0"));

        let binding = controller
            .get_binding_params()
            .expect("binding params")
            .expect("a settled record must keep publishing its binding");
        assert_eq!(
            binding["imageArn"],
            "arn:aws:lambda:us-east-2:111122223333:microvm-image:sbx-image"
        );
        assert_eq!(binding["imageVersion"], "1.0");
        assert_eq!(binding["region"], "us-east-2");
    }

    /// The failure error must carry AWS's own reasons — the version's and each failed
    /// build's. A generic error at the end of a ~160s build is the worst debugging
    /// experience this resource can produce.
    #[tokio::test]
    async fn a_failed_build_surfaces_the_services_own_reason() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .times(1)
            .returning(|_| Ok(created_response()));
        client
            .expect_get_microvm_image_version()
            .times(1)
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    state: Some("FAILED".to_string()),
                    status: Some("INACTIVE".to_string()),
                    state_reason: Some("One or more builds failed".to_string()),
                    ..active_version()
                })
            });
        client
            .expect_list_microvm_image_builds()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "1.0")
            .times(1)
            .returning(|_, _| {
                Ok(vec![MicrovmImageBuild {
                    build_id: Some("build-1".to_string()),
                    build_state: Some("FAILED".to_string()),
                    state_reason: Some(
                        "pull access denied for the Dockerfile base image".to_string(),
                    ),
                }])
            });

        let mut executor = executor(runtime_seeded_controller(), client).await;
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a failed build must fail the create flow");

        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("One or more builds failed"),
            "the version's own reason must survive into the error: {rendered}"
        );
        assert!(
            rendered.contains("pull access denied for the Dockerfile base image"),
            "the failed build's own reason must survive into the error: {rendered}"
        );
    }

    /// A version still building must not be reported Running or publish a binding: every
    /// field a binding needs is known from the moment the create call returns, ~160s before
    /// a session could actually start. This is the ACTIVE-polling guard.
    #[tokio::test]
    async fn a_still_building_image_is_not_reported_running() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .times(1)
            .returning(|_| Ok(created_response()));
        client.expect_get_microvm_image_version().returning(|_, _| {
            Ok(MicrovmImageVersion {
                state: Some("IN_PROGRESS".to_string()),
                status: Some("INACTIVE".to_string()),
                state_reason: None,
                ..active_version()
            })
        });

        let mut executor = executor(runtime_seeded_controller(), client).await;
        executor.step().await.expect("create step");
        executor.step().await.expect("first poll");
        executor.step().await.expect("second poll");

        assert_eq!(
            executor.status(),
            ResourceStatus::Provisioning,
            "a building image must keep the sandbox in Provisioning"
        );
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert!(
            controller
                .get_binding_params()
                .expect("binding params")
                .is_none(),
            "a building image must not publish a binding"
        );
    }

    /// `SUCCESSFUL` alone is not runnable — `RunMicrovm` only accepts `ACTIVE` versions —
    /// and an unknown state waits rather than binding to a half-built image.
    #[test]
    fn only_a_successful_and_active_version_is_ready() {
        assert!(matches!(
            version_readiness(Some("SUCCESSFUL"), Some("ACTIVE")),
            VersionReadiness::Ready
        ));
        assert!(matches!(
            version_readiness(Some("successful"), Some("active")),
            VersionReadiness::Ready
        ));
        for (state, status) in [
            (Some("PENDING"), Some("INACTIVE")),
            (Some("IN_PROGRESS"), Some("INACTIVE")),
            (Some("SUCCESSFUL"), Some("INACTIVE")),
            (Some("SUCCESSFUL"), None),
            (Some("SOMETHING_NEW"), Some("ACTIVE")),
            (None, None),
        ] {
            assert!(
                matches!(version_readiness(state, status), VersionReadiness::Building),
                "{state:?}/{status:?} must read as still building"
            );
        }
        assert!(matches!(
            version_readiness(Some("FAILED"), Some("INACTIVE")),
            VersionReadiness::Failed
        ));
        assert!(matches!(
            version_readiness(Some("failed"), None),
            VersionReadiness::Failed
        ));
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    /// Versions hold the image: the API accepts a delete on an image with versions present
    /// while removing nothing, so the order is load-bearing, not stylistic.
    #[tokio::test]
    async fn delete_removes_every_version_before_the_image() {
        let mut sequence = mockall::Sequence::new();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![
                    MicrovmImage {
                        image_identifier: None,
                        image_arn: Some(IMAGE_ARN.to_string()),
                        image_version: Some("1.0".to_string()),
                        state: Some("SUCCESSFUL".to_string()),
                    },
                    MicrovmImage {
                        image_identifier: None,
                        image_arn: Some(IMAGE_ARN.to_string()),
                        image_version: Some("2.0".to_string()),
                        state: Some("SUCCESSFUL".to_string()),
                    },
                ])
            });
        client
            .expect_delete_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "1.0")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        client
            .expect_delete_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "2.0")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("delete flow should succeed");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(executor.outputs().is_none());
    }

    /// A version record with no identifier cannot be deleted around: the image delete
    /// no-ops while versions survive, which would report Deleted without deleting.
    #[tokio::test]
    async fn a_versionless_record_fails_the_delete_instead_of_being_skipped() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .times(1)
            .returning(|_| {
                Ok(vec![MicrovmImage {
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: None,
                    state: Some("SUCCESSFUL".to_string()),
                }])
            });
        // No version delete and no image delete may run: refusing loudly is the point.

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the refusal must surface, not be retried");
        assert!(
            error.to_string().contains("no version identifier"),
            "the refusal names its cause: {error}"
        );
    }

    /// Deletion is best-effort: an image someone already removed is the goal state, not a
    /// failure.
    #[tokio::test]
    async fn deleting_an_absent_image_succeeds() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_delete_microvm_image()
            .times(1)
            .returning(|_| Err(not_found()));

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("an absent image deletes cleanly");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// A sandbox whose create failed before the image existed has nothing to delete, and
    /// must not call the API at all — the mock has no expectations, so any call panics.
    #[tokio::test]
    async fn a_sandbox_that_never_recorded_its_image_still_sweeps_by_name() {
        // A create can succeed without its ARN reaching state; walking past it would leak a
        // live image forever, so the delete sweeps the deterministic name instead.
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .withf(|identifier| identifier.ends_with("-agents"))
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier.ends_with("-agents"))
            .times(1)
            .returning(|_| Err(not_found()));
        let controller = AwsSandboxController {
            state: AwsSandboxState::Ready,
            ..runtime_seeded_controller()
        };
        let mut executor = executor(controller, client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("nothing to delete");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// The image of a setup-baked sandbox belongs to the setup stack; the runtime credentials
    /// were never granted its deletion, and issuing one anyway is the ownership violation this
    /// pins. An expectation-free mock panics on any call.
    #[tokio::test]
    async fn a_setup_baked_sandbox_deletes_without_touching_its_image() {
        let controller = AwsSandboxController {
            state: AwsSandboxState::Ready,
            image_identifier: Some(IMAGE_ARN.to_string()),
            image_arn: Some(IMAGE_ARN.to_string()),
            active_version: Some("1.0".to_string()),
            region: Some("us-east-1".to_string()),
            // The importer's Frozen arm leaves the build inputs empty, and that absence is
            // what says setup owns the image.
            ..Default::default()
        };
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .service_provider(provider(MockLambdaMicrovmsApi::new()))
            .build()
            .await
            .expect("executor should build");
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("delete must complete without cloud calls");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// The binding promises sessions can start now; it must be withdrawn before the first
    /// delete, not after the last one.
    #[tokio::test]
    async fn the_binding_is_withdrawn_the_moment_deletion_begins() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .times(1)
            .returning(|_| {
                Ok(vec![MicrovmImage {
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: None,
                    state: Some("SUCCESSFUL".to_string()),
                }])
            });
        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect_err("the refusal must surface, not be retried");
        let controller: &AwsSandboxController = executor
            .internal_state()
            .expect("the sandbox controller is inspectable");
        assert!(
            controller
                .get_binding_params()
                .expect("binding params readable")
                .is_none(),
            "a sandbox being deleted must not keep publishing a binding"
        );
    }

    // ─────────────── WIRING ───────────────────────────────────────────────

    /// A controller must round-trip by tag. Miss the by-tag arm and the executor cannot
    /// resolve it, which surfaces as InitialSetupFailed with no per-resource error to read —
    /// it fails above the handler layer, so nothing logs a cause.
    #[test]
    fn controller_round_trips_by_tag() {
        let controller = ready_controller();

        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "AwsSandboxController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    /// Resolving a controller for a new deployment is a different path from deserializing
    /// saved state, so registering one does not imply the other.
    #[test]
    fn the_registry_resolves_an_aws_sandbox_controller() {
        let registry = crate::core::ResourceRegistry::with_built_ins();

        let controller = registry
            .get_controller(Sandbox::RESOURCE_TYPE, Platform::Aws)
            .expect("AWS must have a registered Sandbox controller");
        assert_eq!(controller.controller_type(), "AwsSandboxController");
    }

    #[test]
    fn arns_follow_the_partition_of_the_region() {
        assert_eq!(
            managed_base_image_arn("us-gov-west-1"),
            "arn:aws-us-gov:lambda:us-gov-west-1:aws:microvm-image:al2023-1"
        );
        assert_eq!(
            internet_egress_connector_arn("cn-north-1"),
            "arn:aws-cn:lambda:cn-north-1:aws:network-connector:aws-network-connector:INTERNET_EGRESS"
        );
        assert_eq!(
            managed_base_image_arn("eu-west-1"),
            "arn:aws:lambda:eu-west-1:aws:microvm-image:al2023-1"
        );
    }
}
