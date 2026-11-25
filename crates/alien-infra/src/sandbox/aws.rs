//! AWS Sandbox controller.
//!
//! The durable parent is a Lambda MicroVM image. A Live sandbox's image is built here, after
//! the deployment registers: setup installs the build role and egress connector and registers
//! the bundle, and only at runtime does a customer account exist as a principal Alien's
//! registry can open to. A release that changes the bundle re-enters that flow against an image
//! that already exists and rolls a new version onto it. A Frozen sandbox a template set up
//! arrives through the importer with its image already built by stack creation, and this
//! controller only watches it; one a direct setup registered is built here while setup runs.
//!
//! Sessions are MicroVMs started from the image at runtime. `RunMicrovm` has no `tags`, so
//! image plus version *is* the session identity; `lambda:ListMicrovms` is account-wide and
//! granted to no sandbox set, so sessions cannot be enumerated here. Each lasts until its caller
//! terminates it or AWS ends it, after eight hours at the latest.

use std::collections::BTreeMap;
use std::time::Duration;

use tracing::{debug, info, warn};

use crate::core::{
    state_utils::StackResourceStateExt, LaunchableSandboxVersions, ResourceControllerContext,
    ResourcePermissionsHelper,
};
use crate::error::{ErrorData, Result};
use alien_aws_clients::lambda_microvms::{is_refused_for_running_microvms, MicrovmImageBuild};
use alien_aws_clients::sandbox_image_build::image_build_inputs;
pub(crate) use alien_aws_clients::sandbox_image_build::{aws_partition, sandbox_image_arn};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::sandbox_build_role::{
    sandbox_build_role_arn, sandbox_build_role_name, SANDBOX_BUNDLE_STORAGE_ID,
};
use alien_core::sandbox_image::AWS_MICROVM;
use alien_core::sandbox_setup_inputs::aws_sandbox_renders_bundle;
use alien_core::{
    parse_bundle_uri, standard_resource_tags, BundleUri, ResourceOutputs as CoreResourceOutputs,
    ResourceRef, ResourceStatus, Sandbox, SandboxCode, SandboxOutputs, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use sha2::{Digest, Sha256};

/// A build takes ~110-160s, gated on the in-image `Ready` hook (itself capped at 120s).
/// 90 polls at this interval is a 15-minute ceiling — an order of magnitude past a healthy
/// build, so hitting it means the image is wedged, not slow.
const BUILD_POLL_INTERVAL: Duration = Duration::from_secs(10);
const BUILD_MAX_POLLS: u32 = 90;
/// How many times a delete refused while the image settles is retried, at
/// `BUILD_POLL_INTERVAL`, before the delete fails: ten minutes.
const IMAGE_DELETE_MAX_POLLS: u32 = 60;
/// How often a delete refused for running sessions is asked again.
const RUNNING_SESSIONS_POLL_INTERVAL: Duration = Duration::from_secs(60);
/// How long a delete waits for the image's sessions to end before it fails. Sessions cannot be
/// listed or terminated here (see the module doc), but AWS ends each one within eight hours of
/// its start, and none can start once the delete has withdrawn the binding; the margin covers a
/// session started just before that.
const RUNNING_SESSIONS_MAX_WAIT_MINUTES: i64 = 8 * 60 + 30;
/// AWS ends every MicroVM within this long of its start, suspended time included, and the limit
/// cannot be raised. A retired version can only hold sessions that started before it went
/// inactive, so it is gone from use this long after.
const SESSION_LIFESPAN_SECONDS: i64 = 28_800;
/// Extra wait after the session lifespan before a retired version is deleted.
const RETIRED_VERSION_DELETE_MARGIN_MINUTES: i64 = 30;
/// Versions AWS keeps per image. The limit is not read from the service; it only words the
/// message when a roll is refused for it.
const IMAGE_VERSION_QUOTA: usize = 50;
/// How often a roll waiting for a version slot runs the reclaim pass again. Each pass lists the
/// image, retires what no binding names and deletes what the session fence has released.
const VERSION_SLOT_POLL_INTERVAL: Duration = Duration::from_secs(300);
/// How long a roll waits for draining versions before it fails: the session fence, its margin
/// and an hour for the passes to land.
const VERSION_SLOT_MAX_WAIT_MINUTES: i64 =
    SESSION_LIFESPAN_SECONDS / 60 + RETIRED_VERSION_DELETE_MARGIN_MINUTES + 60;
/// Most retired versions marked inactive in one pass.
const MAX_RETIREMENTS_PER_PASS: usize = 10;

/// How long a replaced version stays ACTIVE before it is retired. A dependent whose own config
/// did not change picks up the new binding at its next binding comparison (every 330 s), and a
/// failed read skips a window, so the grace covers many windows. Nothing here can see which
/// binding a dependent's Lambda actually runs.
const RETIRE_GRACE_MINUTES: i64 = 60;
/// How long to leave retiring alone after AWS refused it for a missing grant, so a role that
/// predates the grant is not asked on every pass.
const RETIRE_DENIED_BACKOFF_HOURS: i64 = 6;

/// Why the inventory is reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// A build is about to be asked for and needs a free version slot.
    Needed,
    /// Steady-state maintenance: every version past its fence is deleted.
    Maintenance,
}

/// What the sandbox's dependents can still launch.
#[derive(Debug, Default)]
struct DependentVersions {
    /// Versions an observed binding names.
    named: std::collections::BTreeSet<String>,
    /// Dependents whose launchable versions could not be proven.
    unproven: Vec<String>,
}

/// A version a roll replaced, and when it stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetiredVersion {
    pub(crate) version: String,
    pub(crate) retired_at: chrono::DateTime<chrono::Utc>,
    /// When the version was set `INACTIVE`, which blocks new sessions on it. Deleting waits out
    /// the session lifespan after this. Absent on state written before versions were retired,
    /// and until the version has been seen not serving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) inactive_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl RetiredVersion {
    pub(crate) fn new(version: String) -> Self {
        Self {
            version,
            retired_at: chrono::Utc::now(),
            inactive_at: None,
        }
    }

    /// When the version may be deleted: after its sessions can no longer be running.
    fn deletable_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.inactive_at.map(|at| {
            at + chrono::Duration::seconds(SESSION_LIFESPAN_SECONDS)
                + chrono::Duration::minutes(RETIRED_VERSION_DELETE_MARGIN_MINUTES)
        })
    }
}

/// AWS Sandbox controller.
#[controller]
pub struct AwsSandboxController {
    /// Withhold new internet bindings until actual EC2 routes have passed verification.
    #[serde(default)]
    pub(crate) egress_network_verified: bool,
    /// Bound installed with the parent and published to runtime bindings.
    #[serde(default)]
    pub(crate) egress: Option<alien_core::bindings::SandboxEgressBinding>,
    /// Public signing material; the private key remains only in the secrets vault and is never
    /// read back. Other internet sandboxes take the key from the owner's copy of this field.
    #[serde(default)]
    pub(crate) egress_public_key: Option<super::egress::GatewayPublicKey>,
    /// Digest of the gateway configuration this owner last published, without the replica
    /// secret. An unchanged configuration is not read or rewritten.
    #[serde(default)]
    pub(crate) egress_config_digest: Option<String>,
    /// The gateway reads its replica secret from Secrets Manager and that secret exists, so the
    /// published configuration names the source and never embeds or reads the secret.
    #[serde(default)]
    pub(crate) egress_replica_secret_ready: bool,
    /// Resource default retained independently of the gateway metadata.
    #[serde(default)]
    pub(crate) default_policy: Option<alien_core::EgressPolicy>,
    /// Startup security contract of the active version (empty for legacy images).
    #[serde(default)]
    pub(crate) supervisor_environment: BTreeMap<String, String>,
    /// Startup security contract promoted only after a successful build.
    #[serde(default)]
    pub(crate) pending_supervisor_environment: BTreeMap<String, String>,
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
    /// Previously published versions. Retained for checkpoint compatibility and
    /// explicit recovery; elapsed time since promotion does not prove consumer drain.
    #[serde(default)]
    pub(crate) retired_versions: Vec<RetiredVersion>,
    /// When AWS last refused to mark a version inactive for a missing grant. Retiring waits
    /// out a backoff after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retire_denied_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Dependents whose launchable sandbox versions could not be proven by this step's retiring
    /// pass, so replaced versions were kept for them. Reported with a refused roll; not saved.
    #[serde(skip)]
    pub(crate) retire_blocked_by: Vec<String>,
    /// Why a roll is waiting for a version slot, shown as the step's progress; not saved.
    #[serde(skip)]
    pub(crate) version_slot_wait: Option<String>,
    /// When the roll first found the image full with versions draining. Bounds the wait for
    /// them. The `wait_for_*_started_at` name makes a retry start a fresh window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) wait_for_version_slot_started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Accepted deletion, retained until the provider confirms absence.
    #[serde(default)]
    pub(crate) deleting_version: Option<String>,
    /// When the image delete was first refused because sessions started from the image were
    /// still running. Bounds the wait for them to end. The `wait_for_*_started_at` name makes a
    /// retry clear it, so a retried delete gets a fresh window.
    #[serde(default)]
    pub(crate) wait_for_running_sessions_started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Image deletes refused while the image settles, in this delete. Counted here rather than
    /// through the framework's stay budget, which the running-sessions wait also draws on. The
    /// `wait_for_*_iterations` name makes a retry reset it.
    #[serde(default)]
    pub(crate) wait_for_image_settle_iterations: u32,
    /// Changes only after a build is observed failed, so retries do not
    /// replay an idempotency receipt for a deleted failed build.
    #[serde(default)]
    pub(crate) build_retry_generation: u64,
    /// Region the MicroVMs run in. Held here because the binding is built without a context.
    pub(crate) region: Option<String>,
    /// Role passed to `CreateMicrovmImage`. Setup owns it because `sandbox/provision` grants
    /// `iam:PassRole` and no `iam:CreateRole`; the registration hands it over.
    #[serde(default)]
    pub(crate) build_role_arn: Option<String>,
    /// Bundle the image is built from, handed over by the registration.
    #[serde(default)]
    pub(crate) bundle_uri: Option<String>,
    /// Bundle setup last granted the build role, for a Frozen sandbox a direct setup registered:
    /// its policy names one bundle object, so a roll waits until setup has granted the new one.
    /// For a Live sandbox that builds from published bundles it names the store the build role
    /// reads.
    #[serde(default)]
    pub(crate) granted_bundle_uri: Option<String>,
    /// The rendered bundle last written to the deployment's bundle Storage. Its key is its
    /// content's digest, so a build whose bundle is already there skips the write.
    #[serde(default)]
    pub(crate) written_bundle_uri: Option<String>,
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
    pub(crate) idle_pause_seconds: Option<u32>,
    /// Session lifetime ceiling for the binding, captured from the declaration.
    #[serde(default)]
    pub(crate) max_lifetime_seconds: Option<u32>,
}

impl super::egress::RecordsEgressKey for AwsSandboxController {
    fn egress_public_key(&self) -> Option<&super::egress::GatewayPublicKey> {
        self.egress_public_key.as_ref()
    }
}

#[controller]
impl AwsSandboxController {
    fn get_progress(&self) -> Option<&str> {
        match self.state {
            AwsSandboxState::UpdatingImage => self.version_slot_wait.as_deref(),
            _ => None,
        }
    }

    fn has_runtime_setup_inputs(&self) -> bool {
        // Built-image imports omit the role. Runtime registrations and resumed
        // runtime builds retain it, including Frozen-to-Live handoffs.
        self.build_role_arn.is_some()
    }

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
        self.egress_network_verified = false;
        self.egress = Some(config.into());

        // Direct setup steps a Frozen sandbox before its build role exists; the registration lands
        // only once the role is ready, so until then the build waits rather than fails.
        if self.build_role_arn.is_none()
            && resource_lifecycle(ctx, &config.id) == Some(alien_core::ResourceLifecycle::Frozen)
        {
            return Err(AlienError::new(ErrorData::DependencyNotReady {
                resource_id: config.id.clone(),
                dependency_id: sandbox_build_role_name(ctx.resource_prefix, &config.id),
            }));
        }

        let aws_config = ctx.get_aws_config()?;
        // Derived, not read back from what setup registered. Both are functions of the desired
        // config and the deployment's own names, so a create that restarts after a failure still
        // has them — and the adopt below, which is what makes that restart safe, is reachable.
        let build_role_arn = sandbox_build_role_arn(
            aws_partition(&aws_config.region),
            &aws_config.account_id,
            ctx.resource_prefix,
            &config.id,
        );
        let bundle = self.bundle_to_build(ctx, config, &aws_config.region)?;
        let bundle_uri = bundle.uri();
        if self.needs_bundle_write(&bundle) {
            return Ok(HandlerAction::Continue {
                state: WritingBundle,
                suggested_delay: None,
            });
        }
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let image_name = sandbox_image_name(ctx.resource_prefix, &config.id);
        let tier = config
            .microvm_tier()
            .context(ErrorData::ResourceConfigInvalid {
                message: "declared limits do not fit any MicroVM tier".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        self.idle_pause_seconds = config.lifecycle.idle_pause_seconds;
        self.max_lifetime_seconds = config.lifecycle.max_lifetime_seconds;
        self.preview_ports = config.preview_ports.clone();

        // The vendor's base image travels inside the bundle's Dockerfile, already resolved to
        // a registry the build role can reach; the only image this call names is AWS's managed
        // base, and the build's connector is AWS's own — it must reach a registry, which the
        // session-time deny connector cannot.
        let inputs = image_build_inputs(
            &aws_config.region,
            &config.id,
            &image_name,
            &build_role_arn,
            &bundle_uri,
            tier.baseline_memory_mib,
        )
        .with_agent_environment(
            agent_environment(ctx, config)?,
            alien_core::sandbox_image_permissions::aws_microvm_session_hooks(config),
        )
        .with_receipt()
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: "Cannot encode MicroVM build request".to_string(),
            resource_id: Some(config.id.clone()),
        })?;

