//! Render every setup template of a protected stack, the way a signed release records them.
//!
//! The service that hosts the templates renders them for installs, and `verify` renders them
//! again from the signed stack to check what an install holds. Both must produce the same
//! bytes, so [`render_setup_templates`] depends only on its inputs: the stack, its signing
//! settings, the emitter registries, the hosting options and the renderer version, rendered
//! by the alien version that is running. It reads no environment, clock, or cloud.
//!
//! Every protected template records the renderer version and the hosting inputs it was
//! rendered with ([`SetupRecord`]), where `verify` can read them from the install.

use std::collections::BTreeMap;

use alien_cloudformation::{
    CfRegistry, CloudFormationOptions, CloudFormationTarget, ManagementTrustOptions,
    RegistrationMode,
};
use alien_core::{
    compute_planner::plan_compute, remote_bindings::remote_binding_for_entry, AwsManagementConfig,
    AzureManagementConfig, ComputeCluster, ComputePoolSelection, ComputeSettings, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, GcpManagementConfig, ManagementConfig,
    NetworkSettings, Platform, ResourceEntry, ResourceLifecycle, SetupPins, Stack, StackSettings,
    StackSigning, StackState, TelemetryMode,
};
use alien_error::{AlienError, AlienErrorData, Context, IntoAlienError};
use alien_helm::{HelmOptions, HelmRegistry};
use alien_preflights::compile_time::stack_requires_network;
use alien_signing::release::SETUP_TEMPLATES;
use alien_terraform::{TerraformOptions, TerraformSetupIdentity, TerraformTarget, TfRegistry};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Errors from rendering setup templates.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A setup template could not be rendered for the stack.
    #[error(
        code = "SETUP_TEMPLATE_RENDER_FAILED",
        message = "Rendering the {template} setup template failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    RenderFailed {
        /// Template name from `SETUP_TEMPLATES`.
        template: String,
        /// What went wrong.
        message: String,
    },
}

/// Result type for setup template rendering.
pub type Result<T> = alien_error::Result<T, ErrorData>;

/// What the service that hosts the setup templates renders them with: who manages the install
/// and how setup reports back. The CLI takes these from that service, so both render the same
/// bytes. None of them is trusted: on a protected install the management limits hold whoever
/// management is, and `verify` prints the management principal from the installed template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupHostingOptions {
    /// How the CloudFormation stack reports itself after it is created. None when the service
    /// hosts no CloudFormation setup (a project without AWS); rendering the CloudFormation
    /// template then fails. Terraform modules need none: a protected module reports through its
    /// outputs and `register --import terraform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<RegistrationMode>,
    /// CloudFormation template description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Who may manage the stack.
    #[serde(default)]
    pub management_trust: ManagementTrustOptions,
    /// AWS regions the templates accept; empty accepts any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_aws_regions: Vec<String>,
}

/// The version of the renderer in this build: the alien version.
pub const RENDERER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Metadata key of a protected CloudFormation template that holds its [`SetupRecord`].
pub const CLOUDFORMATION_RECORD_KEY: &str = "Setup";

/// File of a protected Terraform module or Helm chart that holds its [`SetupRecord`].
pub const RECORD_FILE: &str = "setup-render.json";

/// What a protected template says about how it was rendered, so a customer can render it
/// again: the renderer version and the hosting inputs. It is part of the template's bytes, so
/// a record that lies renders other bytes and the comparison fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupRecord {
    /// Alien version that rendered the template.
    pub renderer: String,
    /// The hosting inputs it was rendered with.
    pub hosting: SetupHostingOptions,
}

impl SetupRecord {
    /// The record as JSON with every object's keys sorted, so the same record is the same
    /// bytes whatever order its inputs were built in.
    pub fn to_value(&self) -> serde_json::Value {
        sort_keys(serde_json::to_value(self).expect("a record is JSON"))
    }

