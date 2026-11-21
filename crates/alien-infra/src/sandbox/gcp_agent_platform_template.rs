//! GCP Agent Platform sandbox template controller.
//!
//! Reconciles the `SandboxEnvironmentTemplate`: the Live, release-owned object that carries
//! the image, ceilings and egress and warms the session pool. The reasoning engine it hangs
//! under is a separate Live resource with its own controller; this one reads the engine's id as a
//! dependency and creates templates beneath it, never creating the engine itself.
//!
//! Template config is immutable: there is no update verb, so reconciliation is replace-not-update.
//! A change to the identity fields creates a new template, waits for it to become `ACTIVE`, and
//! only then reaps the old one, so a release never leaves a session pointing at a template that
//! has already been deleted. [`template_identity`] names those fields and says what a change
//! outside them does.

use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::sandbox::gcp_placement::{self, Connection, Placement};
use crate::sandbox::GcpAgentPlatformEngineController;
use alien_core::sandbox_image::GCP_AGENT_PLATFORM;
use alien_core::{
    DeploymentConfig, GcpAgentPlatformEngine, ResourceOutputs, ResourceRef, ResourceStatus,
    Sandbox, SandboxCode, SandboxLimits,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::agent_platform::{
    ContainerPort, ContainerResources, CustomContainerEnvironment, CustomContainerSpec,
    EgressControlConfig, IngressControlConfig, SandboxEnvironmentTemplate,
};
use alien_gcp_clients::longrunning::OperationResult;
use alien_gcp_clients::platform::GcpClientConfigExt;
use alien_macros::controller;

use super::gcp_image_copy::{copy_image, CopyDestination, CopySource};

/// Lifecycle state the API reports for a template that is ready to cut sessions from.
const TEMPLATE_ACTIVE: &str = "ACTIVE";

/// How many 2-second polls an ACTIVE template waits for its attachment to connect: 3 minutes.
/// Connections arrived 30 to 70 seconds after the template was created.
const ATTACHMENT_CONNECT_POLLS: u32 = 90;

/// State the API reports for a template whose container exited (it gives no reason).
const TEMPLATE_FAILED: &str = "FAILED";

/// Last path segment of a resource name — the id the client interpolates back into its paths.
fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The fields of a template that, when changed, force a replace.
///
/// The template is immutable, so any of these differing between the desired and previous
/// declaration means the old template cannot be updated in place — it is torn down and rebuilt.
/// `image` is the declared reference, digest or tag, before [`pulled_image`] resolves its host, so
/// a re-pushed tag moves nothing here.
/// The whole `SandboxLimits` is compared but only cpu and memory reach the body, so a disk-only
/// edit replaces a template with a byte-identical one; `max_processes` cannot vary at all, because
/// `process_limit: false` refuses it at plan time. The `bool` is `internet_access`, which picks
/// which egress config the body carries. A body field that is not here, such as the
/// declared port, cannot force a replace on its own — an existing template picks up a new value
/// only at the next replace.
fn template_identity(sandbox: &Sandbox) -> Result<(String, SandboxLimits, bool, bool)> {
    let image = match &sandbox.code {
        SandboxCode::Image { image, .. } => image.clone(),
        SandboxCode::Source { .. } => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "the sandbox still has unbuilt source code. Run 'alien build' first"
                    .to_string(),
                resource_id: Some(sandbox.id().to_string()),
            }));
        }
    };
    let internet_access = internet_access_or_refuse(sandbox)?;
    // The last element is the gateway path: it changes the template's placement, so it is part of
    // what forces a replace even though the cloud's boolean reads `false` for it and for `none`.
    Ok((
        image,
        sandbox.resolved_limits(),
        internet_access,
        uses_gateway_path(sandbox),
    ))
}

/// The image Agent Platform pulls. Agent Platform pulls from the registry directly, as its own
/// service agent, which the manager's registry grant names; it cannot pull through the manager's
/// proxy. An image in a per-project repository on any host but Artifact Registry's own was pushed
/// through the manager, so it is named on the native registry host, whichever host the release
/// stamped on it. Any other reference is pulled as declared, including one already on an Artifact
/// Registry host, whose region the native host may not share.
fn pulled_image(
    image: &str,
    deployment_config: &DeploymentConfig,
    resource_id: &str,
) -> Result<String> {
    let Some((host, path)) = image.split_once('/') else {
        return Ok(image.to_string());
    };
    if host.ends_with("-docker.pkg.dev") || per_project_image_project_id(path).is_none() {
        return Ok(image.to_string());
    }
    match (
        deployment_config.native_image_host.as_deref(),
        deployment_config.manager_url.is_some(),
    ) {
        (Some(native_host), _) => Ok(format!("{}/{path}", native_host.trim_end_matches('/'))),
        // The manager derives the native host from its registry binding each time it hands out
        // this config, so a binding it could not load is retried, not refused.
        (None, true) => Err(AlienError::new(ErrorData::DependencyNotReady {
            resource_id: resource_id.to_string(),
            dependency_id: "native image registry".to_string(),
        })),
        (None, false) => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "cannot name '{image}' for Agent Platform to pull: it is in a manager's \
                 per-project repository, and the deployment has no manager to name that \
                 registry's native host"
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

/// The Alien project whose per-project repository `path` (a reference without its host) is in:
/// `{gcpProject}/{repository}/{projectId}/...`, where `repository` is the name
/// [`alien_core::gar_repository::project_repository`] gives `projectId`. `None` for any other path.
fn per_project_image_project_id(path: &str) -> Option<&str> {
    let name = path.split_once('@').map_or(path, |(name, _)| name);
    let name = match name.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => name,
        _ => name,
    };
    let (_, repository_path) = name.split_once('/')?;
    let (repository, rest) = repository_path.split_once('/')?;
    let project_id = rest.split('/').next()?;
    // The deployment does not carry the registry's repository prefix, so it is read back from the
    // name; `project_id_for_path` then applies the naming rule's character and length checks.
    let suffix = match project_id {
        "default" => "-default".to_string(),
        id => format!("-prj-{}", id.strip_prefix("prj_")?),
    };
    let prefix = repository.strip_suffix(suffix.as_str())?;
    alien_core::gar_repository::project_id_for_path(repository_path, prefix)
}

/// The egress switch, or a refusal naming the sandbox and both accepted modes.
///
/// `AllowDomains` has no representation in the single internet-access switch, so it is refused
/// rather than approximated. The mode→switch mapping is `SandboxEgress::internet_access_switch`, so
/// this cannot disagree with the emitter or the provider on what a mode means.
fn internet_access_or_refuse(sandbox: &Sandbox) -> Result<bool> {
    // `internet` maps to a closed switch there: its only way out is the network attachment.
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

/// Whether the sandbox's egress goes through the deployment's egress gateway: a network attachment
/// into the VPC instead of the cloud's own internet switch.
fn uses_gateway_path(sandbox: &Sandbox) -> bool {
    sandbox.egress == alien_core::SandboxEgress::Internet
}

/// The egress switch a template reports. The API omits a false `internetAccess`, answering `{}`
/// for a deny template and `{"internetAccess": true}` for an open one, so absence reads as deny.
fn served_internet_access(template: &SandboxEnvironmentTemplate) -> bool {
    template
        .egress_control_config
        .as_ref()
        .and_then(|egress| egress.internet_access)
        .unwrap_or(false)
}

/// Builds the immutable template body from the declaration.
///
/// `network_attachment` is the full path of the attachment setup created for the deployment's
/// egress gateway. An `internet` sandbox names it and turns on private ingress, which the API
/// requires beside it; any other sandbox must not, or it would leave the default placement.
fn build_template_body(
    sandbox: &Sandbox,
    display_name: &str,
    deployment_config: &DeploymentConfig,
    network_attachment: Option<&str>,
) -> Result<SandboxEnvironmentTemplate> {
    let (image, limits, internet_access, gateway) = template_identity(sandbox)?;
    let network_attachment = match (gateway, network_attachment) {
        (true, Some(attachment)) => Some(attachment.to_string()),
        (true, None) => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "sandbox '{}' has egress internet, but the deployment has no network \
                     attachment for it; re-run setup to create it",
                    sandbox.id()
                ),
                resource_id: Some(sandbox.id().to_string()),
            }))
        }
        (false, _) => None,
    };
    let image = pulled_image(&image, deployment_config, sandbox.id())?;

    // cpu, memory and ephemeral-storage are the ceilings the API's resource map expresses;
    // max_processes has no field on this template. The API refuses ephemeral-storage above its
    // cap, which `validate_for_platform` already refuses at plan time.
    let resources = ContainerResources {
        requests: None,
        limits: Some(HashMap::from([
            ("cpu".to_string(), limits.cpu),
            ("memory".to_string(), limits.memory),
            ("ephemeral-storage".to_string(), limits.disk),
        ])),
    };

    // Ingress only reaches a port the template declares: the agent's own, then every preview port.
    // The agent's port is first because `:execute` goes to the FIRST declared port (measured: with
    // the ports sorted, a preview port below the agent's received the control traffic).
    let agent_port = i32::from(GCP_AGENT_PLATFORM.port);
    let mut preview_ports: Vec<i32> = sandbox
        .preview_ports
        .iter()
        .map(|port| i32::from(*port))
        .filter(|port| *port != agent_port)
        .collect();
    preview_ports.sort_unstable();
    preview_ports.dedup();
    let mut declared_ports = vec![agent_port];
    declared_ports.extend(preview_ports);

    Ok(SandboxEnvironmentTemplate {
        name: None,
        display_name: Some(display_name.to_string()),
        custom_container_environment: Some(CustomContainerEnvironment {
            // No env: the command shares the agent's uid and can read the supervisor's
            // environment, so a secret placed here would leak. Capability auth, if ever wanted,
            // needs a carrier that is not the container env.
            custom_container_spec: Some(CustomContainerSpec {
                image_uri: image,
                extra: Default::default(),
            }),
            resources: Some(resources),
            // Must match the port the published image EXPOSEs and listens on. `TCP` is spelled
            // out because every create this preview API has accepted carried it, and no request
            // omitting it has ever been shown to be accepted.
            ports: declared_ports
                .into_iter()
                .map(|port| ContainerPort {
                    port,
                    protocol: Some("TCP".to_string()),
                })
                .collect(),
            extra: Default::default(),
        }),
        egress_control_config: Some(EgressControlConfig {
            internet_access: Some(internet_access),
            network_attachment: network_attachment.clone(),
            extra: Default::default(),
        }),
        ingress_control_config: network_attachment.map(|_| IngressControlConfig {
            enable_private_service_connect: Some(true),
            extra: Default::default(),
        }),
        state: None,
        extra: Default::default(),
    })
}