        // A create whose response never reached state leaves an image under this
        // account-unique name, and a second create would collide with it. Read first and adopt
        // what is there; a bundle that changed later is rolled by the update flow, not here.
        let existing = match client
            .get_microvm_image(&sandbox_image_arn(
                &aws_config.region,
                &aws_config.account_id,
                &image_name,
            ))
            .await
        {
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
                let request =
                    inputs.create_request(image_name.clone(), image_tags(ctx, &config.id));
                let created = create_image(ctx, client.as_ref(), &config.id, request)
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
            // Recover an accepted request by its exact receipt. Legacy images without
            // a receipt need a build before we can claim the desired bundle is deployed.
            Some(image) => {
                let identifier = image.image_arn.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "GetMicrovmImage for '{image_name}' returned no image ARN"
                        ),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
                let versions = client
                    .list_microvm_image_versions(&identifier)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to recover MicroVM image '{image_name}' build"),
                        resource_id: Some(config.id.clone()),
                    })?;
                if let Some(recovered) = versions.into_iter().find(|version| {
                    version.description.as_deref() == Some(inputs.description.as_str())
                }) {
                    (
                        "CreateMicrovmImage",
                        Some(identifier),
                        recovered.image_version,
                    )
                } else {
                    // An older build has no matching receipt. Reconcile failed
                    // inventory before the create-recovery request, too.
                    self.image_identifier = Some(identifier.clone());
                    if self
                        .reconcile_image_inventory(ctx, &client, &config.id, Slot::Needed)
                        .await?
                    {
                        return Ok(HandlerAction::Continue {
                            state: CreatingImage,
                            suggested_delay: Some(BUILD_POLL_INTERVAL),
                        });
                    }
                    let rolled = self
                        .roll_image(
                            ctx,
                            client.as_ref(),
                            &config.id,
                            &identifier,
                            inputs.update_request(),
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to rebuild adopted MicroVM image '{image_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    if let Some(replaced) = image.image_version {
                        self.retired_versions.push(RetiredVersion::new(replaced));
                    }
                    ("UpdateMicrovmImage", Some(identifier), rolled.image_version)
                }
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
        self.pending_supervisor_environment = agent_environment(ctx, config)?;
        self.region = Some(aws_config.region.clone());

        info!(sandbox_id = %config.id, image = %image_name, "MicroVM image build started");

        Ok(HandlerAction::Continue {
            state: WaitingForImageActive,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    /// Writes the rendered bundle the create builds from into the deployment's own bucket.
    #[handler(
        state = WritingBundle,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn writing_bundle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.write_bundle(ctx).await?;
        Ok(HandlerAction::Continue {
            state: CreatingImage,
            suggested_delay: None,
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
        // A retry resumes here after a failed build. With no version left to poll, the fix is
        // a fresh build, which the update path submits from the desired bundle.
        if self.pending_version.is_none() && self.image_identifier.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdatingImage,
                suggested_delay: None,
            });
        }
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
                // Drop the dead version so a retry resubmits the build instead of polling it
                // again; the serving version, if any, is untouched.
                self.pending_version = None;
                self.pending_bundle_uri = None;
                self.build_retry_generation += 1;
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
                    state: ApplyingResourcePermissions,
                    suggested_delay: None,
                })
            }
        }
    }

    /// Grants each permission profile what it was given on this sandbox, scoped to its image.
    ///
    /// A Worker that creates sessions needs `lambda:RunMicrovm` and friends on this image, and
    /// those come only from the profile's resource-scoped sets. The helper writes them only
    /// while setup owns the sandbox (a Frozen one a direct setup builds); a template-installed
    /// sandbox gets the same policies from the template.
    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        // The bare id: the sandbox sets name the image as `${stackPrefix}-${resourceName}`, and
        // the image is named `<resource prefix>-<id>`.
        ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
            ctx,
            &config.id,
            &config.id,
            Sandbox::RESOURCE_TYPE.as_ref(),
        )
        .await?;

        Ok(HandlerAction::Continue {
            state: InitializingEgressKey,
            suggested_delay: None,
        })
    }

    #[handler(state = InitializingEgressKey, on_failure = ProvisionFailed, status = ResourceStatus::Provisioning)]
    async fn initializing_egress_key(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        self.default_policy = config.default_policy.clone();
        let ids = super::egress::internet_sandbox_ids(ctx.desired_stack);
        if alien_core::sandbox_egress::has_sandbox_gateway(ctx.desired_stack)
            && ids.iter().any(|id| id == ctx.desired_config.id())
        {
            let owner = alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack)
                .unwrap_or_default();
            if owner == ctx.desired_config.id() {
                // Keep the key this deployment already uses, whichever sandbox recorded it;
                // only a deployment with none writes one. The private key is never read.
                if self.egress_public_key.is_none() {
                    self.egress_public_key = match super::egress::recorded_public_key::<Self>(ctx)?
                    {
                        Some(key) => Some(key),
                        None => {
                            let vault = super::egress::deployment_vault(ctx).await?;
                            Some(super::egress::create_signing_key(vault.as_ref()).await?)
                        }
                    };
                }
            } else {
                self.egress_public_key = Some(super::egress::owner_public_key::<Self>(ctx, owner)?);
            }
            let key = self
                .egress_public_key
                .as_ref()
                .expect("key initialized above");
            self.egress = Some(alien_core::bindings::SandboxEgressBinding::Gateway(
                alien_core::bindings::SandboxEgressGateway {
                    links: alien_core::sandbox_links::cluster_link_bindings(
                        ctx.desired_stack,
                        config,
                    ),
                    container: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into(),
                    audience: format!("{}/{}", ctx.resource_prefix, config.id),
                    signing_key: alien_core::bindings::SandboxEgressSigningKey {
                        vault: alien_core::SECRETS_VAULT_ID.into(),
                        key: super::egress::SIGNING_KEY.into(),
                    },
                    kid: key.kid.clone(),
                    admin_ca_pem: None,
                    replicas: None,
                },
            ));
        } else {
            self.egress_public_key = None;
            self.egress_config_digest = None;
            self.egress_replica_secret_ready = false;
            self.egress_network_verified = false;
            self.egress = Some(config.into());
        }
        Ok(HandlerAction::Continue {
            state: PublishingEgressConfig,
            suggested_delay: None,
        })
    }

    #[handler(state = PublishingEgressConfig, on_failure = ProvisionFailed, status = ResourceStatus::Provisioning)]
    async fn publishing_egress_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if let Some(key) = &self.egress_public_key {
            self.egress_network_verified = false;
            if super::egress::is_declared(ctx) {
                super::egress::publish_aws_config(
                    ctx,
                    key,
                    &mut self.egress_config_digest,
                    &mut self.egress_replica_secret_ready,
                )
                .await?;
                self.egress_network_verified = true;
            }
        }
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
        // Preview tokens follow the current release, including old Ready checkpoints whose
        // update already completed without refreshing their authorization list.
        self.preview_ports = config.preview_ports.clone();

        let declared = super::egress::is_declared(ctx);
        if matches!(config.egress, alien_core::SandboxEgress::Internet) {
            self.egress_network_verified = false;
            if declared {
                super::egress::validate_aws_network(ctx).await?;
                self.egress_network_verified = true;
            }
        }

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

            // A Frozen image belongs to setup. The lifecycle check, not incidental
            // IAM from a Live sibling, prevents runtime inventory mutations.
            if self.owns_image_deletion(ctx, &config.id) {
                if self
                    .reconcile_image_inventory(ctx, &client, &config.id, Slot::Maintenance)
                    .await?
                {
                    return Ok(HandlerAction::Continue {
                        state: Ready,
                        suggested_delay: Some(BUILD_POLL_INTERVAL),
                    });
                }
            }

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
            state: if self.egress_public_key.is_some() && declared {
                RefreshingEgressConfig
            } else {
                Ready
            },
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // Refresh the audience list when another Sandbox is added or removed, even
    // when this owner's own resource configuration did not change. A sandbox saved in this
    // state before its deletion began publishes nothing.
    #[handler(state = RefreshingEgressConfig, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn refreshing_egress_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if let Some(key) = &self.egress_public_key {
            self.egress_network_verified = false;
            if super::egress::is_declared(ctx) {
                super::egress::publish_aws_config(
                    ctx,
                    key,
                    &mut self.egress_config_digest,
                    &mut self.egress_replica_secret_ready,
                )
                .await?;
                self.egress_network_verified = true;
            }
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshingEgressConfig, RefreshFailed])]
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
        self.egress_network_verified = false;
        self.egress = Some(config.into());

        // The session-policy ceilings feed the binding and cost nothing to refresh.
        self.idle_pause_seconds = config.lifecycle.idle_pause_seconds;
        self.max_lifetime_seconds = config.lifecycle.max_lifetime_seconds;
        self.preview_ports = config.preview_ports.clone();

        // Ownership decides before the bundle is even read. A template's Frozen image belongs to
        // the setup stack, which owns its bundle too and hands none over — so there is nothing
        // here to compare against, and rebuilding would use credentials never granted it.
        if !self.owns_image_builds(ctx, &config.id) {
            info!(sandbox_id = %config.id, "Updated AWS sandbox configuration");
            return Ok(HandlerAction::Continue {
                state: InitializingEgressKey,
                suggested_delay: None,
            });
        }

        let aws_config = ctx.get_aws_config()?;
        let desired_bundle = self.bundle_to_build(ctx, config, &aws_config.region)?.uri();

        if self.bundle_uri.as_deref() == Some(desired_bundle.as_str())
            && self.supervisor_environment == agent_environment(ctx, config)?
        {
            info!(sandbox_id = %config.id, "Updated AWS sandbox configuration");
            return Ok(HandlerAction::Continue {
                state: InitializingEgressKey,
                suggested_delay: None,
            });
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

        let image_identifier = self.image_identifier.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "no MicroVM image is recorded for this sandbox; there is nothing to \
                          roll a new version onto"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let bundle = self.bundle_to_build(ctx, config, &aws_config.region)?;
        let desired_bundle = bundle.uri();
        if resource_lifecycle(ctx, &config.id) == Some(alien_core::ResourceLifecycle::Frozen)
            && self.granted_bundle_uri.as_deref() != Some(desired_bundle.as_str())
        {
            return Err(AlienError::new(ErrorData::DependencyNotReady {
                resource_id: config.id.clone(),
                dependency_id: sandbox_build_role_name(ctx.resource_prefix, &config.id),
            }));
        }
        let build_role_arn = sandbox_build_role_arn(
            aws_partition(&aws_config.region),
            &aws_config.account_id,
            ctx.resource_prefix,
            &config.id,
        );
        let tier = config
            .microvm_tier()
            .context(ErrorData::ResourceConfigInvalid {
                message: "declared limits do not fit any MicroVM tier".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        if self.needs_bundle_write(&bundle) {
            return Ok(HandlerAction::Continue {
                state: WritingRolledBundle,
                suggested_delay: None,
            });
        }
        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        let request = image_build_inputs(
            &aws_config.region,
            &config.id,
            &sandbox_image_name(ctx.resource_prefix, &config.id),
            &build_role_arn,
            &desired_bundle,
            tier.baseline_memory_mib,
        )
        .with_agent_environment(
            agent_environment(ctx, config)?,
            alien_core::sandbox_image_permissions::aws_microvm_session_hooks(config),
        )
        .for_roll(self.active_version.as_deref())
        .for_retry(self.build_retry_generation)
        .with_receipt()
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: "Cannot encode MicroVM build request".to_string(),
            resource_id: Some(config.id.clone()),
        })?
        .update_request();
        let inventory = client
            .list_microvm_image_versions(&image_identifier)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to recover MicroVM build for '{image_identifier}'"),
                resource_id: Some(config.id.clone()),
            })?;
        let recovered = inventory
            .iter()
            .find(|version| version.description == request.description);
        let rolled = if let Some(recovered) = recovered {
            alien_aws_clients::lambda_microvms::UpdateMicrovmImageResponse {
                image_arn: recovered.image_arn.clone(),
                image_version: recovered.image_version.clone(),
                name: None,
                state: recovered.state.clone(),
            }
        } else {
            // Saved UpdatingImage retries enter here directly. Accepted builds
            // are recovered above before any failed inventory can be removed.
            if self
                .reconcile_image_inventory(ctx, &client, &config.id, Slot::Needed)
                .await?
            {
                return Ok(HandlerAction::Continue {
                    state: UpdatingImage,
                    suggested_delay: Some(BUILD_POLL_INTERVAL),
                });
            }
            // Reclaim has run and no deletion is under way. A full image refuses the build, so
            // it is not asked: while versions drain the roll waits on this state and runs the
            // reclaim again, and only a cap with nothing draining is an error.
            if inventory.len() >= IMAGE_VERSION_QUOTA {
                let message =
                    self.version_cap_message(&image_identifier, &inventory.len().to_string());
                let draining = self
                    .retired_versions
                    .iter()
                    .any(|entry| entry.inactive_at.is_some());
                let now = chrono::Utc::now();
                let since = *self.wait_for_version_slot_started_at.get_or_insert(now);
                if !draining
                    || now - since > chrono::Duration::minutes(VERSION_SLOT_MAX_WAIT_MINUTES)
                {
                    self.version_slot_wait = None;
                    self.wait_for_version_slot_started_at = None;
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message,
                        resource_id: Some(config.id.clone()),
                    }));
                }
                info!(sandbox_id = %config.id, image = %image_identifier, "{message}");
                self.version_slot_wait = Some(message);
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(VERSION_SLOT_POLL_INTERVAL),
                });
            }
            self.version_slot_wait = None;
            self.wait_for_version_slot_started_at = None;
            self.roll_image(ctx, client.as_ref(), &config.id, &image_identifier, request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to roll MicroVM image '{image_identifier}'"),
                    resource_id: Some(config.id.clone()),
                })?
        };

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
        self.pending_supervisor_environment = agent_environment(ctx, config)?;
        if let Some(arn) = rolled.image_arn {
            self.image_arn = Some(arn);
        }

        info!(sandbox_id = %config.id, image = %image_identifier, "MicroVM image roll started");

        Ok(HandlerAction::Continue {
            state: WaitingForRolledImageActive,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    /// The roll's [`Self::writing_bundle`], routed to `UpdateFailed`.
    #[handler(
        state = WritingRolledBundle,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn writing_rolled_bundle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.write_bundle(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdatingImage,
            suggested_delay: None,
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

    // Sessions cannot be enumerated and are not stopped here (see the module doc). A failed image
    // delete fails loudly here and the executor retries it; nothing is fired and forgotten.

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
        // Waits left by an earlier delete that ended another way must not count against this one.
        self.wait_for_running_sessions_started_at = None;
        self.wait_for_image_settle_iterations = 0;

        // A setup-baked image belongs to the setup stack and is removed by stack teardown;
        // deleting it here would act with credentials that were never granted it — and would
        // destroy a resource this controller does not own if they ever were.
        if !self.owns_image_deletion(ctx, &config.id) {
            return Ok(HandlerAction::Continue {
                state: DeletingEgressSecrets,
                suggested_delay: None,
            });
        }

        // No per-version sweep: deleting the image removes every version with it, and AWS
        // refuses to delete the last one on its own — "This is the last version. Please delete
        // the entire image". Sweeping first therefore failed teardown for every sandbox that
        // built once and was never rolled.
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
                state: DeletingEgressSecrets,
                suggested_delay: None,
            });
        }
        let aws_config = ctx.get_aws_config()?;
        let image_identifier = self.image_identifier.clone().unwrap_or_else(|| {
            sandbox_image_arn(
                &aws_config.region,
                &aws_config.account_id,
                &format!("{}-{}", ctx.resource_prefix, config.id),
            )
        });

        let client = ctx
            .service_provider
            .get_aws_microvms_client(aws_config)
            .await?;

        match client.delete_microvm_image(&image_identifier).await {
            Ok(()) => {}
            Err(error) if is_remote_resource_absent(&error) => {
                debug!(sandbox_id = %config.id, image = %image_identifier, "MicroVM image already gone");
            }
            // AWS refuses to delete an image while sessions started from it are running, and
            // ends every session within eight hours. Wait them out, bounded by that cap.
            Err(error) if is_refused_for_running_microvms(&error) => {
                let now = chrono::Utc::now();
                let since = *self.wait_for_running_sessions_started_at.get_or_insert(now);
                if now - since > chrono::Duration::minutes(RUNNING_SESSIONS_MAX_WAIT_MINUTES) {
                    // A retry starts a new wait rather than failing on this one at once.
                    self.wait_for_running_sessions_started_at = None;
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "MicroVM image '{image_identifier}' still has running sessions after \
                             waiting {} hours for them to end; terminate the sessions started \
                             from it and retry the delete",
                            RUNNING_SESSIONS_MAX_WAIT_MINUTES / 60
                        ),
                        resource_id: Some(config.id.clone()),
                    });
                }
                info!(
                    sandbox_id = %config.id,
                    image = %image_identifier,
                    waiting_since = %since,
                    "MicroVM image still has running sessions; deleting it once they end"
                );
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(RUNNING_SESSIONS_POLL_INTERVAL),
                });
            }
            // An image that is still settling refuses deletion for a while; ask again, bounded.
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                self.wait_for_image_settle_iterations += 1;
                if self.wait_for_image_settle_iterations >= IMAGE_DELETE_MAX_POLLS {
                    return Err(AlienError::new(ErrorData::PollingTimeout {
                        state: format!("{:?}", self.state),
                        max_times: IMAGE_DELETE_MAX_POLLS,
                    }));
                }
                info!(sandbox_id = %config.id, image = %image_identifier, "MicroVM image cannot be deleted in its current state yet");
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(BUILD_POLL_INTERVAL),
                });
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete MicroVM image '{image_identifier}'"),
                    resource_id: Some(config.id.clone()),
                });
            }
        }

        self.active_version = None;
        self.wait_for_running_sessions_started_at = None;
        self.wait_for_image_settle_iterations = 0;
        info!(sandbox_id = %config.id, "AWS sandbox image deleted");

        Ok(HandlerAction::Continue {
            state: DeletingBundles,
            suggested_delay: None,
        })
    }

    /// The bundles go after the image, the last thing that could read them, and leave the
    /// deployment's bundle Storage empty for setup to delete.
    #[handler(
        state = DeletingBundles,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting_bundles(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        // The bucket the bundles went to, or the deployment's bundle Storage: a write that failed
        // after S3 accepted the object leaves no record of it.
        let bucket = match self.written_bundle_uri.as_deref().and_then(split_s3_uri) {
            Some((bucket, _)) => Some(bucket.to_string()),
            None if ctx
                .desired_stack
                .resources
                .contains_key(SANDBOX_BUNDLE_STORAGE_ID) =>
            {
                ctx.require_dependency::<crate::storage::AwsStorageController>(&ResourceRef::new(
                    Storage::RESOURCE_TYPE,
                    SANDBOX_BUNDLE_STORAGE_ID,
                ))?
                .bucket_name
                .clone()
            }
            None => None,
        };
        if let Some(bucket) = bucket {
            let client = ctx
                .service_provider
                .get_aws_s3_client(ctx.get_aws_config()?)
                .await?;
            super::aws_bundle::delete_all(client.as_ref(), &bucket, &config.id).await?;
        }
        self.written_bundle_uri = None;
        Ok(HandlerAction::Continue {
            state: DeletingEgressSecrets,
            suggested_delay: None,
        })
    }

    /// The deployment's shared egress signing key and gateway configuration are written into its
    /// own vault by the first internet sandbox and are used by every other one and by the
    /// gateway. They go with the last internet sandbox: the others depend on the writer, so it is
    /// deleted after them, and a sandbox removed while others remain leaves them in use.
    #[handler(
        state = DeletingEgressSecrets,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting_egress_secrets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        // A vault the deployment binds to rather than creates is not Alien-owned.
        let vault_is_owned = !ctx
            .deployment_config
            .external_bindings
            .has(alien_core::SECRETS_VAULT_ID);
        if self.egress_public_key.is_some()
            && vault_is_owned
            && !super::egress::other_internet_sandbox_remains(ctx, &config.id)
        {
            let vault = super::egress::deployment_vault(ctx).await?;
            let vault_name = format!("{}-{}", ctx.resource_prefix, alien_core::SECRETS_VAULT_ID);
            super::egress::delete_shared_secrets(vault.as_ref(), &vault_name).await?;
            // A direct setup has no setup stack to remove the replica secret this controller
            // created; a setup stack deletes its own.
            if self.egress_replica_secret_ready
                && ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            {
                let client = super::egress::replica_secret_client(ctx).await?;
                super::egress::delete_aws_replica_secret(client.as_ref(), ctx.resource_prefix)
                    .await?;
            }
            info!(sandbox_id = %config.id, "Deleted the deployment's sandbox egress key and gateway configuration");
        }
        self.egress_public_key = None;
        self.egress_config_digest = None;
        self.egress_replica_secret_ready = false;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{AwsSandboxBinding, BindingValue, SandboxBinding};

        if self.egress.as_ref().is_some_and(|egress| {
            matches!(egress.declaration(), alien_core::SandboxEgress::Internet)
        }) && !self.egress_network_verified
        {
            return Ok(None);
        }

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
            default_policy: self.default_policy.clone().or_else(|| {
                self.egress
                    .as_ref()
                    .and_then(|value| value.default_policy().cloned())
            }),
            egress: self.egress.as_ref().map(|value| match value {
                alien_core::bindings::SandboxEgressBinding::Gateway(_) => value.clone(),
                _ => value.declaration().clone().into(),
            }),
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
            idle_pause_seconds: self.idle_pause_seconds,
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

    /// A connection target that publishes new addresses changes the image's contract with no
    /// change to any config, so the update that rolls the image is asked for here: until it runs,
    /// new sessions still reach only the old addresses. A target that published nothing usable
    /// asks for nothing; the roll could not be built.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let connects = config
            .image_permissions
            .as_ref()
            .is_some_and(|permissions| !permissions.connections.is_empty());
        if !connects || self.image_identifier.is_none() || !self.owns_image_builds(ctx, &config.id)
        {
            return Ok(false);
        }
        Ok(agent_environment(ctx, config)
            .is_ok_and(|environment| environment != self.supervisor_environment))
    }

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
    /// Reads persisted state, mapping the two state names an earlier controller version wrote
    /// that this one does not have: `awaitImage` (a setup-baked image not yet observed) settles
    /// into `ready`, whose handler re-reads the image, and `deleting` resumes at the version
    /// sweep, where the ownership guard skips a setup-owned image.
    pub(crate) fn from_persisted(mut value: serde_json::Value) -> serde_json::Result<Self> {
        if let Some(state) = value.get("state").and_then(serde_json::Value::as_str) {
            let mapped = match state {
                "awaitImage" => Some("ready"),
                "deleting" => Some("deletingImageVersions"),
                _ => None,
            };
            if let Some(mapped) = mapped {
                value["state"] = serde_json::Value::String(mapped.to_string());
            }
        }
        serde_json::from_value(value)
    }

    /// Whether this controller owns the image's deletion: only a Live sandbox's. A Frozen image is
    /// deleted by whatever set it up, and a state without a lifecycle is never proof of ownership,
    /// because destroying a setup-owned image is the failure that cannot be retried.
    fn owns_image_deletion(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        resource_lifecycle(ctx, resource_id) == Some(alien_core::ResourceLifecycle::Live)
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

    /// Publish only a successfully built version, preserving prior explicit pins.
    fn promote_pending_version(&mut self) {
        let Some(pending) = self.pending_version.take() else {
            return;
        };
        if let Some(replaced) = self.active_version.replace(pending) {
            self.retired_versions.push(RetiredVersion::new(replaced));
        }
        if let Some(bundle) = self.pending_bundle_uri.take() {
            self.bundle_uri = Some(bundle);
            self.supervisor_environment = std::mem::take(&mut self.pending_supervisor_environment);
        }
    }

    /// Reconcile provider inventory: delete failed builds, retire replaced versions and delete
    /// the ones whose sessions can no longer be running.
    ///
    /// A replaced version that nothing serves or builds and no dependent still binds is set
    /// `INACTIVE`, which blocks new sessions but leaves running and suspended ones alone. It is
    /// deleted only once the longest session AWS allows has passed, because deleting a version
    /// under a running or suspended VM kills it. Run before every roll, so a version quota is
    /// reclaimed before it can refuse the roll, and in steady state.
    /// Return true while cleanup owns this step, including asynchronous deletion.
    async fn reconcile_image_inventory(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        client: &std::sync::Arc<dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi>,
        resource_id: &str,
        slot: Slot,
    ) -> Result<bool> {
        let Some(identifier) = self.image_identifier.clone() else {
            return Ok(false);
        };
        let identifier = identifier.as_str();
        if let Some(version) = self.deleting_version.as_deref() {
            match client.get_microvm_image_version(identifier, version).await {
                Ok(_) => return Ok(true),
                Err(error) if is_remote_resource_absent(&error) => {
                    self.retired_versions
                        .retain(|entry| entry.version != version);
                    self.deleting_version = None;
                    return Ok(true);
                }
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to confirm image version deletion for '{identifier}'"
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                }
            }
        }
        let inventory = client
            .list_microvm_image_versions(identifier)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to inventory MicroVM image '{identifier}'"),
                resource_id: Some(resource_id.to_string()),
            })?;
        let held = inventory.len();
        // AWS refuses to delete the last version. Retry a failed initial build
        // first, then reclaim it once the replacement exists.
        if inventory.len() == 1 {
            return Ok(false);
        }
        let mut succeeded = Vec::new();
        for version in inventory {
            let state = version.state.clone();
            let version = version.image_version.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("MicroVM image '{identifier}' inventory omitted a version"),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            if self.active_version.as_ref() == Some(&version)
                || self.pending_version.as_ref() == Some(&version)
            {
                continue;
            }
            match state.as_deref() {
                Some("FAILED") => {
                    match client
                        .delete_microvm_image_version(identifier, &version)
                        .await
                    {
                        Ok(()) => {}
                        Err(error) if is_remote_resource_absent(&error) => {}
                        Err(error) => {
                            return Err(error).context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to delete failed MicroVM image '{identifier}' \
                                     version '{version}'"
                                ),
                                resource_id: Some(resource_id.to_string()),
                            })
                        }
                    }
                    self.deleting_version = Some(version);
                    return Ok(true);
                }
                Some("SUCCESSFUL") => succeeded.push(version),
                _ => {}
            }
        }

        // A retired version AWS no longer lists is done.
        self.retired_versions
            .retain(|entry| succeeded.contains(&entry.version));

        // A successful version this state does not know came from before versions were
        // retired. It starts the same clock, from now.
        // Older than anything this state replaced, so they go first, in listing order.
        // Until a serving version is recorded (an adopted image before its first roll), nothing
        // says which version is in use, so none is treated as replaced.
        let unknown: Vec<RetiredVersion> = succeeded
            .iter()
            .filter(|_| self.active_version.is_some())
            .filter(|version| !self.retired_versions.iter().any(|e| &e.version == *version))
            .map(|version| {
                let mut entry = RetiredVersion::new(version.clone());
                // Not replaced just now, so no dependent is still catching up to a newer binding.
                entry.retired_at -= chrono::Duration::minutes(RETIRE_GRACE_MINUTES);
                entry
            })
            .collect();
        self.retired_versions.splice(0..0, unknown);

        // A dependent that has not yet rolled to the current release can still hold the
        // previous binding and so still start sessions on the version it names.
        //
        // The version replaced most recently stays ACTIVE as well: a dependent whose own config
        // did not change is re-rendered with the new binding only by propagation or the
        // Ready-time refresh, which can land after this pass.
        let retire_backoff = self.retire_denied_in_backoff();
        // Observing dependents costs provider reads, so it waits for a version it could retire.
        let now_for_grace = chrono::Utc::now();
        let keep = self.retired_versions.len().saturating_sub(1);
        let has_candidate = self.retired_versions.iter().take(keep).any(|entry| {
            entry.inactive_at.is_none()
                && now_for_grace
                    >= entry.retired_at + chrono::Duration::minutes(RETIRE_GRACE_MINUTES)
        });
        self.retire_blocked_by.clear();
        let dependents = if retire_backoff || !has_candidate {
            None
        } else {
            Some(self.dependent_versions(ctx, resource_id).await)
        };
        if let Some(dependents) = &dependents {
            self.retire_blocked_by = dependents.unproven.clone();
        }
        if let Some(dependents) = dependents.filter(|dependents| dependents.unproven.is_empty()) {
            let mut retired = 0;
            for entry in self.retired_versions.iter_mut().take(keep) {
                if entry.inactive_at.is_some() {
                    continue;
                }
                // A binding a dependent still holds names it: sessions can start on it.
                if dependents.named.contains(&entry.version) {
                    continue;
                }
                // Too recently replaced for every dependent to have picked up the new binding.
                if now_for_grace
                    < entry.retired_at + chrono::Duration::minutes(RETIRE_GRACE_MINUTES)
                {
                    continue;
                }
                if retired == MAX_RETIREMENTS_PER_PASS {
                    break;
                }
                match client
                    .update_microvm_image_version(
                        identifier,
                        &entry.version,
                        alien_aws_clients::lambda_microvms::MicrovmImageVersionStatus::Inactive,
                    )
                    .await
                {
                    Ok(()) => {
                        entry.inactive_at = Some(chrono::Utc::now());
                        self.retire_denied_at = None;
                        retired += 1;
                    }
                    // Gone since the listing; the next pass drops it.
                    Err(error) if is_remote_resource_absent(&error) => {}
                    // An install whose management role predates this grant cannot retire
                    // versions. It keeps them, as before, and rolls still work until the
                    // image reaches the version cap; setup adds the grant on its next update.
                    Err(error)
                        if matches!(
                            &error.error,
                            Some(CloudClientErrorData::RemoteAccessDenied { .. })
                        ) =>
                    {
                        warn!(
                            sandbox_id = %resource_id,
                            image = %identifier,
                            "Cannot mark replaced MicroVM image versions inactive: \
                             lambda:UpdateMicrovmImageVersion is not granted"
                        );
                        self.retire_denied_at = Some(chrono::Utc::now());
                        break;
                    }
                    // Cleanup never blocks an update: skip it for this pass and try again on
                    // the next.
                    Err(error) => {
                        warn!(
                            sandbox_id = %resource_id,
                            image = %identifier,
                            version = %entry.version,
                            error = %error,
                            "Failed to set a replaced MicroVM image version inactive; \
                             skipping cleanup for this pass"
                        );
                        break;
                    }
                }
            }
        }

        let now = chrono::Utc::now();
        let oldest_deletable = self
            .retired_versions
            .iter()
            .filter(|entry| entry.deletable_at().is_some_and(|at| now > at))
            .min_by_key(|entry| entry.inactive_at)
            .map(|entry| entry.version.clone());
        // A roll needs one free slot, not an empty image: the rest is the maintenance pass's.
        let slot_free = slot == Slot::Needed && held < IMAGE_VERSION_QUOTA;
        if let Some(version) = oldest_deletable.filter(|_| !slot_free) {
            match client
                .delete_microvm_image_version(identifier, &version)
                .await
            {
                Ok(()) => {}
                Err(error) if is_remote_resource_absent(&error) => {}
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete retired MicroVM image '{identifier}' version \
                             '{version}'"
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                }
            }
            self.deleting_version = Some(version);
            return Ok(true);
        }
        Ok(false)
    }

    /// The image versions the sandbox's dependents can still start a session on, from what each
    /// one runs and not from the release being applied: a dependent waiting on this sandbox's
    /// new version still holds the old one.
    ///
    /// Every resource with saved state that links this sandbox counts, including one whose link
    /// the pending release removed. Resources that never linked it are asked too, but only an
    /// answer that names versions or says it cannot tell counts (see below). A dependent that reports its launchable versions
    /// ([`ResourceController::launchable_sandbox_versions`]) names them, or says it cannot
    /// tell, which keeps every replaced version. A dependent that does not observe bindings is
    /// proven only by the older check: running with the config being applied, which holds the
    /// current binding. A dependent whose state cannot be read is unproven.
    async fn dependent_versions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> DependentVersions {
        let links_sandbox = |refs: &[alien_core::ResourceRef]| {
            refs.iter().any(|dependency| dependency.id() == resource_id)
        };
        let mut found = DependentVersions::default();
        // Linked dependents that do not observe bindings, with the compute cluster each
        // runs on. A cluster that answered completely covers them (see below).
        let mut legacy: Vec<(String, Option<String>)> = Vec::new();
        let mut proved_clusters = std::collections::BTreeSet::<String>::new();
        for (id, state) in &ctx.state.resources {
            let desired = ctx.desired_stack.resources.get(id);
            let linked = links_sandbox(&state.dependencies)
                || links_sandbox(&state.config.get_dependencies())
                || state
                    .previous_config
                    .as_ref()
                    .is_some_and(|previous| links_sandbox(&previous.get_dependencies()))
                || desired.is_some_and(|entry| links_sandbox(&entry.combined_dependencies()));
            if id == resource_id {
                continue;
            }
            // Nothing was ever applied for it: no workload holds a binding.
            if matches!(
                state.status,
                ResourceStatus::Pending | ResourceStatus::Deleted
            ) {
                continue;
            }
            let observed = match state.get_internal_controller() {
                Ok(Some(controller)) => {
                    controller
                        .launchable_sandbox_versions(ctx, id, resource_id)
                        .await
                }
                // A workload that exists but whose controller state is gone cannot be asked.
                Ok(None) if linked => Ok(LaunchableSandboxVersions::Unproven {
                    reason: "its controller state is not available".to_string(),
                }),
                Ok(None) => Ok(LaunchableSandboxVersions::NotObserved),
                Err(error) if linked => Err(error),
                Err(_) => Ok(LaunchableSandboxVersions::NotObserved),
            };
            match observed {
                Ok(LaunchableSandboxVersions::Known(versions)) => {
                    // Only a saved compute cluster speaks for the workloads on it; any
                    // other resource's answer covers just itself.
                    if state.resource_type == alien_core::ComputeCluster::RESOURCE_TYPE.to_string() {
                        proved_clusters.insert(id.clone());
                    }
                    found.named.extend(versions)
                }
                // A resource that never linked this sandbox and does not observe bindings
                // holds none. One that observes its scheduler's whole inventory answers for
                // every instance it can see, including a replica a finished rollout left
                // behind after the link that bound it was removed.
                Ok(LaunchableSandboxVersions::NotObserved) if !linked => {}
                Ok(LaunchableSandboxVersions::NotObserved) => {
                    // The older check. A setup-owned consumer holds the binding setup gave it,
                    // which a runtime release does not change.
                    let settled = state.lifecycle == Some(alien_core::ResourceLifecycle::Frozen)
                        || desired.is_some_and(|entry| {
                            state.status == ResourceStatus::Running && state.config == entry.config
                        });
                    if !settled {
                        // Only a container or daemon names a cluster that could observe it.
                        let cluster = state
                            .config
                            .downcast_ref::<alien_core::Container>()
                            .and_then(|container| container.cluster.clone())
                            .or_else(|| {
                                state
                                    .config
                                    .downcast_ref::<alien_core::Daemon>()
                                    .and_then(|daemon| daemon.cluster.clone())
                            });
                        legacy.push((id.clone(), cluster));
                    }
                }
                Ok(LaunchableSandboxVersions::Unproven { reason }) => {
                    warn!(
                        sandbox_id = %resource_id,
                        dependent = %id,
                        reason = %reason,
                        "A dependent's launchable sandbox versions are not proven"
                    );
                    found.unproven.push(id.clone());
                }
                Err(error) => {
                    warn!(
                        sandbox_id = %resource_id,
                        dependent = %id,
                        error = %error,
                        "Could not read which sandbox versions a dependent can launch"
                    );
                    found.unproven.push(id.clone());
                }
            }
        }
        // A container runs on the compute cluster its saved config names, and that cluster's
        // controller reports every replica its scheduler holds, whichever release linked
        // it. When that exact cluster answered completely, the container is proven and the
        // older config comparison, which only a release can satisfy, no longer applies.
        // A worker, or a container whose cluster is unknown or did not answer, stays unproven.
        for (id, cluster) in legacy {
            if !cluster.is_some_and(|cluster| proved_clusters.contains(&cluster)) {
                found.unproven.push(id);
            }
        }
        found
    }

    /// Rolls a new version, and says why when AWS refuses it for the version cap: how many
    /// versions the image holds and when the oldest retired one can be deleted.
    async fn roll_image(
        &self,
        ctx: &ResourceControllerContext<'_>,
        client: &dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi,
        sandbox_id: &str,
        image: &str,
        request: alien_aws_clients::lambda_microvms::UpdateMicrovmImageRequest,
    ) -> Result<alien_aws_clients::lambda_microvms::UpdateMicrovmImageResponse> {
        let error = match update_image(ctx, client, sandbox_id, image, request).await {
            Ok(rolled) => return Ok(rolled),
            Err(error) => error,
        };
        if !error_chain_has_code(&error, "VERSION_LIMIT_EXCEEDED") {
            return Err(error);
        }
        let held = client
            .list_microvm_image_versions(image)
            .await
            .map(|versions| versions.len().to_string())
            .unwrap_or_else(|_| "all".to_string());
        Err(error).context(ErrorData::CloudPlatformError {
            message: self.version_cap_message(image, &held),
            resource_id: Some(sandbox_id.to_string()),
        })
    }

    /// Why a roll cannot have a version slot: how many versions the image holds and what must
    /// happen before one frees.
    /// Whether AWS refused a retire for a missing grant recently enough that retiring waits.
    fn retire_denied_in_backoff(&self) -> bool {
        self.retire_denied_at.is_some_and(|at| {
            chrono::Utc::now() < at + chrono::Duration::hours(RETIRE_DENIED_BACKOFF_HOURS)
        })
    }

    fn version_cap_message(&self, image: &str, held: &str) -> String {
        let next = self
            .retired_versions
            .iter()
            .filter_map(RetiredVersion::deletable_at)
            .min();
        let when = match next {
            Some(at) => format!("the oldest retired version becomes deletable at {at}"),
            None if self.retire_denied_in_backoff() => "no version could be retired: the \
                 installed role cannot call lambda:UpdateMicrovmImageVersion, so replaced \
                 versions stay active until the setup stack is updated to grant it"
                .to_string(),
            None if !self.retire_blocked_by.is_empty() => format!(
                "no version has been retired yet: {} could not be proven to have stopped using \
                 the replaced versions, so they are kept until it is",
                self.retire_blocked_by.join(", ")
            ),
            None => "no replaced version is free to retire yet; versions a dependent still \
                     binds are kept until it rolls, and the newest replaced one stays for a \
                     grace period"
                .to_string(),
        };
        format!("MicroVM image '{image}' holds {held} of {IMAGE_VERSION_QUOTA} versions; {when}")
    }

    /// Whether this controller built the image and may therefore rebuild it: a Live sandbox, or a
    /// Frozen one a direct setup registered build inputs for, which only setup steps. A template's
    /// Frozen image belongs to the setup stack, which owns its bundle too.
    fn owns_image_builds(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        self.owns_image_deletion(ctx, resource_id)
            || (self.build_role_arn.is_some()
                && ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup)
    }
}

/// The image a rendered bundle builds from: the release's container image, or `bundledFrom` when a
/// publisher put its own bundle in `code.image`. The release stores the manager's proxy
/// reference; the build pulls from the registry behind it.
fn rendered_image(
    ctx: &ResourceControllerContext<'_>,
    image: &String,
    bundled_from: Option<&String>,
) -> String {
    let image = bundled_from.unwrap_or(image);
    match ctx.deployment_config.native_image_host.as_deref() {
        Some(host) => alien_core::image_rewrite::resolve_native_image_uri(image, host)
            .unwrap_or_else(|| image.clone()),
        None => image.clone(),
    }
}

/// Starts the image's first build. On a protected install the verifier sends it, after checking
/// it against the signed sandbox; the request is the same either way.
async fn create_image(
    ctx: &ResourceControllerContext<'_>,
    client: &dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi,
    sandbox_id: &str,
    request: alien_aws_clients::lambda_microvms::CreateMicrovmImageRequest,
) -> Result<alien_aws_clients::lambda_microvms::CreateMicrovmImageResponse> {
    let sent = serde_json::to_value(&request).into_alien_error().context(
        ErrorData::ResourceConfigInvalid {
            message: "Cannot encode MicroVM build request".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        },
    )?;
    match crate::core::verifier::aws_sandbox_write(
        ctx,
        sandbox_id,
        crate::core::verifier::ResourceWrite::CreateMicrovmImage { request: sent },
    )
    .await?
    {
        Some(answer) => serde_json::from_value(answer).into_alien_error().context(
            ErrorData::CloudPlatformError {
                message: "The verifier's answer to the image build is not a build".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            },
        ),
        None => client
            .create_microvm_image(request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "CreateMicrovmImage failed".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            }),
    }
}

