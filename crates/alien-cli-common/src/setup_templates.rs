//! Render every setup template of a protected stack, the way a signed release records them.
//!
//! `alien release` signs the digest of each template in
//! [`alien_signing::release::SETUP_TEMPLATES`] together with the parameters a customer may
//! choose, and the service that hosts the templates renders them again for installs. Both
//! must produce the same bytes, so [`render_setup_templates`] depends only on its inputs: the
//! stack, its signing settings, the emitter registries and the hosting options, rendered by
//! the alien version that is running. It reads no environment, clock, or cloud.

use std::collections::BTreeMap;

use alien_cloudformation::{
    CfRegistry, CloudFormationOptions, CloudFormationTarget, ManagementTrustOptions,
    RegistrationMode,
};
use alien_core::{
    AwsManagementConfig, AzureManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, GcpManagementConfig, ManagementConfig, Platform, SetupPins, Stack,
    StackSettings, StackSigning, StackState,
};
use alien_error::{AlienError, AlienErrorData, Context, IntoAlienError};
use alien_helm::{HelmOptions, HelmRegistry};
use alien_signing::release::{SetupTemplate, SETUP_TEMPLATES};
use alien_terraform::{TerraformOptions, TerraformRegistration, TerraformTarget, TfRegistry};
use bon::Builder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// How the CloudFormation stack reports itself after it is created.
    pub registration: RegistrationMode,
    /// CloudFormation template description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Who may manage the stack.
    #[serde(default)]
    pub management_trust: ManagementTrustOptions,
    /// AWS regions the templates accept; empty accepts any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_aws_regions: Vec<String>,
    /// How Terraform modules report themselves; none for modules registered out of band.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terraform_registration: Option<TerraformRegistration>,
    /// The hosting service's fingerprint of this setup's shape, reported at registration.
    pub setup_fingerprint: String,
    /// Version of the scheme behind `setup_fingerprint`.
    pub setup_fingerprint_version: u32,
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
    /// Machine-image publisher and verifier image. Leave the default, this build's own, except
    /// in tests: a release records templates rendered with it.
    #[builder(default = SetupPins::compiled_in())]
    pub pins: SetupPins,
}

/// One rendered setup template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSetupTemplate {
    /// The template bytes. CloudFormation is the YAML template. Terraform modules and Helm
    /// charts are several files; their bytes are the JSON object of file path to contents,
    /// keys sorted, so a reader can recompute the digest from the files.
    pub bytes: Vec<u8>,
    /// CloudFormation parameters, Terraform variables or top-level Helm values the customer
    /// may choose. Everything else keeps the template's pinned value.
    pub allowed: Vec<String>,
}

impl RenderedSetupTemplate {
    /// The digest and allow-list a signed release records for this template.
    pub fn setup_template(&self) -> SetupTemplate {
        SetupTemplate {
            sha256: format!("sha256:{}", hex::encode(Sha256::digest(&self.bytes))),
            allowed: self.allowed.clone(),
        }
    }
}

/// Render each template in [`SETUP_TEMPLATES`] for `stack` with signing on, keyed by name.
pub async fn render_setup_templates(
    stack: &Stack,
    options: SetupRenderOptions<'_>,
) -> Result<BTreeMap<String, RenderedSetupTemplate>> {
    let settings = StackSettings {
        signing: Some(options.signing.clone()),
        ..StackSettings::default()
    };
    let mut rendered = BTreeMap::new();
    for &name in SETUP_TEMPLATES {
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
/// [`render_setup_templates`] calls this with signing on and nothing else set; other settings
/// make templates a release does not record. `options.signing` is not used here.
pub async fn render_setup_template(
    name: &str,
    source: &Stack,
    settings: &StackSettings,
    options: &SetupRenderOptions<'_>,
) -> Result<RenderedSetupTemplate> {
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
    let stack = prepare(name, source_stack.clone(), platform, base_platform, settings).await?;

    match name {
        "cloudformation" => {
            let template = alien_cloudformation::generate_cloudformation_template(
                &stack,
                CloudFormationOptions {
                    pins: options.pins.clone(),
                    registry: options.cloudformation_registry,
                    source_stack: &source_stack,
                    target: CloudFormationTarget::Aws,
                    stack_settings: settings.clone(),
                    setup_target: "aws".to_string(),
                    setup_fingerprint: options.hosting.setup_fingerprint.clone(),
                    setup_fingerprint_version: options.hosting.setup_fingerprint_version,
                    registration: options.hosting.registration.clone(),
                    description: options.hosting.description.clone(),
                    management_trust: options.hosting.management_trust.clone(),
                },
            )
            .context(step(name, "generate the template"))?;
            let allowed = template
                .parameters
                .keys()
                .filter(|parameter| parameter.as_str() != alien_cloudformation::SIGNING_FINGERPRINT_PARAMETER)
                .cloned()
                .collect();
            let yaml = alien_cloudformation::to_yaml(&template)
                .context(step(name, "write the template as YAML"))?;
            Ok(RenderedSetupTemplate {
                bytes: yaml.into_bytes(),
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
            let files: BTreeMap<String, String> = chart.files.into_iter().collect();
            Ok(RenderedSetupTemplate {
                bytes: file_map_bytes(name, &files)?,
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
                    pins: options.pins.clone(),
                    display_name: None,
                    registry: options.terraform_registry,
                    stack_settings: settings.clone(),
                    registration: options.hosting.terraform_registration.clone(),
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
            let files: BTreeMap<String, String> = module
                .iter()
                .map(|(path, contents)| (path.to_string(), contents.to_string()))
                .collect();
            Ok(RenderedSetupTemplate {
                bytes: file_map_bytes(name, &files)?,
                allowed,
            })
        }
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

fn file_map_bytes(name: &str, files: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    serde_json::to_vec(files)
        .into_alien_error()
        .context(step(name, "encode the files as JSON"))
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