/// Whether `image` is one the manager's registry serves from another project, so the caller of the
/// template create cannot read it and Agent Platform's agent would not be allowed to either.
/// Returns the proxy path (everything after the host) and the reference (`sha256:...` or a tag).
fn image_to_copy<'a>(
    image: &'a str,
    deployment_config: &DeploymentConfig,
    own_project: &str,
) -> Option<(&'a str, &'a str)> {
    let (host, path) = image.split_once('/')?;
    deployment_config.manager_url.as_ref()?;
    per_project_image_project_id(path)?;
    // Already in the deployment's own Artifact Registry project: Agent Platform reads it as is.
    if host.ends_with("-docker.pkg.dev") && path.split('/').next() == Some(own_project) {
        return None;
    }
    if let Some((name, digest)) = path.split_once('@') {
        return Some((name, digest));
    }
    match path.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => Some((name, tag)),
        _ => Some((path, "latest")),
    }
}

/// Copy the sandbox image into a repository of the deployment's own project and name the copy, by
/// digest, in `body`. Without this the create is refused: Agent Platform requires the caller to
/// be able to read the image. An image already in the project is left alone. There is no fallback
/// to the uncopied reference when the copy fails.
async fn copy_image_into_own_project(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    body: &mut SandboxEnvironmentTemplate,
) -> Result<()> {
    let Some(spec) = body
        .custom_container_environment
        .as_mut()
        .and_then(|environment| environment.custom_container_spec.as_mut())
    else {
        return Ok(());
    };
    let gcp_config = ctx.get_gcp_config()?;
    // `build_template_body` already named the native host; the manager's path is what follows it.
    let Some((path, reference)) = image_to_copy(
        &spec.image_uri,
        ctx.deployment_config,
        &gcp_config.project_id,
    ) else {
        return Ok(());
    };
    let (path, reference) = (path.to_string(), reference.to_string());
    let (Some(manager_url), Some(token)) = (
        ctx.deployment_config.manager_url.as_deref(),
        ctx.deployment_config.deployment_token.as_deref(),
    ) else {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "the sandbox image is in the manager's registry, and the deployment has no \
                      manager token to copy it with"
                .to_string(),
            resource_id: Some(sandbox_id.to_string()),
        }));
    };
    let repository =
        alien_core::gar_repository::sandbox_image_repository(ctx.resource_prefix, sandbox_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "'{}-{sandbox_id}-img' is not a valid repository id",
                        ctx.resource_prefix
                    ),
                    resource_id: Some(sandbox_id.to_string()),
                })
            })?;
    require_image_repository(ctx, sandbox_id, &repository).await?;

    let access_token = gcp_config
        .get_bearer_token("https://www.googleapis.com/auth/cloud-platform")
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to get a Google access token to copy the sandbox image".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    // No overall timeout: a layer of several GB is legitimate. A stalled stream, though, must not
    // hold the step forever: each connect and each read gets a bound.
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(120))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to build the HTTP client to copy the sandbox image".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    // `path` is `<vendor project>/<repository>/<alien project>/<image>`; the proxy serves it by name.
    let source = CopySource {
        base_url: manager_url,
        repository: &path,
        token,
    };
    // The service override lets tests point the registry host at a local server.
    let destination_base = gcp_config.get_service_endpoint(
        "artifactregistry-docker",
        &format!("https://{}-docker.pkg.dev", gcp_config.region),
    );
    let destination_repository = format!("{}/{repository}/{sandbox_id}", gcp_config.project_id);
    let destination = CopyDestination {
        base_url: &destination_base,
        repository: &destination_repository,
        access_token: &access_token,
    };
    // The tag keeps the digest through the repository's cleanup of untagged versions.
    let digest =
        crate::sandbox::gcp_image_copy::resolve_digest(&http, &source, &reference, sandbox_id)
            .await?;
    let keep_tag = crate::sandbox::gcp_image_copy::keep_tag_for(&digest);
    copy_image(
        &http,
        &source,
        &destination,
        &digest,
        Some(&keep_tag),
        sandbox_id,
    )
    .await?;
    spec.image_uri = format!(
        "{destination_base_host}/{destination_repository}@{digest}",
        destination_base_host = destination_base
            .split_once("://")
            .map_or(destination_base.as_str(), |(_, host)| host)
    );
    info!(id = %sandbox_id, "Copied the sandbox image into the deployment's own project");
    Ok(())
}

/// Check the sandbox's dedicated repository exists and is readable. Setup creates it (direct
/// setup or the Terraform module), never this controller: at runtime it holds the management
/// identity, which cannot create repositories. A missing or unreadable repository means the
/// install was set up before sandbox images were copied into the project, so the answer is
/// SETUP_RERUN_REQUIRED, before anything is copied or created.
async fn require_image_repository(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    repository: &str,
) -> Result<()> {
    let gcp_config = ctx.get_gcp_config()?;
    let client = ctx
        .service_provider
        .get_gcp_artifact_registry_client(gcp_config)?;
    let error = match client
        .get_repository(
            gcp_config.project_id.clone(),
            gcp_config.region.clone(),
            repository.to_string(),
        )
        .await
    {
        Ok(_) => return Ok(()),
        Err(e) => e,
    };
    let missing = matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    );
    let denied = matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteAccessDenied { .. })
    );
    if !missing && !denied {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!("Failed to read the sandbox image repository '{repository}': {error}"),
            resource_id: Some(sandbox_id.to_string()),
        }));
    }
    // A management identity that may read repositories project-wide sees a missing repository as
    // not found, one that may not sees it as denied. Both mean setup has not made it.
    if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
        && ctx.deployment_config.setup_run
    {
        // Setup's scaffolding makes it in this same run.
        return Err(AlienError::new(ErrorData::DependencyNotReady {
            resource_id: sandbox_id.to_string(),
            dependency_id: repository.to_string(),
        }));
    }
    let how = if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup {
        "run the deployment's setup again"
    } else {
        "update the setup (apply the current setup package)"
    };
    Err(AlienError::new(ErrorData::SetupRerunRequired {
        resource_id: sandbox_id.to_string(),
        message: format!(
            "the sandbox image repository '{repository}' is missing or not readable. Setup \
             creates it and the manager's write access to it; {how}. Nothing was copied or \
             created"
        ),
    }))
}

/// Create the template under `engine` and answer with the operation to poll. A protected install
/// has its verifier create it, so the image a sandbox boots is the signed one; any other creates
/// it directly.
async fn create_template(
    ctx: &ResourceControllerContext<'_>,
    client: &std::sync::Arc<dyn alien_gcp_clients::gcp::agent_platform::AgentPlatformApi>,
    sandbox_id: &str,
    engine: &str,
    body: SandboxEnvironmentTemplate,
) -> Result<Option<String>> {
    let wire =
        serde_json::to_value(&body)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "The sandbox template does not serialize".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            })?;
    if let Some(name) = crate::core::verifier::gcp_sandbox_operation(
        ctx,
        sandbox_id,
        crate::core::verifier::ResourceWrite::CreateSandboxTemplate {
            engine: engine.to_string(),
            body: wire,
        },
    )
    .await?
    {
        return Ok(Some(name));
    }
    Ok(client
        .create_template(engine, body)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create sandbox template under engine '{engine}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?
        .name)
}

/// The network attachment an `internet` sandbox's template names: the one setup recorded on the
/// deployment's Network. `None` for any other egress. Setup releases that predate the gateway
/// recorded none, so an `internet` sandbox there is refused with a request to re-run setup.
fn gateway_attachment(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
) -> Result<Option<String>> {
    use crate::core::state_utils::StackResourceStateExt;
    if !uses_gateway_path(sandbox) {
        return Ok(None);
    }
    let no_gateway = |message: &str| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "sandbox '{}' has egress internet, but {message}; re-run setup to create it",
                sandbox.id()
            ),
            resource_id: Some(sandbox.id().to_string()),
        })
    };
    let network_id = ctx
        .desired_stack
        .resources()
        .find_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Network>()
                .map(|_| id.to_string())
        })
        .ok_or_else(|| no_gateway("the deployment has no network"))?;
    let state = ctx.state.resources.get(&network_id).ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: sandbox.id().to_string(),
            dependency_id: network_id.clone(),
        })
    })?;
    let network = state.get_internal_controller_typed::<crate::network::GcpNetworkController>()?;
    network
        .sandbox_network
        .map(|declared| declared.network_attachment)
        .map(Some)
        .ok_or_else(|| no_gateway("setup created no network attachment and gateway for it"))
}

