//! AWS Sandbox controller.
//!
//! The durable parent is a Lambda MicroVM image. A Live sandbox's image is built here, after
//! the deployment registers: setup installs the build role and egress connector and registers
//! the bundle, and only at runtime does a customer account exist as a principal Alien's
//! registry can open to. A Frozen sandbox arrives through the importer with its image already
//! built by stack creation, and this controller only watches it.
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
    MicrovmLifecycleHooks,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox,
    SandboxOutputs,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

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

/// AWS Sandbox controller.
#[controller]
pub struct AwsSandboxController {
    /// MicroVM image backing this sandbox's sessions; the ARN, which every call accepts.
    pub(crate) image_identifier: Option<String>,
    /// Image ARN, published in outputs so the binding can address it.
    pub(crate) image_arn: Option<String>,
    /// Image version sessions are enumerated by, together with the image. `1.0` for a fresh
    /// image, not `1`.
    pub(crate) image_version: Option<String>,
    /// Region the MicroVMs run in. Held here because the binding is built without a context.
    pub(crate) region: Option<String>,
    /// Role passed to `CreateMicrovmImage`. Setup owns it because `sandbox/provision` grants
    /// `iam:PassRole` and no `iam:CreateRole`; the registration hands it over.
    #[serde(default)]
    pub(crate) build_role_arn: Option<String>,
    /// Bundle the image is built from, handed over by the registration.
    #[serde(default)]
    pub(crate) bundle_uri: Option<String>,
    /// Whether the built version has been observed ACTIVE. Gates the binding: the image
    /// fields above are known from the moment the create call returns, long before a session
    /// could start from them.
    #[serde(default)]
    pub(crate) image_active: bool,
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
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

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
        let request = CreateMicrovmImageRequest::builder()
            .name(image_name.clone())
            .description(format!("Sandbox {}", config.id))
            .base_image_arn(managed_base_image_arn(&aws_config.region))
            .base_image_version("1")
            .build_role_arn(build_role_arn)
            .code_artifact(MicrovmCodeArtifact { uri: bundle_uri })
            // The switch behind "control plane never sees sandbox contents".
            .logging(MicrovmImageLogging::Disabled {})
            .egress_network_connectors(vec![internet_egress_connector_arn(&aws_config.region)])
            .cpu_configurations(vec![MicrovmCpuConfiguration {
                architecture: ARCHITECTURE.to_string(),
            }])
            .resources(vec![MicrovmImageResources {
                minimum_memory_in_mib: tier.baseline_memory_mib,
            }])
            .hooks(agent_hooks())
            .environment_variables(agent_environment())
            // The deployed `sandbox/provision` grant conditions `CreateMicrovmImage` on the
            // stack tag and `managed-by: runtime` arriving as request tags; without them the
            // call is denied by a policy that is already installed.
            .tags(
                standard_resource_tags(ctx.resource_prefix, &config.id)
                    .into_iter()
                    .collect::<BTreeMap<_, _>>(),
            )
            // Stable per resource, so a retried create returns the prior success instead of
            // failing on the duplicate name.
            .client_token(image_name.clone())
            .build();

        let created =
            client
                .create_microvm_image(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create MicroVM image '{image_name}'"),
                    resource_id: Some(config.id.clone()),
                })?;

        let image_arn = created.image_arn.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("CreateMicrovmImage for '{image_name}' returned no image ARN"),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let image_version = created.image_version.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("CreateMicrovmImage for '{image_name}' returned no version"),
                resource_id: Some(config.id.clone()),
            })
        })?;

        self.image_identifier = Some(image_arn.clone());
        self.image_arn = Some(image_arn);
        self.image_version = Some(image_version);
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
                self.image_active = true;
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
            // A rolled version changes what sessions enumerate under; kept when the read
            // carries none, because dropping it would unpublish a binding in use.
            if image.image_version.is_some() {
                self.image_version = image.image_version.clone();
            }
            self.region = Some(aws_config.region.clone());

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

        // The session-policy ceilings feed the binding and cost nothing to refresh. The image
        // itself does not rebuild here: its inputs — bundle and build role — are registration
        // facts, and a changed registration arrives through re-import, not through this
        // handler.
        self.idle_suspend_seconds = config.session.idle_suspend_seconds;
        self.max_lifetime_seconds = config.session.max_lifetime_seconds;

        info!(sandbox_id = %config.id, "Updated AWS sandbox configuration");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
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
        self.image_active = false;

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

        self.image_active = false;
        info!(sandbox_id = %config.id, "AWS sandbox image deleted; sessions expire on their own");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{AwsSandboxBinding, BindingValue, SandboxBinding};

        // No binding until the version has been observed ACTIVE: the fields below are known
        // from the moment the create call returns, ~160s before a session could start from
        // them, and a published binding is a promise sessions can be created now.
        if !self.image_active {
            return Ok(None);
        }
        let (Some(image_arn), Some(image_version), Some(region)) = (
            self.image_arn.as_ref(),
            self.image_version.as_ref(),
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
        match (self.image_identifier.clone(), self.image_version.clone()) {
            (Some(identifier), Some(version)) => Ok((identifier, version)),
            _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "waiting on a MicroVM image build with no image identifier or version \
                          recorded"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }
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
                image: "manager.example.com/alien-artifacts-proj:base".to_string(),
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
            image_version: Some("1.0".to_string()),
            image_active: true,
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

    /// The full runtime build: the create call must satisfy the already-deployed
    /// `sandbox/provision` grant — the `deployment` and `managed-by: runtime` request tags
    /// are what the policy conditions on, the build role is what it authorizes as a pass —
    /// and the recorded version must be the string AWS mints, `1.0`.
    #[tokio::test]
    async fn a_live_sandbox_builds_its_image_and_reaches_running() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_create_microvm_image()
            .withf(|request| {
                request.name == "test-agents"
                    && request.client_token == "test-agents"
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
        assert_eq!(controller.image_version.as_deref(), Some("1.0"));
        assert!(controller.image_active);

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

    /// The failure error must carry AWS's own reasons — the version's and each failed
    /// build's. A generic error at the end of a ~160s build is the worst debugging
    /// experience this resource can produce.
    #[tokio::test]
    async fn a_failed_build_surfaces_the_services_own_reason() {
        let mut client = MockLambdaMicrovmsApi::new();
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
            image_version: Some("1.0".to_string()),
            image_active: true,
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