    /// The record as the file a Terraform module or Helm chart holds.
    pub fn to_file(&self) -> String {
        let mut text = serde_json::to_string_pretty(&self.to_value()).expect("a record is JSON");
        text.push('\n');
        text
    }

    /// The record of a protected CloudFormation template, from its YAML.
    pub fn from_cloudformation(template: &str) -> Result<Self> {
        let value: serde_json::Value = serde_yaml::from_str(template)
            .into_alien_error()
            .context(step("cloudformation", "read the template as YAML"))?;
        let record = value
            .get("Metadata")
            .and_then(|metadata| metadata.get(CLOUDFORMATION_RECORD_KEY))
            .ok_or_else(|| {
                render_failed(
                    "cloudformation",
                    "the template has no Metadata.Setup: it was not rendered by a setup renderer",
                )
            })?;
        Self::from_value("cloudformation", record.clone())
    }

    /// The record of a protected Terraform module or Helm chart, from its files.
    pub fn from_files(name: &str, files: &BTreeMap<String, String>) -> Result<Self> {
        let text = files.get(RECORD_FILE).ok_or_else(|| {
            render_failed(
                name,
                format!("there is no {RECORD_FILE}: it was not rendered by a setup renderer"),
            )
        })?;
        let value = serde_json::from_str(text)
            .into_alien_error()
            .context(step(name, "read the render record as JSON"))?;
        Self::from_value(name, value)
    }

    fn from_value(name: &str, value: serde_json::Value) -> Result<Self> {
        serde_json::from_value(value)
            .into_alien_error()
            .context(step(name, "read the render record"))
    }
}

fn sort_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .into_iter()
                .map(|(key, value)| (key, sort_keys(value)))
                .collect();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sort_keys).collect())
        }
        other => other,
    }
}

/// Everything a setup template depends on besides the stack.
#[derive(Builder)]
pub struct SetupRenderOptions<'a> {
    /// Fingerprint and channel the templates pin.
    pub signing: &'a StackSigning,
    pub cloudformation_registry: &'a CfRegistry,
    pub terraform_registry: &'a TfRegistry,
    pub helm_registry: &'a HelmRegistry,
    /// What the hosting service renders with.
    pub hosting: SetupHostingOptions,
    /// Machine-image publisher, verifier image and Operator image. Leave the default, this
    /// build's own, except in tests: they come with the renderer build, so `verify` computes
    /// them again by rendering with the version a template records.
    #[builder(default = SetupPins::compiled_in())]
    pub pins: SetupPins,
    /// The renderer version a protected template records. Leave the default, this build's
    /// own, except in tests.
    #[builder(default = RENDERER_VERSION.to_string())]
    pub renderer: String,
}

/// One rendered setup template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSetupTemplate {
    /// The template bytes. CloudFormation is the YAML template. Terraform modules and Helm
    /// charts are several files; their bytes are the JSON object of file path to contents,
    /// keys sorted, so a reader can recompute the digest from the files.
    pub bytes: Vec<u8>,
    /// The files of a Terraform module or Helm chart; none for CloudFormation.
    pub files: Option<BTreeMap<String, String>>,
    /// CloudFormation parameters, Terraform variables or top-level Helm values the customer
    /// may choose. Everything else keeps the template's pinned value.
    pub allowed: Vec<String>,
}