/// Rolls a new version onto the image, through the verifier on a protected install.
async fn update_image(
    ctx: &ResourceControllerContext<'_>,
    client: &dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi,
    sandbox_id: &str,
    image: &str,
    request: alien_aws_clients::lambda_microvms::UpdateMicrovmImageRequest,
) -> Result<alien_aws_clients::lambda_microvms::UpdateMicrovmImageResponse> {
    let sent = serde_json::to_value(&request).into_alien_error().context(
        ErrorData::ResourceConfigInvalid {
            message: "Cannot encode MicroVM build request".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        },
    )?;
    match crate::core::verifier::aws_sandbox_write(
        ctx,
        sandbox_id,
        crate::core::verifier::ResourceWrite::UpdateMicrovmImage {
            image: image.to_string(),
            request: sent,
        },
    )
    .await?
    {
        Some(answer) => serde_json::from_value(answer).into_alien_error().context(
            ErrorData::CloudPlatformError {
                message: "The verifier's answer to the image roll is not a build".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            },
        ),
        None => client.update_microvm_image(image, request).await.context(
            ErrorData::CloudPlatformError {
                message: "UpdateMicrovmImage failed".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            },
        ),
    }
}

/// Whether an error or any error it was caused by has this code.
fn error_chain_has_code<T>(error: &AlienError<T>, code: &str) -> bool
where
    T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    if error.code == code {
        return true;
    }
    let mut source = error.source.as_deref();
    while let Some(cause) = source {
        if cause.code == code {
            return true;
        }
        source = cause.source.as_deref();
    }
    false
}

fn resource_lifecycle(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Option<alien_core::ResourceLifecycle> {
    ctx.state
        .resources
        .get(resource_id)
        .and_then(|resource| resource.lifecycle)
}

/// A Frozen image is setup's, tagged as the templates tag it; a Live one is the runtime's.
fn image_tags(ctx: &ResourceControllerContext<'_>, resource_id: &str) -> BTreeMap<String, String> {
    if resource_lifecycle(ctx, resource_id) == Some(alien_core::ResourceLifecycle::Frozen) {
        alien_core::setup_resource_tags(
            ctx.resource_prefix,
            resource_id,
            Sandbox::RESOURCE_TYPE.as_ref(),
        )
    } else {
        standard_resource_tags(ctx.resource_prefix, resource_id)
            .into_iter()
            .collect()
    }
}

impl AwsSandboxController {
    /// The bundle this sandbox builds from.
    ///
    /// A Live sandbox that depends on the deployment's bundle Storage renders its bundle there,
    /// from the release's container image: `code.image`, or `bundledFrom` when a publisher put its
    /// bundle in `code.image`. One whose setup predates that Storage builds from the publisher's
    /// bundle, read from the store its setup granted.
    ///
    /// A Live build role grants one store's stable prefix, and every published bundle is written to
    /// each store under the same key. A release naming another store (stores moved after this setup
    /// ran) is therefore read from the granted one, which this role can read and which still holds
    /// the key. A Frozen role grants one exact object, so its bundle is taken as declared.
    fn bundle_to_build(
        &self,
        ctx: &ResourceControllerContext<'_>,
        config: &Sandbox,
        region: &str,
    ) -> Result<BundleToBuild> {
        let SandboxCode::Image {
            image,
            bundled_from,
        } = &config.code
        else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "an AWS sandbox is built from a released image, not from source"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            }));
        };
        if resource_lifecycle(ctx, &config.id) == Some(alien_core::ResourceLifecycle::Frozen) {
            return Ok(BundleToBuild::Published(regional_bundle_uri(
                image, region, &config.id,
            )?));
        }
        if aws_sandbox_renders_bundle(ctx.desired_stack, &config.id) {
            let storage = ctx.require_dependency::<crate::storage::AwsStorageController>(
                &ResourceRef::new(Storage::RESOURCE_TYPE, SANDBOX_BUNDLE_STORAGE_ID),
            )?;
            let bucket = storage.bucket_name.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: config.id.clone(),
                    dependency_id: SANDBOX_BUNDLE_STORAGE_ID.to_string(),
                })
            })?;
            let image = rendered_image(ctx, image, bundled_from.as_ref());
            return Ok(BundleToBuild::Rendered(super::aws_bundle::render(
                bucket, &image, &config.id,
            )?));
        }
        if !image.starts_with("s3://") {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "this sandbox's setup granted a published bundle store rather than the \
                          deployment's bundle Storage, and the release has no bundle published \
                          there; run setup again to add the Storage"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
        let desired = regional_bundle_uri(image, region, &config.id)?;
        Ok(BundleToBuild::Published(
            match (
                self.granted_bundle_uri.as_deref().and_then(split_s3_uri),
                split_s3_uri(&desired),
            ) {
                (Some((granted_bucket, _)), Some((desired_bucket, key)))
                    if granted_bucket != desired_bucket =>
                {
                    format!("s3://{granted_bucket}/{key}")
                }
                _ => desired,
            },
        ))
    }

    /// Whether the build needs a rendered bundle written first. A published bundle is already in
    /// its store.
    fn needs_bundle_write(&self, bundle: &BundleToBuild) -> bool {
        matches!(bundle, BundleToBuild::Rendered(rendered)
            if self.written_bundle_uri.as_deref() != Some(rendered.uri().as_str()))
    }

    /// Writes the desired bundle into the deployment's bundle Storage.
    async fn write_bundle(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        let aws_config = ctx.get_aws_config()?;
        let BundleToBuild::Rendered(rendered) =
            self.bundle_to_build(ctx, config, &aws_config.region)?
        else {
            return Ok(());
        };
        let client = ctx.service_provider.get_aws_s3_client(aws_config).await?;
        // On a protected install the verifier writes it: it renders the same bundle from the
        // image the release signed, so a bundle names no image but that one.
        if let SandboxCode::Image {
            image,
            bundled_from,
        } = &config.code
        {
            if let Some(answer) = crate::core::verifier::aws_sandbox_write(
                ctx,
                &config.id,
                crate::core::verifier::ResourceWrite::PutSandboxBundle {
                    image: rendered_image(ctx, image, bundled_from.as_ref()),
                },
            )
            .await?
            {
                if answer["uri"].as_str() != Some(rendered.uri().as_str()) {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "The verifier wrote the sandbox bundle to {}, not {}",
                            answer["uri"],
                            rendered.uri()
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                super::aws_bundle::prune(
                    client.as_ref(),
                    &rendered,
                    self.bundle_uri.as_deref(),
                    &config.id,
                )
                .await;
                self.written_bundle_uri = Some(rendered.uri());
                return Ok(());
            }
        }
        super::aws_bundle::write(
            client.as_ref(),
            &rendered,
            self.bundle_uri.as_deref(),
            &config.id,
        )
        .await?;
        self.written_bundle_uri = Some(rendered.uri());
        Ok(())
    }
}

/// What a build reads its bundle from.
enum BundleToBuild {
    /// A bundle already in a store the build role can read.
    Published(String),
    /// A bundle this controller renders into the deployment's bundle Storage before the build.
    Rendered(super::aws_bundle::RenderedBundle),
}

impl BundleToBuild {
    fn uri(&self) -> String {
        match self {
            BundleToBuild::Published(uri) => uri.clone(),
            BundleToBuild::Rendered(bundle) => bundle.uri(),
        }
    }
}

/// `s3://bucket/key` as `(bucket, key)`.
fn split_s3_uri(uri: &str) -> Option<(&str, &str)> {
    uri.strip_prefix("s3://")
        .and_then(|path| path.split_once('/'))
}

/// The MicroVM image's name, unique in the account because it carries the deployment's
/// resource prefix.
fn sandbox_image_name(resource_prefix: &str, resource_id: &str) -> String {
    format!("{resource_prefix}-{resource_id}")
}

/// A published bundle URI, with the region token resolved.
///
/// AWS builds a MicroVM image only from a bucket in the image's own region, so a vendor's one
/// stored URI resolves per region here exactly as the emitters resolve it.
fn regional_bundle_uri(uri: &str, region: &str, resource_id: &str) -> Result<String> {
    match parse_bundle_uri(uri).map_err(|reason| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: reason,
            resource_id: Some(resource_id.to_string()),
        })
    })? {
        BundleUri::Literal(uri) => Ok(uri.to_string()),
        BundleUri::Regional { before, after } => Ok(format!("{before}{region}{after}")),
    }
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