/// Refuses a template the project's placement cannot host, before it is created.
///
/// Reading placement needs `compute.networkAttachments.list`, which setup grants with the gateway.
/// A default-placement sandbox on a deployment that predates it cannot be read, and must not start
/// failing for that, so there the check is best effort. For `internet` it is required.
async fn check_project_placement(
    ctx: &ResourceControllerContext<'_>,
    client: &std::sync::Arc<dyn alien_gcp_clients::gcp::agent_platform::AgentPlatformApi>,
    sandbox_id: &str,
    attachment: Option<&str>,
) -> Result<()> {
    // While Google Cloud `internet` is not available no template asks for Private Service
    // Connect, so a default-network template is created exactly as before, with no extra reads.
    if attachment.is_none() && !alien_core::gcp_sandbox_gateway::internet_egress_available() {
        return Ok(());
    }
    let gcp_config = ctx.get_gcp_config()?;
    let wanted = if attachment.is_some() {
        Placement::PrivateServiceConnect
    } else {
        Placement::Default
    };
    let read = async {
        let mut templates = Vec::new();
        for engine in client
            .list_engines()
            .await
            .map_err(|error| error.into_generic())?
        {
            if let Some(name) = engine.name.as_deref() {
                templates.extend(
                    client
                        .list_templates(last_segment(name))
                        .await
                        .map_err(|error| error.into_generic())?,
                );
            }
        }
        let attachments = ctx
            .service_provider
            .get_gcp_compute_client(gcp_config)
            .map_err(|error| error.into_generic())?
            .list_network_attachments(gcp_config.region.clone())
            .await
            .map_err(|error| error.into_generic())?;
        Ok::<_, AlienError<alien_error::GenericError>>((templates, attachments))
    };
    let (templates, attachments) = match read.await {
        Ok(facts) => facts,
        Err(error) if wanted == Placement::Default => {
            warn!(id=%sandbox_id, error=%error, "could not read the project's sandbox placement; creating the template anyway");
            return Ok(());
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: "Failed to read the project's sandbox placement".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };
    gcp_placement::check_placement(
        sandbox_id,
        wanted,
        attachment.map(last_segment),
        &templates,
        &attachments,
    )
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
    pub(crate) max_lifetime_seconds: Option<u32>,
    /// Original declaration: the cloud's boolean cannot distinguish Internet from legacy Allow.
    #[serde(default)]
    pub(crate) egress: Option<alien_core::SandboxEgress>,
    #[serde(default)]
    pub(crate) default_policy: Option<alien_core::EgressPolicy>,
    /// Egress switch the serving template reports, carried into the binding and re-read on every
    /// Ready. `None` is state saved before the field existed; the binding reads it as deny.
    #[serde(default)]
    pub(crate) allow_egress: Option<bool>,
    /// Preview ports the declaration names, carried into the binding.
    #[serde(default)]
    pub(crate) preview_ports: Vec<u16>,
    /// Identity setup created to sign the preview edge credential; imported, never created here,
    /// and carried into the binding so a deployed workload can sign.
    #[serde(default)]
    pub(crate) preview_service_account: Option<String>,
    /// What a direct setup run created around the engine (signing identity, signing role, engine
    /// grants), so teardown removes exactly that. Empty when a setup package made them.
    #[serde(default)]
    pub(crate) engine_setup: crate::sandbox::gcp_engine_setup::EngineSetupRecord,
    /// Fingerprint of the grant inputs the last direct setup run reconciled, so a rerun updates
    /// only when a profile or caller changed.
    #[serde(default)]
    pub(crate) reconciled_grants: Option<String>,
    /// Full path of the network attachment an `internet` template names, from setup.
    #[serde(default)]
    pub(crate) network_attachment: Option<String>,
    /// Polls of the attachment while it waits for Agent Platform's connection.
    #[serde(default)]
    pub(crate) attachment_polls: u32,
    /// The binding an `internet` sandbox publishes: the gateway, its signing key and replicas.
    #[serde(default)]
    pub(crate) egress_gateway: Option<alien_core::bindings::SandboxEgressBinding>,
    /// Public signing material; the private key stays in the secrets vault and is never read
    /// back. Other internet sandboxes take the key from the owner's copy of this field.
    #[serde(default)]
    pub(crate) egress_public_key: Option<super::egress::GatewayPublicKey>,
    /// Digest of the gateway configuration this owner last published.
    #[serde(default)]
    pub(crate) egress_config_digest: Option<String>,
    /// The owner has written the gateway's replica secret, which it never reads.
    #[serde(default)]
    pub(crate) egress_replica_secret_ready: bool,
    /// The key is initialized and the configuration published, so the template can be created.
    #[serde(default)]
    pub(crate) egress_ready: bool,
    /// `address:port` of the deployment's private endpoint, once it exists. A template with a
    /// network attachment is reached only through it.
    #[serde(default)]
    pub(crate) private_endpoint: Option<String>,
    #[serde(default)]
    pub(crate) endpoint_progress: super::gcp_private_endpoint::EndpointProgress,
}

impl super::egress::RecordsEgressKey for GcpAgentPlatformTemplateController {
    fn egress_public_key(&self) -> Option<&super::egress::GatewayPublicKey> {
        self.egress_public_key.as_ref()
    }
}

#[controller]
impl GcpAgentPlatformTemplateController {
    fn has_runtime_setup_inputs(&self) -> bool {
        // Setup registers a region but never creates the runtime-owned template.
        self.region.is_some()
    }

    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        // An `internet` sandbox's gateway key and configuration come before its template: the
        // sandboxes it creates later need both.
        if uses_gateway_path(config) && !self.egress_ready {
            return Ok(HandlerAction::Continue {
                state: InitializingEgressKey,
                suggested_delay: None,
            });
        }
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx
            .service_provider
            .get_gcp_agent_platform_client(gcp_config)?;

        // The engine id is server-assigned; read it from the engine dependency's state, keyed by
        // the `{id}-engine` convention the engine mutation writes.
        let engine_ref = ResourceRef::new(
            GcpAgentPlatformEngine::RESOURCE_TYPE,
            GcpAgentPlatformEngine::id_for_sandbox(&config.id),
        );
        let engine = ctx
            .require_dependency::<GcpAgentPlatformEngineController>(&engine_ref)?
            .engine_id
            .ok_or_else(|| missing_state(&config.id, "engine id from the engine dependency"))?;
        let display_name = format!("{}-{}", ctx.resource_prefix, config.id);
        let attachment = gateway_attachment(ctx, config)?;
        let mut body = build_template_body(
            config,
            &display_name,
            ctx.deployment_config,
            attachment.as_deref(),
        )?;
        copy_image_into_own_project(ctx, &config.id, &mut body).await?;
        check_project_placement(ctx, &client, &config.id, attachment.as_deref()).await?;
        self.network_attachment = attachment;

        self.engine = Some(engine.clone());
        self.project_id = Some(gcp_config.project_id.clone());
        self.region = Some(gcp_config.region.clone());
        self.max_lifetime_seconds = config.lifecycle.max_lifetime_seconds;
        self.preview_ports = config.preview_ports.clone();
        self.egress = Some(config.egress.clone());
        self.default_policy = config.default_policy.clone();

        // A direct setup run has no package to place the grants and the preview signing identity
        // around the engine, so it places them here, the same ones the package would.
        if ctx.is_direct_setup_run() {
            let signer = crate::sandbox::gcp_engine_setup::set_up_engine_access(
                ctx,
                config,
                &engine,
                &mut self.engine_setup,
            )
            .await?;
            if signer.is_some() {
                self.preview_service_account = signer;
            }
            self.reconciled_grants =
                Some(crate::sandbox::grant_fingerprint::grant_inputs_fingerprint(
                    ctx,
                    crate::core::ResourcePermissionsHelper::get_gcp_management_service_account_email(ctx),
                )?);
        }

        info!(id=%config.id, engine=%engine, "Creating sandbox environment template");
        let operation_name = create_template(ctx, &client, &config.id, &engine, body).await?;
        self.pending_operation = Some(require_operation_name(operation_name, &config.id)?);
        Ok(HandlerAction::Continue {
            state: AwaitingTemplateOperation,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = InitializingEgressKey,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn initializing_egress_key(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.egress_gateway =
            Some(super::gcp_egress::initialize_key(ctx, &mut self.egress_public_key).await?);
        Ok(HandlerAction::Continue {
            state: PublishingEgressConfig,
            suggested_delay: None,
        })
    }

    #[handler(
        state = PublishingEgressConfig,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn publishing_egress_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        super::gcp_egress::publish_config(
            ctx,
            self.egress_gateway.as_ref(),
            self.egress_public_key.as_ref(),
            &mut self.egress_config_digest,
            &mut self.egress_replica_secret_ready,
        )
        .await?;
        self.egress_ready = true;
        Ok(HandlerAction::Continue {
            state: CreateStart,
            suggested_delay: None,
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

        if template.state.as_deref() == Some(TEMPLATE_FAILED) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "template '{pending}' FAILED without a reason. Agent Platform reports this \
                     when the image's container exits at start: its ENTRYPOINT must keep running \
                     in the foreground and serve the declared ports, and the image must be \
                     linux/amd64. {}",
                    gcp_placement::PLACEMENT_HINT
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        if template.state.as_deref() != Some(TEMPLATE_ACTIVE) {
            return Ok(HandlerAction::Stay {
                max_times: Some(150),
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        // A template on the gateway path is only useful once Agent Platform has connected to the
        // attachment. It goes ACTIVE either way: an attachment that failed its permission check
        // earlier stays unconnected for good and the template says nothing about it.
        if let Some(attachment) = self.network_attachment.clone() {
            let gcp = ctx.get_gcp_config()?;
            let attachments = ctx
                .service_provider
                .get_gcp_compute_client(gcp)?
                .list_network_attachments(gcp.region.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to read the sandbox network attachment".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            match gcp_placement::connection(last_segment(&attachment), &attachments) {
                Connection::Connected(_) => {}
                Connection::Missing => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "network attachment '{attachment}' does not exist; re-run setup"
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
                Connection::NotYet => {
                    self.attachment_polls += 1;
                    if self.attachment_polls > ATTACHMENT_CONNECT_POLLS {
                        return Err(gcp_placement::never_connected(
                            &config.id,
                            last_segment(&attachment),
                        ));
                    }
                    return Ok(HandlerAction::Stay {
                        max_times: None,
                        suggested_delay: Some(Duration::from_secs(2)),
                    });
                }
            }
            self.attachment_polls = 0;
        }

        // Read off the template rather than the config: desired config can change while this
        // template is being built, and the binding must describe the one that serves.
        let allow_egress = served_internet_access(&template);
        // The new template is live; only now does it become the serving one, so the reap that
        // follows can delete the old without a window where sessions point at a deleted template.
        self.template_id = Some(pending);
        self.pending_template_id = None;
        self.allow_egress = Some(allow_egress);

        Ok(HandlerAction::Continue {
            state: CreatingPrivateEndpoint,
            suggested_delay: None,
        })
    }

    /// A template with an attachment has private ingress: its sandboxes are reached through an
    /// endpoint in the VPC, which needs the tenant's service attachment, which a sandbox reveals.
    #[handler(
        state = CreatingPrivateEndpoint,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_private_endpoint(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        if self.network_attachment.is_none() || self.private_endpoint.is_some() {
            return Ok(HandlerAction::Continue {
                state: ReapingOldTemplates,
                suggested_delay: None,
            });
        }
        let (engine, template_id) = self.engine_and_template(&config.id)?;
        let project = self.project_id.clone().unwrap_or_default();
        let region = self.region.clone().unwrap_or_default();
        let template = format!(
            "projects/{project}/locations/{region}/reasoningEngines/{engine}/sandboxEnvironmentTemplates/{template_id}"
        );
        let network = super::gcp_egress::workload_network(ctx)?;
        let inputs = super::gcp_private_endpoint::Inputs {
            resource_id: &config.id,
            resource_prefix: ctx.resource_prefix,
            region: &region,
            engine: &engine,
            template: &template,
            subnetwork: &network.subnetwork,
            network: &network.network,
        };
        let gcp = ctx.get_gcp_config()?;
        let compute = ctx.service_provider.get_gcp_compute_client(gcp)?;
        let platform = ctx.service_provider.get_gcp_agent_platform_client(gcp)?;
        match super::gcp_private_endpoint::run(
            compute.as_ref(),
            platform.as_ref(),
            &inputs,
            &mut self.endpoint_progress,
        )
        .await?
        {
            super::gcp_private_endpoint::Step::Ready(endpoint) => {
                self.private_endpoint = Some(endpoint);
                Ok(HandlerAction::Continue {
                    state: ReapingOldTemplates,
                    suggested_delay: None,
                })
            }
            super::gcp_private_endpoint::Step::Wait => Ok(HandlerAction::Stay {
                max_times: Some(150),
                suggested_delay: Some(Duration::from_secs(4)),
            }),
        }
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
        self.allow_egress = Some(served_internet_access(&template));
        self.egress = Some(config.egress.clone());

        ctx.emit_heartbeat(alien_core::ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Sandbox::RESOURCE_TYPE,
            controller_platform: alien_core::Platform::Gcp,
            backend: alien_core::HeartbeatBackend::Gcp,
            observed_at: chrono::Utc::now(),
            data: alien_core::ResourceHeartbeatData::Sandbox(
                alien_core::SandboxHeartbeatData::GcpAgentPlatform(
                    alien_core::GcpAgentPlatformSandboxHeartbeatData {
                        status: alien_core::SandboxHeartbeatStatus::default(),
                        engine,
                        template_id,
                    },
                ),
            ),
            raw: vec![],
        });

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
        self.egress = Some(config.egress.clone());
        self.default_policy = config.default_policy.clone();
        self.preview_ports = config.preview_ports.clone();
        let previous = ctx.previous_resource_config::<Sandbox>()?;

        // A direct setup run owns the grants around the engine, so each setup run places the ones
        // a newly added profile or remote caller needs, as applying the package again does.
        if ctx.is_direct_setup_run() {
            if let Some(engine) = self.engine.clone() {
                let signer = crate::sandbox::gcp_engine_setup::set_up_engine_access(
                    ctx,
                    config,
                    &engine,
                    &mut self.engine_setup,
                )
                .await?;
                if signer.is_some() {
                    self.preview_service_account = signer;
                }
                self.reconciled_grants =
                    Some(crate::sandbox::grant_fingerprint::grant_inputs_fingerprint(
                        ctx,
                        crate::core::ResourcePermissionsHelper::get_gcp_management_service_account_email(ctx),
                    )?);
            }
        }

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
        let attachment = gateway_attachment(ctx, config)?;
        let mut body = build_template_body(
            config,
            &display_name,
            ctx.deployment_config,
            attachment.as_deref(),
        )?;
        copy_image_into_own_project(ctx, &config.id, &mut body).await?;
        check_project_placement(ctx, &client, &config.id, attachment.as_deref()).await?;
        self.network_attachment = attachment;

        info!(id=%config.id, "sandbox template body changed; creating a replacement");
        let operation_name = create_template(ctx, &client, &config.id, &engine, body).await?;

        // The old template stays in `template_id` and keeps serving; the reap after ACTIVE removes
        // it. Routing through the create flow's await states keeps one mutable op per state.
        self.pending_operation = Some(require_operation_name(operation_name, &config.id)?);
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

        if self.private_endpoint.is_some() || self.endpoint_progress.probe_requested {
            let compute = ctx.service_provider.get_gcp_compute_client(gcp_config)?;
            // The endpoint is the deployment's, shared by every internet sandbox: it goes with
            // the last one. A failed delete fails the step, so it is retried rather than left
            // in the subnet, where it would block the network's own teardown.
            let shared = super::gcp_private_endpoint::endpoint_still_needed(ctx, &config.id);
            super::gcp_private_endpoint::delete(
                compute.as_ref(),
                client.as_ref(),
                &engine,
                ctx.resource_prefix,
                &gcp_config.region,
                &config.id,
                !shared,
            )
            .await?;
        }

        super::gcp_egress::delete_shared_secrets(ctx, &config.id, self.egress_public_key.is_some())
            .await?;
        self.egress_public_key = None;

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

        if self.engine_setup != Default::default() {
            crate::sandbox::gcp_engine_setup::tear_down_engine_access(
                ctx,
                &engine,
                &self.engine_setup,
            )
            .await?;
        } else if config.needs_gcp_edge_signer()
            && ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
        {
            // The record may have been dropped by an older manager rewriting the state; the
            // signer's own description still names this deployment. Only a direct-setup
            // deployment with preview ports or an internet sandbox can have a signer, and a package install's
            // management identity cannot read service accounts (GCP answers 403).
            crate::sandbox::gcp_engine_setup::remove_marked_signer(ctx, &config.id).await?;
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

    /// A direct setup run places the grants around the engine for the profiles and callers the
    /// stack declares now. It updates once when those inputs differ from what it last placed,
    /// and not at all in a steady state.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if !(matches!(self.state, GcpAgentPlatformTemplateState::Ready)
            && ctx.deployment_config.setup_run
            && ctx.is_direct_setup_run())
        {
            return Ok(false);
        }
        Ok(self.reconciled_grants.as_deref()
            != Some(
                crate::sandbox::grant_fingerprint::grant_inputs_fingerprint(
                    ctx,
                    crate::core::ResourcePermissionsHelper::get_gcp_management_service_account_email(ctx),
                )?
                .as_str(),
            ))
    }

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
        let mut binding = SandboxBinding::gcp_agent_platform(
            BindingValue::value(engine_name),
            BindingValue::value(template_name),
            BindingValue::value(region.clone()),
            self.max_lifetime_seconds,
            self.allow_egress.unwrap_or(false),
        );
        if let SandboxBinding::GcpAgentPlatform(value) = &mut binding {
            // Old saved rows predate the declaration and keep the legacy boolean interpretation.
            if let Some(egress) = &self.egress {
                value.egress = Some(egress.clone().into());
            }
            // An `internet` sandbox publishes its gateway: the signing key and the replicas.
            if let Some(gateway) = &self.egress_gateway {
                value.egress = Some(gateway.clone());
            }
            value.private_endpoint = self.private_endpoint.clone();
            value.default_policy = self.default_policy.clone();
            value.preview_ports = self.preview_ports.clone();
            value.preview_service_account = self
                .preview_service_account
                .clone()
                .map(BindingValue::value);
        }

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
        self.max_lifetime_seconds = None;
        self.allow_egress = None;
        self.preview_ports = Vec::new();
        self.egress = None;
        self.default_policy = None;
        self.engine_setup = Default::default();
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
            max_lifetime_seconds: None,
            default_policy: None,
            egress: None,
            allow_egress: None,
            preview_ports: Vec::new(),
            preview_service_account: None,
            engine_setup: Default::default(),
            reconciled_grants: None,
            network_attachment: None,
            attachment_polls: 0,
            egress_gateway: None,
            egress_public_key: None,
            egress_config_digest: None,
            egress_replica_secret_ready: false,
            egress_ready: false,
            private_endpoint: None,
            endpoint_progress: Default::default(),
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
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use alien_core::{SandboxEgress, SandboxLifecyclePolicy};
    use alien_gcp_clients::agent_platform::MockAgentPlatformApi;
    use alien_gcp_clients::gcp::compute::MockComputeApi;
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
                bundled_from: None,
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: ttl,
                idle_pause_seconds: None,
                ..Default::default()
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
            ingress_control_config: None,
            state: Some(TEMPLATE_ACTIVE.to_string()),
            extra: Default::default(),
        }
    }

    /// Serves every template ACTIVE with the egress of the last create body, as the API reports it:
    /// a false `internetAccess` is omitted.
    fn serve_created_egress(m: &mut MockAgentPlatformApi) {
        let created = Arc::new(std::sync::Mutex::new(None));
        let recorded = created.clone();
        m.expect_create_template().returning(move |_, body| {
            *recorded.lock().expect("unpoisoned") =
                body.egress_control_config
                    .map(|egress| EgressControlConfig {
                        internet_access: egress.internet_access.filter(|open| *open),
                        ..egress
                    });
            Ok(pending_op())
        });
        m.expect_get_template().returning(move |_, id| {
            Ok(SandboxEnvironmentTemplate {
                egress_control_config: created.lock().expect("unpoisoned").clone(),
                ..active_template(id)
            })
        });
    }

    /// A mock that carries one sandbox from create through a heartbeat and a clean delete.
    fn happy_client() -> Arc<MockAgentPlatformApi> {
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        serve_created_egress(&mut m);
        m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
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
        // Teardown looks for a preview signer this deployment may have made; none exists here.
        let mut iam = alien_gcp_clients::iam::MockIamApi::new();
        iam.expect_get_service_account().returning(|name| {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "ServiceAccount".to_string(),
                    resource_name: name,
                },
            ))
        });
        let iam = Arc::new(iam);
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let compute: Arc<dyn alien_gcp_clients::gcp::compute::ComputeApi> = {
            let mut compute = MockComputeApi::new();
            compute
                .expect_list_network_attachments()
                .returning(|_| Ok(Vec::new()));
            Arc::new(compute)
        };
        provider
            .expect_get_gcp_compute_client()
            .returning(move |_| Ok(compute.clone()));
        Arc::new(provider)
    }

    /// A project that has never hosted a sandbox: no engines, so no templates.
    fn serve_empty_project(m: &mut MockAgentPlatformApi) {
        m.expect_list_engines().returning(|| Ok(Vec::new()));
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
            // The engine the sandbox depends on, already provisioned: create_start reads its
            // server-assigned id ("eng") from here rather than fabricating one.
            .with_dependency(
                GcpAgentPlatformEngine::new(GcpAgentPlatformEngine::id_for_sandbox("agent-sbx"))
                    .build(),
                GcpAgentPlatformEngineController::mock_ready("eng"),
            )
            .build()
            .await
            .expect("executor builds")
    }

    /// A controller must round-trip by tag: the executor persists and reloads state between
    /// reconciles, and a missing by-tag arm fails above the handler layer with no cause to read.
    #[test]
    fn controller_round_trips_by_tag() {
        use crate::core::{deserialize_controller, serialize_controller, ResourceController};

        let controller = GcpAgentPlatformTemplateController {
            engine: Some("eng".to_string()),
            template_id: Some("tpl1".to_string()),
            ..Default::default()
        };
        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "GcpAgentPlatformTemplateController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
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
        assert_eq!(
            controller.engine.as_deref(),
            Some("eng"),
            "the template is created under the engine's real id from the dependency"
        );

        executor.delete().expect("delete is accepted");
        executor
            .run_until_terminal()
            .await
            .expect("delete runs to terminal");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Ready must re-report on every reconcile, not once on the way in: emitting only on entry
    /// stops reporting after create.
    #[tokio::test]
    async fn every_ready_reconcile_emits_one_healthy_heartbeat() {
        use alien_core::{
            ObservedHealth, ProviderLifecycleState, ResourceHeartbeatData, SandboxHeartbeatData,
        };

        let provider = provider_with(happy_client());
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None),
            provider,
        )
        .await;

        executor
            .run_until_terminal()
            .await
            .expect("create runs to a steady state");
        assert_eq!(executor.status(), ResourceStatus::Running);

        for reconcile in 1..=3 {
            executor.step().await.expect("a Ready reconcile succeeds");
            assert_eq!(
                executor.status(),
                ResourceStatus::Running,
                "reconcile {reconcile} stays Ready"
            );

            let heartbeats = executor.last_heartbeats();
            assert_eq!(
                heartbeats.len(),
                1,
                "reconcile {reconcile} emits exactly one heartbeat"
            );
            let heartbeat = &heartbeats[0];
            assert_eq!(heartbeat.resource_id, "agent-sbx");
            assert_eq!(heartbeat.resource_type, Sandbox::RESOURCE_TYPE);
            assert_eq!(heartbeat.controller_platform, Platform::Gcp);
            assert_eq!(heartbeat.backend, alien_core::HeartbeatBackend::Gcp);
            assert!(heartbeat.raw.is_empty(), "no raw payload is collected");

            let ResourceHeartbeatData::Sandbox(SandboxHeartbeatData::GcpAgentPlatform(data)) =
                &heartbeat.data
            else {
                panic!(
                    "expected a GCP Agent Platform sandbox heartbeat, got {:?}",
                    heartbeat.data
                );
            };
            assert_eq!(
                data.status.health,
                ObservedHealth::Healthy,
                "an ACTIVE template was read, so health is measured healthy"
            );
            assert_eq!(
                data.status.lifecycle,
                ProviderLifecycleState::Running,
                "an ACTIVE template maps to lifecycle running"
            );
            assert_eq!(data.status.message, None);
            assert!(!data.status.stale);
            assert!(!data.status.partial);
            assert!(data.status.collection_issues.is_empty());
            assert_eq!(data.engine, "eng");
            assert_eq!(data.template_id, "tpl1");
        }
    }

    #[tokio::test]
    async fn create_delete_deny_egress_default_limits() {
        create_then_delete(sandbox_with(
            SandboxEgress::None,
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
            SandboxEgress::None,
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
            sandbox_with(SandboxEgress::None, "ubuntu:24.04", Some(3600), None),
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
        let binding: alien_core::bindings::SandboxBinding = serde_json::from_value(params)
            .expect("binding parses back to the Agent Platform binding type");

        match binding {
            alien_core::bindings::SandboxBinding::GcpAgentPlatform(b) => {
                let region = b
                    .region
                    .into_value("gcp-agent-platform", "region")
                    .expect("region is a literal in a test");
                assert_eq!(region, "us-central1");
                assert_eq!(b.max_lifetime_seconds, Some(3600));
                let template = b
                    .template
                    .into_value("gcp-agent-platform", "template")
                    .expect("template is a literal in a test");
                assert!(
                    template.ends_with("/sandboxEnvironmentTemplates/tpl1"),
                    "the binding points at the ACTIVE template: {template}"
                );
                assert!(
                    template.contains("/reasoningEngines/eng/"),
                    "the binding hangs the template under the engine's real id: {template}"
                );
            }
            other => panic!("expected a GCP Agent Platform binding, got {other:?}"),
        }
    }

    /// The binding a deployed workload reads must carry what preview needs: the declared ports
    /// and the signing identity setup created. Without the identity a deployed sandbox's
    /// `preview()` fails at runtime.
    #[test]
    fn the_binding_carries_the_preview_ports_and_the_signer_setup_created() {
        use crate::core::ResourceController;
        let mut controller = GcpAgentPlatformTemplateController::mock_ready("eng", "tpl1");
        controller.preview_ports = vec![8080, 8081];
        controller.preview_service_account =
            Some("agents-preview@p.iam.gserviceaccount.com".into());
        let params = controller
            .get_binding_params()
            .expect("binding serializes")
            .expect("a ready template has a binding");
        let alien_core::bindings::SandboxBinding::GcpAgentPlatform(b) =
            serde_json::from_value(params).expect("parses")
        else {
            panic!("expected a GCP Agent Platform binding");
        };
        assert_eq!(b.preview_ports, vec![8080, 8081]);
        assert_eq!(
            b.preview_service_account
                .expect("signer present")
                .into_value("b", "previewServiceAccount")
                .expect("literal"),
            "agents-preview@p.iam.gserviceaccount.com"
        );
    }

    #[test]
    fn old_saved_open_template_still_publishes_a_legacy_binding() {
        use crate::core::{deserialize_controller, serialize_controller, ResourceController};
        let mut controller = GcpAgentPlatformTemplateController::mock_ready("eng", "tpl1");
        controller.allow_egress = Some(true);
        let mut saved = serialize_controller(&controller).unwrap();
        saved.as_object_mut().unwrap().remove("egress").unwrap();
        let restored = deserialize_controller(saved).unwrap();
        let binding: alien_core::SandboxBinding =
            serde_json::from_value(restored.get_binding_params().unwrap().unwrap()).unwrap();
        assert_eq!(
            binding.egress().unwrap().declaration(),
            &SandboxEgress::Allow
        );
    }

    #[tokio::test]
    async fn binding_preserves_the_declaration_across_create_and_updates() {
        use crate::core::ResourceController;
        for initial in [SandboxEgress::None, SandboxEgress::Allow] {
            let mut executor = build_executor(
                sandbox_with(initial.clone(), "ubuntu:24.04", None, None),
                provider_with(happy_client()),
            )
            .await;
            executor.run_until_terminal().await.unwrap();
            let declaration = |executor: &SingleControllerExecutor| {
                let params = executor
                    .internal_state::<GcpAgentPlatformTemplateController>()
                    .unwrap()
                    .get_binding_params()
                    .unwrap()
                    .unwrap();
                let binding: alien_core::SandboxBinding = serde_json::from_value(params).unwrap();
                binding.egress().unwrap().declaration().clone()
            };
            assert_eq!(declaration(&executor), initial);
            let updated = if initial == SandboxEgress::None {
                SandboxEgress::Allow
            } else {
                SandboxEgress::None
            };
            executor
                .update(sandbox_with(updated.clone(), "ubuntu:24.04", None, None))
                .unwrap();
            executor.run_until_terminal().await.unwrap();
            assert_eq!(declaration(&executor), updated);
            assert_eq!(
                executor
                    .internal_state::<GcpAgentPlatformTemplateController>()
                    .unwrap()
                    .template_id
                    .as_deref(),
                Some("tpl1")
            );
        }
    }

    /// The binding reports the egress of the template that is serving: the declared mode after
    /// create, the new mode once a replacement that flips the switch goes ACTIVE, and on Ready what
    /// the template itself says, whatever the desired config asks for.
    #[tokio::test]
    async fn the_binding_reports_the_serving_templates_egress() {
        use crate::core::ResourceController;
        let allow_egress = |executor: &SingleControllerExecutor| {
            let params = executor
                .internal_state::<GcpAgentPlatformTemplateController>()
                .expect("downcasts")
                .get_binding_params()
                .expect("binding serializes")
                .expect("a running template has a binding");
            match serde_json::from_value(params).expect("binding parses") {
                alien_core::bindings::SandboxBinding::GcpAgentPlatform(b) => b.allow_egress,
                other => panic!("expected a GCP Agent Platform binding, got {other:?}"),
            }
        };

        for egress in [SandboxEgress::Allow, SandboxEgress::None] {
            let mut executor = build_executor(
                sandbox_with(egress.clone(), "ubuntu:24.04", None, None),
                provider_with(happy_client()),
            )
            .await;
            executor.run_until_terminal().await.expect("create runs");
            assert_eq!(
                allow_egress(&executor),
                egress == SandboxEgress::Allow,
                "{egress:?}"
            );
        }

        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        serve_created_egress(&mut m);
        m.expect_get_operation().returning(|_| Ok(done_op("tpl2")));
        m.expect_list_templates()
            .returning(|_| Ok(vec![active_template("tpl1"), active_template("tpl2")]));
        m.expect_delete_template().returning(|_, _| Ok(()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox_with(
                SandboxEgress::None,
                "ubuntu:24.04",
                None,
                None,
            ))
            .controller(GcpAgentPlatformTemplateController::mock_ready(
                "eng", "tpl1",
            ))
            .platform(Platform::Gcp)
            .service_provider(provider_with(Arc::new(m)))
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");
        executor
            .update(sandbox_with(
                SandboxEgress::Allow,
                "ubuntu:24.04",
                None,
                None,
            ))
            .expect("update accepted");
        executor.run_until_terminal().await.expect("replace runs");
        assert!(allow_egress(&executor), "the replacement opened egress");

        // Desired config is the opposite of what serves in both cases, and state saved before the
        // field existed reads as deny until the first Ready.
        for (served, desired) in [(true, SandboxEgress::None), (false, SandboxEgress::Allow)] {
            let mut m = MockAgentPlatformApi::new();
            serve_empty_project(&mut m);
            m.expect_get_template().returning(move |_, id| {
                Ok(SandboxEnvironmentTemplate {
                    egress_control_config: Some(EgressControlConfig {
                        internet_access: served.then_some(true),
                        network_attachment: None,
                        extra: Default::default(),
                    }),
                    ..active_template(id)
                })
            });
            let mut executor = SingleControllerExecutor::builder()
                .resource(sandbox_with(desired, "ubuntu:24.04", None, None))
                .controller(GcpAgentPlatformTemplateController::mock_ready(
                    "eng", "tpl1",
                ))
                .platform(Platform::Gcp)
                .service_provider(provider_with(Arc::new(m)))
                .with_test_dependencies()
                .build()
                .await
                .expect("executor builds");
            assert!(
                !allow_egress(&executor),
                "state saved without the field reads as deny"
            );
            executor.step().await.expect("a Ready reconcile succeeds");
            assert_eq!(allow_egress(&executor), served, "served {served}");
        }
    }

    // ---- 2. Update flow: no-op when the body is unchanged. -------------------------------------

    #[tokio::test]
    async fn update_with_unchanged_body_creates_no_template() {
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        // The heartbeat still reads the template; a replace would create, and it must not.
        m.expect_get_template()
            .returning(|_, id| Ok(active_template(id)));
        m.expect_create_template().never();
        let provider = provider_with(Arc::new(m));

        let resource = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
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
        serve_empty_project(&mut m);
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
            .resource(sandbox_with(SandboxEgress::Allow, "old:v1", None, None))
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
            .update(sandbox_with(SandboxEgress::Allow, "new:v2", None, None))
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
        serve_empty_project(&mut m);
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
                SandboxEgress::None,
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
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        let provider = provider_with(Arc::new(m));
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None),
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

    /// A container that exits leaves the template FAILED with no reason. The create must stop at
    /// once and say what usually causes it, not poll a template that will never become ACTIVE.
    #[tokio::test]
    async fn a_failed_template_stops_the_create_and_names_the_likely_cause() {
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        m.expect_create_template()
            .returning(|_, _| Ok(pending_op()));
        m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
        m.expect_get_template().returning(|_, id| {
            Ok(SandboxEnvironmentTemplate {
                state: Some("FAILED".to_string()),
                ..active_template(id)
            })
        });
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None),
            provider_with(Arc::new(m)),
        )
        .await;
        let result = executor.run_until_terminal().await;
        let message = format!(
            "{:?}",
            result.expect_err("a FAILED template must fail the create")
        );
        assert!(message.contains("exits"), "{message}");
        assert!(message.contains("foreground"), "{message}");
    }

    #[tokio::test]
    async fn create_carries_the_declared_image_limits_and_egress() {
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
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

    /// Asserts the serialized body rather than the struct because `protocol` is skipped when it is
    /// `None`, so a struct that looks right can still put a shape on the wire the API rejects.
    #[test]
    fn build_template_body_declares_the_port_the_agent_image_serves() {
        let sandbox = sandbox_with(SandboxEgress::Allow, "ubuntu:24.04", None, None);
        let body = build_template_body(&sandbox, "agent-sbx", &deployment_config(None, None), None)
            .expect("a valid sandbox builds a body");
        let wire = serde_json::to_value(&body).expect("the template body serializes");
        assert_eq!(
            wire["customContainerEnvironment"]["ports"],
            serde_json::json!([{ "port": GCP_AGENT_PLATFORM.port, "protocol": "TCP" }]),
            "the create body must carry the port and protocol the API has accepted"
        );
    }

    /// The verifier signs off on the body the controller builds for an `internet` sandbox: the
    /// install's own attachment, private ingress and the closed switch. A `none` sandbox's body
    /// with that attachment added is refused.
    #[test]
    fn the_verifier_accepts_the_controllers_internet_template_body() {
        use alien_core::gcp_sandbox_gateway::{attachment_path, resource_name, suffix};
        use alien_verifier::cloud_write::resource::check_sandbox_template;
        let attachment = attachment_path(
            "proj",
            "us-central1",
            &resource_name("pfx", suffix::ATTACHMENT),
        );
        let sandbox = sandbox_with(SandboxEgress::Internet, "ubuntu:24.04", None, None);
        let body = build_template_body(
            &sandbox,
            "pfx-agent-sbx",
            &deployment_config(None, None),
            Some(&attachment),
        )
        .expect("an internet sandbox builds a body with its attachment");
        let wire = serde_json::to_value(&body).expect("the template body serializes");
        assert_eq!(
            wire["egressControlConfig"]["internetAccess"],
            serde_json::json!(false)
        );
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &wire)
            .expect("the controller's own internet body");
        check_sandbox_template(&sandbox, "pfx", "other", "us-central1", "123", &wire)
            .expect_err("an attachment in a project the verifier does not write to");

        let none = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
        let mut none_body = serde_json::to_value(
            build_template_body(&none, "pfx-agent-sbx", &deployment_config(None, None), None)
                .expect("a none sandbox builds a body"),
        )
        .expect("the template body serializes");
        none_body["egressControlConfig"]["networkAttachment"] = serde_json::json!(attachment);
        none_body["ingressControlConfig"] =
            serde_json::json!({ "enablePrivateServiceConnect": true });
        check_sandbox_template(&none, "pfx", "proj", "us-central1", "123", &none_body)
            .expect_err("a none sandbox with the install's attachment");
    }

    /// The verifier signs off on exactly the body the controller builds, disk ceiling and agent
    /// port included, and refuses the same body with a different disk ceiling or an extra key.
    #[test]
    fn the_verifier_accepts_the_controllers_full_template_body() {
        use alien_verifier::cloud_write::resource::check_sandbox_template;
        let sandbox = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
        let body = build_template_body(
            &sandbox,
            "pfx-agent-sbx",
            &deployment_config(None, None),
            None,
        )
        .expect("a valid sandbox builds a body");
        let wire = serde_json::to_value(&body).expect("the template body serializes");
        let disk = sandbox.resolved_limits().disk;
        assert_eq!(
            wire["customContainerEnvironment"]["resources"]["limits"]["ephemeral-storage"],
            serde_json::json!(disk)
        );
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &wire)
            .expect("the controller's own body");

        let mut other_disk = wire.clone();
        other_disk["customContainerEnvironment"]["resources"]["limits"]["ephemeral-storage"] =
            serde_json::json!("1Gi");
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &other_disk)
            .expect_err("a disk ceiling that is not the signed one");
        let mut extra = wire.clone();
        extra["customContainerEnvironment"]["resources"]["limits"]["gpu"] = serde_json::json!("1");
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &extra)
            .expect_err("an extra limit");
        let mut other_port = wire;
        other_port["customContainerEnvironment"]["ports"][0]["port"] = serde_json::json!(8080);
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &other_port)
            .expect_err("another port");
    }

    /// The preview ports the signed sandbox declares are the ones the verifier lets the template
    /// carry: the controller's own body passes, a port nobody signed or a missing one does not.
    #[test]
    fn the_verifier_accepts_exactly_the_signed_preview_ports() {
        use alien_verifier::cloud_write::resource::check_sandbox_template;
        let mut sandbox = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
        sandbox.preview_ports = vec![8081, 8090, 8080];
        let body = build_template_body(
            &sandbox,
            "pfx-agent-sbx",
            &deployment_config(None, None),
            None,
        )
        .expect("a valid sandbox builds a body");
        let wire = serde_json::to_value(&body).expect("the template body serializes");
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &wire)
            .expect("the controller's own body");

        let mut extra = wire.clone();
        extra["customContainerEnvironment"]["ports"]
            .as_array_mut()
            .expect("ports")
            .push(serde_json::json!({"port": 9999, "protocol": "TCP"}));
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &extra)
            .expect_err("an unsigned port");

        // `:execute` goes to the first declared port, so the agent's port must be the first.
        let mut sorted = wire.clone();
        sorted["customContainerEnvironment"]["ports"]
            .as_array_mut()
            .expect("ports")
            .sort_by_key(|port| port["port"].as_i64());
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &sorted)
            .expect_err("the agent's port is not first");

        let mut missing = wire;
        missing["customContainerEnvironment"]["ports"]
            .as_array_mut()
            .expect("ports")
            .remove(0);
        check_sandbox_template(&sandbox, "pfx", "proj", "us-central1", "123", &missing)
            .expect_err("a signed port left out");
    }

    #[test]
    fn build_template_body_declares_preview_ports_and_disk() {
        let mut sandbox = sandbox_with(SandboxEgress::Allow, "ubuntu:24.04", None, None);
        sandbox.preview_ports = vec![8081, 8090, 8080];
        let body = build_template_body(&sandbox, "agent-sbx", &deployment_config(None, None), None)
            .expect("a valid sandbox builds a body");
        let wire = serde_json::to_value(&body).expect("the template body serializes");
        let ports: Vec<i64> = wire["customContainerEnvironment"]["ports"]
            .as_array()
            .expect("ports")
            .iter()
            .map(|p| p["port"].as_i64().expect("port"))
            .collect();
        assert_eq!(
            ports,
            vec![8971, 8080, 8081, 8090],
            "agent port first (`:execute` goes to the first declared port), then each preview port"
        );
        assert_eq!(
            wire["customContainerEnvironment"]["resources"]["limits"]["ephemeral-storage"],
            "8Gi"
        );
    }

    #[test]
    fn build_template_body_leaves_network_placement_default() {
        let sandbox = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
        let body = build_template_body(&sandbox, "agent-sbx", &deployment_config(None, None), None)
            .expect("builds");
        let wire = serde_json::to_value(&body).expect("serializes");
        assert!(wire["egressControlConfig"]
            .get("networkAttachment")
            .is_none());
        assert!(wire.get("ingressControlConfig").is_none());
    }

    fn deployment_config(
        manager_url: Option<&str>,
        native_image_host: Option<&str>,
    ) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(Default::default())
            .allow_frozen_changes(false)
            .maybe_manager_url(manager_url.map(str::to_string))
            .maybe_native_image_host(native_image_host.map(str::to_string))
            .build()
    }

    fn template_image(image: &str, config: &DeploymentConfig) -> Result<String> {
        let sandbox = sandbox_with(SandboxEgress::None, image, None, None);
        build_template_body(&sandbox, "agent-sbx", config, None).map(|body| {
            body.custom_container_environment
                .and_then(|environment| environment.custom_container_spec)
                .expect("the body names an image")
                .image_uri
        })
    }

    #[test]
    fn only_an_image_in_another_project_is_copied() {
        let config = deployment_config(Some("https://manager.example.com"), Some(NATIVE_HOST));
        let other = "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc/agent";
        assert_eq!(
            image_to_copy(&format!("{other}@sha256:0123"), &config, "customer"),
            Some(("vendor/artifacts-prj-abc/prj_abc/agent", "sha256:0123"))
        );
        assert_eq!(
            image_to_copy(&format!("{other}:v1"), &config, "customer"),
            Some(("vendor/artifacts-prj-abc/prj_abc/agent", "v1"))
        );
        // Already in the deployment's own project: nothing to copy.
        assert_eq!(
            image_to_copy(&format!("{other}:v1"), &config, "vendor"),
            None
        );
        // Not a manager image, or no manager: untouched.
        assert_eq!(image_to_copy("ubuntu:24.04", &config, "customer"), None);
        assert_eq!(
            image_to_copy(
                &format!("{other}:v1"),
                &deployment_config(None, None),
                "customer"
            ),
            None
        );
    }

    const NATIVE_HOST: &str = "us-central1-docker.pkg.dev";

    /// Agent Platform cannot pull through the manager's proxy, so an image a release pushed
    /// through the manager reaches it by its native registry path, whichever host the release
    /// stamped. The CLI deploying in push mode reaches the manager on a URL other than the public
    /// host it stamps on images, so the first case has hosts that differ.
    #[test]
    fn a_per_project_image_is_sent_by_its_native_registry_path() {
        for (manager_url, image, native) in [
            (
                "http://localhost:8080",
                "registry.example.com/vendor/artifacts-prj-abc/prj_abc:sbx-agent",
                "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc:sbx-agent",
            ),
            (
                "https://manager.example.com",
                "manager.example.com/vendor/artifacts-prj-abc/prj_abc/agent@sha256:0123",
                "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc/agent@sha256:0123",
            ),
            (
                "https://manager.example.com",
                "manager.example.com/vendor/artifacts-prj-abc/prj_abc@sha256:0123",
                "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc@sha256:0123",
            ),
            (
                "https://manager.example.com",
                "manager.example.com/vendor/artifacts-default/default:sbx-agent",
                "us-central1-docker.pkg.dev/vendor/artifacts-default/default:sbx-agent",
            ),
        ] {
            let config = deployment_config(Some(manager_url), Some(NATIVE_HOST));
            assert_eq!(
                template_image(image, &config).expect("a per-project image has a native path"),
                native
            );
        }
    }

    /// Only a per-project repository is moved to the native host: any other reference is already
    /// pullable where it is, including one on the manager's own host, and a repository whose name
    /// and project path disagree is not a per-project repository. A per-project path already on an
    /// Artifact Registry host stays there, because the same path in another region is a different
    /// repository.
    #[test]
    fn a_native_or_public_image_is_sent_as_declared() {
        let config = deployment_config(Some("https://manager.example.com"), Some(NATIVE_HOST));
        for image in [
            "us-central1-docker.pkg.dev/proj/repo/sandbox:v1",
            "europe-west1-docker.pkg.dev/acme/artifacts-default/default:sbx",
            "europe-west1-docker.pkg.dev/acme/artifacts-prj-abc/prj_abc/agent@sha256:0123",
            "ghcr.io/acme/sandbox:v1",
            "ubuntu:24.04",
            "manager.example.com/proj/repo/default:sbx-abc-agent",
            "ghcr.io/acme/tools-prj-abc/prj_other:v1",
        ] {
            assert_eq!(
                template_image(image, &config).expect("a pullable image is accepted"),
                image
            );
        }
    }

    /// The manager fills in the native host from a registry binding it may fail to load once, so a
    /// per-project image without one waits for the next pass instead of failing the deployment.
    #[test]
    fn a_per_project_image_without_a_native_host_is_retried() {
        let error = template_image(
            "registry.example.com/vendor/artifacts-prj-abc/prj_abc:sbx-agent",
            &deployment_config(Some("https://manager.example.com"), None),
        )
        .expect_err("nothing names the image natively yet");
        assert_eq!(error.code, "DEPENDENCY_NOT_READY", "{error}");
        assert!(error.retryable, "{error}");
    }

    /// With no manager, nothing will ever name the native host, so the image is refused.
    #[test]
    fn a_per_project_image_without_a_manager_is_refused() {
        let error = template_image(
            "registry.example.com/vendor/artifacts-prj-abc/prj_abc:sbx-agent",
            &deployment_config(None, None),
        )
        .expect_err("nothing can name the image natively");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID", "{error}");
        assert!(!error.retryable, "{error}");
        assert!(
            error.to_string().contains("prj_abc:sbx-agent"),
            "names the image: {error}"
        );
    }

    mod copy_into_own_project {
        use super::*;
        use alien_core::ClientConfig;
        use alien_core::{GcpClientConfig, GcpServiceOverrides as ServiceOverrides};
        use alien_gcp_clients::artifactregistry::{MockArtifactRegistryApi, Repository};
        use httpmock::prelude::*;
        use sha2::{Digest, Sha256};

        const LAYER: &[u8] = b"layer";
        const CONFIG: &[u8] = b"{}";
        const SOURCE_IMAGE: &str =
            "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc/agent:v1";

        fn digest_of(bytes: &[u8]) -> String {
            format!("sha256:{:x}", Sha256::digest(bytes))
        }

        fn manifest() -> (String, String) {
            let body = serde_json::json!({
                "schemaVersion": 2,
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "config": {"digest": digest_of(CONFIG)},
                "layers": [{"digest": digest_of(LAYER)}],
            })
            .to_string();
            let digest = digest_of(body.as_bytes());
            (body, digest)
        }

        /// The manager serves the image by name; Artifact Registry accepts every write.
        fn serve_source(source: &MockServer, body: &str) {
            source.mock(|w, t| {
                w.method(GET)
                    .path("/v2/vendor/artifacts-prj-abc/prj_abc/agent/manifests/v1")
                    .header("authorization", "Bearer test-deployment-token");
                t.status(200)
                    .header("content-type", "application/vnd.oci.image.manifest.v1+json")
                    .body(body);
            });
            // The copy then reads the manifest again by the digest it resolved.
            let by_digest = digest_of(body.as_bytes());
            source.mock(|w, t| {
                w.method(GET).path(format!(
                    "/v2/vendor/artifacts-prj-abc/prj_abc/agent/manifests/{by_digest}"
                ));
                t.status(200)
                    .header("content-type", "application/vnd.oci.image.manifest.v1+json")
                    .body(body);
            });
            for blob in [CONFIG, LAYER] {
                let d = digest_of(blob);
                source.mock(|w, t| {
                    w.method(GET).path(format!(
                        "/v2/vendor/artifacts-prj-abc/prj_abc/agent/blobs/{d}"
                    ));
                    t.status(200).body(blob);
                });
            }
        }

        fn serve_destination(dest: &MockServer) {
            dest.mock(|w, t| {
                w.method(httpmock::Method::HEAD);
                t.status(404);
            });
            dest.mock(|w, t| {
                w.method(POST);
                t.status(202).header("location", "/upload");
            });
            dest.mock(|w, t| {
                w.method(PUT).path("/upload");
                t.status(201);
            });
            dest.mock(|w, t| {
                w.method(PUT).path_contains("/manifests/");
                t.status(201);
            });
        }

        /// The repository setup made exists and is readable.
        fn provider(client: Arc<MockAgentPlatformApi>) -> Arc<MockPlatformServiceProvider> {
            provider_reading(client, None)
        }

        /// The repository read fails: `denied` is a refusal, otherwise not found. The controller
        /// never creates a repository, so a create call fails the test.
        fn provider_reading(
            client: Arc<MockAgentPlatformApi>,
            failure: Option<bool>,
        ) -> Arc<MockPlatformServiceProvider> {
            let mut registry = MockArtifactRegistryApi::new();
            registry
                .expect_get_repository()
                .returning(move |_, _, _| match failure {
                    None => Ok(Repository::default()),
                    Some(true) => Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteAccessDenied {
                            resource_type: "repository".to_string(),
                            resource_name: "r".to_string(),
                        },
                    )),
                    Some(false) => Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "repository".to_string(),
                            resource_name: "r".to_string(),
                        },
                    )),
                });
            registry.expect_create_repository().never();
            registry.expect_delete_repository().never();
            let registry = Arc::new(registry);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_gcp_agent_platform_client()
                .returning(move |_| Ok(client.clone()));
            provider
                .expect_get_gcp_artifact_registry_client()
                .returning(move |_| Ok(registry.clone()));
            Arc::new(provider)
        }

        async fn executor(
            image: &str,
            source: &MockServer,
            dest: &MockServer,
            provider: Arc<MockPlatformServiceProvider>,
        ) -> SingleControllerExecutor {
            executor_as(
                alien_core::InitialSetupAuthority::ImportedHandoff,
                false,
                image,
                source,
                dest,
                provider,
            )
            .await
        }

        async fn executor_as(
            authority: alien_core::InitialSetupAuthority,
            setup_run: bool,
            image: &str,
            source: &MockServer,
            dest: &MockServer,
            provider: Arc<MockPlatformServiceProvider>,
        ) -> SingleControllerExecutor {
            let config = GcpClientConfig::mock().with_service_overrides(ServiceOverrides {
                endpoints: std::collections::HashMap::from([(
                    "artifactregistry-docker".to_string(),
                    dest.base_url(),
                )]),
            });
            SingleControllerExecutor::builder()
                .resource(sandbox_with(SandboxEgress::None, image, None, None))
                .controller(GcpAgentPlatformTemplateController::default())
                .platform(Platform::Gcp)
                .client_config(ClientConfig::Gcp(Box::new(config)))
                .initial_setup_authority(authority)
                .setup_run(setup_run)
                .manager_url(source.base_url())
                .native_image_host("us-central1-docker.pkg.dev")
                .service_provider(provider)
                .with_test_dependencies()
                .with_dependency(
                    GcpAgentPlatformEngine::new(GcpAgentPlatformEngine::id_for_sandbox(
                        "agent-sbx",
                    ))
                    .build(),
                    GcpAgentPlatformEngineController::mock_ready("eng"),
                )
                .build()
                .await
                .expect("executor builds")
        }

        #[tokio::test]
        async fn an_image_in_the_managers_project_is_copied_and_the_template_uses_the_copy() {
            let (body, digest) = manifest();
            let (source, dest) = (MockServer::start(), MockServer::start());
            serve_source(&source, &body);
            serve_destination(&dest);
            let expected_prefix = format!(
                "{}/test-project-123/test-agent-sbx-img/agent-sbx@{digest}",
                dest.base_url().trim_start_matches("http://")
            );
            let mut m = MockAgentPlatformApi::new();
            m.expect_create_template()
                .withf(move |_, template| {
                    template
                        .custom_container_environment
                        .as_ref()
                        .and_then(|e| e.custom_container_spec.as_ref())
                        .map(|s| s.image_uri == expected_prefix)
                        .unwrap_or(false)
                })
                .times(1)
                .returning(|_, _| Ok(pending_op()));
            m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
            m.expect_get_template()
                .returning(|_, id| Ok(active_template(id)));
            m.expect_list_templates()
                .returning(|_| Ok(vec![active_template("tpl1")]));
            let mut executor = executor(SOURCE_IMAGE, &source, &dest, provider(Arc::new(m))).await;
            executor.run_until_terminal().await.expect("create runs");
            assert_eq!(executor.status(), ResourceStatus::Running);
        }

        fn healthy_client() -> MockAgentPlatformApi {
            let mut m = MockAgentPlatformApi::new();
            m.expect_create_template()
                .returning(|_, _| Ok(pending_op()));
            m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
            m.expect_get_template()
                .returning(|_, id| Ok(active_template(id)));
            m.expect_list_templates()
                .returning(|_| Ok(vec![active_template("tpl1")]));
            m.expect_delete_template().returning(|_, _| Ok(()));
            m
        }

        /// A repository setup created (Terraform) belongs to setup; the controller leaves it.
        #[tokio::test]
        async fn deleting_the_sandbox_leaves_a_repository_setup_created() {
            let (body, _) = manifest();
            let (source, dest) = (MockServer::start(), MockServer::start());
            serve_source(&source, &body);
            serve_destination(&dest);
            let mut executor = executor(
                SOURCE_IMAGE,
                &source,
                &dest,
                provider(Arc::new(healthy_client())),
            )
            .await;
            executor.run_until_terminal().await.expect("create runs");
            executor.delete().expect("delete accepted");
            executor.run_until_terminal().await.expect("delete runs");
            assert_eq!(executor.status(), ResourceStatus::Deleted);
        }

        #[tokio::test]
        async fn an_image_already_in_the_deployments_project_is_not_copied() {
            let (source, dest) = (MockServer::start(), MockServer::start());
            let own =
                "us-central1-docker.pkg.dev/test-project-123/artifacts-prj-abc/prj_abc/agent:v1";
            let mut m = MockAgentPlatformApi::new();
            m.expect_create_template()
                .withf(move |_, template| {
                    template
                        .custom_container_environment
                        .as_ref()
                        .and_then(|e| e.custom_container_spec.as_ref())
                        .map(|s| s.image_uri == own)
                        .unwrap_or(false)
                })
                .times(1)
                .returning(|_, _| Ok(pending_op()));
            m.expect_get_operation().returning(|_| Ok(done_op("tpl1")));
            m.expect_get_template()
                .returning(|_, id| Ok(active_template(id)));
            m.expect_list_templates()
                .returning(|_| Ok(vec![active_template("tpl1")]));
            let mut executor = executor(own, &source, &dest, provider(Arc::new(m))).await;
            executor.run_until_terminal().await.expect("create runs");
            assert_eq!(executor.status(), ResourceStatus::Running);
        }

        /// A failed copy is an error of the step; the template is never created from the
        /// uncopied reference, which Agent Platform would refuse anyway.
        #[tokio::test]
        async fn a_failed_copy_creates_no_template_and_the_step_can_run_again() {
            let (source, dest) = (MockServer::start(), MockServer::start());
            source.mock(|w, t| {
                w.method(GET).path_contains("/manifests/");
                t.status(403);
            });
            let mut m = MockAgentPlatformApi::new();
            m.expect_create_template().never();
            let mut executor = executor(SOURCE_IMAGE, &source, &dest, provider(Arc::new(m))).await;
            let first = executor.step().await;
            let error = first.expect_err("a refused source stops the create");
            assert!(error.to_string().contains("403"), "{error}");
            assert!(
                !error.to_string().contains("test-deployment-token"),
                "{error}"
            );
            // The same step runs again, and the source is refused again.
            assert!(executor.step().await.is_err());
        }

        /// An install set up before images were copied has no repository, or none the manager can
        /// read. Cloud answers 403, or 404 for an identity that may read repositories project-wide;
        /// both say to rerun setup, and write nothing: no repository, no copy, no template.
        #[tokio::test]
        async fn a_missing_or_refused_repository_asks_for_the_setup_rerun_and_writes_nothing() {
            for denied in [true, false] {
                let (source, dest) = (MockServer::start(), MockServer::start());
                let source_requests = source.mock(|w, t| {
                    w.any_request();
                    t.status(200);
                });
                let dest_requests = dest.mock(|w, t| {
                    w.any_request();
                    t.status(200);
                });
                let mut m = MockAgentPlatformApi::new();
                m.expect_create_template().never();
                let provider = provider_reading(Arc::new(m), Some(denied));
                let mut executor = executor(SOURCE_IMAGE, &source, &dest, provider).await;
                let error = executor
                    .step()
                    .await
                    .expect_err("an unusable repository stops the step");
                assert_eq!(error.code, "SETUP_RERUN_REQUIRED", "{error}");
                assert!(!error.retryable);
                assert_eq!(source_requests.hits(), 0);
                assert_eq!(dest_requests.hits(), 0);
            }
        }

        /// A direct setup's scaffolding creates the repository and the manager's write access with
        /// the deployer's credentials; this controller never creates it on a direct install. During
        /// the setup run it waits for the scaffolding; afterwards a missing repository says to run
        /// setup again. Neither creates, copies or touches the template.
        #[tokio::test]
        async fn a_direct_install_waits_for_the_repository_setup_makes() {
            for (setup_run, code) in [
                (true, "DEPENDENCY_NOT_READY"),
                (false, "SETUP_RERUN_REQUIRED"),
            ] {
                let (source, dest) = (MockServer::start(), MockServer::start());
                let source_requests = source.mock(|w, t| {
                    w.any_request();
                    t.status(200);
                });
                let dest_requests = dest.mock(|w, t| {
                    w.any_request();
                    t.status(200);
                });
                let mut m = MockAgentPlatformApi::new();
                m.expect_create_template().never();
                // No repository, and no create call allowed.
                let provider = provider_reading(Arc::new(m), Some(false));
                let mut executor = executor_as(
                    alien_core::InitialSetupAuthority::DirectSetup,
                    setup_run,
                    SOURCE_IMAGE,
                    &source,
                    &dest,
                    provider,
                )
                .await;
                let error = executor
                    .step()
                    .await
                    .expect_err("a missing repository stops the step");
                assert_eq!(error.code, code, "{error}");
                assert_eq!(source_requests.hits(), 0);
                assert_eq!(dest_requests.hits(), 0);
            }
        }

        /// What setup made is adopted: the copy goes into it, the template uses the copy, and the
        /// controller records no repository, so its teardown leaves setup's to setup.
        #[tokio::test]
        async fn a_direct_install_adopts_the_repository_setup_made() {
            let (body, _) = manifest();
            let (source, dest) = (MockServer::start(), MockServer::start());
            serve_source(&source, &body);
            serve_destination(&dest);
            let m = healthy_client();
            let provider = provider(Arc::new(m));
            let mut executor = executor_as(
                alien_core::InitialSetupAuthority::DirectSetup,
                false,
                SOURCE_IMAGE,
                &source,
                &dest,
                provider,
            )
            .await;
            executor.run_until_terminal().await.expect("create runs");
            assert_eq!(executor.status(), ResourceStatus::Running);
        }
    }

    // ---- Egress internet: the gateway path and the project's placement. ----------------------

    const ATTACHMENT: &str = "projects/p/regions/us-central1/networkAttachments/stack-sandbox";

    /// The two settings the API wants together, and the switch that stays closed so nothing can
    /// bypass the gateway.
    #[test]
    fn an_internet_template_names_the_attachment_with_private_ingress_and_a_closed_switch() {
        let sandbox = sandbox_with(SandboxEgress::Internet, "ubuntu:24.04", None, None);
        let body = build_template_body(
            &sandbox,
            "agent-sbx",
            &deployment_config(None, None),
            Some(ATTACHMENT),
        )
        .expect("builds");
        let wire = serde_json::to_value(&body).expect("serializes");
        assert_eq!(wire["egressControlConfig"]["networkAttachment"], ATTACHMENT);
        assert!(
            wire["egressControlConfig"].get("internetAccess").is_none()
                || wire["egressControlConfig"]["internetAccess"] == false,
            "{wire}"
        );
        assert_eq!(
            wire["ingressControlConfig"]["enablePrivateServiceConnect"],
            true
        );
    }

    #[test]
    fn an_internet_template_without_the_setup_attachment_asks_for_setup_to_be_rerun() {
        let sandbox = sandbox_with(SandboxEgress::Internet, "ubuntu:24.04", None, None);
        let error =
            build_template_body(&sandbox, "agent-sbx", &deployment_config(None, None), None)
                .expect_err("a deployment that predates the gateway has no attachment");
        assert!(error.message.contains("re-run setup"), "{}", error.message);
    }

    #[test]
    fn the_gateway_path_forces_a_replace_but_a_plain_deny_does_not_look_like_it() {
        let internet = sandbox_with(SandboxEgress::Internet, "ubuntu:24.04", None, None);
        let none = sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None);
        // Both close the cloud's own switch, but only one changes the project's placement.
        assert_ne!(
            template_identity(&internet).unwrap(),
            template_identity(&none).unwrap()
        );
    }

    #[tokio::test]
    async fn a_default_sandbox_is_refused_before_the_template_in_a_psc_project() {
        // A default template is checked against the project's placement once `internet` is
        // available; before that nothing can have pinned a project to Private Service Connect.
        let _on = alien_core::gcp_sandbox_gateway::enable_internet_egress_for_test();
        use alien_gcp_clients::gcp::compute::{
            NetworkAttachment, NetworkAttachmentConnectedEndpoint,
        };
        let mut m = MockAgentPlatformApi::new();
        serve_empty_project(&mut m);
        m.expect_create_template().never();
        let mut provider = MockPlatformServiceProvider::new();
        let client = Arc::new(m);
        provider
            .expect_get_gcp_agent_platform_client()
            .returning(move |_| Ok(client.clone()));
        let mut compute = MockComputeApi::new();
        compute.expect_list_network_attachments().returning(|_| {
            Ok(vec![NetworkAttachment {
                name: Some("elsewhere".into()),
                connection_endpoints: vec![NetworkAttachmentConnectedEndpoint {
                    status: Some("ACCEPTED".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }])
        });
        let compute: Arc<dyn alien_gcp_clients::gcp::compute::ComputeApi> = Arc::new(compute);
        provider
            .expect_get_gcp_compute_client()
            .returning(move |_| Ok(compute.clone()));
        let mut executor = build_executor(
            sandbox_with(SandboxEgress::None, "ubuntu:24.04", None, None),
            Arc::new(provider),
        )
        .await;
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the project is pinned to Private Service Connect");
        assert!(
            format!("{error:?}").contains("Private Service Connect"),
            "{error:?}"
        );
    }
}