/// The stack settings the setup package of `stack` for template `name` is rendered with, before
/// any signing: on a cloud the network is left to the customer, a new one by default, unless the
/// stack only reaches resources that run elsewhere and needs no network of its own; a stack that
/// runs nothing (it only gives access) registers telemetry off; and the stack's compute
/// clusters get the pools they plan. The CLI and the service that hosts packages both render
/// with this, so a protected install keeps what its unprotected package created.
pub fn package_stack_settings(name: &str, stack: &Stack) -> Result<StackSettings> {
    let platform = match name {
        "cloudformation" | "terraform-aws" => Platform::Aws,
        "terraform-gcp" => Platform::Gcp,
        "terraform-azure" => Platform::Azure,
        "helm" => return Ok(StackSettings::default()),
        other => return Err(render_failed(other, "no renderer for this template name")),
    };
    let mut settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let reached_elsewhere = |entry: &ResourceEntry| remote_binding_for_entry(entry).is_some();
    let reaches_any = stack.resources().any(|(_, entry)| reached_elsewhere(entry));
    // A Live resource reached elsewhere (a Live sandbox) needs no network of its own, so only
    // other Live resources keep the default network.
    if reaches_any
        && !stack.resources().any(|(_, entry)| {
            entry.lifecycle == ResourceLifecycle::Live && !reached_elsewhere(entry)
        })
        && !stack_requires_network(stack)
    {
        settings.network = None;
    }
    if reaches_any
        && !stack
            .resources()
            .any(|(_, entry)| entry.lifecycle == ResourceLifecycle::Live)
    {
        settings.telemetry = TelemetryMode::Off;
    }
    let plan = plan_compute(stack, platform, None).context(ErrorData::RenderFailed {
        template: name.to_string(),
        message: "planning the stack's compute pools".to_string(),
    })?;
    if !plan.pools.is_empty() {
        settings.compute = Some(ComputeSettings {
            containers: Default::default(),
            pools: plan
                .pools
                .into_iter()
                .map(|pool| {
                    // A machine the stack's cluster names stays its choice.
                    let machine = stack
                        .resources
                        .values()
                        .filter_map(|entry| entry.config.downcast_ref::<ComputeCluster>())
                        .flat_map(|cluster| &cluster.capacity_groups)
                        .find(|group| group.group_id == pool.pool_id)
                        .and_then(|group| group.instance_type.clone());
                    let mut selection = pool.selected;
                    match &mut selection {
                        ComputePoolSelection::Fixed {
                            machine: chosen, ..
                        }
                        | ComputePoolSelection::Autoscale {
                            machine: chosen, ..
                        } => *chosen = machine,
                    }
                    (pool.pool_id, selection)
                })
                .collect(),
        });
    }
    Ok(settings)
}

/// The stack settings a release renders setup template `name` for `stack` with: the package's
/// ([`package_stack_settings`]) with signing on. Protecting an install replaces its unprotected
/// setup with this one, which must keep what the install already has.
pub fn protected_stack_settings(
    name: &str,
    stack: &Stack,
    signing: &StackSigning,
) -> Result<StackSettings> {
    Ok(StackSettings {
        signing: Some(signing.clone()),
        ..package_stack_settings(name, stack)?
    })
}

/// Render each template in [`SETUP_TEMPLATES`] for `stack` with signing on, keyed by name.
pub async fn render_setup_templates(
    stack: &Stack,
    options: SetupRenderOptions<'_>,
) -> Result<BTreeMap<String, RenderedSetupTemplate>> {
    let mut rendered = BTreeMap::new();
    for &name in SETUP_TEMPLATES {
        let settings = protected_stack_settings(name, stack, options.signing)?;
        let template = render_setup_template(name, stack, &settings, &options)
            .await
            .context(ErrorData::RenderFailed {
                template: name.to_string(),
                message: format!("stack '{}'", stack.id()),
            })?;
        rendered.insert(name.to_string(), template);
    }
    Ok(rendered)
}

