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
//! granted to no sandbox set, so sessions cannot be enumerated here and self-reap at their
//! declared lifetime ceiling.

use std::collections::BTreeMap;
use std::time::Duration;

use tracing::{debug, info};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_aws_clients::lambda_microvms::{
    CreateMicrovmImageRequest, MicrovmCodeArtifact, MicrovmCpuConfiguration, MicrovmImageBuild,
    MicrovmImageBuildHooks, MicrovmImageHooks, MicrovmImageLogging, MicrovmImageResources,
    MicrovmLifecycleHooks, UpdateMicrovmImageRequest,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::sandbox_build_role::{sandbox_build_role_arn, sandbox_build_role_name};
use alien_core::sandbox_image::AWS_MICROVM;
use alien_core::{
    parse_bundle_uri, standard_resource_tags, BundleUri, ResourceOutputs as CoreResourceOutputs,
    ResourceStatus, Sandbox, SandboxCode, SandboxOutputs,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use sha2::{Digest, Sha256};

/// The only architecture MicroVM images accept.
const ARCHITECTURE: &str = "ARM_64";

/// A build takes ~110-160s, gated on the in-image `Ready` hook (itself capped at 120s).
/// 90 polls at this interval is a 15-minute ceiling — an order of magnitude past a healthy
/// build, so hitting it means the image is wedged, not slow.
const BUILD_POLL_INTERVAL: Duration = Duration::from_secs(10);
const BUILD_MAX_POLLS: u32 = 90;

/// A version a roll replaced, and when it stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetiredVersion {
    pub(crate) version: String,
    pub(crate) retired_at: chrono::DateTime<chrono::Utc>,
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
    /// Public signing material; the private key remains only in the secrets vault.
    #[serde(default)]
    pub(crate) egress_public_key: Option<super::egress::GatewayPublicKey>,
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
    /// Accepted deletion, retained until the provider confirms absence.
    #[serde(default)]
    pub(crate) deleting_version: Option<String>,
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
    #[serde(default)]
    pub(crate) granted_bundle_uri: Option<String>,
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

#[controller]
impl AwsSandboxController {
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
        let bundle_uri = desired_bundle_uri(config, &aws_config.region)?;
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
        .with_agent_environment(alien_core::sandbox_egress::aws_sandbox_agent_environment(
            ctx.desired_stack,
            config,
        ))
        .with_receipt()?;

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
                let created = client
                    .create_microvm_image(
                        inputs.create_request(image_name.clone(), image_tags(ctx, &config.id)),
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
                    if self.reconcile_image_inventory(&client, &config.id).await? {
                        return Ok(HandlerAction::Continue {
                            state: CreatingImage,
                            suggested_delay: Some(BUILD_POLL_INTERVAL),
                        });
                    }
                    let rolled = client
                        .update_microvm_image(&identifier, inputs.update_request())
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to rebuild adopted MicroVM image '{image_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    if let Some(replaced) = image.image_version {
                        self.retired_versions.push(RetiredVersion {
                            version: replaced,
                            retired_at: chrono::Utc::now(),
                        });
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
        self.pending_supervisor_environment =
            alien_core::sandbox_egress::aws_sandbox_agent_environment(ctx.desired_stack, config);
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
            let vault = super::egress::deployment_vault(ctx).await?;
            self.egress_public_key = Some(
                if alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack)
                    == Some(ctx.desired_config.id())
                {
                    super::egress::ensure_signing_key(vault.as_ref()).await?
                } else {
                    super::egress::read_signing_public_key(vault.as_ref()).await?
                },
            );
            let key = self
                .egress_public_key
                .as_ref()
                .expect("key initialized above");
            self.egress = Some(alien_core::bindings::SandboxEgressBinding::Gateway(
                alien_core::bindings::SandboxEgressGateway {
                    container: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into(),
                    audience: format!("{}/{}", ctx.resource_prefix, config.id),
                    signing_key: alien_core::bindings::SandboxEgressSigningKey {
                        vault: alien_core::SECRETS_VAULT_ID.into(),
                        key: super::egress::SIGNING_KEY.into(),
                    },
                    kid: key.kid.clone(),
                    admin_ca_pem: None,
                },
            ));
        } else {
            self.egress_public_key = None;
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
            super::egress::publish_aws_config(ctx, key).await?;
            self.egress_network_verified = true;
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

        if matches!(config.egress, alien_core::SandboxEgress::Internet) {
            self.egress_network_verified = false;
            // A deployment being deleted no longer declares this sandbox or its network. Its
            // binding stays withdrawn, and the refresh must not block the deletion.
            if ctx.desired_stack.resources.contains_key(&config.id) {
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
                if self.reconcile_image_inventory(&client, &config.id).await? {
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
            state: if self.egress_public_key.is_some() {
                RefreshingEgressConfig
            } else {
                Ready
            },
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // Refresh the audience list when another Sandbox is added or removed, even
    // when this owner's own resource configuration did not change.
    #[handler(state = RefreshingEgressConfig, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn refreshing_egress_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if let Some(key) = &self.egress_public_key {
            self.egress_network_verified = false;
            super::egress::publish_aws_config(ctx, key).await?;
            self.egress_network_verified = true;
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
        let desired_bundle = desired_bundle_uri(&config, &aws_config.region)?;

        if self.bundle_uri.as_deref() == Some(desired_bundle.as_str())
            && self.supervisor_environment
                == alien_core::sandbox_egress::aws_sandbox_agent_environment(
                    ctx.desired_stack,
                    config,
                )
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
        let desired_bundle = desired_bundle_uri(&config, &aws_config.region)?;
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
        .with_agent_environment(alien_core::sandbox_egress::aws_sandbox_agent_environment(
            ctx.desired_stack,
            config,
        ))
        .for_roll(self.active_version.as_deref())
        .for_retry(self.build_retry_generation)
        .with_receipt()?
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
            if self.reconcile_image_inventory(&client, &config.id).await? {
                return Ok(HandlerAction::Continue {
                    state: UpdatingImage,
                    suggested_delay: Some(BUILD_POLL_INTERVAL),
                });
            }
            client
                .update_microvm_image(&image_identifier, request)
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
        self.pending_supervisor_environment =
            alien_core::sandbox_egress::aws_sandbox_agent_environment(ctx.desired_stack, config);
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

    // Sessions cannot be enumerated (see the module doc) and self-reap; a delete refused while
    // they run fails loudly here and the executor retries — nothing is fired and forgotten.

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
            info!(sandbox_id = %config.id, "Deleted the deployment's sandbox egress key and gateway configuration");
        }
        self.egress_public_key = None;
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
            self.retired_versions.push(RetiredVersion {
                version: replaced,
                retired_at: chrono::Utc::now(),
            });
        }
        if let Some(bundle) = self.pending_bundle_uri.take() {
            self.bundle_uri = Some(bundle);
            self.supervisor_environment = std::mem::take(&mut self.pending_supervisor_environment);
        }
    }

    /// Reconcile provider inventory, including builds absent from legacy checkpoints.
    /// Successful versions remain protected: old pinned consumers can still launch
    /// sessions, and deleting a backing image can fault a running or resumed VM.
    /// Neither promotion age nor new-replica readiness establishes safe deletion.
    /// Return true while cleanup owns this step, including asynchronous deletion.
    async fn reconcile_image_inventory(
        &mut self,
        client: &std::sync::Arc<dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi>,
        resource_id: &str,
    ) -> Result<bool> {
        let Some(identifier) = self.image_identifier.as_deref() else {
            return Ok(false);
        };
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
        // AWS refuses to delete the last version. Retry a failed initial build
        // first, then reclaim it once the replacement exists.
        if inventory.len() == 1 {
            return Ok(false);
        }
        for version in inventory {
            let failed = version.state.as_deref() == Some("FAILED");
            if !failed {
                continue;
            }
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
            match client
                .delete_microvm_image_version(identifier, &version)
                .await
            {
                Ok(()) => {}
                Err(error) if is_remote_resource_absent(&error) => {}
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                        "Failed to delete failed MicroVM image '{identifier}' version '{version}'"
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

    /// Whether this controller built the image and may therefore rebuild it: a Live sandbox, or a
    /// Frozen one a direct setup registered build inputs for, which only setup steps. A template's
    /// Frozen image belongs to the setup stack, which owns its bundle too.
    fn owns_image_builds(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        self.owns_image_deletion(ctx, resource_id)
            || (self.build_role_arn.is_some()
                && ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup)
    }
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

/// Assembles the build inputs from the declaration, so a create and a roll of the same sandbox
/// differ only in the bundle.
fn image_build_inputs(
    region: &str,
    resource_id: &str,
    image_name: &str,
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
        // Keyed on the account-unique image name and the bundle: a retry of the same release
        // replays the prior success, a new release's bundle asks for a real build, and another
        // deployment in the same account rolling the same bundle onto its own image never reuses
        // this image's token.
        client_token: build_client_token(image_name, bundle_uri),
        environment: agent_environment(),
        additional_os_capabilities: vec![],
    }
}

/// The MicroVM image's name, unique in the account because it carries the deployment's
/// resource prefix.
fn sandbox_image_name(resource_prefix: &str, resource_id: &str) -> String {
    format!("{resource_prefix}-{resource_id}")
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
#[derive(Clone)]
struct ImageBuildInputs {
    description: String,
    base_image_arn: String,
    build_role_arn: String,
    code_artifact: MicrovmCodeArtifact,
    egress_network_connectors: Vec<String>,
    resources: Vec<MicrovmImageResources>,
    client_token: String,
    environment: BTreeMap<String, String>,
    additional_os_capabilities: Vec<String>,
}

impl ImageBuildInputs {
    fn with_agent_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.environment.extend(environment);
        if self.environment.contains_key("ALIEN_SANDBOX_EGRESS") {
            // AWS rejects named capabilities (including NET_ADMIN): the API enum is only ALL.
            // The agent drops to eight capabilities before starting worker threads and gives
            // commands none. Omitting ALL prevents netlink firewall installation.
            // https://docs.aws.amazon.com/lambda/latest/dg/microvms-images.html#microvms-images-os-capabilities
            self.additional_os_capabilities = vec!["ALL".to_string()];
            // Fold the complete policy into the token without truncating away its digest.
            let digest =
                Sha256::digest(format!("{}{:?}", self.client_token, self.environment).as_bytes());
            self.client_token = format!("agent-{:x}", digest)[..64].to_string();
        }
        self
    }

    fn for_roll(mut self, previous_version: Option<&str>) -> Self {
        self.client_token = roll_client_token(&self.client_token, previous_version);
        self
    }

    fn for_retry(mut self, generation: u64) -> Self {
        if generation != 0 {
            let digest = Sha256::digest(format!("{}:retry:{generation}", self.client_token));
            self.client_token = format!("{digest:x}");
        }
        self
    }

    fn with_receipt(mut self) -> Result<Self> {
        let encoded = serde_json::to_vec(&self.clone().update_request())
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                message: "Cannot encode MicroVM build request".to_string(),
                resource_id: None,
            })?;
        self.client_token = format!("{:x}", Sha256::digest(encoded));
        self.description = format!("{} [build:{}]", self.description, self.client_token);
        Ok(self)
    }

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
            .environment_variables(self.environment)
            .additional_os_capabilities(self.additional_os_capabilities)
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
            .environment_variables(self.environment)
            .additional_os_capabilities(self.additional_os_capabilities)
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

fn roll_client_token(token: &str, previous_version: Option<&str>) -> String {
    let digest = Sha256::digest(format!("{token}:{previous_version:?}").as_bytes());
    format!("roll-{:x}", digest)[..64].to_string()
}

/// The pre-create probe has no ARN to adopt yet, and the API answers a bare name with a 400
/// that no absent-resource check can read as absence. The name is account-unique, so the ARN
/// it will carry is derivable before the image exists.
pub(crate) fn sandbox_image_arn(region: &str, account_id: &str, image_name: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:{account_id}:microvm-image:{image_name}",
        aws_partition(region)
    )
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
pub(crate) fn aws_partition(region: &str) -> &'static str {
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
        .port(AWS_MICROVM.port)
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
///
/// Identity and isolation come from the image: runtime defaults would overwrite its USER.
fn agent_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "ALIEN_SANDBOX_ROOT".to_string(),
            AWS_MICROVM.session_root.to_string(),
        ),
        (
            "ALIEN_SANDBOX_PORT".to_string(),
            AWS_MICROVM.port.to_string(),
        ),
        (
            "ALIEN_SANDBOX_AUTHORIZATION".to_string(),
            AWS_MICROVM.authorization.env_value().to_string(),
        ),
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
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageResponse, MicrovmImage, MicrovmImageVersion, MockLambdaMicrovmsApi,
        UpdateMicrovmImageResponse,
    };
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use alien_core::{Platform, SandboxCode, SandboxEgress, SandboxLifecyclePolicy};
    use std::sync::{Arc, Mutex};

    const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";
    const BUILD_ROLE_ARN: &str = "arn:aws:iam::123456789012:role/test-agents-build";
    const BUNDLE_URI: &str = "s3://alien-bundles-test/sandbox/bundle.zip";
    const DENY_CONNECTOR_ARN: &str =
        "arn:aws:lambda:us-east-1:123456789012:network-connector:nc-deny123";

    /// The setup-owned image creates the key while the earlier Live image is still pending.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn frozen_sandbox_initializes_the_key_before_the_live_sandbox() {
        use crate::{LocalVaultController, LocalVaultState};
        use alien_bindings::{providers::vault::LocalVault, Vault as _};
        use alien_core::{ClientConfig, ResourceLifecycle, Vault, SECRETS_VAULT_ID};

        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let mut frozen = sandbox();
        frozen.id = "z-frozen".into();
        frozen.egress = SandboxEgress::Internet;
        let mut live = frozen.clone();
        live.id = "a-live".into();
        let mut accepted = None;
        for (current, lifecycle, other, other_lifecycle) in [
            (
                frozen.clone(),
                ResourceLifecycle::Frozen,
                live.clone(),
                ResourceLifecycle::Live,
            ),
            (
                live,
                ResourceLifecycle::Live,
                frozen,
                ResourceLifecycle::Frozen,
            ),
        ] {
            let mut executor = SingleControllerExecutor::builder()
                .resource(current)
                .resource_lifecycle(lifecycle)
                .controller(AwsSandboxController {
                    state: AwsSandboxState::InitializingEgressKey,
                    ..Default::default()
                })
                .platform(Platform::Aws)
                .client_config(ClientConfig::Local {
                    state_directory: directory.path().to_string_lossy().into_owned(),
                })
                .with_stack_resource(other, other_lifecycle)
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
            executor
                .step()
                .await
                .expect("shared key initialization advances");
            let controller = executor.internal_state::<AwsSandboxController>().unwrap();
            assert_eq!(controller.state, AwsSandboxState::PublishingEgressConfig);
            let public_key = controller
                .egress_public_key
                .clone()
                .expect("public key published");
            let stored = vault
                .get_secret(super::super::egress::SIGNING_KEY)
                .await
                .unwrap();
            if let Some((expected_public, expected_stored)) = &accepted {
                assert_eq!(&public_key, expected_public);
                assert!(
                    &stored == expected_stored,
                    "Live initialization must not rotate the key"
                );
            } else {
                accepted = Some((public_key, stored));
            }
        }
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
        let key = super::super::egress::ensure_signing_key(&vault)
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
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(1800),
                idle_pause_seconds: Some(600),
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
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(1800),
                idle_pause_seconds: Some(600),
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
                        .is_some_and(|hooks| hooks.port == AWS_MICROVM.port)
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
                ("execution-sa", "alien-agents-sandbox-management"),
                ("execution-sa", "alien-agents-sandbox-execute"),
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