/// The agent's environment for this sandbox's image: the egress contract and, when the sandbox
/// declares them, its image permissions, with each connection narrowed to the addresses its
/// target published (a dependency, so it exists before this image is built).
fn agent_environment(
    ctx: &ResourceControllerContext<'_>,
    config: &Sandbox,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut environment =
        alien_core::sandbox_egress::aws_sandbox_agent_environment(ctx.desired_stack, config);
    if !config.declares_image_permissions() {
        return Ok(environment);
    }
    if config
        .image_permissions
        .as_ref()
        .is_some_and(|permissions| !permissions.connections.is_empty())
    {
        alien_core::sandbox_image_permissions::validate_connections(ctx.desired_stack, config)
            .context(ErrorData::ResourceConfigInvalid {
                message: "sandbox image connections do not resolve".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
    }
    let published = |connection: &alien_core::SandboxImageConnection| {
        let outputs = ctx
            .state
            .resources
            .get(&connection.to.id)
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| serde_json::to_value(outputs).ok());
        alien_core::sandbox_image_permissions::published_connection_addresses(
            config,
            connection,
            outputs.as_ref(),
        )
    };
    environment.extend(
        alien_core::sandbox_image_permissions::image_permissions_environment(config, published)
            .context(ErrorData::ResourceConfigInvalid {
                message: "cannot build the sandbox image permissions".to_string(),
                resource_id: Some(config.id.clone()),
            })?,
    );
    Ok(environment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{
        deserialize_controller, serialize_controller, state_utils::StackResourceStateExt,
        MockPlatformServiceProvider, ResourceController,
    };
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageResponse, MicrovmImage, MicrovmImageVersion, MockLambdaMicrovmsApi,
        UpdateMicrovmImageResponse,
    };
    use alien_aws_clients::lambda_microvms::{MicrovmImageLogging, MicrovmImageResources};
    use alien_aws_clients::sandbox_image_build::{
        build_client_token, internet_egress_connector_arn, managed_base_image_arn,
        roll_client_token,
    };
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use alien_core::{Platform, SandboxCode, SandboxEgress, SandboxLifecyclePolicy};
    use std::sync::{Arc, Mutex};

    const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";
    const BUILD_ROLE_ARN: &str = "arn:aws:iam::123456789012:role/test-agents-build";
    const BUNDLE_URI: &str = "s3://alien-bundles-test/sandbox/bundle.zip";
    const DENY_CONNECTOR_ARN: &str =
        "arn:aws:lambda:us-east-1:123456789012:network-connector:nc-deny123";

    /// An internet sandbox with its secrets vault, stepped once from `InitializingEgressKey`.
    #[cfg(feature = "local")]
    async fn step_egress_key(
        subject: Sandbox,
        lifecycle: alien_core::ResourceLifecycle,
        controller: AwsSandboxController,
        other: (
            Sandbox,
            alien_core::ResourceLifecycle,
            Option<AwsSandboxController>,
        ),
        directory: &std::path::Path,
    ) -> Result<AwsSandboxController> {
        use crate::{LocalVaultController, LocalVaultState};
        use alien_core::{ClientConfig, Vault, SECRETS_VAULT_ID};

        let (other, other_lifecycle, other_state) = other;
        let other_id = other.id.clone();
        let mut executor = SingleControllerExecutor::builder()
            .resource(subject)
            .resource_lifecycle(lifecycle)
            .controller(AwsSandboxController {
                state: AwsSandboxState::InitializingEgressKey,
                ..controller
            })
            .platform(Platform::Aws)
            .client_config(ClientConfig::Local {
                state_directory: directory.to_string_lossy().into_owned(),
            })
            .with_stack_resource(other.clone(), other_lifecycle)
            .with_dependency(
                Vault::new(SECRETS_VAULT_ID.into()).build(),
                LocalVaultController {
                    state: LocalVaultState::Ready,
                    vault_path: Some(directory.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .build()
            .await
            .unwrap();
        if let Some(other_state) = other_state {
            let mut saved = alien_core::StackResourceState::builder()
                .resource_type(Sandbox::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(other))
                .lifecycle(other_lifecycle)
                .build();
            saved
                .set_internal_controller(Some(Box::new(other_state)))
                .unwrap();
            executor.insert_saved_resource(&other_id, saved);
        }
        executor.step().await?;
        Ok(executor
            .internal_state::<AwsSandboxController>()
            .unwrap()
            .clone())
    }

    /// The setup-owned sandbox owns the key. It writes the key once and keeps the public half in
    /// its state; a later pass keeps that key, and the Live sandbox takes it from the owner's
    /// state. Nothing reads the private key back.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn the_owner_writes_the_key_once_and_others_take_it_from_its_state() {
        use alien_bindings::{providers::vault::LocalVault, Vault as _};
        use alien_core::ResourceLifecycle::{Frozen, Live};

        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let mut frozen = sandbox();
        frozen.id = "z-frozen".into();
        frozen.egress = SandboxEgress::Internet;
        let mut live = frozen.clone();
        live.id = "a-live".into();

        let owner = step_egress_key(
            frozen.clone(),
            Frozen,
            AwsSandboxController::default(),
            (live.clone(), Live, None),
            directory.path(),
        )
        .await
        .expect("the owner writes the key");
        assert_eq!(owner.state, AwsSandboxState::PublishingEgressConfig);
        let key = owner
            .egress_public_key
            .clone()
            .expect("public key recorded");
        let stored = vault
            .get_secret(super::super::egress::SIGNING_KEY)
            .await
            .unwrap();
        assert_eq!(
            super::super::egress::read_signing_public_key(&vault)
                .await
                .unwrap(),
            key,
            "the recorded public key belongs to the stored private key"
        );
        alien_bindings::sandbox_egress_token::EgressTokenIssuer::from_vault_json(&stored, &key.kid)
            .unwrap();

        let again = step_egress_key(
            frozen.clone(),
            Frozen,
            owner.clone(),
            (live.clone(), Live, None),
            directory.path(),
        )
        .await
        .expect("a later owner pass keeps its key");
        assert_eq!(again.egress_public_key.as_ref(), Some(&key));
        assert!(
            vault
                .get_secret(super::super::egress::SIGNING_KEY)
                .await
                .unwrap()
                == stored,
            "a later pass must not rotate the key"
        );

        let other = step_egress_key(
            live.clone(),
            Live,
            AwsSandboxController::default(),
            (frozen.clone(), Frozen, Some(owner)),
            directory.path(),
        )
        .await
        .expect("the Live sandbox takes the owner's key");
        assert_eq!(other.egress_public_key.as_ref(), Some(&key));
        assert!(
            vault
                .get_secret(super::super::egress::SIGNING_KEY)
                .await
                .unwrap()
                == stored
        );

        let error = step_egress_key(
            live,
            Live,
            AwsSandboxController::default(),
            (frozen, Frozen, None),
            directory.path(),
        )
        .await
        .expect_err("without the owner's state there is no key to take");
        assert_eq!(error.code, "DEPENDENCY_NOT_READY", "{error:?}");
    }

    /// A sandbox that becomes the owner without a key of its own, such as one added with a lower
    /// id, adopts the key another sandbox recorded instead of writing a new one.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn a_new_owner_adopts_the_recorded_key() {
        use alien_bindings::{providers::vault::LocalVault, Vault as _};
        use alien_core::ResourceLifecycle::Live;

        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let mut previous = sandbox();
        previous.id = "b-previous".into();
        previous.egress = SandboxEgress::Internet;
        let mut added = previous.clone();
        added.id = "a-added".into();

        let first = step_egress_key(
            previous.clone(),
            Live,
            AwsSandboxController::default(),
            (
                Sandbox {
                    id: "c-other".into(),
                    ..previous.clone()
                },
                Live,
                None,
            ),
            directory.path(),
        )
        .await
        .expect("the first owner writes the key");
        let key = first.egress_public_key.clone().unwrap();
        let stored = vault
            .get_secret(super::super::egress::SIGNING_KEY)
            .await
            .unwrap();

        let owner = step_egress_key(
            added,
            Live,
            AwsSandboxController::default(),
            (previous, Live, Some(first)),
            directory.path(),
        )
        .await
        .expect("the new owner adopts the key");
        assert_eq!(owner.egress_public_key.as_ref(), Some(&key));
        assert!(
            vault
                .get_secret(super::super::egress::SIGNING_KEY)
                .await
                .unwrap()
                == stored,
            "adopting must not rotate the key"
        );
    }

    /// The owner initializes the key against real SSM wire calls through a management identity
    /// that may write the vault but not read it, as setup now grants. The key must still be
    /// created, with one create-only PutParameter and no GetParameter.
    #[tokio::test]
    async fn the_owner_initializes_the_key_without_reading_the_vault() {
        use crate::{AwsVaultController, AwsVaultState};
        use alien_core::{
            AwsClientConfig, AwsCredentials, AwsServiceOverrides, ClientConfig, ResourceLifecycle,
            Vault, SECRETS_VAULT_ID,
        };
        use httpmock::{Method::POST, MockServer};
        use std::collections::HashMap;

        let server = MockServer::start_async().await;
        let read = server
            .mock_async(|when, then| {
                when.method(POST).header("x-amz-target", "AmazonSSM.GetParameter");
                then.status(400).json_body(serde_json::json!({
                    "__type": "AccessDeniedException",
                    "message": "User: arn:aws:sts::123456789012:assumed-role/test-management/session is not authorized to perform: ssm:GetParameter with an explicit deny"
                }));
            })
            .await;
        let write = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "AmazonSSM.PutParameter")
                    .body_contains("test-secrets-sandbox-egress-signing-key")
                    .body_contains("\"Overwrite\":false");
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1, "Tier": "Standard"}));
            })
            .await;
        let mut subject = sandbox();
        subject.egress = SandboxEgress::Internet;
        let mut executor = SingleControllerExecutor::builder()
            .resource(subject)
            .resource_lifecycle(ResourceLifecycle::Live)
            .controller(AwsSandboxController {
                state: AwsSandboxState::InitializingEgressKey,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .client_config(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: "123456789012".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test".to_string(),
                    secret_access_key: "test".to_string(),
                    session_token: None,
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: HashMap::from([("ssm".to_string(), server.base_url())]),
                }),
            })))
            .with_dependency(
                Vault::new(SECRETS_VAULT_ID.into()).build(),
                AwsVaultController {
                    state: AwsVaultState::Ready,
                    account_id: Some("123456789012".to_string()),
                    region: Some("us-east-1".to_string()),
                    vault_prefix: Some("test-secrets".to_string()),
                    ..Default::default()
                },
            )
            .build()
            .await
            .unwrap();

        executor
            .step()
            .await
            .expect("the key is initialized without a read");

        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(controller.state, AwsSandboxState::PublishingEgressConfig);
        let key = controller.egress_public_key.as_ref().expect("public key");
        assert!(!key.kid.is_empty());
        assert_eq!(
            read.hits_async().await,
            0,
            "the private key must not be read"
        );
        assert_eq!(write.hits_async().await, 1);
    }

    /// Deletes a Frozen internet sandbox that holds the deployment's egress key, next to an
    /// optional other internet sandbox, and reports which shared vault entries remain.
    #[cfg(feature = "local")]
    async fn delete_internet_sandbox_with_shared_secrets(
        other: Option<Sandbox>,
        saved_status: Option<ResourceStatus>,
    ) -> Vec<&'static str> {
        use crate::{LocalVaultController, LocalVaultState};
        use alien_bindings::{providers::vault::LocalVault, Vault as _};
        use alien_core::{ClientConfig, ResourceLifecycle, Vault, SECRETS_VAULT_ID};

        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let key = super::super::egress::create_signing_key(&vault)
            .await
            .unwrap();
        vault
            .set_secret(super::super::egress::GATEWAY_CONFIG, "{}")
            .await
            .unwrap();
        let mut subject = sandbox();
        subject.egress = SandboxEgress::Internet;
        let mut builder = SingleControllerExecutor::builder()
            .resource(subject)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(AwsSandboxController {
                egress_public_key: Some(key),
                ..ready_controller()
            })
            .platform(Platform::Aws)
            .client_config(ClientConfig::Local {
                state_directory: directory.path().to_string_lossy().into_owned(),
            })
            .with_dependency(
                Vault::new(SECRETS_VAULT_ID.into()).build(),
                LocalVaultController {
                    state: LocalVaultState::Ready,
                    vault_path: Some(directory.path().to_string_lossy().into_owned()),
                    ..Default::default()
                },
            );
        let saved = other.clone();
        if let Some(other) = other {
            builder = builder.with_stack_resource(other, ResourceLifecycle::Live);
        }
        let mut executor = builder.build().await.unwrap();
        // A sibling the teardown has dropped from the desired stack but not yet deleted.
        if let (Some(other), Some(status)) = (saved, saved_status) {
            executor.remove_desired_resource(&other.id);
            let id = other.id.clone();
            executor.insert_saved_resource(
                &id,
                alien_core::StackResourceState::builder()
                    .resource_type(Sandbox::RESOURCE_TYPE.to_string())
                    .status(status)
                    .config(alien_core::Resource::new(other))
                    .lifecycle(ResourceLifecycle::Live)
                    .build(),
            );
        }
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("delete flow should succeed");
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        let mut remaining = Vec::new();
        for name in [
            super::super::egress::SIGNING_KEY,
            super::super::egress::GATEWAY_CONFIG,
        ] {
            match vault.get_secret(name).await {
                Ok(_) => remaining.push(name),
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
                    ) => {}
                Err(error) => panic!("vault read failed: {error:?}"),
            }
        }
        remaining
    }

    /// The last internet sandbox removes the deployment's egress signing key and gateway
    /// configuration. A live teardown left both parameters behind.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn the_last_internet_sandbox_deletes_the_shared_egress_secrets() {
        assert_eq!(
            delete_internet_sandbox_with_shared_secrets(None, None).await,
            Vec::<&str>::new()
        );
    }

    fn other_internet_sandbox() -> Sandbox {
        let mut other = sandbox();
        other.id = "workers".into();
        other.egress = SandboxEgress::Internet;
        other
    }

    /// Another declared internet sandbox and the gateway still sign and read with them.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn the_shared_egress_secrets_stay_while_another_internet_sandbox_is_declared() {
        assert_eq!(
            delete_internet_sandbox_with_shared_secrets(Some(other_internet_sandbox()), None).await,
            vec![
                super::super::egress::SIGNING_KEY,
                super::super::egress::GATEWAY_CONFIG
            ]
        );
    }

    /// In a full teardown the desired stack is empty; a sibling still being deleted keeps them.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn the_shared_egress_secrets_stay_while_another_internet_sandbox_is_still_deleting() {
        assert_eq!(
            delete_internet_sandbox_with_shared_secrets(
                Some(other_internet_sandbox()),
                Some(ResourceStatus::Deleting)
            )
            .await,
            vec![
                super::super::egress::SIGNING_KEY,
                super::super::egress::GATEWAY_CONFIG
            ]
        );
    }

    /// Once the sibling is deleted, the last internet sandbox removes them.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn the_shared_egress_secrets_go_once_every_other_internet_sandbox_is_deleted() {
        assert_eq!(
            delete_internet_sandbox_with_shared_secrets(
                Some(other_internet_sandbox()),
                Some(ResourceStatus::Deleted)
            )
            .await,
            Vec::<&str>::new()
        );
    }

    fn sandbox() -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: BUNDLE_URI.to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .preview_ports(vec![8080])
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(1800),
                idle_pause_seconds: Some(600),
                ..Default::default()
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
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .preview_ports(vec![8080])
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(1800),
                idle_pause_seconds: Some(600),
                ..Default::default()
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

    fn provider(mut client: MockLambdaMicrovmsApi) -> Arc<MockPlatformServiceProvider> {
        // Most flow tests model an image without abandoned builds. Inventory tests
        // install their own expectations before this catch-all.
        client
            .expect_list_microvm_image_versions()
            .returning(|_| Ok(vec![]));
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

    /// The release's image, already carrying the agent, in another account's registry.
    const RELEASED_IMAGE: &str = "210987654321.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:\
                                  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// A Live sandbox as a release carries it: the image alone, or, when a publisher still
    /// publishes bundles for the project, its bundle in `image` and the image in `bundledFrom`.
    fn released_sandbox(published_bundle: Option<&str>) -> Sandbox {
        let mut sandbox = sandbox_with_bundle(RELEASED_IMAGE);
        if let Some(bundle) = published_bundle {
            sandbox.code = SandboxCode::Image {
                image: bundle.to_string(),
                bundled_from: Some(RELEASED_IMAGE.to_string()),
            };
        }
        sandbox
    }

    fn dockerfile_in(zip: &[u8]) -> String {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).expect("a zip");
        let mut dockerfile = String::new();
        std::io::Read::read_to_string(
            &mut archive
                .by_name("Dockerfile")
                .expect("Dockerfile at the root"),
            &mut dockerfile,
        )
        .expect("reads");
        dockerfile
    }

    /// The whole life in the deployment's bundle Storage: the first build writes a bundle naming
    /// the release's image under this sandbox's prefix, deletes the one a failed earlier attempt
    /// left there, and builds from the new one; deletion removes the image, then every bundle this
    /// sandbox wrote, so setup can delete the empty bucket.
    #[tokio::test]
    async fn a_sandbox_renders_its_bundle_into_the_bundle_storage_and_removes_it_on_delete() {
        renders_into_the_bundle_storage_and_removes_it_on_delete(released_sandbox(None)).await;
    }

    /// A release that also carries a publisher's bundle, for the project's deployments whose setup
    /// granted the publisher's store, renders from the image it was built from all the same.
    #[tokio::test]
    async fn a_sandbox_with_its_own_storage_renders_from_the_image_a_published_bundle_names() {
        renders_into_the_bundle_storage_and_removes_it_on_delete(released_sandbox(Some(
            "s3://publisher-{region}/sandbox-bundle/v2/bundle.zip",
        )))
        .await;
    }

    async fn renders_into_the_bundle_storage_and_removes_it_on_delete(sandbox: Sandbox) {
        use alien_aws_clients::s3::{
            DeleteObjectsOutput, ListObjectsV2Output, MockS3Api, Object, PutObjectOutput,
        };
        const BUCKET: &str = "test-stack-sandbox-bundles";
        const LEFT_OVER: &str = "sandbox-bundle/agents/0000/bundle.zip";

        let stored = Arc::new(Mutex::new(vec![LEFT_OVER.to_string()]));
        let written = Arc::new(Mutex::new(None::<(String, String, Vec<u8>)>));
        let mut s3 = MockS3Api::new();
        {
            let stored = stored.clone();
            let written = written.clone();
            s3.expect_put_object().times(1).returning(move |request| {
                stored.lock().unwrap().push(request.key.clone());
                *written.lock().unwrap() = Some((
                    request.bucket.clone(),
                    request.key.clone(),
                    request.body.clone(),
                ));
                Ok(PutObjectOutput {
                    e_tag: None,
                    version_id: None,
                    server_side_encryption: None,
                })
            });
        }
        {
            let stored = stored.clone();
            s3.expect_list_objects_v2()
                .withf(|bucket, prefix, _| {
                    bucket == BUCKET && prefix.as_deref() == Some("sandbox-bundle/agents/")
                })
                .returning(move |_, _, _| {
                    Ok(ListObjectsV2Output {
                        contents: stored
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|key| Object {
                                key: key.clone(),
                                last_modified: String::new(),
                                e_tag: String::new(),
                                size: 0,
                                storage_class: String::new(),
                            })
                            .collect(),
                        name: BUCKET.to_string(),
                        max_keys: 1000,
                        key_count: 0,
                        is_truncated: false,
                        next_continuation_token: None,
                    })
                });
        }
        {
            let stored = stored.clone();
            s3.expect_delete_objects()
                .withf(|bucket, _, _| bucket == BUCKET)
                .returning(move |_, objects, _| {
                    stored
                        .lock()
                        .unwrap()
                        .retain(|key| !objects.iter().any(|object| object.key == *key));
                    Ok(DeleteObjectsOutput {
                        deleted: vec![],
                        errors: vec![],
                    })
                });
        }

        let built_from = Arc::new(Mutex::new(None::<String>));
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .returning(|_| Err(not_found()));
        {
            let built_from = built_from.clone();
            client
                .expect_create_microvm_image()
                .times(1)
                .returning(move |request| {
                    *built_from.lock().unwrap() = Some(request.code_artifact.uri.clone());
                    Ok(created_response())
                });
        }
        client
            .expect_get_microvm_image_version()
            .returning(|_, _| Ok(active_version()));
        client
            .expect_delete_microvm_image()
            .times(1)
            .returning(|_| Ok(()));
        client
            .expect_list_microvm_image_versions()
            .returning(|_| Ok(vec![]));

        let s3 = Arc::new(s3);
        let client = Arc::new(client);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(client.clone()));
        provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(s3.clone()));

        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox)
            .controller(AwsSandboxController {
                state: AwsSandboxState::CreatingImage,
                ..runtime_seeded_controller()
            })
            .with_dependency(
                Storage::new(SANDBOX_BUNDLE_STORAGE_ID.to_string()).build(),
                crate::storage::AwsStorageController::mock_ready(SANDBOX_BUNDLE_STORAGE_ID),
            )
            .dependency_lifecycle(
                SANDBOX_BUNDLE_STORAGE_ID,
                alien_core::ResourceLifecycle::Frozen,
            )
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");
        executor
            .run_until_terminal()
            .await
            .expect("create flow should succeed");
        assert_eq!(executor.status(), ResourceStatus::Running);

        let (bucket, key, zip) = written.lock().unwrap().clone().expect("a bundle written");
        assert_eq!(bucket, BUCKET);
        assert!(key.starts_with("sandbox-bundle/agents/"), "{key}");
        assert_eq!(dockerfile_in(&zip), format!("FROM {RELEASED_IMAGE}\n"));
        assert_eq!(
            built_from.lock().unwrap().as_deref(),
            Some(format!("s3://{BUCKET}/{key}").as_str()),
            "the image is built from the bundle just written"
        );
        assert_eq!(*stored.lock().unwrap(), vec![key.clone()]);

        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("delete flow should succeed");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(stored.lock().unwrap().is_empty(), "every bundle deleted");
    }

    /// A setup that granted a publisher's store keeps building from the bundle the publisher put
    /// in `code.image`, as a controller that predates `bundledFrom` does: it reads the bundle's
    /// key from the store it was granted, and never touches S3 itself (the provider here has no
    /// S3 client to give).
    #[tokio::test]
    async fn a_setup_granted_a_publishers_store_builds_from_the_published_bundle() {
        const GRANTED: &str = "s3://publisher-us-east-1/sandbox-bundle/v1/bundle.zip";
        let built_from = Arc::new(Mutex::new(None::<String>));
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .returning(|_| Err(not_found()));
        {
            let built_from = built_from.clone();
            client
                .expect_create_microvm_image()
                .times(1)
                .returning(move |request| {
                    *built_from.lock().unwrap() = Some(request.code_artifact.uri.clone());
                    Ok(created_response())
                });
        }
        let mut executor = SingleControllerExecutor::builder()
            .resource(released_sandbox(Some(
                "s3://publisher-renamed-{region}/sandbox-bundle/v2/bundle.zip",
            )))
            .controller(AwsSandboxController {
                state: AwsSandboxState::CreatingImage,
                granted_bundle_uri: Some(GRANTED.to_string()),
                bundle_uri: Some(GRANTED.to_string()),
                ..runtime_seeded_controller()
            })
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");
        executor.step().await.expect("the build starts");
        assert_eq!(
            built_from.lock().unwrap().as_deref(),
            Some("s3://publisher-us-east-1/sandbox-bundle/v2/bundle.zip"),
            "the published key, read from the store this setup granted"
        );
    }

    /// With neither the bundle Storage nor a published bundle, there is nothing the build role
    /// could read; the sandbox says to run setup instead of starting a build that cannot succeed.
    /// A release carries only its image once no deployment of the project builds from a
    /// publisher's store.
    #[tokio::test]
    async fn a_release_with_no_bundle_for_a_publishers_store_asks_for_setup() {
        const GRANTED: &str = "s3://publisher-us-east-1/sandbox-bundle/v1/bundle.zip";
        let mut executor = SingleControllerExecutor::builder()
            .resource(released_sandbox(None))
            .controller(AwsSandboxController {
                state: AwsSandboxState::CreatingImage,
                granted_bundle_uri: Some(GRANTED.to_string()),
                bundle_uri: Some(GRANTED.to_string()),
                ..runtime_seeded_controller()
            })
            .platform(Platform::Aws)
            .service_provider(provider(MockLambdaMicrovmsApi::new()))
            .build()
            .await
            .expect("executor should build");
        let error = executor.step().await.expect_err("no bundle to build from");
        assert!(
            error.to_string().contains("run setup again"),
            "unexpected error: {error}"
        );
    }

    fn not_found() -> alien_error::AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Microvm".to_string(),
            resource_name: "probe".to_string(),
        })
    }

    #[tokio::test]
    async fn legacy_empty_retirement_history_reclaims_failed_inventory() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: Some(IMAGE_ARN.to_string()),
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("UPDATED".to_string()),
            })
        });
        client
            .expect_list_microvm_image_versions()
            .times(1)
            .returning(|_| {
                Ok(vec![
                    MicrovmImage {
                        description: None,
                        image_identifier: None,
                        image_arn: Some(IMAGE_ARN.to_string()),
                        image_version: Some("1.0".to_string()),
                        state: Some("SUCCESSFUL".to_string()),
                    },
                    MicrovmImage {
                        description: None,
                        image_identifier: None,
                        image_arn: Some(IMAGE_ARN.to_string()),
                        image_version: Some("2.0".to_string()),
                        state: Some("FAILED".to_string()),
                    },
                ])
            });
        client
            .expect_delete_microvm_image_version()
            .withf(|image, version| image == IMAGE_ARN && version == "2.0")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut executor = executor(ready_controller(), client).await;
        executor
            .step()
            .await
            .expect("inventory reconciliation should succeed");
    }

    /// Exercise quota recovery from a persisted request handler. DELETE acceptance
    /// does not free the slot: the provider reports the version until two reads.
    #[tokio::test]
    async fn saved_build_handler_waits_for_failed_version_absence_before_building() {
        let inventory = Arc::new(Mutex::new(vec![
            ("1.0".to_string(), "SUCCESSFUL".to_string()),
            ("2.0".to_string(), "FAILED".to_string()),
            ("3.0".to_string(), "SUCCESSFUL".to_string()),
            ("4.0".to_string(), "IN_PROGRESS".to_string()),
        ]));
        let mut client = MockLambdaMicrovmsApi::new();
        let listed = inventory.clone();
        client
            .expect_list_microvm_image_versions()
            .returning(move |_| {
                Ok(listed
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(version, state)| MicrovmImage {
                        description: None,
                        image_identifier: None,
                        image_arn: Some(IMAGE_ARN.to_string()),
                        image_version: Some(version.clone()),
                        state: Some(state.clone()),
                    })
                    .collect())
            });
        client
            .expect_delete_microvm_image_version()
            .withf(|_, version| version == "2.0")
            .times(1)
            .returning(|_, _| Ok(()));
        let checked = inventory.clone();
        let mut reads = 0;
        client
            .expect_get_microvm_image_version()
            .withf(|_, version| version == "2.0")
            .times(2)
            .returning(move |_, _| {
                reads += 1;
                if reads == 1 {
                    Ok(MicrovmImageVersion {
                        image_version: Some("2.0".to_string()),
                        state: Some("FAILED".to_string()),
                        ..active_version()
                    })
                } else {
                    checked
                        .lock()
                        .unwrap()
                        .retain(|(version, _)| version != "2.0");
                    Err(not_found())
                }
            });
        // The unrecorded successful version is retired once the failed one is gone, which
        // frees no quota.
        client
            .expect_update_microvm_image_version()
            .withf(|_, version, _| version == "3.0")
            .returning(|_, _, _| Ok(()));
        let built = inventory.clone();
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(move |_, _| {
                let versions = built.lock().unwrap();
                assert_eq!(
                    versions.len(),
                    3,
                    "quota is freed only on confirmed absence"
                );
                assert_eq!(
                    versions.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(),
                    ["1.0", "3.0", "4.0"],
                    "published and in-flight versions stay protected"
                );
                Ok(UpdateMicrovmImageResponse {
                    image_version: Some("5.0".to_string()),
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: None,
                    state: None,
                })
            });
        let mut controller = ready_controller();
        controller.state = AwsSandboxState::UpdatingImage;
        let provider = provider(client);
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider.clone())
            .build()
            .await
            .unwrap();
        executor.step().await.expect("delete accepted");
        let saved = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(saved.deleting_version.as_deref(), Some("2.0"));
        let encoded = serde_json::to_value(&saved).unwrap();
        let restored: AwsSandboxController = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored.deleting_version.as_deref(), Some("2.0"));
        drop(executor);
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(restored)
            .platform(Platform::Aws)
            .service_provider(provider)
            .build()
            .await
            .unwrap();
        executor.step().await.expect("deletion still pending");
        assert!(executor
            .internal_state::<AwsSandboxController>()
            .unwrap()
            .pending_version
            .is_none());
        executor
            .step()
            .await
            .expect("absence confirmed and checkpointed");
        executor.step().await.expect("next build accepted");
        assert_eq!(
            executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .as_deref(),
            Some("5.0")
        );
    }

    #[tokio::test]
    async fn a_lost_build_response_is_recovered_without_building_or_deleting_again() {
        let accepted = Arc::new(Mutex::new(None::<MicrovmImage>));
        let saved_receipt = accepted.clone();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(move |_, request| {
                *saved_receipt.lock().unwrap() = Some(MicrovmImage {
                    description: request.description,
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("2.0".to_string()),
                    state: Some("SUCCESSFUL".to_string()),
                });
                Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: "response lost after accepting the build".to_string(),
                }))
            });
        let mut controller = ready_controller();
        controller.state = AwsSandboxState::UpdatingImage;
        let mut first = executor(controller, client).await;
        first
            .step()
            .await
            .expect_err("the transport failure surfaces");
        let checkpoint =
            serde_json::to_value(first.internal_state::<AwsSandboxController>().unwrap()).unwrap();
        drop(first);
        let restored: AwsSandboxController = serde_json::from_value(checkpoint).unwrap();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .returning(move |_| {
                Ok(vec![accepted
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("accepted receipt")])
            });
        client
            .expect_get_microvm_image_version()
            .withf(|_, version| version == "2.0")
            .times(1)
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_version: Some("2.0".to_string()),
                    ..active_version()
                })
            });
        let mut resumed = executor(restored, client).await;
        resumed.step().await.expect("recover exact request receipt");
        assert_eq!(
            resumed
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .as_deref(),
            Some("2.0")
        );
        resumed
            .step()
            .await
            .expect("observe accepted build readiness");
        assert_eq!(
            resumed
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .active_version
                .as_deref(),
            Some("2.0")
        );
    }

    #[tokio::test]
    async fn a_lost_create_response_recovers_the_accepted_version() {
        let accepted = Arc::new(Mutex::new(None::<MicrovmImage>));
        let saved = accepted.clone();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .times(1)
            .returning(move |request| {
                *saved.lock().unwrap() = Some(MicrovmImage {
                    description: request.description,
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("SUCCESSFUL".to_string()),
                });
                Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: "create response lost after acceptance".to_string(),
                }))
            });
        let mut first = executor(runtime_seeded_controller(), client).await;
        first
            .run_until_terminal()
            .await
            .expect_err("transport error surfaces");
        let checkpoint =
            serde_json::to_value(first.internal_state::<AwsSandboxController>().unwrap()).unwrap();
        drop(first);
        let restored = serde_json::from_value(checkpoint).unwrap();
        let mut client = MockLambdaMicrovmsApi::new();
        let found = accepted.clone();
        client
            .expect_get_microvm_image()
            .returning(move |_| Ok(found.lock().unwrap().clone().unwrap()));
        client
            .expect_list_microvm_image_versions()
            .returning(move |_| Ok(vec![accepted.lock().unwrap().clone().unwrap()]));
        client
            .expect_get_microvm_image_version()
            .withf(|_, version| version == "1.0")
            .returning(|_, _| Ok(active_version()));
        let mut resumed = executor(restored, client).await;
        resumed
            .run_until_terminal()
            .await
            .expect("recover without another create or update");
        let controller = resumed.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(controller.active_version.as_deref(), Some("1.0"));
        assert_eq!(controller.bundle_uri.as_deref(), Some(BUNDLE_URI));
    }

    #[tokio::test]
    async fn a_sole_failed_initial_version_is_replaced_before_cleanup() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_list_microvm_image_versions().returning(|_| {
            Ok(vec![MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("FAILED".to_string()),
            }])
        });
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(|_, _| {
                Ok(UpdateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("2.0".to_string()),
                    name: None,
                    state: None,
                })
            });
        let mut controller = ready_controller();
        controller.state = AwsSandboxState::UpdatingImage;
        controller.active_version = None;
        controller.build_retry_generation = 1;
        let mut executor = executor(controller, client).await;
        executor
            .step()
            .await
            .expect("replace the failed initial build without forbidden last-version deletion");
        assert_eq!(
            executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .as_deref(),
            Some("2.0")
        );
    }

    #[tokio::test]
    async fn a_deletion_confirmation_error_preserves_the_receipt_and_blocks_builds() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image_version()
            .withf(|_, v| v == "2.0")
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: "inventory unavailable".to_string(),
                }))
            });
        let mut controller = ready_controller();
        controller.state = AwsSandboxState::UpdatingImage;
        controller.deleting_version = Some("2.0".to_string());
        let mut executor = executor(controller, client).await;
        let error = executor
            .step()
            .await
            .expect_err("confirmation errors must stop the build");
        assert!(format!("{error:?}").contains("inventory unavailable"));
        assert_eq!(
            executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .deleting_version
                .as_deref(),
            Some("2.0")
        );
    }

    #[tokio::test]
    async fn successful_versions_survive_old_retirement_times_and_failed_inventory_cleanup() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("3.0".to_string()),
                state: Some("UPDATED".to_string()),
            })
        });
        client
            .expect_list_microvm_image_versions()
            .times(1)
            .returning(|_| {
                Ok([
                    ("1.0", "SUCCESSFUL"),
                    ("2.0", "SUCCESSFUL"),
                    ("3.0", "SUCCESSFUL"),
                    ("4.0", "IN_PROGRESS"),
                    ("5.0", "FAILED"),
                ]
                .into_iter()
                .map(|(version, state)| MicrovmImage {
                    description: None,
                    image_identifier: None,
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some(version.to_string()),
                    state: Some(state.to_string()),
                })
                .collect())
            });
        client
            .expect_delete_microvm_image_version()
            .withf(|_, version| version == "5.0")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.pending_version = Some("4.0".to_string());
        controller.retired_versions = ["1.0", "2.0"]
            .into_iter()
            .map(|version| RetiredVersion {
                version: version.to_string(),
                retired_at: chrono::Utc::now() - chrono::Duration::days(30),
                inactive_at: None,
            })
            .collect();
        let mut executor = executor(controller, client).await;
        executor
            .step()
            .await
            .expect("reclaim only the failed build");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(
            controller
                .retired_versions
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            ["1.0", "2.0"]
        );
        assert_eq!(controller.active_version.as_deref(), Some("3.0"));
        assert_eq!(controller.pending_version.as_deref(), Some("4.0"));
        assert_eq!(controller.deleting_version.as_deref(), Some("5.0"));
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

    /// Every deployment's image has its own name, so two deployments in one account rolling
    /// the same bundle must not send the same token: the API rejects a token reused with
    /// different parameters. One deployment's create and roll of a bundle stay one build.
    #[test]
    fn the_roll_token_is_scoped_to_the_deployment_image() {
        let bundle = "s3://b/sandbox-bundle/aaaa/bundle.zip";
        let inputs = |prefix: &str| {
            image_build_inputs(
                "us-east-1",
                "sandboxes",
                &sandbox_image_name(prefix, "sandboxes"),
                "arn:aws:iam::123456789012:role/build",
                bundle,
                2048,
            )
        };

        let first = inputs("deployment-a").client_token;
        let second = inputs("deployment-b").client_token;

        assert_ne!(
            first, second,
            "two deployments rolling one bundle must send different tokens"
        );
        assert_eq!(
            first,
            build_client_token("deployment-a-sandboxes", bundle),
            "a roll uses the same token as the create of that image and bundle"
        );
    }

    /// Back-to-back rolls must record both previously published versions, not overwrite the
    /// first: a version that leaves the record is a version nothing ever deletes.
    #[test]
    fn back_to_back_rolls_retire_every_replaced_version() {
        let mut controller = ready_controller();
        controller.pending_version = Some("2.0".to_string());
        controller.promote_pending_version();
        controller.pending_version = Some("3.0".to_string());
        controller.promote_pending_version();

        assert_eq!(controller.active_version.as_deref(), Some("3.0"));
        let retired: Vec<&str> = controller
            .retired_versions
            .iter()
            .map(|r| r.version.as_str())
            .collect();
        assert_eq!(
            retired,
            vec!["1.0", "2.0"],
            "both previously published versions remain recorded"
        );
    }

    /// A retry after a failed build resumes at the poll. With the dead version dropped, the
    /// poll hands off to a fresh build instead of reporting the same failure forever.
    #[tokio::test]
    async fn a_retry_after_a_failed_build_resubmits_instead_of_repolling() {
        let mut client = MockLambdaMicrovmsApi::new();
        // No get_microvm_image_version expectation: polling the dead version again would panic.
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(|_, _| {
                Ok(
                    alien_aws_clients::lambda_microvms::UpdateMicrovmImageResponse {
                        image_arn: Some(IMAGE_ARN.to_string()),
                        name: Some("test-agents".to_string()),
                        state: Some("CREATED".to_string()),
                        image_version: Some("2.0".to_string()),
                    },
                )
            });
        let mut controller = ready_controller();
        controller.state = AwsSandboxState::WaitingForRolledImageActive;
        controller.pending_version = None;
        controller.pending_bundle_uri = None;
        let mut executor = executor(controller, client).await;
        executor
            .step()
            .await
            .expect("the poll hands off to a fresh build");
        executor.step().await.expect("the fresh build is submitted");
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.pending_version.as_deref(), Some("2.0"));
        assert_eq!(
            controller.active_version.as_deref(),
            Some("1.0"),
            "serving version untouched"
        );
    }

    /// A create whose response was lost leaves an image under the derived name whose running
    /// version cannot be tied to any bundle. Claiming the desired bundle for it would let a
    /// release that changed in between count as deployed forever, so the probe adopts only the
    /// identity and rolls the desired bundle onto it.
    #[tokio::test]
    async fn an_existing_image_is_adopted_by_identity_and_rolled_not_claimed() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| {
                Ok(MicrovmImage {
                    description: None,
                    image_identifier: Some("test-agents".to_string()),
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("CREATED".to_string()),
                })
            });
        // No expect_create_microvm_image: a second create under the name would panic the mock.
        client
            .expect_update_microvm_image()
            .withf(|identifier, request| {
                identifier == IMAGE_ARN && request.code_artifact.uri == BUNDLE_URI
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
        client
            .expect_get_microvm_image_version()
            .withf(|identifier, version| identifier == IMAGE_ARN && version == "2.0")
            .returning(|_, _| Ok(active_version()));
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("2.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });

        let mut executor = executor(runtime_seeded_controller(), client).await;
        executor
            .run_until_terminal()
            .await
            .expect("the adopted image rolls onto the desired bundle and becomes active");
        assert_eq!(executor.status(), ResourceStatus::Running);
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            controller.active_version.as_deref(),
            Some("2.0"),
            "the rolled version serves"
        );
        assert_eq!(
            controller.bundle_uri.as_deref(),
            Some(BUNDLE_URI),
            "recorded only after its build"
        );
        assert_eq!(
            controller
                .retired_versions
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1.0"],
            "the previous explicit pin remains recorded"
        );
    }

    /// A create that restarts after a failure comes back with a `Default` controller: the
    /// executor rebuilds the resource through `new_pending`, which keeps no controller state.
    /// The build inputs must therefore be recomputed, not read back from what setup registered,
    /// or the restarted create dies on its own guard and the sandbox can never be recovered.
    #[tokio::test]
    async fn a_restarted_create_builds_without_anything_registered() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .withf(|request| {
                request.name == "test-agents"
                    && request.build_role_arn == BUILD_ROLE_ARN
                    && request.code_artifact.uri == BUNDLE_URI
            })
            .times(1)
            .returning(|_| Ok(created_response()));
        client
            .expect_get_microvm_image_version()
            .returning(|_, _| Ok(active_version()));

        // Nothing registered, nothing remembered: the state a restarted create starts from.
        let controller = AwsSandboxController::default();
        assert!(controller.build_role_arn.is_none());
        assert!(controller.bundle_uri.is_none());

        let mut executor = executor(controller, client).await;
        executor
            .step()
            .await
            .expect("the restarted create derives its build inputs");
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            controller.pending_version.as_deref(),
            Some("1.0"),
            "the build the restart submitted is the one being tracked"
        );
    }

    /// The restart that finds its image already built: a create whose response was lost and
    /// whose state went with it. The adopt-and-roll must still carry the derived role and bundle.
    #[tokio::test]
    async fn a_restarted_create_adopts_its_existing_image_without_anything_registered() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| {
                Ok(MicrovmImage {
                    description: None,
                    image_identifier: Some("test-agents".to_string()),
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("CREATED".to_string()),
                })
            });
        client
            .expect_update_microvm_image()
            .withf(|identifier, request| {
                identifier == IMAGE_ARN
                    && request.build_role_arn == BUILD_ROLE_ARN
                    && request.code_artifact.uri == BUNDLE_URI
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

        let mut executor = executor(AwsSandboxController::default(), client).await;
        executor
            .step()
            .await
            .expect("the restarted create adopts the image with derived build inputs");
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.pending_version.as_deref(), Some("2.0"));
        assert_eq!(controller.pending_bundle_uri.as_deref(), Some(BUNDLE_URI));
    }

    /// The full runtime build: the create call must satisfy the already-deployed
    /// `sandbox/provision` grant — the `deployment` and `managed-by: runtime` request tags
    /// are what the policy conditions on, the build role is what it authorizes as a pass —
    /// and the recorded version must be the string AWS mints, `1.0`.
    #[tokio::test]
    async fn a_live_sandbox_builds_its_image_and_reaches_running() {
        let mut client = MockLambdaMicrovmsApi::new();
        // The pre-create probe reads by ARN: the API rejects a bare name with a 400 that no
        // absent-resource check can read as absence, so the create would never be reached.
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .withf(|request| {
                request.name == "test-agents"
                    // Token carries the image name and the bundle, so a new release is a new
                    // logical create and a roll of the same image and bundle sends the same one.
                    && request.description.as_ref().is_some_and(|description| description.ends_with(&format!("[build:{}]", request.client_token)))
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
                        .is_some_and(|hooks| {
                            hooks.port == AWS_MICROVM.port
                                // The image build fails a broken agent here rather than at the
                                // first session, and a roll that dropped it would say nothing.
                                && hooks.microvm_image_hooks.as_ref().is_some_and(|build| {
                                    build.validate == "ENABLED"
                                        && build.validate_timeout_in_seconds == 30
                                })
                        })
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
                    description: None,
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
        assert_eq!(binding["idlePauseSeconds"], 600);
        assert_eq!(binding["maxLifetimeSeconds"], 1800);
    }

    /// A Live sandbox that declares image permissions builds its image with the agent's
    /// contract: each connection resolved to the address range of the network setup created,
    /// and the image-command value a released agent refuses, so an older agent fails the
    /// image build instead of running without the limits.
    #[tokio::test]
    async fn a_live_sandbox_with_image_permissions_builds_with_the_agent_contract() {
        use crate::{AwsNetworkController, AwsNetworkState};
        use alien_core::{
            CustomResource, Network, NetworkSettings, ResourceType, SandboxImageConnection,
            SandboxImageMount, SandboxImageMountType, SandboxImagePermissions,
        };

        let manifest: alien_core::ExtensionManifest = serde_json::from_value(serde_json::json!({
            "name": "acme",
            "version": "0.1.0",
            "digest": "sha256:00",
            "resourceTypes": {
                "acme/files": {
                    "lifecycles": ["live"],
                    "holdsData": true,
                    "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                    "controllerPlatforms": ["aws"],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} },
                    "requiresNetwork": true,
                    "privateNetworkEndpoints": "stackNetwork"
                }
            }
        }))
        .expect("extension manifest");
        let mut subject = sandbox();
        subject.image_user = Some(0);
        subject.lifecycle.on_pause = Some(vec!["/usr/local/bin/app".into(), "pause".into()]);
        subject.image_permissions = Some(SandboxImagePermissions {
            mounts: vec![SandboxImageMount {
                mount_type: SandboxImageMountType::Nfs,
                path: "/workspace".into(),
                size: None,
                from: None,
            }],
            connections: vec![SandboxImageConnection {
                to: ResourceRef::new(ResourceType::from("acme/files"), "workspace-files"),
                port: 2049,
            }],
        });

        let environment = Arc::new(Mutex::new(None));
        let captured = environment.clone();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .times(1)
            .returning(move |request| {
                *captured.lock().unwrap() = Some(request.environment_variables.clone());
                Ok(created_response())
            });
        client
            .expect_get_microvm_image_version()
            .returning(|_, _| Ok(active_version()));
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });

        let mut executor = SingleControllerExecutor::builder()
            .resource(subject)
            .controller(runtime_seeded_controller())
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .extension(manifest)
            .with_stack_resource(
                CustomResource::from_untagged(
                    "acme/files",
                    serde_json::json!({ "id": "workspace-files" }),
                )
                .unwrap(),
                alien_core::ResourceLifecycle::Live,
            )
            .stack_resource_outputs(
                "workspace-files",
                alien_core::ResourceOutputs::new(
                    alien_core::CustomResourceOutputs::from_untagged(
                        "acme/files",
                        serde_json::json!({ "privateIpv4Addresses": ["10.42.3.9", "10.42.1.4"] }),
                    )
                    .unwrap(),
                ),
            )
            .with_dependency(
                Network::new("default-network".to_string())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                AwsNetworkController {
                    state: AwsNetworkState::Ready,
                    vpc_id: Some("vpc-0123".to_string()),
                    cidr_block: Some("10.42.0.0/16".to_string()),
                    ..Default::default()
                },
            )
            .build()
            .await
            .expect("executor should build");
        executor
            .run_until_terminal()
            .await
            .expect("create flow should succeed");
        assert_eq!(executor.status(), ResourceStatus::Running);

        let environment = environment.lock().unwrap().clone().expect("image created");
        assert_eq!(
            environment
                .get(alien_core::sandbox_image::ENV_IMAGE_COMMAND)
                .map(String::as_str),
            Some(alien_core::sandbox_image::IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS)
        );
        assert_eq!(
            environment
                .get(alien_core::sandbox_image::ENV_IMAGE_PERMISSIONS_VERSION)
                .map(String::as_str),
            Some(alien_core::sandbox_image::IMAGE_PERMISSIONS_VERSION)
        );
        let contract: serde_json::Value = serde_json::from_str(
            environment
                .get(alien_core::sandbox_image::ENV_IMAGE_PERMISSIONS)
                .expect("the contract is set"),
        )
        .expect("the contract is JSON");
        assert_eq!(
            contract,
            serde_json::json!({
                "mounts": [{ "type": "nfs", "path": "/workspace" }],
                "connections": [
                    { "cidr": "10.42.1.4/32", "port": 2049 },
                    { "cidr": "10.42.3.9/32", "port": 2049 }
                ],
                "onPause": ["/usr/local/bin/app", "pause"]
            })
        );
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            controller
                .supervisor_environment
                .get(alien_core::sandbox_image::ENV_IMAGE_PERMISSIONS),
            environment.get(alien_core::sandbox_image::ENV_IMAGE_PERMISSIONS),
            "the recorded environment matches, so the next pass does not roll"
        );
        assert!(
            !executor.needs_update().expect("update check"),
            "nothing moved, so no update is asked for"
        );

        // The target publishes new addresses with its own config unchanged: the image's contract
        // is stale, so the sandbox asks for the update that rolls it.
        let mut target = executor.stack_state().resources["workspace-files"].clone();
        target.outputs = Some(alien_core::ResourceOutputs::new(
            alien_core::CustomResourceOutputs::from_untagged(
                "acme/files",
                serde_json::json!({ "privateIpv4Addresses": ["10.42.3.9", "10.42.2.7"] }),
            )
            .unwrap(),
        ));
        executor.insert_saved_resource("workspace-files", target.clone());
        assert!(
            executor.needs_update().expect("update check"),
            "a moved address asks for the update that rolls the image"
        );

        // A target that published nothing (yet) cannot be rolled onto; no update is asked for.
        target.outputs = None;
        executor.insert_saved_resource("workspace-files", target);
        assert!(!executor.needs_update().expect("update check"));
    }

    /// A Worker creates sessions under its profile's role, so a direct setup that builds a Frozen
    /// sandbox has to grant each profile the sets it was given on this sandbox, scoped to this
    /// image. Without it the Worker's first `RunMicrovm` is denied.
    #[tokio::test]
    async fn a_direct_setup_grants_each_profile_its_sandbox_sets_on_this_image() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .times(1)
            .returning(|_| Ok(created_response()));
        client
            .expect_get_microvm_image_version()
            .times(1)
            .returning(|_, _| Ok(active_version()));
        let client = Arc::new(client);

        let put_policies: Arc<Mutex<Vec<(String, String, serde_json::Value)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let mut iam = MockIamApi::new();
        // A fresh role holds no inline policies yet.
        iam.expect_list_role_policies().returning(|_| {
            Ok(alien_aws_clients::iam::ListRolePoliciesResponse {
                list_role_policies_result: alien_aws_clients::iam::ListRolePoliciesResult {
                    policy_names: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        let recorded = put_policies.clone();
        iam.expect_put_role_policy()
            .returning(move |role, name, document| {
                recorded.lock().expect("lock").push((
                    role.to_string(),
                    name.to_string(),
                    serde_json::from_str(document).expect("policy is JSON"),
                ));
                Ok(())
            });
        let iam = Arc::new(iam);

        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(client.clone()));
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));

        let mut profile = PermissionProfile::new();
        profile.0.insert(
            "agents".to_string(),
            vec![
                PermissionSetReference::from_name("sandbox/management"),
                PermissionSetReference::from_name("sandbox/execute"),
            ],
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(runtime_seeded_controller())
            .platform(Platform::Aws)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
            .permission_profile("execution", profile)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");

        // Step through the create flow only; `Ready` polls the image, which this test is not about.
        while executor.status() != ResourceStatus::Running {
            executor.step().await.expect("create flow step");
        }

        let put_policies = put_policies.lock().expect("lock");
        let granted: Vec<(&str, &str)> = put_policies
            .iter()
            .map(|(role, name, _)| (role.as_str(), name.as_str()))
            .collect();
        assert_eq!(
            granted,
            vec![
                ("execution-sa", "deployment-agents-sandbox-management"),
                ("execution-sa", "deployment-agents-sandbox-execute"),
            ]
        );

        // Every action each set grants, mapped to the resources it is granted on.
        let mut actions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, _, document) in put_policies.iter() {
            for statement in document["Statement"].as_array().expect("statements") {
                assert_eq!(statement["Effect"], "Allow");
                let resources: Vec<String> = match &statement["Resource"] {
                    serde_json::Value::String(resource) => vec![resource.clone()],
                    serde_json::Value::Array(resources) => resources
                        .iter()
                        .map(|resource| resource.as_str().expect("string").to_string())
                        .collect(),
                    other => panic!("unexpected Resource {other}"),
                };
                let statement_actions: Vec<String> = match &statement["Action"] {
                    serde_json::Value::String(action) => vec![action.clone()],
                    serde_json::Value::Array(actions) => actions
                        .iter()
                        .map(|action| action.as_str().expect("string").to_string())
                        .collect(),
                    other => panic!("unexpected Action {other}"),
                };
                for action in statement_actions {
                    actions.entry(action).or_default().extend(resources.clone());
                }
            }
        }
        for action in [
            "lambda:RunMicrovm",
            "lambda:TerminateMicrovm",
            "lambda:CreateMicrovmAuthToken",
            "lambda:GetMicrovm",
        ] {
            assert_eq!(
                actions.get(action),
                Some(&vec![IMAGE_ARN.to_string()]),
                "{action} is granted on this sandbox's image alone"
            );
        }
        // AWS authorizes the connector pass against an id it assigns, so no name can scope it.
        assert_eq!(
            actions.get("lambda:PassNetworkConnector"),
            Some(&vec!["*".to_string()])
        );
    }

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
                    // Keyed on this deployment's image, as the create is, not the stack resource
                    // id another deployment in the account shares.
                    && request.description.as_ref().is_some_and(|description| description.ends_with(&format!("[build:{}]", request.client_token)))
                    // PUT semantics: a field left out is dropped, and this is the one that keeps
                    // sandbox contents out of the customer's logs.
                    && request.logging == Some(MicrovmImageLogging::Disabled {})
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
            controller
                .retired_versions
                .first()
                .map(|r| r.version.as_str()),
            Some("1.0"),
            "the replaced version is retained until its sessions can no longer be running"
        );
        assert!(
            !controller.retired_versions.is_empty(),
            "the previous explicit pin remains recorded at the switch"
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

        let failed_token = Arc::new(Mutex::new(None));
        let captured_token = failed_token.clone();
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(move |_, request| {
                *captured_token.lock().unwrap() = Some(request.client_token);
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
        assert_eq!(controller.build_retry_generation, 1);
        let restored: AwsSandboxController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        drop(executor);
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .times(1)
            .returning(move |_, request| {
                assert_ne!(
                    Some(request.client_token),
                    *failed_token.lock().unwrap(),
                    "retry must not replay the failed build token"
                );
                Ok(UpdateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("3.0".to_string()),
                    name: None,
                    state: None,
                })
            });
        let mut resumed = SingleControllerExecutor::builder()
            .resource(sandbox_with_bundle(NEXT_BUNDLE))
            .controller(restored)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .unwrap();
        resumed
            .step()
            .await
            .expect("failed poll hands off to a fresh build");
        resumed
            .step()
            .await
            .expect("fresh token submits a replacement");
        assert_eq!(
            resumed
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .as_deref(),
            Some("3.0")
        );
    }

    /// Stores move: a release may name a new bundle store while this sandbox's setup granted the
    /// old one. The build role can read only the granted store, which holds the same key, so the
    /// roll reads it there.
    #[tokio::test]
    async fn a_release_naming_another_store_rolls_from_the_store_setup_granted() {
        const GRANTED: &str = "s3://old-store-us-east-1/sandbox-bundle/v1/bundle.zip";
        const NEXT: &str = "s3://new-store-us-east-1/sandbox-bundle/v2/bundle.zip";
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .withf(|identifier, request| {
                identifier == IMAGE_ARN
                    && request.code_artifact.uri
                        == "s3://old-store-us-east-1/sandbox-bundle/v2/bundle.zip"
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
        let mut controller = ready_controller();
        controller.bundle_uri = Some(GRANTED.to_string());
        controller.granted_bundle_uri = Some(GRANTED.to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with_bundle(GRANTED))
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(NEXT))
            .expect("transition to update");
        executor.step().await.expect("updating_sandbox");
        executor.step().await.expect("updating_image");
        let rolled = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            rolled.pending_bundle_uri.as_deref(),
            Some("s3://old-store-us-east-1/sandbox-bundle/v2/bundle.zip")
        );
    }

    /// The same release seen through the granted store is not a change: a store move alone must
    /// not rebuild the image. The expectation-free mock panics on any MicroVM call.
    #[tokio::test]
    async fn a_store_move_alone_rebuilds_nothing() {
        const GRANTED: &str = "s3://old-store-us-east-1/sandbox-bundle/v1/bundle.zip";
        let mut controller = ready_controller();
        controller.bundle_uri = Some(GRANTED.to_string());
        controller.granted_bundle_uri = Some(GRANTED.to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with_bundle(GRANTED))
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(MockLambdaMicrovmsApi::new()))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(
                "s3://new-store-us-east-1/sandbox-bundle/v1/bundle.zip",
            ))
            .expect("transition to update");
        executor.step().await.expect("updating_sandbox");
        let after = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(after.active_version.as_deref(), Some("1.0"));
        assert_eq!(after.pending_version, None);
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

    #[tokio::test]
    async fn changing_supervisor_policy_rolls_the_same_bundle_with_an_internal_capability_grant() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_update_microvm_image()
            .withf(|identifier, request| {
                identifier == IMAGE_ARN
                    && request.code_artifact.uri == BUNDLE_URI
                    && request.additional_os_capabilities == vec!["ALL"]
                    && request
                        .environment_variables
                        .get("ALIEN_SANDBOX_EXPECTED_EXEC_UID")
                        .map(String::as_str)
                        == Some("60001")
                    && request
                        .environment_variables
                        .get("ALIEN_SANDBOX_EGRESS")
                        .map(String::as_str)
                        == Some(r#"{"mode":"deny"}"#)
            })
            .times(1)
            .returning(|_, _| {
                Ok(UpdateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("2.0".to_string()),
                    name: None,
                    state: Some("UPDATING".to_string()),
                })
            });
        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        let mut executor = executor(controller, client).await;
        let mut desired = sandbox();
        desired.privileged_supervisor =
            Some(alien_core::SandboxPrivilegedSupervisor { command_uid: 60001 });
        executor.update(desired).expect("transition to update");
        executor.step().await.expect("updating_sandbox");
        executor.step().await.expect("updating_image");
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.pending_version.as_deref(), Some("2.0"));
        assert_eq!(controller.active_version.as_deref(), Some("1.0"));
        assert!(
            controller.supervisor_environment.is_empty(),
            "the serving security contract is retained until activation"
        );
    }

    /// A template's Frozen image belongs to the setup stack. Rebuilding it at runtime would use
    /// credentials that were never granted it, so no declaration reaches the build API — the mock
    /// answers only the Ready tick's read and panics on any other call.
    #[tokio::test]
    async fn a_frozen_sandbox_never_rebuilds_its_image() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
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
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");

        executor
            .update(sandbox_with_bundle(
                "s3://alien-bundles-test/sandbox/bundle-v2.zip",
            ))
            .expect("transition to update");
        for _ in 0..3 {
            executor
                .step()
                .await
                .expect("a Frozen sandbox settles without touching its image");
        }

        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(
            controller.active_version.as_deref(),
            Some("1.0"),
            "the setup-built version keeps serving"
        );
        assert!(controller.pending_version.is_none(), "nothing was rolled");
    }

    const NEXT_BUNDLE: &str = "s3://alien-bundles-test/sandbox/bundle-v2.zip";

    /// A Frozen sandbox a direct setup built, stepped as the runtime steps it.
    async fn direct_frozen_executor(
        controller: AwsSandboxController,
        client: MockLambdaMicrovmsApi,
        authority: alien_core::InitialSetupAuthority,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .initial_setup_authority(authority)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build")
    }

    fn direct_frozen_ready_controller() -> AwsSandboxController {
        AwsSandboxController {
            build_role_arn: Some(BUILD_ROLE_ARN.to_string()),
            bundle_uri: Some(BUNDLE_URI.to_string()),
            granted_bundle_uri: Some(BUNDLE_URI.to_string()),
            ..ready_controller()
        }
    }

    /// Only setup rolls a Frozen image. Outside it, the runtime identity may reach the image
    /// through a Live sibling's provision grant, so ownership is decided by who is running, not by
    /// the build role alone. Any build call panics the mock.
    #[tokio::test]
    async fn the_runtime_never_rolls_a_frozen_image_setup_built() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        let mut executor = direct_frozen_executor(
            direct_frozen_ready_controller(),
            client,
            alien_core::InitialSetupAuthority::ImportedHandoff,
        )
        .await;

        executor
            .update(sandbox_with_bundle(NEXT_BUNDLE))
            .expect("transition to update");
        for _ in 0..3 {
            executor
                .step()
                .await
                .expect("the update settles without a build");
        }
        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert!(controller.pending_version.is_none(), "nothing was rolled");
    }

    /// The build role's policy names one bundle object, so setup regrants it before a roll can
    /// read the new bundle. Until the registration says it has, the roll waits.
    #[tokio::test]
    async fn a_setup_roll_waits_until_setup_has_granted_the_new_bundle() {
        let mut executor = direct_frozen_executor(
            direct_frozen_ready_controller(),
            MockLambdaMicrovmsApi::new(),
            alien_core::InitialSetupAuthority::DirectSetup,
        )
        .await;

        executor
            .update(sandbox_with_bundle(NEXT_BUNDLE))
            .expect("transition to update");
        executor.step().await.expect("updating_sandbox");
        let error = executor
            .step()
            .await
            .expect_err("the roll must wait for the grant");
        assert_eq!(error.code, "DEPENDENCY_NOT_READY", "{error}");
    }

    /// A Frozen image is setup's, and carries setup's tags as the templates' does.
    #[tokio::test]
    async fn a_frozen_image_is_created_with_setups_tags() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_get_microvm_image()
            .returning(|_| Err(not_found()));
        client
            .expect_create_microvm_image()
            .withf(|request| {
                request.tags.get("managed-by").map(String::as_str) == Some("setup")
                    && request.tags.get("resource-type").map(String::as_str) == Some("sandbox")
            })
            .times(1)
            .returning(|_| Ok(created_response()));
        let mut executor = direct_frozen_executor(
            runtime_seeded_controller(),
            client,
            alien_core::InitialSetupAuthority::DirectSetup,
        )
        .await;
        executor.step().await.expect("creating_image");
    }

    /// A Frozen sandbox carries no bundle of its own — setup owns the image and never hands
    /// one over — so an ordinary update must still settle back to Ready rather than reading
    /// that absence as a changed declaration.
    #[tokio::test]
    async fn a_frozen_sandbox_updates_without_a_recorded_bundle() {
        let controller = AwsSandboxController {
            state: AwsSandboxState::Ready,
            image_identifier: Some(IMAGE_ARN.to_string()),
            image_arn: Some(IMAGE_ARN.to_string()),
            active_version: Some("1.0".to_string()),
            region: Some("us-east-1".to_string()),
            // Exactly what the importer's Frozen arm produces: no bundle, no build role.
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

        executor.update(sandbox()).expect("transition to update");
        executor
            .step()
            .await
            .expect("a Frozen sandbox must settle, not fail, on an ordinary update");

        let controller = executor
            .internal_state::<AwsSandboxController>()
            .expect("typed controller");
        assert_eq!(controller.active_version.as_deref(), Some("1.0"));
        assert!(
            controller
                .get_binding_params()
                .expect("binding params")
                .is_some(),
            "an ordinary update must not withdraw a Frozen sandbox's binding"
        );
    }

    /// A Frozen sandbox's Ready tick runs as the runtime identity and leaves its retired versions to
    /// setup, whatever IAM a Live sibling grants. Any version delete panics the mock.
    #[tokio::test]
    async fn a_frozen_sandbox_keeps_its_retired_versions_at_runtime() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("2.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        let mut controller = ready_controller();
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        controller.max_lifetime_seconds = Some(1800);
        controller.retired_versions = vec![RetiredVersion {
            version: "1.0".to_string(),
            retired_at: chrono::Utc::now() - chrono::Duration::seconds(3600),
            inactive_at: None,
        }];
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");
        executor.step().await.expect("ready tick past the window");

        assert_eq!(
            executor
                .internal_state::<AwsSandboxController>()
                .expect("typed controller")
                .retired_versions
                .len(),
            1
        );
    }

    /// The two state names an earlier controller version wrote that this one lacks must still
    /// load, and land where their meaning survives: a not-yet-observed setup image becomes a
    /// Ready that re-reads it; an in-flight delete resumes at the sweep.
    #[test]
    fn states_written_by_an_earlier_controller_version_still_load() {
        let record = |state: &str| {
            serde_json::json!({
                "_controllerStateVersion": 1,
                "allowEgress": false,
                "egressConnectorArns": [],
                "imageArn": IMAGE_ARN,
                "imageIdentifier": "sbx-image",
                "imageVersion": "1.0",
                "internalStayCount": null,
                "previewPorts": [],
                "region": "us-east-1",
                "state": state,
                "type": "AwsSandboxController"
            })
        };
        let awaiting = AwsSandboxController::from_persisted(record("awaitImage"))
            .expect("awaitImage must load");
        assert!(matches!(awaiting.state, AwsSandboxState::Ready));
        assert!(
            awaiting.get_binding_params().expect("readable").is_some(),
            "a setup-baked image with its fields recorded keeps its binding"
        );
        let deleting =
            AwsSandboxController::from_persisted(record("deleting")).expect("deleting must load");
        assert!(matches!(
            deleting.state,
            AwsSandboxState::DeletingImageVersions
        ));
        let unknown = AwsSandboxController::from_persisted(record("neverExisted"));
        assert!(unknown.is_err(), "no other unknown state is guessed at");
    }

    /// State written before a sandbox's image could be rebuilt carried one version field and no
    /// notion of an active one. It must re-hydrate with that version serving: reading it as "no
    /// active version" would withdraw the binding of a deployment that never changed.
    ///
    /// Only settled `Ready` state carries across. The transient states such a record could also
    /// hold name a build or a delete that was already in flight, and resuming one from a
    /// different controller's notion of progress is not something this can honour.
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

    /// Deleting the image reaps its versions with it, so teardown makes exactly one call.
    /// Sweeping versions first is not merely redundant: AWS refuses to delete an image's last
    /// version at all, which failed teardown for every sandbox built once and never rolled.
    #[tokio::test]
    async fn delete_removes_the_image_without_sweeping_its_versions() {
        let mut client = MockLambdaMicrovmsApi::new();
        // No version expectations: the mock panics on any call, so a reintroduced sweep fails here.
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(1)
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

    fn settling_refusal() -> alien_error::AlienError<CloudClientErrorData> {
        alien_error::AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: "Microvm".to_string(),
            resource_name: "DeleteMicrovmImage".to_string(),
            message: "Cannot delete MicroVM image in its current state".to_string(),
        })
    }

    /// AWS refuses to delete an image that is still settling and accepts the same call shortly
    /// after. The first refusal must not fail the delete.
    #[tokio::test]
    async fn a_delete_refused_while_the_image_settles_is_asked_again() {
        let mut client = MockLambdaMicrovmsApi::new();
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(2)
            .returning(move |_| {
                if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Err(settling_refusal())
                } else {
                    Ok(())
                }
            });

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("the second delete succeeds");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    fn running_sessions_refusal() -> alien_error::AlienError<CloudClientErrorData> {
        alien_error::AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: "MicrovmImage".to_string(),
            resource_name: "DeleteMicrovmImage".to_string(),
            message: alien_aws_clients::lambda_microvms::IMAGE_HAS_RUNNING_MICROVMS.to_string(),
        })
    }

    /// AWS refuses to delete an image while sessions started from it are running, and accepts
    /// the same call once they end. The refusal waits rather than failing the delete, past the
    /// ten minutes a settling image gets.
    #[tokio::test]
    async fn a_delete_refused_for_running_sessions_waits_for_them_to_end() {
        // One past the settling budget: under that budget this refusal would fail the delete.
        let refusals = IMAGE_DELETE_MAX_POLLS + 1;
        let mut client = MockLambdaMicrovmsApi::new();
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
            .times(refusals as usize + 1)
            .returning(move |_| {
                if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < refusals {
                    Err(running_sessions_refusal())
                } else {
                    Ok(())
                }
            });

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("the delete succeeds once the sessions end");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            refusals + 1
        );
        assert_eq!(
            executor
                .suggested_delays()
                .iter()
                .filter(|delay| **delay == RUNNING_SESSIONS_POLL_INTERVAL)
                .count(),
            refusals as usize,
            "each refusal waits the running-sessions interval"
        );
    }

    /// The wait for running sessions is bounded by the eight hours AWS lets a session run. Past
    /// that, the delete fails and says what to do.
    #[tokio::test]
    async fn sessions_still_running_past_the_session_cap_fail_the_delete_with_the_cause() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_delete_microvm_image()
            .times(1)
            .returning(|_| Err(running_sessions_refusal()));
        // Already waiting in the delete flow, since before the cap.
        let controller = AwsSandboxController {
            state: AwsSandboxState::DeletingImage,
            active_version: None,
            wait_for_running_sessions_started_at: Some(
                chrono::Utc::now()
                    - chrono::Duration::minutes(RUNNING_SESSIONS_MAX_WAIT_MINUTES + 1),
            ),
            ..ready_controller()
        };

        let mut executor = executor(controller, client).await;
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("sessions past the cap fail the delete");
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("still has running sessions")
                && rendered.contains("terminate the sessions"),
            "the failure names the cause and the fix: {rendered}"
        );
    }

    /// Sessions ending and the image then settling is the order a real teardown sees. The
    /// settling wait keeps its own budget, whatever the session wait already used.
    #[tokio::test]
    async fn an_image_that_settles_after_its_sessions_end_gets_the_full_settling_budget() {
        let session_refusals = IMAGE_DELETE_MAX_POLLS + 1;
        let settling_refusals = 3;
        let mut client = MockLambdaMicrovmsApi::new();
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        client
            .expect_delete_microvm_image()
            .times((session_refusals + settling_refusals + 1) as usize)
            .returning(move |_| {
                let call = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call < session_refusals {
                    Err(running_sessions_refusal())
                } else if call < session_refusals + settling_refusals {
                    Err(settling_refusal())
                } else {
                    Ok(())
                }
            });

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("the delete succeeds once the image settles");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            session_refusals + settling_refusals + 1
        );
    }

    /// A retry resumes the failed delete in `DeletingImage`, not at the flow entry. Both waits
    /// must start over there: a delete that ran out of its settling budget, or failed while
    /// sessions ran, is given a full budget again rather than failing on its first refusal.
    #[tokio::test]
    async fn a_retried_delete_gets_fresh_settling_and_session_waits() {
        for (refusal, exhausted) in [
            (
                settling_refusal as fn() -> alien_error::AlienError<CloudClientErrorData>,
                AwsSandboxController {
                    state: AwsSandboxState::DeletingImage,
                    active_version: None,
                    wait_for_image_settle_iterations: IMAGE_DELETE_MAX_POLLS,
                    ..ready_controller()
                },
            ),
            (
                running_sessions_refusal as fn() -> alien_error::AlienError<CloudClientErrorData>,
                AwsSandboxController {
                    state: AwsSandboxState::DeletingImage,
                    active_version: None,
                    wait_for_running_sessions_started_at: Some(
                        chrono::Utc::now()
                            - chrono::Duration::minutes(RUNNING_SESSIONS_MAX_WAIT_MINUTES + 60),
                    ),
                    ..ready_controller()
                },
            ),
        ] {
            // The resource as a delete that failed in `DeletingImage` leaves it, retried the way
            // the executor retries it.
            let mut resource_state = alien_core::StackResourceState::new_pending(
                Sandbox::RESOURCE_TYPE.as_ref().to_string(),
                alien_core::Resource::new(sandbox()),
                None,
                Vec::new(),
            );
            resource_state.status = ResourceStatus::DeleteFailed;
            resource_state
                .set_last_failed_controller(Some(Box::new(exhausted)))
                .expect("store the failed checkpoint");
            assert!(resource_state.retry_failed().expect("retry"));
            assert_eq!(resource_state.status, ResourceStatus::Deleting);
            let retried = resource_state
                .get_internal_controller_typed::<AwsSandboxController>()
                .expect("the retried checkpoint");
            assert_eq!(retried.state, AwsSandboxState::DeletingImage);
            assert_eq!(retried.wait_for_image_settle_iterations, 0);
            assert!(retried.wait_for_running_sessions_started_at.is_none());

            let mut client = MockLambdaMicrovmsApi::new();
            let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let seen = calls.clone();
            client
                .expect_delete_microvm_image()
                .times(2)
                .returning(move |_| {
                    if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        Err(refusal())
                    } else {
                        Ok(())
                    }
                });
            let mut executor = executor(retried, client).await;
            executor
                .run_until_terminal()
                .await
                .expect("the retried delete waits out its first refusal");
            assert_eq!(executor.status(), ResourceStatus::Deleted);
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        }
    }

    /// A wait recorded by an earlier delete that failed some other way is not carried into a new
    /// delete: its first refusal waits instead of failing on the old clock.
    #[tokio::test]
    async fn a_new_delete_does_not_inherit_an_earlier_running_sessions_wait() {
        let mut client = MockLambdaMicrovmsApi::new();
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        client
            .expect_delete_microvm_image()
            .times(2)
            .returning(move |_| {
                if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Err(running_sessions_refusal())
                } else {
                    Ok(())
                }
            });
        let controller = AwsSandboxController {
            wait_for_running_sessions_started_at: Some(
                chrono::Utc::now()
                    - chrono::Duration::minutes(RUNNING_SESSIONS_MAX_WAIT_MINUTES + 60),
            ),
            ..ready_controller()
        };

        let mut executor = executor(controller, client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect("the new delete waits out its own refusal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// The wait is bounded: an image that never leaves its state fails the delete.
    #[tokio::test]
    async fn an_image_that_never_settles_fails_the_delete_within_its_poll_budget() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_delete_microvm_image()
            .times(IMAGE_DELETE_MAX_POLLS as usize)
            .returning(|_| Err(settling_refusal()));

        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("an image that never settles fails the delete");
        assert!(
            matches!(error.error, Some(ErrorData::PollingTimeout { .. })),
            "the delete fails on its poll budget: {error:?}"
        );
    }

    /// A sandbox whose create failed before the image existed still issues the delete, against
    /// the ARN its name derives to; an image that never existed answers NotFound, which the
    /// delete treats as done.
    #[tokio::test]
    async fn a_sandbox_that_never_recorded_its_image_still_deletes_by_its_derived_arn() {
        // A create can succeed without its ARN reaching state; walking past it would leak a
        // live image forever, so the delete targets the ARN derived from the name instead.
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_delete_microvm_image()
            .withf(|identifier| identifier == IMAGE_ARN)
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
        client.expect_delete_microvm_image().returning(|_| {
            Err(AlienError::new(CloudClientErrorData::GenericError {
                message: "throttled".to_string(),
            }))
        });
        let mut executor = executor(ready_controller(), client).await;
        executor.delete().expect("transition to delete");
        executor
            .run_until_terminal()
            .await
            .expect_err("the failure must surface, not be retried forever");
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

    // ─────────────── INTERNET EGRESS REFRESH AND DELETION ──────────────────

    #[cfg(all(feature = "local", feature = "test-utils"))]
    mod internet_egress {
        use super::*;

        const OTHER_IMAGE_ARN: &str =
            "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-other";

        fn internet_sandbox(id: &str) -> Sandbox {
            let mut sandbox = sandbox();
            sandbox.id = id.to_string();
            sandbox.egress = SandboxEgress::Internet;
            sandbox
        }

        /// A running internet sandbox that holds the deployment's egress public key, as every
        /// internet sandbox does once it has initialized its egress.
        fn running_internet_controller(image_arn: &str) -> AwsSandboxController {
            AwsSandboxController {
                image_identifier: Some(image_arn.to_string()),
                image_arn: Some(image_arn.to_string()),
                egress: Some(SandboxEgress::Internet.into()),
                egress_network_verified: true,
                egress_public_key: Some(crate::sandbox::egress::GatewayPublicKey {
                    kid: "kid-test".to_string(),
                    public_key: "public-key-test".to_string(),
                }),
                ..ready_controller()
            }
        }

        fn sandbox_network() -> alien_core::Network {
            alien_core::Network::new("network".into())
                .settings(alien_core::NetworkSettings::Create {
                    cidr: Some("10.0.0.0/16".into()),
                    availability_zones: 2,
                })
                .build()
        }

        /// The imported network as a CloudFormation deployment records it, with the dedicated
        /// connector and gateway subnets setup provided.
        fn sandbox_network_controller() -> crate::AwsNetworkController {
            crate::AwsNetworkController {
                vpc_id: Some("vpc-test".into()),
                private_subnet_ids: vec!["workload".into()],
                sandbox_network: Some(alien_core::import::data::AwsSandboxNetwork {
                    sandbox_subnet_ids: vec!["connector-a".into(), "connector-b".into()],
                    gateway_subnet_ids: vec!["gateway-a".into(), "gateway-b".into()],
                    sandbox_cidrs: vec![],
                    gateway_cidrs: vec![],
                    workload_cidrs: vec![],
                    vpc_cidrs: vec![],
                }),
                ..Default::default()
            }
        }

        /// EC2 as a valid isolated sandbox network reads: route-less connector subnets on the main
        /// table, and gateway and workload subnets sharing one NAT route.
        fn valid_sandbox_network_ec2() -> alien_aws_clients::ec2::MockEc2Api {
            use serde_json::json;
            let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
            ec2.expect_describe_vpcs().returning(|_| {
                Ok(serde_json::from_value(json!({
                    "vpcSet":{"item":[{"vpcId":"vpc-test","cidrBlock":"10.0.0.0/16"}]}
                }))
                .unwrap())
            });
            ec2.expect_describe_subnets().returning(|_| {
                let items: Vec<_> = ["connector-a", "connector-b", "gateway-a", "gateway-b", "workload"]
                    .iter()
                    .enumerate()
                    .map(|(i, id)| json!({
                        "subnetId":id,"vpcId":"vpc-test","state":"available",
                        "cidrBlock":format!("10.0.{i}.0/24"),"availabilityZoneId":format!("zone-{}",i%2)
                    }))
                    .collect();
                Ok(serde_json::from_value(json!({"subnetSet":{"item":items}})).unwrap())
            });
            ec2.expect_describe_route_tables().returning(|_| {
                let local = json!({"destinationCidrBlock":"10.0.0.0/16","gatewayId":"local","state":"active"});
                Ok(serde_json::from_value(json!({"routeTableSet":{"item":[
                    {"routeTableId":"main","vpcId":"vpc-test","routeSet":{"item":[local]},
                     "associationSet":{"item":[{"main":true}]}},
                    {"routeTableId":"nat","vpcId":"vpc-test","routeSet":{"item":[
                        local,
                        {"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-shared","state":"active"}
                     ]},
                     "associationSet":{"item":[{"subnetId":"gateway-a"},{"subnetId":"gateway-b"},{"subnetId":"workload"}]}}
                ]}}))
                .unwrap())
            });
            ec2
        }

        fn served_image(image_arn: &str) -> MicrovmImage {
            MicrovmImage {
                description: None,
                image_identifier: Some(image_arn.to_string()),
                image_arn: Some(image_arn.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            }
        }

        /// A live key owner keeps refreshing the gateway configuration: it validates the network
        /// and publishes the audiences of every internet sandbox the stack declares.
        #[tokio::test]
        async fn a_running_key_owner_refreshes_the_gateway_configuration() {
            use crate::{LocalVaultController, LocalVaultState};
            use alien_bindings::{providers::vault::LocalVault, Vault as _};
            use alien_core::{Vault, SECRETS_VAULT_ID};

            let directory = tempfile::tempdir().unwrap();
            let mut microvms = MockLambdaMicrovmsApi::new();
            microvms
                .expect_get_microvm_image()
                .returning(|_| Ok(served_image(IMAGE_ARN)));
            microvms
                .expect_list_microvm_image_versions()
                .returning(|_| Ok(vec![]));
            let microvms = Arc::new(microvms);
            let ec2 = Arc::new(valid_sandbox_network_ec2());
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_microvms_client()
                .returning(move |_| Ok(microvms.clone()));
            provider
                .expect_get_aws_ec2_client()
                .returning(move |_| Ok(ec2.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource(internet_sandbox("agents"))
                .controller(AwsSandboxController {
                    egress_network_verified: false,
                    ..running_internet_controller(IMAGE_ARN)
                })
                .platform(Platform::Aws)
                .service_provider(Arc::new(provider))
                .with_stack_resource(
                    internet_sandbox("other"),
                    alien_core::ResourceLifecycle::Live,
                )
                .with_dependency(sandbox_network(), sandbox_network_controller())
                .with_dependency(
                    Vault::new(SECRETS_VAULT_ID.into()).build(),
                    LocalVaultController {
                        state: LocalVaultState::Ready,
                        vault_path: Some(directory.path().to_string_lossy().into_owned()),
                        ..Default::default()
                    },
                )
                .build()
                .await
                .unwrap();

            run_gateway_on(&mut executor, EMBEDDING_GATEWAY);

            executor.step().await.expect("Ready verifies the network");
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            assert_eq!(controller.state, AwsSandboxState::RefreshingEgressConfig);
            executor.step().await.expect("the refresh publishes");
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            assert_eq!(controller.state, AwsSandboxState::Ready);
            assert!(controller.egress_network_verified);
            assert!(controller.get_binding_params().unwrap().is_some());

            let vault = LocalVault::new("secrets".into(), directory.path().into());
            let published: serde_json::Value = serde_json::from_str(
                &vault
                    .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                    .await
                    .expect("the key owner published the gateway configuration"),
            )
            .unwrap();
            let audiences: Vec<_> = published["audiences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|audience| audience.as_str().unwrap().rsplit('/').next().unwrap())
                .collect();
            assert_eq!(audiences, ["agents", "other"]);
            assert_eq!(published["keys"][0]["kid"], "kid-test");
            // A stack without links publishes no `links` field at all.
            assert!(published.get("links").is_none());
        }

        /// The key owner publishes each sandbox's links by their overlay name inside the
        /// cluster's overlay range, and the linking sandbox's binding carries the overlay URL.
        #[tokio::test]
        async fn the_key_owner_publishes_links_by_their_overlay_name() {
            use crate::{LocalVaultController, LocalVaultState};
            use alien_bindings::{providers::vault::LocalVault, Vault as _};
            use alien_core::{Vault, SECRETS_VAULT_ID};

            let directory = tempfile::tempdir().unwrap();
            let mut microvms = MockLambdaMicrovmsApi::new();
            microvms
                .expect_get_microvm_image()
                .returning(|_| Ok(served_image(IMAGE_ARN)));
            microvms
                .expect_list_microvm_image_versions()
                .returning(|_| Ok(vec![]));
            let microvms = Arc::new(microvms);
            let ec2 = Arc::new(valid_sandbox_network_ec2());
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_microvms_client()
                .returning(move |_| Ok(microvms.clone()));
            provider
                .expect_get_aws_ec2_client()
                .returning(move |_| Ok(ec2.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource({
                    let mut agents = internet_sandbox("agents");
                    agents.gateway_links = vec![alien_core::ResourceRef::new(
                        alien_core::Container::RESOURCE_TYPE,
                        "api",
                    )];
                    agents
                })
                .controller(AwsSandboxController {
                    // An update passes here, so adding a link rebuilds the binding.
                    state: AwsSandboxState::InitializingEgressKey,
                    egress_network_verified: false,
                    ..running_internet_controller(IMAGE_ARN)
                })
                .platform(Platform::Aws)
                .service_provider(Arc::new(provider))
                .with_stack_resource(
                    internet_sandbox("other"),
                    alien_core::ResourceLifecycle::Live,
                )
                .with_stack_resource(
                    alien_core::ComputeCluster::new("compute".into())
                        .capacity_group(alien_core::CapacityGroup {
                            group_id: "apps".into(),
                            instance_type: Some("m7g.large".into()),
                            profile: None,
                            min_size: 1,
                            max_size: 1,
                            scale_policy: None,
                            nested_virtualization: None,
                            data_disk_gib_range: None,
                        })
                        .build(),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .with_stack_resource(
                    {
                        let mut api = gateway_container("example.invalid/api:1");
                        api.id = "api".into();
                        api.cluster = Some("compute".into());
                        api.permissions = "api".into();
                        api.ports = vec![alien_core::ContainerPort { port: 8080 }];
                        api
                    },
                    alien_core::ResourceLifecycle::Live,
                )
                .with_dependency(sandbox_network(), sandbox_network_controller())
                .with_dependency(
                    Vault::new(SECRETS_VAULT_ID.into()).build(),
                    LocalVaultController {
                        state: LocalVaultState::Ready,
                        vault_path: Some(directory.path().to_string_lossy().into_owned()),
                        ..Default::default()
                    },
                )
                .build()
                .await
                .unwrap();

            run_gateway_on(&mut executor, EMBEDDING_GATEWAY);

            for _ in 0..8 {
                let state = executor
                    .internal_state::<AwsSandboxController>()
                    .unwrap()
                    .state
                    .clone();
                if state == AwsSandboxState::Ready {
                    break;
                }
                executor
                    .step()
                    .await
                    .unwrap_or_else(|error| panic!("{state:?}: {error}"));
            }
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            assert_eq!(controller.state, AwsSandboxState::Ready);

            let vault = LocalVault::new("secrets".into(), directory.path().into());
            let published: serde_json::Value = serde_json::from_str(
                &vault
                    .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                    .await
                    .expect("the key owner published the gateway configuration"),
            )
            .unwrap();
            let audiences: Vec<_> = published["audiences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|audience| audience.as_str().unwrap().rsplit('/').next().unwrap())
                .collect();
            assert_eq!(audiences, ["agents", "other"]);
            let audience = published["audiences"][0].as_str().unwrap();
            assert_eq!(
                published["links"],
                serde_json::json!({audience: [{
                    "id": "api", "host": "api.svc", "ports": [8080], "ranges": ["10.244.0.0/16"]
                }]})
            );
            let binding = controller.get_binding_params().unwrap().unwrap();
            assert_eq!(
                binding["egress"]["links"]["api"]["internalUrl"],
                "http://api.svc:8080"
            );
        }

        /// One refresh pass of internet sandbox `id` in a stack that also declares `others`,
        /// against a local secrets vault in `directory`, with the gateway running an image that
        /// embeds its replica secret. Returns the controller afterwards.
        async fn refresh_pass(
            id: &str,
            others: &[&str],
            controller: AwsSandboxController,
            directory: &std::path::Path,
        ) -> AwsSandboxController {
            refresh_with(
                id,
                others,
                controller,
                directory,
                GatewayFixture {
                    image: Some(EMBEDDING_GATEWAY),
                    ..GatewayFixture::default()
                },
            )
            .await
        }

        /// Runs the gateway the stack mutations add, whatever image is pinned, on `image`.
        fn run_gateway_on(executor: &mut SingleControllerExecutor, image: &str) {
            let id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
            executor.insert_saved_resource(
                id,
                alien_core::StackResourceState::builder()
                    .resource_type(alien_core::Container::RESOURCE_TYPE.to_string())
                    .status(ResourceStatus::Running)
                    .config(alien_core::Resource::new(gateway_container(image)))
                    .lifecycle(alien_core::ResourceLifecycle::Live)
                    .build(),
            );
        }

        /// The running gateway, the replica secret's Secrets Manager answers, and who set up the
        /// deployment, for a refresh pass. A `fresh` gateway is only in the desired stack, on
        /// `image`: it has not been created yet.
        #[derive(Default)]
        struct GatewayFixture {
            image: Option<&'static str>,
            secrets_manager: Option<alien_aws_clients::secrets_manager::MockSecretsManagerApi>,
            direct_setup: bool,
            fresh: bool,
        }

        fn gateway_container(image: &str) -> alien_core::Container {
            alien_core::Container::new(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into())
                .code(alien_core::ContainerCode::Image {
                    image: image.into(),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "0.25".into(),
                    desired: "0.25".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "512Mi".into(),
                    desired: "512Mi".into(),
                })
                .replicas(2)
                .permissions(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into())
                .port(3128)
                .build()
        }

        async fn refresh_with(
            id: &str,
            others: &[&str],
            controller: AwsSandboxController,
            directory: &std::path::Path,
            gateway: GatewayFixture,
        ) -> AwsSandboxController {
            use crate::{LocalVaultController, LocalVaultState};
            use alien_core::{Vault, SECRETS_VAULT_ID};

            let mut microvms = MockLambdaMicrovmsApi::new();
            microvms
                .expect_get_microvm_image()
                .returning(|_| Ok(served_image(IMAGE_ARN)));
            microvms
                .expect_list_microvm_image_versions()
                .returning(|_| Ok(vec![]));
            let microvms = Arc::new(microvms);
            let ec2 = Arc::new(valid_sandbox_network_ec2());
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_microvms_client()
                .returning(move |_| Ok(microvms.clone()));
            provider
                .expect_get_aws_ec2_client()
                .returning(move |_| Ok(ec2.clone()));
            if let Some(secrets_manager) = gateway.secrets_manager {
                let secrets_manager: Arc<
                    dyn alien_aws_clients::secrets_manager::SecretsManagerApi,
                > = Arc::new(secrets_manager);
                // The replica secret is a credential, so it is reached only through the provider
                // without the deployment's interceptors.
                let mut uninterceptable = MockPlatformServiceProvider::new();
                uninterceptable
                    .expect_get_aws_secrets_manager_client()
                    .returning(move |_| Ok(secrets_manager.clone()));
                let uninterceptable: Arc<dyn crate::PlatformServiceProvider> =
                    Arc::new(uninterceptable);
                provider
                    .expect_without_interceptors()
                    .returning(move || Some(uninterceptable.clone()));
            }
            let mut builder = SingleControllerExecutor::builder()
                .resource(internet_sandbox(id))
                .controller(AwsSandboxController {
                    state: AwsSandboxState::RefreshingEgressConfig,
                    ..controller
                })
                .platform(Platform::Aws)
                .service_provider(Arc::new(provider));
            if gateway.image.is_some() {
                builder = builder.initial_setup_authority(if gateway.direct_setup {
                    alien_core::InitialSetupAuthority::DirectSetup
                } else {
                    alien_core::InitialSetupAuthority::ImportedHandoff
                });
            }
            for other in others {
                builder = builder.with_stack_resource(
                    internet_sandbox(other),
                    alien_core::ResourceLifecycle::Live,
                );
            }
            let mut executor = builder
                .with_dependency(sandbox_network(), sandbox_network_controller())
                .with_dependency(
                    Vault::new(SECRETS_VAULT_ID.into()).build(),
                    LocalVaultController {
                        state: LocalVaultState::Ready,
                        vault_path: Some(directory.to_string_lossy().into_owned()),
                        ..Default::default()
                    },
                )
                .build()
                .await
                .unwrap();
            if let (Some(image), true) = (gateway.image, gateway.fresh) {
                // The stack mutations add the managed gateway; this release starts it on `image`
                // and it has never been created.
                let id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
                executor.remove_saved_resource(id);
                executor
                    .desired_resource_config_mut(id)
                    .and_then(|config| config.downcast_mut::<alien_core::Container>())
                    .expect("the stack mutations add the gateway")
                    .code = alien_core::ContainerCode::Image {
                    image: image.into(),
                };
            }
            if let (Some(image), false) = (gateway.image, gateway.fresh) {
                run_gateway_on(&mut executor, image);
            }
            executor.step().await.expect("the refresh runs");
            executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .clone()
        }

        const SOURCE_READING_GATEWAY: &str =
            "ghcr.io/alienplatform/alien-egress-gateway@sha256:0000000000000000000000000000000000000000000000000000000000000001";
        const EMBEDDING_GATEWAY: &str = "ghcr.io/alienplatform/alien-egress-gateway@sha256:b252a1b423b95385e5df0c57a37daae6937daa274e3c7783c71704584b18483d";

        async fn stored_gateway_config(directory: &std::path::Path) -> serde_json::Value {
            use alien_bindings::{providers::vault::LocalVault, Vault as _};
            let vault = LocalVault::new("secrets".into(), directory.into());
            serde_json::from_str(
                &vault
                    .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        fn describe_answers(
            answer: fn() -> alien_client_core::Result<
                alien_aws_clients::secrets_manager::DescribeSecretResponse,
            >,
            times: usize,
        ) -> alien_aws_clients::secrets_manager::MockSecretsManagerApi {
            let mut secrets_manager =
                alien_aws_clients::secrets_manager::MockSecretsManagerApi::new();
            secrets_manager
                .expect_describe_secret()
                .withf(|request| request.secret_id == "test-sandbox-egress-replica-secret")
                .times(times)
                .returning(move |_| answer());
            secrets_manager
        }

        fn secret_exists(
        ) -> alien_client_core::Result<alien_aws_clients::secrets_manager::DescribeSecretResponse>
        {
            Ok(serde_json::from_value(
                serde_json::json!({"Name": "test-sandbox-egress-replica-secret"}),
            )
            .unwrap())
        }

        fn secret_absent(
        ) -> alien_client_core::Result<alien_aws_clients::secrets_manager::DescribeSecretResponse>
        {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "secret".into(),
                    resource_name: "test-sandbox-egress-replica-secret".into(),
                },
            ))
        }

        fn existence_denied(
        ) -> alien_client_core::Result<alien_aws_clients::secrets_manager::DescribeSecretResponse>
        {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "secret".into(),
                    resource_name: "test-sandbox-egress-replica-secret".into(),
                },
            ))
        }

        /// Once every gateway replica reads its replica secret from Secrets Manager and setup
        /// has created it, the owner publishes a configuration that names the source and holds
        /// no secret, and checks the secret's existence only once.
        #[tokio::test]
        async fn a_gateway_that_reads_the_source_gets_a_configuration_without_the_secret() {
            let directory = tempfile::tempdir().unwrap();
            let embedded = refresh_pass(
                "agents",
                &[],
                running_internet_controller(IMAGE_ARN),
                directory.path(),
            )
            .await;
            assert!(stored_gateway_config(directory.path()).await["replicaSecret"].is_string());

            let moved = refresh_with(
                "agents",
                &[],
                embedded,
                directory.path(),
                GatewayFixture {
                    image: Some(SOURCE_READING_GATEWAY),
                    secrets_manager: Some(describe_answers(secret_exists, 1)),
                    direct_setup: false,
                    fresh: false,
                },
            )
            .await;
            assert!(moved.egress_replica_secret_ready);
            let stored = stored_gateway_config(directory.path()).await;
            assert!(stored.get("replicaSecret").is_none(), "{stored}");
            assert_eq!(
                stored["replicaSecretSource"],
                serde_json::json!({"awsSecretsManager": {"secretId": "test-sandbox-egress-replica-secret"}})
            );

            // Later refreshes neither look the secret up again nor rewrite the entry.
            refresh_with(
                "agents",
                &["other"],
                moved,
                directory.path(),
                GatewayFixture {
                    image: Some(SOURCE_READING_GATEWAY),
                    secrets_manager: Some(describe_answers(secret_exists, 0)),
                    direct_setup: false,
                    fresh: false,
                },
            )
            .await;
            assert_eq!(
                stored_gateway_config(directory.path()).await["audiences"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }

        /// A gateway on an image that predates sourced secrets, or a setup that has not created
        /// the secret (or predates the existence check), keeps the embedded secret.
        #[tokio::test]
        async fn the_embedded_secret_stays_until_the_gateway_and_setup_are_ready() {
            for (image, answer, times) in [
                (EMBEDDING_GATEWAY, secret_exists as fn() -> _, 0),
                (SOURCE_READING_GATEWAY, secret_absent as fn() -> _, 1),
                (SOURCE_READING_GATEWAY, existence_denied as fn() -> _, 1),
            ] {
                let directory = tempfile::tempdir().unwrap();
                let controller = refresh_with(
                    "agents",
                    &[],
                    running_internet_controller(IMAGE_ARN),
                    directory.path(),
                    GatewayFixture {
                        image: Some(image),
                        secrets_manager: Some(describe_answers(answer, times)),
                        direct_setup: false,
                        fresh: false,
                    },
                )
                .await;
                assert!(!controller.egress_replica_secret_ready, "{image}");
                let stored = stored_gateway_config(directory.path()).await;
                assert!(stored["replicaSecret"].is_string(), "{image}: {stored}");
                assert!(stored.get("replicaSecretSource").is_none());
            }
        }

        /// A direct setup has no setup stack to generate the secret, so the controller creates
        /// it once with a value it never reads back.
        #[tokio::test]
        async fn a_direct_setup_creates_the_replica_secret_once() {
            let directory = tempfile::tempdir().unwrap();
            let mut secrets_manager = describe_answers(secret_absent, 1);
            secrets_manager
                .expect_create_secret()
                .withf(|request| {
                    request.name == "test-sandbox-egress-replica-secret"
                        && request
                            .secret_string
                            .as_ref()
                            .is_some_and(|value| value.len() >= 32)
                })
                .times(1)
                .returning(|_| {
                    Ok(serde_json::from_value(
                        serde_json::json!({"Name": "test-sandbox-egress-replica-secret"}),
                    )
                    .unwrap())
                });
            secrets_manager.expect_get_secret_value().times(0);
            let controller = refresh_with(
                "agents",
                &[],
                running_internet_controller(IMAGE_ARN),
                directory.path(),
                GatewayFixture {
                    image: Some(SOURCE_READING_GATEWAY),
                    secrets_manager: Some(secrets_manager),
                    direct_setup: true,
                    fresh: false,
                },
            )
            .await;
            assert!(controller.egress_replica_secret_ready);
            assert!(stored_gateway_config(directory.path())
                .await
                .get("replicaSecret")
                .is_none());
        }

        /// A fresh install: the gateway is not created yet, so no replica holds an embedded
        /// secret. The owner publishes the source-only configuration right away, without reading
        /// the stored entry, which setup forbids the managing identity to read.
        #[tokio::test]
        async fn a_fresh_install_publishes_the_source_before_the_gateway_starts() {
            let directory = tempfile::tempdir().unwrap();
            let controller = refresh_with(
                "agents",
                &[],
                running_internet_controller(IMAGE_ARN),
                directory.path(),
                GatewayFixture {
                    image: Some(SOURCE_READING_GATEWAY),
                    secrets_manager: Some(describe_answers(secret_exists, 1)),
                    direct_setup: false,
                    fresh: true,
                },
            )
            .await;
            assert!(controller.egress_replica_secret_ready);
            let stored = stored_gateway_config(directory.path()).await;
            assert!(stored.get("replicaSecret").is_none(), "{stored}");
            assert_eq!(
                stored["replicaSecretSource"],
                serde_json::json!({"awsSecretsManager": {"secretId": "test-sandbox-egress-replica-secret"}})
            );
        }

        /// The owner skips republishing an unchanged configuration, so it must forget what it
        /// published once another sandbox owns the entry: that sandbox rewrites it, and when the
        /// first becomes the owner again the stored configuration is not the one it remembers.
        #[tokio::test]
        async fn an_owner_that_regains_the_configuration_publishes_it_again() {
            use alien_bindings::{providers::vault::LocalVault, Vault as _};

            let directory = tempfile::tempdir().unwrap();
            let vault = LocalVault::new("secrets".into(), directory.path().into());
            let stored_audiences = || async {
                let value: serde_json::Value = serde_json::from_str(
                    &vault
                        .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                        .await
                        .unwrap(),
                )
                .unwrap();
                value["audiences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|audience| {
                        audience
                            .as_str()
                            .unwrap()
                            .rsplit('/')
                            .next()
                            .unwrap()
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            };

            let owner = refresh_pass(
                "b",
                &["c"],
                running_internet_controller(IMAGE_ARN),
                directory.path(),
            )
            .await;
            assert!(owner.egress_config_digest.is_some());
            assert_eq!(stored_audiences().await, ["b", "c"]);

            // A lower id takes the entry over and publishes its own audiences.
            let displaced = refresh_pass("b", &["a"], owner, directory.path()).await;
            assert_eq!(displaced.egress_config_digest, None);
            let mut taken_over: serde_json::Value = serde_json::from_str(
                &vault
                    .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                    .await
                    .unwrap(),
            )
            .unwrap();
            taken_over["audiences"] = serde_json::json!(["test/a", "test/b"]);
            vault
                .set_secret(
                    crate::sandbox::egress::GATEWAY_CONFIG,
                    &taken_over.to_string(),
                )
                .await
                .unwrap();

            // The lower id goes and `c` is back: `b` owns the entry again and must rewrite it.
            refresh_pass("b", &["c"], displaced, directory.path()).await;
            assert_eq!(stored_audiences().await, ["b", "c"]);
        }

        /// Runs the runtime cleanup a CloudFormation deployment's delete runs: an empty target
        /// stack, the setup-owned network and vault left to setup teardown, and a Lambda Worker that
        /// links the key-owning internet sandbox and takes several steps to delete. Returns the
        /// final state and the MicroVM images deleted.
        async fn delete_internet_sandboxes(
            owner: AwsSandboxController,
            owner_status: ResourceStatus,
        ) -> (alien_core::StackState, Vec<String>) {
            use crate::core::state_utils::{StackResourceStateExt, StackStateExt};
            use crate::{
                AwsWorkerController, LocalVaultController, LocalVaultState, StackExecutor,
            };
            use alien_aws_clients::lambda::MockLambdaApi;
            use alien_aws_clients::AwsClientConfigExt;
            use alien_core::{
                ClientConfig, Resource, ResourceLifecycle, ResourceRef, StackResourceState,
                StackState, Vault, Worker, WorkerCode, SECRETS_VAULT_ID,
            };

            let deleted = Arc::new(Mutex::new(Vec::new()));
            let mut microvms = MockLambdaMicrovmsApi::new();
            microvms
                .expect_get_microvm_image()
                .returning(|identifier| Ok(served_image(identifier)));
            microvms
                .expect_list_microvm_image_versions()
                .returning(|_| Ok(vec![]));
            let recorded = deleted.clone();
            microvms
                .expect_delete_microvm_image()
                .returning(move |identifier| {
                    recorded.lock().unwrap().push(identifier.to_string());
                    Ok(())
                });
            let microvms = Arc::new(microvms);
            let mut lambda = MockLambdaApi::new();
            lambda.expect_delete_function().returning(|_, _| Ok(()));
            lambda
                .expect_get_function_configuration()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Worker".to_string(),
                            resource_name: "test-worker".to_string(),
                        },
                    ))
                });
            let lambda = Arc::new(lambda);
            let mut logs = alien_aws_clients::cloudwatch_logs::MockCloudWatchLogsApi::new();
            logs.expect_delete_log_group().returning(|_| Ok(()));
            let logs = Arc::new(logs);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_microvms_client()
                .returning(move |_| Ok(microvms.clone()));
            provider
                .expect_get_aws_lambda_client()
                .returning(move |_| Ok(lambda.clone()));
            provider
                .expect_get_aws_logs_client()
                .returning(move |_| Ok(logs.clone()));
            // No EC2 client: nothing being deleted may read the network.

            let mut state = StackState::new(Platform::Aws);
            state.resource_prefix = "test".to_string();
            let mut add = |resource: Resource,
                           lifecycle: ResourceLifecycle,
                           status: ResourceStatus,
                           dependencies: Vec<ResourceRef>,
                           controller: Box<dyn ResourceController>| {
                let id = resource.id().to_string();
                let mut entry = StackResourceState::new_pending(
                    resource.resource_type().to_string(),
                    resource,
                    Some(lifecycle),
                    dependencies,
                );
                entry.status = status;
                entry.set_internal_controller(Some(controller)).unwrap();
                state.resources.insert(id, entry);
            };
            let network_ref = ResourceRef::new(alien_core::Network::RESOURCE_TYPE, "network");
            let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, SECRETS_VAULT_ID);
            let owner_ref = ResourceRef::new(Sandbox::RESOURCE_TYPE, "agents");
            add(
                Resource::new(sandbox_network()),
                ResourceLifecycle::Frozen,
                ResourceStatus::Running,
                vec![],
                Box::new(sandbox_network_controller()),
            );
            add(
                Resource::new(Vault::new(SECRETS_VAULT_ID.into()).build()),
                ResourceLifecycle::Frozen,
                ResourceStatus::Running,
                vec![],
                Box::new(LocalVaultController {
                    state: LocalVaultState::Ready,
                    vault_path: Some("/nonexistent".into()),
                    ..Default::default()
                }),
            );
            add(
                Resource::new(internet_sandbox("agents")),
                ResourceLifecycle::Live,
                owner_status,
                vec![network_ref.clone(), vault_ref.clone()],
                Box::new(owner),
            );
            add(
                Resource::new(internet_sandbox("other")),
                ResourceLifecycle::Live,
                ResourceStatus::Running,
                vec![network_ref, vault_ref, owner_ref.clone()],
                Box::new(running_internet_controller(OTHER_IMAGE_ARN)),
            );
            let worker = Worker::new("api".into())
                .code(WorkerCode::Image {
                    image: "test-image-api".into(),
                })
                .permissions("execution".into())
                .link(&internet_sandbox("agents"))
                .build();
            add(
                Resource::new(worker),
                ResourceLifecycle::Live,
                ResourceStatus::Running,
                vec![owner_ref],
                Box::new(AwsWorkerController::mock_ready("test-api")),
            );

            state
                .prepare_for_runtime_cleanup_destroy(alien_core::SetupManagement::Manual)
                .expect("the delete request prepares the stack");
            let config = alien_core::DeploymentConfig::builder()
                .stack_settings(alien_core::StackSettings::default())
                .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(alien_core::ExternalBindings::default())
                .allow_frozen_changes(false)
                .build();
            let executor = StackExecutor::for_runtime_cleanup_deletion_with_service_provider(
                ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
                &config,
                Arc::new(provider),
            )
            .unwrap();
            for _ in 0..40 {
                state = executor
                    .step(state)
                    .await
                    .expect("runtime cleanup step")
                    .next_state;
                let failed = state
                    .resources
                    .iter()
                    .find(|(_, resource)| resource.error.is_some());
                if let Some((id, resource)) = failed {
                    panic!(
                        "'{id}' failed with {:?} in {:?}: {}",
                        resource.status,
                        resource
                            .last_failed_state
                            .as_ref()
                            .and_then(|saved| saved.get("state")),
                        resource.error.as_ref().unwrap()
                    );
                }
                if ["agents", "other", "api"].iter().all(|id| {
                    state
                        .resources
                        .get(*id)
                        .is_none_or(|resource| resource.status == ResourceStatus::Deleted)
                }) {
                    break;
                }
            }
            let deleted = deleted.lock().unwrap().clone();
            (state, deleted)
        }

        fn assert_runtime_cleanup_finished(state: &alien_core::StackState, deleted: &[String]) {
            for id in ["agents", "other", "api"] {
                assert!(
                    state
                        .resources
                        .get(id)
                        .is_none_or(|resource| resource.status == ResourceStatus::Deleted),
                    "'{id}' was not deleted: {:?}",
                    state.resources.get(id).map(|resource| resource.status)
                );
            }
            let mut deleted = deleted.to_vec();
            deleted.sort();
            assert_eq!(deleted, [IMAGE_ARN, OTHER_IMAGE_ARN]);
            // Setup teardown owns these; runtime cleanup leaves them running.
            for id in ["network", alien_core::SECRETS_VAULT_ID] {
                assert_eq!(state.resources[id].status, ResourceStatus::Running);
            }
        }

        /// The first delete of a deployment with a running key-owning internet sandbox and a Worker
        /// that links it. The Worker is deleted first, and the sandboxes keep running their Ready
        /// handler meanwhile against an empty target stack that declares no network. Before the fix
        /// they moved on to refresh the gateway configuration, failed with "this stack declares no
        /// network", and the delete stopped in DeleteFailed until it was retried.
        #[tokio::test]
        async fn the_first_delete_of_a_key_owning_internet_sandbox_with_a_worker_completes() {
            let (state, deleted) = delete_internet_sandboxes(
                running_internet_controller(IMAGE_ARN),
                ResourceStatus::Running,
            )
            .await;
            assert_runtime_cleanup_finished(&state, &deleted);
        }

        /// A key owner saved mid-refresh when the delete arrives does not publish either.
        #[tokio::test]
        async fn a_key_owner_saved_mid_refresh_deletes_without_publishing() {
            let (state, deleted) = delete_internet_sandboxes(
                AwsSandboxController {
                    state: AwsSandboxState::RefreshingEgressConfig,
                    ..running_internet_controller(IMAGE_ARN)
                },
                ResourceStatus::Running,
            )
            .await;
            assert_runtime_cleanup_finished(&state, &deleted);
        }

        /// The retry of a delete that failed this way: preparing the delete moves the refresh
        /// failure straight to the delete flow, which is why retrying worked before the fix.
        #[tokio::test]
        async fn retrying_a_delete_whose_refresh_failed_completes() {
            let (state, deleted) = delete_internet_sandboxes(
                AwsSandboxController {
                    state: AwsSandboxState::RefreshFailed,
                    ..running_internet_controller(IMAGE_ARN)
                },
                ResourceStatus::RefreshFailed,
            )
            .await;
            assert_runtime_cleanup_finished(&state, &deleted);
        }

        /// The upgrade release: setup has removed the managing identity's read of the gateway
        /// configuration, the gateway still runs an image that embeds its replica secret, and the
        /// same release changes the configuration. Failing that publish kept this sandbox from
        /// Running, and the gateway, which depends on it, never rolled. The change must wait
        /// instead, then reach the gateway right after the roll.
        mod deferred_until_the_roll {
            use super::*;
            use crate::{AwsVaultController, AwsVaultState};
            use alien_core::{
                AwsClientConfig, AwsCredentials, AwsServiceOverrides, ClientConfig, Vault,
                SECRETS_VAULT_ID,
            };
            use httpmock::{Method::POST, MockServer};
            use std::collections::HashMap;

            /// Management may write the vault but not read it, as setup now grants. Returns the
            /// read and write mocks.
            async fn denied_reads(server: &MockServer) -> (httpmock::Mock<'_>, httpmock::Mock<'_>) {
                let read = server
                    .mock_async(|when, then| {
                        when.method(POST).header("x-amz-target", "AmazonSSM.GetParameter");
                        then.status(400).json_body(serde_json::json!({
                            "__type": "AccessDeniedException",
                            "message": "User: arn:aws:sts::123456789012:assumed-role/test-management/session is not authorized to perform: ssm:GetParameter with an explicit deny"
                        }));
                    })
                    .await;
                let write = server
                    .mock_async(|when, then| {
                        when.method(POST)
                            .header("x-amz-target", "AmazonSSM.PutParameter")
                            .body_contains("test-secrets-sandbox-egress-gateway-config");
                        then.status(200)
                            .json_body(serde_json::json!({"Version": 2, "Tier": "Standard"}));
                    })
                    .await;
                (read, write)
            }

            /// One refresh pass of the key owner `agents` beside `others`, with the gateway running
            /// `gateway_image`. Returns the controller and its status afterwards.
            async fn pass(
                ssm: &MockServer,
                controller: AwsSandboxController,
                others: &[&str],
                gateway_image: &str,
                secrets_manager: alien_aws_clients::secrets_manager::MockSecretsManagerApi,
            ) -> (AwsSandboxController, ResourceStatus) {
                let mut microvms = MockLambdaMicrovmsApi::new();
                microvms
                    .expect_get_microvm_image()
                    .returning(|_| Ok(served_image(IMAGE_ARN)));
                microvms
                    .expect_list_microvm_image_versions()
                    .returning(|_| Ok(vec![]));
                let microvms = Arc::new(microvms);
                let ec2 = Arc::new(valid_sandbox_network_ec2());
                let secrets_manager: Arc<
                    dyn alien_aws_clients::secrets_manager::SecretsManagerApi,
                > = Arc::new(secrets_manager);
                let mut provider = MockPlatformServiceProvider::new();
                provider
                    .expect_get_aws_microvms_client()
                    .returning(move |_| Ok(microvms.clone()));
                provider
                    .expect_get_aws_ec2_client()
                    .returning(move |_| Ok(ec2.clone()));
                provider
                    .expect_get_aws_secrets_manager_client()
                    .returning(move |_| Ok(secrets_manager.clone()));
                provider.expect_without_interceptors().returning(|| None);
                let mut builder = SingleControllerExecutor::builder()
                    .resource(internet_sandbox("agents"))
                    .controller(AwsSandboxController {
                        state: AwsSandboxState::RefreshingEgressConfig,
                        ..controller
                    })
                    .platform(Platform::Aws)
                    .client_config(ClientConfig::Aws(Box::new(AwsClientConfig {
                        account_id: "123456789012".to_string(),
                        region: "us-east-1".to_string(),
                        credentials: AwsCredentials::AccessKeys {
                            access_key_id: "test".to_string(),
                            secret_access_key: "test".to_string(),
                            session_token: None,
                        },
                        service_overrides: Some(AwsServiceOverrides {
                            endpoints: HashMap::from([("ssm".to_string(), ssm.base_url())]),
                        }),
                    })))
                    .service_provider(Arc::new(provider))
                    .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff);
                for other in others {
                    builder = builder.with_stack_resource(
                        internet_sandbox(other),
                        alien_core::ResourceLifecycle::Live,
                    );
                }
                let mut executor = builder
                    .with_dependency(sandbox_network(), sandbox_network_controller())
                    .with_dependency(
                        Vault::new(SECRETS_VAULT_ID.into()).build(),
                        AwsVaultController {
                            state: AwsVaultState::Ready,
                            account_id: Some("123456789012".to_string()),
                            region: Some("us-east-1".to_string()),
                            vault_prefix: Some("test-secrets".to_string()),
                            ..Default::default()
                        },
                    )
                    .build()
                    .await
                    .unwrap();
                run_gateway_on(&mut executor, gateway_image);
                executor.step().await.expect("the refresh pass never fails");
                (
                    executor
                        .internal_state::<AwsSandboxController>()
                        .unwrap()
                        .clone(),
                    executor.status(),
                )
            }

            /// The owner as it was before the upgrade release: Running, with the configuration it
            /// published for the agents alone.
            fn owner_before_the_release() -> AwsSandboxController {
                AwsSandboxController {
                    egress_config_digest: Some("digest-published-before-the-release".to_string()),
                    ..running_internet_controller(IMAGE_ARN)
                }
            }

            /// The release that brings the setup update also adds an internet sandbox (a new
            /// audience). On the legacy gateway the change waits: the pass succeeds, the sandbox
            /// stays Running, nothing is read or written, and its state is unchanged pass after
            /// pass, so a deferral costs no state writes. After the gateway rolls, the next pass
            /// publishes the new audience source-only.
            #[tokio::test]
            async fn a_change_in_the_upgrade_release_waits_for_the_roll_then_publishes() {
                let ssm = MockServer::start_async().await;
                let (read, write) = denied_reads(&ssm).await;
                let mut owner = owner_before_the_release();
                for _ in 0..3 {
                    let (after, status) = pass(
                        &ssm,
                        owner.clone(),
                        &["workers"],
                        EMBEDDING_GATEWAY,
                        describe_answers(secret_exists, 0),
                    )
                    .await;
                    assert_eq!(status, ResourceStatus::Running, "deferral is not a failure");
                    let unchanged = |controller: &AwsSandboxController| {
                        let mut value = serialize_controller(controller).unwrap();
                        value.as_object_mut().unwrap().remove("state");
                        value
                    };
                    assert_eq!(
                        unchanged(&after),
                        unchanged(&owner),
                        "a deferred change leaves the controller state unchanged"
                    );
                    owner = after;
                }
                assert_eq!(
                    write.hits_async().await,
                    0,
                    "nothing is written while deferred"
                );

                // The gateway rolled to an image that reads its replica secret from its source.
                let reads = read.hits_async().await;
                let (after, status) = pass(
                    &ssm,
                    owner,
                    &["workers"],
                    SOURCE_READING_GATEWAY,
                    describe_answers(secret_exists, 1),
                )
                .await;
                assert_eq!(status, ResourceStatus::Running);
                assert!(after.egress_replica_secret_ready);
                assert_eq!(
                    read.hits_async().await,
                    reads,
                    "the source-only publish reads nothing"
                );
                assert_eq!(write.hits_async().await, 1);
                assert_ne!(
                    after.egress_config_digest.as_deref(),
                    Some("digest-published-before-the-release")
                );
            }

            /// The key-rotation trigger: a different signing key changes the configuration on the
            /// legacy gateway, and the change waits for the roll the same way.
            #[tokio::test]
            async fn a_new_signing_key_on_the_legacy_gateway_waits_for_the_roll() {
                let ssm = MockServer::start_async().await;
                let (_read, write) = denied_reads(&ssm).await;
                let owner = AwsSandboxController {
                    egress_public_key: Some(crate::sandbox::egress::GatewayPublicKey {
                        kid: "kid-rotated".to_string(),
                        public_key: "public-key-rotated".to_string(),
                    }),
                    ..owner_before_the_release()
                };
                let (after, status) = pass(
                    &ssm,
                    owner.clone(),
                    &[],
                    EMBEDDING_GATEWAY,
                    describe_answers(secret_exists, 0),
                )
                .await;
                assert_eq!(status, ResourceStatus::Running);
                assert_eq!(after.egress_config_digest, owner.egress_config_digest);
                assert_eq!(write.hits_async().await, 0);
            }
        }
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
    /// Preview authorization is mutable release configuration, not an image build input.
    /// Exercise both newly updated state and old persisted Ready state with stale ports.
    #[tokio::test]
    async fn preview_ports_follow_release_updates_and_repair_old_checkpoints() {
        for lifecycle in [
            alien_core::ResourceLifecycle::Live,
            alien_core::ResourceLifecycle::Frozen,
        ] {
            for update in [true, false] {
                for desired_ports in [vec![8080, 8081, 8090], vec![8090], vec![]] {
                    let mut desired = sandbox();
                    desired.preview_ports = desired_ports.clone();
                    let mut old = ready_controller();
                    // No setup-owned image build should be attempted for a ports-only change.
                    old.build_role_arn = None;
                    let old: AwsSandboxController = serde_json::from_value(
                        serde_json::to_value(old).expect("serialize old checkpoint"),
                    )
                    .expect("reload old checkpoint");
                    let mut client = MockLambdaMicrovmsApi::new();
                    if !update {
                        client.expect_get_microvm_image().times(1).returning(|_| {
                            Ok(MicrovmImage {
                                description: None,
                                image_identifier: None,
                                image_arn: Some(IMAGE_ARN.to_string()),
                                image_version: Some("1.0".to_string()),
                                state: Some("AVAILABLE".to_string()),
                            })
                        });
                    }
                    let mut executor = SingleControllerExecutor::builder()
                        .resource(desired.clone())
                        .controller(old)
                        .platform(Platform::Aws)
                        .resource_lifecycle(lifecycle)
                        .service_provider(provider(client))
                        .build()
                        .await
                        .expect("build executor");
                    if update {
                        executor.update(desired).expect("start release update");
                    }
                    executor
                        .step()
                        .await
                        .expect("reconcile preview authorization");
                    let controller = executor
                        .internal_state::<AwsSandboxController>()
                        .expect("controller");
                    assert_eq!(
                        controller.preview_ports, desired_ports,
                        "lifecycle={lifecycle:?}, update={update}"
                    );
                    let reloaded: AwsSandboxController = serde_json::from_value(
                        serde_json::to_value(controller).expect("save reconciled controller"),
                    )
                    .expect("reload reconciled controller");
                    let binding = reloaded
                        .get_binding_params()
                        .expect("binding")
                        .expect("serving image binding must remain available");
                    let alien_core::bindings::SandboxBinding::Aws(binding) =
                        serde_json::from_value(binding).expect("consume published binding")
                    else {
                        panic!("expected AWS sandbox binding")
                    };
                    assert_eq!(binding.preview_ports, desired_ports);
                    assert_ne!(reloaded.state, AwsSandboxState::UpdatingImage);
                    assert_eq!(reloaded.active_version.as_deref(), Some("1.0"));
                    assert!(
                        reloaded.pending_version.is_none(),
                        "ports must not rebuild the image"
                    );
                }
            }
        }
    }

    fn listed(versions: &[(&str, &str)]) -> Vec<MicrovmImage> {
        versions
            .iter()
            .map(|(version, state)| MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some(version.to_string()),
                state: Some(state.to_string()),
            })
            .collect()
    }

    fn retired(version: &str, inactive_ago: Option<chrono::Duration>) -> RetiredVersion {
        RetiredVersion {
            version: version.to_string(),
            retired_at: chrono::Utc::now() - chrono::Duration::days(30),
            inactive_at: inactive_ago.map(|ago| chrono::Utc::now() - ago),
        }
    }

    /// A controller serving `3.0` over the given retired versions, ticked once in `Ready`.
    async fn tick_ready_over(
        client: MockLambdaMicrovmsApi,
        retired_versions: Vec<RetiredVersion>,
    ) -> AwsSandboxController {
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.retired_versions = retired_versions;
        let mut executor = executor(controller, client).await;
        executor.step().await.expect("ready tick");
        executor
            .internal_state::<AwsSandboxController>()
            .unwrap()
            .clone()
    }

    fn ready_client(versions: &'static [(&'static str, &'static str)]) -> MockLambdaMicrovmsApi {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("3.0".to_string()),
                state: Some("UPDATED".to_string()),
            })
        });
        client
            .expect_list_microvm_image_versions()
            .returning(move |_| Ok(listed(versions)));
        client
    }

    /// A replaced version stops accepting new sessions, and is not deleted: a VM started on it
    /// may still be running or suspended.
    #[tokio::test]
    async fn a_replaced_version_is_made_inactive_and_kept() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client
            .expect_update_microvm_image_version()
            .withf(|_, version, status| {
                version == "1.0"
                    && *status
                        == alien_aws_clients::lambda_microvms::MicrovmImageVersionStatus::Inactive
            })
            .times(1)
            .returning(|_, _, _| Ok(()));
        let controller =
            tick_ready_over(client, vec![retired("1.0", None), retired("2.0", None)]).await;
        // 2.0 was replaced most recently and stays active.
        assert!(controller.retired_versions[0].inactive_at.is_some());
        assert!(controller.retired_versions[1].inactive_at.is_none());
        assert_eq!(controller.deleting_version, None);
    }

    /// State written before versions were retired knows none of them. They start the clock when
    /// first seen and are never deleted on upgrade, however old the image is.
    #[tokio::test]
    async fn versions_unknown_to_state_start_their_clock_when_first_seen() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client
            .expect_update_microvm_image_version()
            .times(1)
            .returning(|_, _, _| Ok(()));
        let controller = tick_ready_over(client, vec![]).await;
        let mut seen: Vec<_> = controller
            .retired_versions
            .iter()
            .map(|v| (v.version.as_str(), v.inactive_at.is_some()))
            .collect();
        seen.sort();
        assert_eq!(seen, [("1.0", true), ("2.0", false)]);
        assert_eq!(controller.deleting_version, None);
    }

    /// Deleting waits for the longest session AWS allows plus the margin, and takes the oldest
    /// first through the confirmed-absence path.
    #[tokio::test]
    async fn the_oldest_version_past_the_session_lifespan_is_deleted_first() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
            ("4.0", "SUCCESSFUL"),
        ]);
        client
            .expect_delete_microvm_image_version()
            .withf(|_, version| version == "1.0")
            .times(1)
            .returning(|_, _| Ok(()));
        let hours = chrono::Duration::hours;
        let controller = tick_ready_over(
            client,
            vec![
                // Just inside the fence: 8 h session lifespan + 30 min margin.
                retired("4.0", Some(hours(8) + chrono::Duration::minutes(29))),
                retired("2.0", Some(hours(9))),
                retired("1.0", Some(hours(20))),
            ],
        )
        .await;
        assert_eq!(controller.deleting_version.as_deref(), Some("1.0"));
    }

    #[tokio::test]
    async fn a_version_inside_the_fence_is_not_deleted() {
        // Any delete call panics the mock.
        let client = ready_client(&[("2.0", "SUCCESSFUL"), ("3.0", "SUCCESSFUL")]);
        let controller = tick_ready_over(
            client,
            vec![retired(
                "2.0",
                Some(chrono::Duration::hours(8) + chrono::Duration::minutes(29)),
            )],
        )
        .await;
        assert_eq!(controller.deleting_version, None);
        assert_eq!(controller.retired_versions.len(), 1);
    }

    /// A version AWS reports gone is dropped from state, not retried.
    #[tokio::test]
    async fn a_retired_version_aws_no_longer_lists_is_forgotten() {
        let client = ready_client(&[("2.0", "SUCCESSFUL"), ("3.0", "SUCCESSFUL")]);
        let controller = tick_ready_over(
            client,
            vec![
                retired("2.0", Some(chrono::Duration::hours(1))),
                retired("1.0", Some(chrono::Duration::hours(1))),
            ],
        )
        .await;
        assert_eq!(
            controller
                .retired_versions
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            ["2.0"]
        );
    }

    /// A dependent that has not rolled to its current config can still hold a binding that
    /// names the replaced version, so the version keeps accepting sessions until it rolls.
    #[tokio::test]
    async fn a_version_stays_active_while_a_dependent_has_not_rolled() {
        // Any version status update panics the mock.
        let client = ready_client(&[("2.0", "SUCCESSFUL"), ("3.0", "SUCCESSFUL")]);
        let consumer = |permissions: &str| {
            alien_core::Build::new("consumer".to_string())
                .permissions(permissions.to_string())
                .link(&sandbox())
                .build()
        };
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.retired_versions = vec![retired("2.0", None)];
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .with_stack_resource(consumer("new"), alien_core::ResourceLifecycle::Live)
            .build()
            .await
            .expect("executor should build");
        let saved = alien_core::StackResourceState::builder()
            .resource_type(alien_core::Build::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(consumer("old")))
            .lifecycle(alien_core::ResourceLifecycle::Live)
            .build();
        executor.insert_saved_resource("consumer", saved);
        executor.step().await.expect("ready tick");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(controller.retired_versions[0].inactive_at, None);
    }

    /// An install whose role predates the grant keeps its versions and keeps running: the
    /// refusal is not a step error, and later passes do not ask again right away.
    #[tokio::test]
    async fn a_missing_retire_grant_is_a_quiet_no_op() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client
            .expect_update_microvm_image_version()
            .times(1)
            .returning(|_, _, _| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "microvm-image-version".to_string(),
                    resource_name: "1.0".to_string(),
                }))
            });
        client.expect_delete_microvm_image_version().never();
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.retired_versions = vec![retired("1.0", None), retired("2.0", None)];
        let mut executor = executor(controller, client).await;
        for _ in 0..3 {
            executor
                .step()
                .await
                .expect("a refused retire is not an error");
        }
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(controller
            .retired_versions
            .iter()
            .all(|v| v.inactive_at.is_none()));
        assert!(controller.retire_denied_at.is_some());
        let message = controller.version_cap_message(IMAGE_ARN, "50");
        assert!(
            message.contains("lambda:UpdateMicrovmImageVersion"),
            "{message}"
        );
    }

    /// Cleanup never blocks an update: any other failure of the retire call is skipped for the
    /// pass, not raised.
    #[tokio::test]
    async fn a_failed_retire_call_does_not_fail_the_step() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client
            .expect_update_microvm_image_version()
            .returning(|_, _, _| {
                Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: "boom".to_string(),
                }))
            });
        client.expect_delete_microvm_image_version().never();
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.retired_versions = vec![retired("1.0", None), retired("2.0", None)];
        let mut executor = executor(controller, client).await;
        executor
            .step()
            .await
            .expect("a failed retire is not an error");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(controller
            .retired_versions
            .iter()
            .all(|v| v.inactive_at.is_none()));
    }

    /// An adopted image has no recorded serving version before its first roll, so nothing says
    /// which listed version is in use: the cleanup that runs before that roll retires none.
    #[tokio::test]
    async fn an_adopted_image_retires_nothing_before_its_first_roll() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: Some("test-agents".to_string()),
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        client
            .expect_list_microvm_image_versions()
            .returning(|_| Ok(listed(&[("1.0", "SUCCESSFUL"), ("0.9", "SUCCESSFUL")])));
        client.expect_update_microvm_image_version().never();
        client.expect_delete_microvm_image_version().never();
        client.expect_update_microvm_image().returning(|_, _| {
            Ok(UpdateMicrovmImageResponse {
                image_arn: Some(IMAGE_ARN.to_string()),
                name: Some("test-agents".to_string()),
                state: Some("UPDATING".to_string()),
                image_version: Some("2.0".to_string()),
            })
        });
        let mut executor = executor(runtime_seeded_controller(), client).await;
        executor.step().await.expect("adopt and roll");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(controller
            .retired_versions
            .iter()
            .all(|v| v.inactive_at.is_none()));
    }

    /// A dependent whose config did not change can still run the previous binding until its
    /// next comparison, so a version replaced minutes ago is not retired yet, whatever its
    /// dependents' saved config says.
    #[tokio::test]
    async fn a_version_replaced_minutes_ago_is_not_retired_yet() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client.expect_update_microvm_image_version().never();
        client.expect_delete_microvm_image_version().never();
        let mut old = retired("1.0", None);
        old.retired_at = chrono::Utc::now() - chrono::Duration::minutes(10);
        let controller = tick_ready_over(client, vec![old, retired("2.0", None)]).await;
        assert!(controller
            .retired_versions
            .iter()
            .all(|v| v.inactive_at.is_none()));
    }

    /// A sandbox-only release leaves dependents' config unchanged, and they pick up the new
    /// binding only after this pass. The previous version keeps accepting sessions meanwhile;
    /// only older ones retire.
    #[tokio::test]
    async fn the_previous_version_stays_active_while_older_ones_retire() {
        let mut client = ready_client(&[
            ("1.0", "SUCCESSFUL"),
            ("2.0", "SUCCESSFUL"),
            ("3.0", "SUCCESSFUL"),
        ]);
        client
            .expect_update_microvm_image_version()
            .withf(|_, version, _| version == "1.0")
            .times(1)
            .returning(|_, _, _| Ok(()));
        let consumer = || {
            alien_core::Build::new("consumer".to_string())
                .permissions("same".to_string())
                .link(&sandbox())
                .build()
        };
        let mut controller = ready_controller();
        controller.active_version = Some("3.0".to_string());
        controller.retired_versions = vec![retired("1.0", None), retired("2.0", None)];
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .with_stack_resource(consumer(), alien_core::ResourceLifecycle::Live)
            .build()
            .await
            .expect("executor should build");
        let mut saved = alien_core::StackResourceState::builder()
            .resource_type(alien_core::Build::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(consumer()))
            .lifecycle(alien_core::ResourceLifecycle::Live)
            .build();
        // A controller that does not observe bindings: the older config check applies.
        saved
            .set_internal_controller(Some(Box::new(crate::TestWorkerController::default())))
            .unwrap();
        executor.insert_saved_resource("consumer", saved);
        executor.step().await.expect("ready tick");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(controller.retired_versions[0].inactive_at.is_some());
        assert_eq!(controller.retired_versions[1].inactive_at, None);
    }

    /// Real AWS: the reclaim deletes every retired version of an image whose fence has passed.
    ///
    /// Needs the probe image from `alien-aws-clients`' `fill_the_image_to_the_version_cap`
    /// (`PROBE_IMAGE_ARN`, credentials in `AWS_TARGET_*`). The versions are really `INACTIVE`
    /// there; only `inactive_at` is set back nine hours here, because the real fence outlives
    /// a short-lived credential.
    #[tokio::test]
    #[ignore]
    async fn live_reclaim_deletes_retired_versions_past_the_fence() {
        use alien_aws_clients::lambda_microvms::{LambdaMicrovmsApi, LambdaMicrovmsClient};
        let credentials = alien_aws_clients::AwsCredentialProvider::from_config_sync(
            alien_aws_clients::AwsClientConfig {
                account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap(),
                region: std::env::var("AWS_TARGET_REGION").unwrap(),
                credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                    access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID").unwrap(),
                    secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY").unwrap(),
                    session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
                },
                service_overrides: None,
            },
        );
        let image = std::env::var("PROBE_IMAGE_ARN").expect("PROBE_IMAGE_ARN");
        let real = Arc::new(LambdaMicrovmsClient::new(
            reqwest::Client::new(),
            credentials,
        ));
        let listed = real.list_microvm_image_versions(&image).await.unwrap();
        let mut versions: Vec<String> = listed
            .iter()
            .filter_map(|v| v.image_version.clone())
            .collect();
        versions.sort_by_key(|v| v.split('.').next().unwrap().parse::<u64>().unwrap());
        let active = versions.pop().expect("a version");
        println!(
            "image holds {} versions; serving {active}",
            versions.len() + 1
        );

        let mut controller = ready_controller();
        controller.image_identifier = Some(image.clone());
        controller.image_arn = Some(image.clone());
        controller.active_version = Some(active.clone());
        controller.retired_versions = versions
            .iter()
            .map(|version| retired(version, Some(chrono::Duration::hours(9))))
            .collect();
        let mut provider = MockPlatformServiceProvider::new();
        let client: Arc<dyn LambdaMicrovmsApi> = real.clone();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(client.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");
        for step in 0..1500 {
            executor.step().await.expect("ready tick");
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            if step % 10 == 0 {
                println!(
                    "step {step}: {} retired left, deleting {:?}",
                    controller.retired_versions.len(),
                    controller.deleting_version
                );
            }
            if controller.retired_versions.is_empty() && controller.deleting_version.is_none() {
                break;
            }
            // `RECLAIM_STOP_AFTER=1` stops once that many versions are gone, to show one
            // freed slot is enough for the next roll.
            let stop_after = std::env::var("RECLAIM_STOP_AFTER")
                .ok()
                .and_then(|value| value.parse::<usize>().ok());
            if stop_after.is_some_and(|n| {
                versions.len() - controller.retired_versions.len() >= n
                    && controller.deleting_version.is_none()
            }) {
                break;
            }
        }
        if std::env::var("RECLAIM_STOP_AFTER").is_ok() {
            let left = real.list_microvm_image_versions(&image).await.unwrap();
            println!("image now holds {} versions", left.len());
            return;
        }
        let left = real.list_microvm_image_versions(&image).await.unwrap();
        println!("image now holds {} versions", left.len());
        assert_eq!(left.len(), 1, "only the serving version remains");
    }

    /// Real AWS: with a role that lacks `lambda:UpdateMicrovmImageVersion`, steady-state
    /// passes over an image with replaced versions end in Ready, no error, and no version
    /// changes. Needs `PROBE_IMAGE_ARN` and `AWS_TARGET_*` credentials of such a role.
    #[tokio::test]
    #[ignore]
    async fn live_role_without_the_retire_grant_is_a_quiet_no_op() {
        use alien_aws_clients::lambda_microvms::{LambdaMicrovmsApi, LambdaMicrovmsClient};
        let credentials = alien_aws_clients::AwsCredentialProvider::from_config_sync(
            alien_aws_clients::AwsClientConfig {
                account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap(),
                region: std::env::var("AWS_TARGET_REGION").unwrap(),
                credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                    access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID").unwrap(),
                    secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY").unwrap(),
                    session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
                },
                service_overrides: None,
            },
        );
        let image = std::env::var("PROBE_IMAGE_ARN").expect("PROBE_IMAGE_ARN");
        let real = Arc::new(LambdaMicrovmsClient::new(
            reqwest::Client::new(),
            credentials,
        ));
        let before = real.list_microvm_image_versions(&image).await.unwrap();
        let mut versions: Vec<String> = before
            .iter()
            .filter_map(|v| v.image_version.clone())
            .collect();
        versions.sort_by_key(|v| v.split('.').next().unwrap().parse::<u64>().unwrap());
        let active = versions.pop().expect("a version");
        assert!(
            versions.len() >= 2,
            "the probe image needs replaced versions"
        );

        let mut controller = ready_controller();
        controller.image_identifier = Some(image.clone());
        controller.image_arn = Some(image.clone());
        controller.active_version = Some(active);
        controller.retired_versions = versions.iter().map(|v| retired(v, None)).collect();
        let mut provider = MockPlatformServiceProvider::new();
        let client: Arc<dyn LambdaMicrovmsApi> = real.clone();
        provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(client.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");
        for step in 0..4 {
            executor
                .step()
                .await
                .expect("a refused retire is not an error");
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            println!(
                "step {step}: status {:?}, denied_at {:?}, inactive {}",
                controller.state,
                controller.retire_denied_at,
                controller
                    .retired_versions
                    .iter()
                    .filter(|v| v.inactive_at.is_some())
                    .count()
            );
        }
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(controller.retire_denied_at.is_some());
        assert!(controller
            .retired_versions
            .iter()
            .all(|v| v.inactive_at.is_none()));
        let after = real.list_microvm_image_versions(&image).await.unwrap();
        assert_eq!(
            after.len(),
            before.len(),
            "no version was changed or deleted"
        );
    }

    /// The version cap is not retried; the error says how full the image is and when room opens.
    #[tokio::test]
    async fn a_roll_refused_for_the_version_cap_says_when_room_opens() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm_image().returning(|_| {
            Ok(MicrovmImage {
                description: None,
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("UPDATED".to_string()),
            })
        });
        client
            .expect_list_microvm_image_versions()
            .returning(|_| Ok(listed(&[("1.0", "SUCCESSFUL"), ("2.0", "SUCCESSFUL")])));
        client.expect_update_microvm_image().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::VersionLimitExceeded {
                    message: "maximum number of versions allowed".to_string(),
                },
            ))
        });
        let mut controller = ready_controller();
        controller.retired_versions = vec![retired("2.0", Some(chrono::Duration::hours(1)))];
        controller.state = AwsSandboxState::UpdatingImage;
        controller.bundle_uri = Some(BUNDLE_URI.to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with_bundle(
                "s3://alien-bundles-test/sandbox/next.zip",
            ))
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(client))
            .build()
            .await
            .expect("executor should build");
        let error = executor.step().await.expect_err("the roll is refused");
        let text = format!("{error:?}");
        assert!(text.contains("of 50 versions"), "{text}");
        assert!(text.contains("becomes deletable at"), "{text}");
    }

    /// A fake MicroVM image at the AWS version cap, shared by the blocked-rollout tests.
    ///
    /// The shape seen in the field: 50 versions held, 49 of them replaced and none marked
    /// inactive, the serving version the newest.
    mod blocked_rollout {
        use super::*;
        use alien_aws_clients::lambda_microvms::MicrovmImageVersionStatus;
        use alien_core::{ResourceLifecycle, Worker, WorkerCode};

        pub(super) const CAP: usize = 50;

        #[derive(Default)]
        pub(super) struct FakeImage {
            /// `(version, inactive)` in creation order.
            pub(super) versions: Mutex<Vec<(String, bool)>>,
            /// Versions set inactive, in call order.
            pub(super) retire_calls: Mutex<Vec<String>>,
            /// Versions deleted, in call order.
            pub(super) delete_calls: Mutex<Vec<String>>,
            /// Build requests AWS accepted or refused.
            pub(super) build_calls: Mutex<u32>,
        }

        impl FakeImage {
            pub(super) fn at_cap() -> Arc<Self> {
                let fake = Self::default();
                *fake.versions.lock().unwrap() =
                    (1..=CAP).map(|n| (format!("{n}.0"), false)).collect();
                Arc::new(fake)
            }

            pub(super) fn held(&self) -> Vec<String> {
                self.versions
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(version, _)| version.clone())
                    .collect()
            }

            pub(super) fn client(self: &Arc<Self>) -> MockLambdaMicrovmsApi {
                let mut client = MockLambdaMicrovmsApi::new();
                let fake = self.clone();
                client
                    .expect_list_microvm_image_versions()
                    .returning(move |_| {
                        Ok(fake
                            .versions
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|(version, _)| MicrovmImage {
                                description: None,
                                image_identifier: None,
                                image_arn: Some(IMAGE_ARN.to_string()),
                                image_version: Some(version.clone()),
                                state: Some("SUCCESSFUL".to_string()),
                            })
                            .collect())
                    });
                let fake = self.clone();
                client.expect_update_microvm_image().returning(move |_, _| {
                    *fake.build_calls.lock().unwrap() += 1;
                    let mut versions = fake.versions.lock().unwrap();
                    if versions.len() >= CAP {
                        return Err(AlienError::new(
                            CloudClientErrorData::VersionLimitExceeded {
                                message: "maximum number of versions allowed".to_string(),
                            },
                        ));
                    }
                    versions.push(("51.0".to_string(), false));
                    Ok(UpdateMicrovmImageResponse {
                        image_version: Some("51.0".to_string()),
                        image_arn: Some(IMAGE_ARN.to_string()),
                        name: None,
                        state: None,
                    })
                });
                let fake = self.clone();
                client.expect_update_microvm_image_version().returning(
                    move |_, version, status| {
                        assert_eq!(status, MicrovmImageVersionStatus::Inactive);
                        fake.retire_calls.lock().unwrap().push(version.to_string());
                        for entry in fake.versions.lock().unwrap().iter_mut() {
                            if entry.0 == version {
                                entry.1 = true;
                            }
                        }
                        Ok(())
                    },
                );
                let fake = self.clone();
                client
                    .expect_delete_microvm_image_version()
                    .returning(move |_, version| {
                        let mut versions = fake.versions.lock().unwrap();
                        let inactive = versions.iter().any(|(v, off)| v == version && *off);
                        assert!(
                            inactive,
                            "AWS cannot delete {version} before it is inactive"
                        );
                        fake.delete_calls.lock().unwrap().push(version.to_string());
                        // Accepted now, gone on the next read.
                        versions.retain(|(v, _)| v != version);
                        Ok(())
                    });
                let fake = self.clone();
                client
                    .expect_get_microvm_image_version()
                    .returning(move |_, version| {
                        let versions = fake.versions.lock().unwrap();
                        match versions.iter().find(|(v, _)| v == version) {
                            Some((v, _)) => Ok(MicrovmImageVersion {
                                image_version: Some(v.clone()),
                                ..active_version()
                            }),
                            None => Err(not_found()),
                        }
                    });
                client
            }
        }

        /// The sandbox as the field had it: serving `50.0`, `1.0`..`49.0` replaced long ago, a
        /// roll to a new bundle saved at `UpdatingImage`.
        pub(super) fn blocked_controller() -> AwsSandboxController {
            let mut controller = ready_controller();
            controller.state = AwsSandboxState::UpdatingImage;
            controller.active_version = Some("50.0".to_string());
            controller.retired_versions =
                (1..CAP).map(|n| retired(&format!("{n}.0"), None)).collect();
            controller
        }

        /// The existing API: a Lambda worker whose saved config is the previous release's and
        /// whose environment carries the sandbox binding it was applied with.
        pub(super) fn consumer(permissions: &str) -> Worker {
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest".to_string(),
                })
                .permissions(permissions.to_string())
                .link(&sandbox())
                .build()
        }

        pub(super) async fn executor_with_consumer(
            controller: AwsSandboxController,
            client: MockLambdaMicrovmsApi,
            observed: Option<LaunchableSandboxVersions>,
            desired_still_links: bool,
        ) -> SingleControllerExecutor {
            let worker_state = crate::TestWorkerController {
                launchable_sandbox_versions: observed,
                ..Default::default()
            };
            let mut builder = SingleControllerExecutor::builder()
                .resource(sandbox_with_bundle(
                    "s3://alien-bundles-test/sandbox/next.zip",
                ))
                .controller(controller)
                .platform(Platform::Aws)
                .service_provider(provider(client));
            if desired_still_links {
                // The pending release changes the API, which waits for the sandbox's new version.
                builder = builder.with_stack_resource(consumer("new"), ResourceLifecycle::Live);
            }
            let mut executor = builder.build().await.expect("executor should build");
            let mut saved = alien_core::StackResourceState::builder()
                .resource_type(alien_core::Worker::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(consumer("old")))
                .lifecycle(ResourceLifecycle::Live)
                .build();
            saved
                .set_internal_controller(Some(Box::new(worker_state)))
                .unwrap();
            executor.insert_saved_resource("api", saved);
            executor
        }

        /// A saved workload whose applied config never linked the sandbox, as a scheduler that
        /// reports every replica of its cluster's inventory answers.
        pub(super) async fn executor_with_unlinked_scheduler(
            controller: AwsSandboxController,
            client: MockLambdaMicrovmsApi,
            observed: Option<LaunchableSandboxVersions>,
        ) -> SingleControllerExecutor {
            let mut executor = SingleControllerExecutor::builder()
                .resource(sandbox_with_bundle(
                    "s3://alien-bundles-test/sandbox/next.zip",
                ))
                .controller(controller)
                .platform(Platform::Aws)
                .service_provider(provider(client))
                .build()
                .await
                .expect("executor should build");
            let unlinked = Worker::new("scheduler".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest".to_string(),
                })
                .permissions("old".to_string())
                .build();
            let mut saved = alien_core::StackResourceState::builder()
                .resource_type(alien_core::Worker::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(unlinked))
                .lifecycle(ResourceLifecycle::Live)
                .build();
            saved
                .set_internal_controller(Some(Box::new(crate::TestWorkerController {
                    launchable_sandbox_versions: observed,
                    ..Default::default()
                })))
                .unwrap();
            executor.insert_saved_resource("scheduler", saved);
            executor
        }

        /// A saved container on `cluster_id` whose controller does not observe bindings, and a
        /// saved compute cluster that answers `cluster_answer`.
        pub(super) async fn executor_with_container_on_cluster(
            controller: AwsSandboxController,
            client: MockLambdaMicrovmsApi,
            container_cluster: Option<&str>,
            cluster_answer: Option<LaunchableSandboxVersions>,
            cluster_type: &str,
            as_daemon: bool,
        ) -> SingleControllerExecutor {
            let mut executor = SingleControllerExecutor::builder()
                .resource(sandbox_with_bundle(
                    "s3://alien-bundles-test/sandbox/next.zip",
                ))
                .controller(controller)
                .platform(Platform::Aws)
                .service_provider(provider(client))
                .build()
                .await
                .expect("executor should build");
            let mut container = alien_core::Container::new("api".to_string())
                .code(alien_core::ContainerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest".to_string(),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "1".to_string(),
                    desired: "1".to_string(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "1Gi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .permissions("old".to_string())
                .link(&sandbox())
                .build();
            container.cluster = container_cluster.map(str::to_string);
            let (workload_type, workload) = if as_daemon {
                let mut daemon = alien_core::Daemon::new("api".to_string())
                    .code(alien_core::DaemonCode::Image {
                        image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest"
                            .to_string(),
                    })
                    .link(&sandbox())
                    .build();
                daemon.cluster = container_cluster.map(str::to_string);
                (
                    alien_core::Daemon::RESOURCE_TYPE.to_string(),
                    alien_core::Resource::new(daemon),
                )
            } else {
                (
                    alien_core::Container::RESOURCE_TYPE.to_string(),
                    alien_core::Resource::new(container),
                )
            };
            let mut saved = alien_core::StackResourceState::builder()
                .resource_type(workload_type)
                .status(ResourceStatus::Running)
                .config(workload)
                .lifecycle(ResourceLifecycle::Live)
                .build();
            saved
                .set_internal_controller(Some(Box::new(crate::TestWorkerController::default())))
                .unwrap();
            executor.insert_saved_resource("api", saved);
            let cluster = Worker::new("compute".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest".to_string(),
                })
                .permissions("old".to_string())
                .build();
            let mut saved = alien_core::StackResourceState::builder()
                .resource_type(cluster_type.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(cluster))
                .lifecycle(ResourceLifecycle::Live)
                .build();
            saved
                .set_internal_controller(Some(Box::new(crate::TestWorkerController {
                    launchable_sandbox_versions: cluster_answer,
                    ..Default::default()
                })))
                .unwrap();
            executor.insert_saved_resource("compute", saved);
            executor
        }

        pub(super) fn known(versions: &[&str]) -> Option<LaunchableSandboxVersions> {
            Some(LaunchableSandboxVersions::Known(
                versions.iter().map(|v| v.to_string()).collect(),
            ))
        }
    }

    /// A Live sandbox at the version cap whose API still runs an installed older binding and
    /// has a pending update that waits for the sandbox's new version. The next build is refused,
    /// and the retry has to free versions on its own: only those no binding names go inactive,
    /// nothing is deleted before the session fence, and once AWS confirms deletions the build is
    /// accepted — across a saved checkpoint.
    #[tokio::test]
    async fn a_blocked_rollout_reclaims_versions_no_binding_names() {
        use blocked_rollout::*;
        let image = FakeImage::at_cap();
        // The API reports the old binding it was applied with.
        let observed = known(&["30.0"]);
        let mut executor =
            executor_with_consumer(blocked_controller(), image.client(), observed.clone(), true)
                .await;

        // Inside the fence the roll waits on its saved state: versions go inactive, AWS is not
        // asked to build, and the wait says when a slot frees.
        for _ in 0..8 {
            executor
                .step()
                .await
                .expect("a draining image is a wait, not a failure");
        }
        let waiting = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(waiting.state, AwsSandboxState::UpdatingImage);
        let reason = waiting.get_progress().expect("the wait is visible");
        assert!(reason.contains("becomes deletable at"), "{reason}");
        let retired = image.retire_calls.lock().unwrap().clone();
        assert!(
            retired.len() >= 40,
            "unreferenced replaced versions go inactive, got {retired:?}"
        );
        assert!(
            !retired
                .iter()
                .any(|v| v == "30.0" || v == "49.0" || v == "50.0"),
            "the API's binding, the previous version and the serving one stay: {retired:?}"
        );
        assert_eq!(
            *image.build_calls.lock().unwrap(),
            0,
            "no build while the cap holds"
        );
        assert!(
            image.delete_calls.lock().unwrap().is_empty(),
            "nothing is deleted inside the session fence"
        );

        // The fence passes across a saved checkpoint and a restart.
        let mut saved = executor
            .internal_state::<AwsSandboxController>()
            .unwrap()
            .clone();
        for entry in saved.retired_versions.iter_mut() {
            if let Some(at) = entry.inactive_at.as_mut() {
                *at -= chrono::Duration::hours(9);
            }
        }
        let restored: AwsSandboxController =
            serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
        drop(executor);
        let mut executor = executor_with_consumer(restored, image.client(), observed, true).await;
        for _ in 0..40 {
            executor.step().await.expect("the roll completes");
            if executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .is_some()
            {
                break;
            }
        }
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(controller.pending_version.as_deref(), Some("51.0"));
        // The binding keeps naming the serving version until the new build is observed ACTIVE.
        let binding = |controller: &AwsSandboxController| {
            controller.get_binding_params().unwrap().unwrap()["imageVersion"].clone()
        };
        assert_eq!(binding(controller), "50.0");
        for _ in 0..5 {
            executor.step().await.expect("the build is observed");
            if executor
                .internal_state::<AwsSandboxController>()
                .unwrap()
                .pending_version
                .is_none()
            {
                break;
            }
        }
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert_eq!(controller.active_version.as_deref(), Some("51.0"));
        assert_eq!(binding(controller), "51.0");
        let held = image.held();
        for kept in ["30.0", "49.0", "50.0"] {
            assert!(held.iter().any(|v| v == kept), "{kept} stays: {held:?}");
        }
        assert!(held.len() < CAP + 1);
    }

    /// Steps the blocked sandbox once and returns the image and the step's error text.
    async fn step_blocked(
        observed: Option<LaunchableSandboxVersions>,
        desired_still_links: bool,
    ) -> (Arc<blocked_rollout::FakeImage>, String) {
        use blocked_rollout::*;
        let image = FakeImage::at_cap();
        let mut executor = executor_with_consumer(
            blocked_controller(),
            image.client(),
            observed,
            desired_still_links,
        )
        .await;
        let outcome = executor.step().await;
        (image, format!("{outcome:?}"))
    }

    /// A scheduler that cannot report what its replicas hold, or whose replica record can be gone
    /// while an unacknowledged execution still runs. Neither is an empty answer: every replaced
    /// version stays, and the refusal names who could not be proven.
    #[tokio::test]
    async fn an_unproven_dependent_keeps_every_replaced_version() {
        let (image, error) = step_blocked(
            Some(LaunchableSandboxVersions::Unproven {
                reason: "a replica of an older server reports no bindings".to_string(),
            }),
            true,
        )
        .await;
        assert!(image.retire_calls.lock().unwrap().is_empty());
        assert_eq!(*image.build_calls.lock().unwrap(), 0);
        assert!(error.contains("api"), "{error}");
        assert!(error.contains("of 50 versions"), "{error}");
    }

    /// A dependent that does not observe bindings and is not running the config being applied
    /// keeps the older behaviour: nothing is retired for it.
    #[tokio::test]
    async fn an_unobserved_dependent_on_an_older_config_keeps_every_replaced_version() {
        let (image, _) = step_blocked(None, true).await;
        assert!(image.retire_calls.lock().unwrap().is_empty());
    }

    /// The pending release dropped the API's link, but its applied revision still holds the
    /// binding: its version stays while the rest is reclaimed.
    #[tokio::test]
    async fn a_removed_link_still_protects_the_applied_binding() {
        let (image, _) = step_blocked(blocked_rollout::known(&["30.0"]), false).await;
        let retired = image.retire_calls.lock().unwrap().clone();
        assert!(!retired.is_empty());
        assert!(!retired.iter().any(|v| v == "30.0"), "{retired:?}");
    }

    /// Two live bindings (both sides of a rollout) are both kept.
    #[tokio::test]
    async fn every_observed_binding_is_kept() {
        let (image, _) = step_blocked(blocked_rollout::known(&["12.0", "30.0"]), true).await;
        let retired = image.retire_calls.lock().unwrap().clone();
        assert!(!retired.is_empty());
        assert!(
            !retired.iter().any(|v| v == "12.0" || v == "30.0"),
            "{retired:?}"
        );
    }

    /// A scheduler whose applied config never named the sandbox but whose cluster still holds a
    /// replica of an older release (the link was removed and the rollout finished) protects the
    /// versions that replica names, and one that cannot say keeps every replaced version.
    #[tokio::test]
    async fn an_unlinked_scheduler_that_observes_its_replicas_is_counted() {
        use blocked_rollout::*;
        let image = FakeImage::at_cap();
        let mut executor = executor_with_unlinked_scheduler(
            blocked_controller(),
            image.client(),
            known(&["30.0"]),
        )
        .await;
        for _ in 0..4 {
            let _ = executor.step().await;
        }
        let retired = image.retire_calls.lock().unwrap().clone();
        assert!(!retired.is_empty());
        assert!(!retired.iter().any(|v| v == "30.0"), "{retired:?}");

        let image = FakeImage::at_cap();
        let mut executor = executor_with_unlinked_scheduler(
            blocked_controller(),
            image.client(),
            Some(LaunchableSandboxVersions::Unproven {
                reason: "a machine does not report what it holds".to_string(),
            }),
        )
        .await;
        for _ in 0..4 {
            let _ = executor.step().await;
        }
        assert!(image.retire_calls.lock().unwrap().is_empty());
        assert_eq!(*image.build_calls.lock().unwrap(), 0);
    }

    /// A workload that never linked the sandbox and does not observe bindings holds none.
    #[tokio::test]
    async fn an_unlinked_workload_that_does_not_observe_bindings_protects_nothing() {
        use blocked_rollout::*;
        let image = FakeImage::at_cap();
        let mut executor =
            executor_with_unlinked_scheduler(blocked_controller(), image.client(), None).await;
        for _ in 0..4 {
            let _ = executor.step().await;
        }
        assert!(!image.retire_calls.lock().unwrap().is_empty());
    }

    /// A dependent that reports it holds no binding protects nothing.
    #[tokio::test]
    async fn a_dependent_holding_no_binding_protects_nothing() {
        use blocked_rollout::*;
        let image = FakeImage::at_cap();
        let mut executor =
            executor_with_consumer(blocked_controller(), image.client(), known(&[]), true).await;
        for _ in 0..6 {
            let _ = executor.step().await;
        }
        let retired = image.retire_calls.lock().unwrap().clone();
        assert!(retired.iter().any(|v| v == "30.0"), "{retired:?}");
    }
    /// A container whose controller does not observe bindings is covered by the compute
    /// cluster its saved config names, when that cluster answered completely: the older
    /// config comparison, which a blocked rollout can never satisfy, does not apply. A cluster
    /// that did not answer, or a container on another cluster, keeps every replaced version.
    #[tokio::test]
    async fn a_container_is_covered_only_by_its_own_proven_cluster() {
        use blocked_rollout::*;
        let cluster = alien_core::ComputeCluster::RESOURCE_TYPE.to_string();
        let silent = || {
            Some(LaunchableSandboxVersions::Unproven {
                reason: "a machine is silent".to_string(),
            })
        };
        for (container_cluster, answer, cluster_type, as_daemon, reclaims) in [
            (Some("compute"), known(&["30.0"]), cluster.as_str(), false, true),
            (Some("compute"), known(&["30.0"]), cluster.as_str(), true, true),
            (Some("other"), known(&["30.0"]), cluster.as_str(), false, false),
            (Some("other"), known(&["30.0"]), cluster.as_str(), true, false),
            (None, known(&["30.0"]), cluster.as_str(), false, false),
            (Some("compute"), silent(), cluster.as_str(), false, false),
            (Some("compute"), None, cluster.as_str(), false, false),
            // A generic resource's answer is not a cluster witness.
            (Some("compute"), known(&["30.0"]), "worker", false, false),
        ] {
            let image = FakeImage::at_cap();
            let mut executor = executor_with_container_on_cluster(
                blocked_controller(),
                image.client(),
                container_cluster,
                answer,
                cluster_type,
                as_daemon,
            )
            .await;
            for _ in 0..4 {
                let _ = executor.step().await;
            }
            let retired = image.retire_calls.lock().unwrap().clone();
            assert_eq!(!retired.is_empty(), reclaims, "{container_cluster:?}: {retired:?}");
            assert!(!retired.iter().any(|v| v == "30.0"), "{retired:?}");
        }
    }
}