/// Render the setup template `name` (from [`SETUP_TEMPLATES`]) for `stack` with `settings`.
/// [`render_setup_templates`] calls this with [`protected_stack_settings`]; other settings make
/// templates a release does not record. `options.signing` is not used here.
pub async fn render_setup_template(
    name: &str,
    source: &Stack,
    settings: &StackSettings,
    options: &SetupRenderOptions<'_>,
) -> Result<RenderedSetupTemplate> {
    let stack_json = serde_json::to_value(source)
        .into_alien_error()
        .context(step(name, "read the stack as JSON"))?;
    let (platform, base_platform) = match name {
        "cloudformation" | "terraform-aws" => (Platform::Aws, None),
        "terraform-gcp" => (Platform::Gcp, None),
        "terraform-azure" => (Platform::Azure, None),
        "helm" => (Platform::Kubernetes, None),
        other => return Err(render_failed(other, "no renderer for this template name")),
    };
    let mut source_stack = source.clone();
    let mut compute = settings.compute.clone();
    alien_core::remove_off_platform_gated_resources(&mut source_stack, platform, compute.as_mut());
    let stack = prepare(
        name,
        source_stack.clone(),
        platform,
        base_platform,
        settings,
    )
    .await?;
    // The hosting service computes the same value from the stack the release sends.
    let (setup_fingerprint, setup_fingerprint_version) =
        crate::setup_fingerprint::setup_fingerprint(&stack_json, platform.as_str());

    match name {
        "cloudformation" => {
            let mut template = alien_cloudformation::generate_cloudformation_template(
                &stack,
                CloudFormationOptions {
                    pins: options.pins.clone(),
                    registry: options.cloudformation_registry,
                    source_stack: &source_stack,
                    target: CloudFormationTarget::Aws,
                    stack_settings: settings.clone(),
                    setup_target: "aws".to_string(),
                    setup_fingerprint: setup_fingerprint.clone(),
                    setup_fingerprint_version,
                    registration: options.hosting.registration.clone().ok_or_else(|| {
                        render_failed(
                            name,
                            "the service that hosts the templates gave no CloudFormation registration",
                        )
                    })?,
                    description: options.hosting.description.clone(),
                    management_trust: options.hosting.management_trust.clone(),
                },
            )
            .context(step(name, "generate the template"))?;
            let allowed = template
                .parameters
                .keys()
                .filter(|parameter| {
                    parameter.as_str() != alien_cloudformation::SIGNING_FINGERPRINT_PARAMETER
                })
                .cloned()
                .collect();
            if settings.signing.is_some() {
                template.metadata.insert(
                    CLOUDFORMATION_RECORD_KEY.to_string(),
                    record(options).to_value(),
                );
                // The registration runs again on every setup update, so its revision covers
                // the record: a new renderer version is a new revision.
                if let Some(registration) = template
                    .resources
                    .get_mut(alien_cloudformation::REGISTRATION_RESOURCE)
                {
                    registration
                        .properties
                        .shift_remove(alien_cloudformation::SETUP_REVISION);
                }
                alien_cloudformation::add_setup_revision(&mut template)
                    .context(step(name, "revise the registration"))?;
            }
            let yaml = alien_cloudformation::to_yaml(&template)
                .context(step(name, "write the template as YAML"))?;
            Ok(RenderedSetupTemplate {
                bytes: yaml.into_bytes(),
                files: None,
                allowed,
            })
        }
        "helm" => {
            let helm_options = HelmOptions {
                registry: options.helm_registry,
                stack_settings: settings.clone(),
                chart_name: stack.id().to_string(),
            };
            let chart = match &settings.signing {
                Some(_) => {
                    let operator_digest = options.pins.operator_digest.clone().ok_or_else(|| {
                        render_failed(
                            name,
                            "this CLI build has no Operator digest; protected setups come from released CLI builds",
                        )
                    })?;
                    alien_helm::generate_signed_helm_chart(
                        &stack,
                        helm_options,
                        &alien_helm::SignedChartPins { operator_digest },
                    )
                }
                None => alien_helm::generate_helm_chart(&stack, helm_options),
            }
            .context(step(name, "generate the chart"))?;
            let allowed = match &settings.signing {
                Some(_) => alien_helm::allowed_values(&chart)
                    .context(step(name, "list the values customers may set"))?,
                None => Vec::new(),
            };
            let mut files: BTreeMap<String, String> = chart.files.into_iter().collect();
            if settings.signing.is_some() {
                files.insert(RECORD_FILE.to_string(), record(options).to_file());
            }
            Ok(RenderedSetupTemplate {
                bytes: file_map_bytes(&files),
                files: Some(files),
                allowed,
            })
        }
        _ => {
            let target = match platform {
                Platform::Aws => TerraformTarget::Aws,
                Platform::Gcp => TerraformTarget::Gcp,
                _ => TerraformTarget::Azure,
            };
            let module = alien_terraform::generate_terraform_module(
                &stack,
                target,
                TerraformOptions {
                    // A protected module carries no registration provider; it reports this
                    // setup through its outputs for `register --import terraform`.
                    setup_identity: settings.signing.as_ref().map(|_| TerraformSetupIdentity {
                        setup_target: platform.as_str().to_string(),
                        setup_fingerprint: setup_fingerprint.clone(),
                        setup_fingerprint_version,
                    }),
                    pins: options.pins.clone(),
                    display_name: None,
                    registry: options.terraform_registry,
                    stack_settings: settings.clone(),
                    registration: None,
                    helm_install: None,
                    supported_aws_regions: options.hosting.supported_aws_regions.clone(),
                },
            )
            .context(step(name, "generate the module"))?;
            let allowed = module
                .variable_names()
                .context(step(name, "list the module variables"))?
                .into_iter()
                .filter(|variable| variable != alien_terraform::SIGNING_FINGERPRINT_VARIABLE)
                .collect();
            let mut files: BTreeMap<String, String> = module
                .iter()
                .map(|(path, contents)| (path.to_string(), contents.to_string()))
                .collect();
            if settings.signing.is_some() {
                files.insert(RECORD_FILE.to_string(), record(options).to_file());
            }
            Ok(RenderedSetupTemplate {
                bytes: file_map_bytes(&files),
                files: Some(files),
                allowed,
            })
        }
    }
}

fn record(options: &SetupRenderOptions<'_>) -> SetupRecord {
    SetupRecord {
        renderer: options.renderer.clone(),
        hosting: options.hosting.clone(),
    }
}

/// Run the setup preflights and mutations `alien render` runs, with nothing install-specific.
async fn prepare(
    name: &str,
    stack: Stack,
    platform: Platform,
    base_platform: Option<Platform>,
    settings: &StackSettings,
) -> Result<Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, platform)
        .await
        .context(step(name, "run the setup preflights"))?;
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
        .maybe_management_config(placeholder_management_config(
            base_platform.unwrap_or(platform),
        ))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .maybe_base_platform(base_platform)
        .build();
    runner
        .apply_mutations(stack, &StackState::new(platform), &config)
        .await
        .context(step(name, "apply the setup mutations"))
}

/// Setup templates take the managing identity as a parameter; preflights only need a value.
fn placeholder_management_config(platform: Platform) -> Option<ManagementConfig> {
    match platform {
        Platform::Aws => Some(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        })),
        Platform::Gcp => Some(ManagementConfig::Gcp(GcpManagementConfig {
            federation_trust: None,
            service_account_email: "rendered-from-terraform-variable".to_string(),
        })),
        Platform::Azure => Some(ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "rendered-from-terraform-variable".to_string(),
            oidc_issuer: "rendered-from-terraform-variable".to_string(),
            oidc_subject: "rendered-from-terraform-variable".to_string(),
        })),
        _ => None,
    }
}

/// The bytes a setup template made of several files (a Terraform module, a Helm chart) is
/// signed as: the JSON object of file path to contents, keys sorted, no whitespace. Whoever
/// holds the files rebuilds these bytes to check them against the signed digest.
pub fn file_map_bytes(files: &BTreeMap<String, String>) -> Vec<u8> {
    serde_json::Value::Object(
        files
            .iter()
            .map(|(path, contents)| (path.clone(), serde_json::Value::String(contents.clone())))
            .collect(),
    )
    .to_string()
    .into_bytes()
}

fn step(name: &str, what: &str) -> ErrorData {
    ErrorData::RenderFailed {
        template: name.to_string(),
        message: format!("could not {what}"),
    }
}

fn render_failed(name: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RenderFailed {
        template: name.to_string(),
        message: message.into(),
    })
}
