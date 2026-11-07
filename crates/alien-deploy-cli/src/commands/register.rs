//! Register an externally-provisioned stack with a manager.
//!
//! Reads CloudFormation Stack Outputs via `DescribeStacks` from the
//! deployer's local AWS credentials, or a Terraform module's outputs via
//! `terraform output -json`. Hosted registration goes through the Platform
//! import boundary; an explicit manager URL selects standalone mode.
//!
//! A protected Terraform module carries no registration provider (that code
//! would run with the customer's credentials during `terraform apply`), so it
//! is registered this way after apply.

use std::collections::HashMap;

use crate::error::{ErrorData, Result};
use alien_core::{
    deployer_secret_value_refusal,
    embedded_config::DeployCliConfig,
    import::{ImportSourceKind, ImportedResource, StackImportRequest},
    ManagementConfig, Platform, ResourceType, StackSettings,
};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, ValueEnum};
use serde::Serialize;
use serde_json::Value as JsonValue;

use super::up::resolve_base_url_option;

/// The custom resource a CloudFormation setup template registers through, and its property
/// that changes with every protected template (alien-cloudformation's `REGISTRATION_RESOURCE`
/// and `SETUP_REVISION`).
const REGISTRATION_RESOURCE: &str = "DeploymentRegistration";
const SETUP_REVISION: &str = "SetupRevision";

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Register an externally-provisioned stack with a manager",
    after_help = "EXAMPLES:
    # Register the outputs of an `alien render --format cloudformation` stack
    alien-deploy register \\
        --import cloudformation \\
        --stack-name acme-prod \\
        --region us-east-1 \\
        --base-url https://api.alien.dev \\
        --token dg_... \\
        --input region=us-east-1 \\
        --input-json replicas=3

    # Register a Terraform module after `terraform apply` in ./setup
    alien-deploy register \\
        --import terraform \\
        --dir ./setup \\
        --token dg_..."
)]
pub struct RegisterArgs {
    /// Source the resolved import payload comes from.
    #[arg(long, value_enum)]
    pub import: ImportKind,

    /// CloudFormation stack name (required when --import cloudformation). Also
    /// used as the default deployment name when `--name` is omitted.
    #[arg(long)]
    pub stack_name: Option<String>,

    /// Directory `terraform apply` ran in, whose outputs are the module's
    /// (required when --import terraform).
    #[arg(long)]
    pub dir: Option<std::path::PathBuf>,

    /// Terraform command that reads the outputs (`terraform` or `tofu`).
    #[arg(long, default_value = "terraform")]
    pub terraform_command: String,

    /// Deployment name. Must be unique within the deployment group; the
    /// manager returns 409 on collision. Defaults to the source-specific
    /// natural name (CloudFormation: `--stack-name`; Terraform: the
    /// `deployment_name` output, which this must match when both are set).
    #[arg(long)]
    pub name: Option<String>,

    /// AWS region the CloudFormation stack lives in.
    #[arg(long, env = "AWS_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Standalone manager URL. Omit for hosted registration through Platform.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API URL for hosted registration. Defaults to the URL embedded
    /// in a packaged CLI, then https://api.alien.dev.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Deployment token authorizing the import.
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: String,

    /// String stack input for registration (id=value). Repeat for multiple inputs.
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Typed JSON stack input for registration (id=<json>). Repeat for multiple inputs.
    #[arg(long = "input-json")]
    pub json_input_values: Vec<String>,

    /// Refused: deployer secrets are written into your own secret store, never
    /// passed to Alien. Kept so an old invocation fails with what to do instead.
    #[arg(long = "secret-input-file", hide = true)]
    pub secret_input_files: Vec<String>,

    /// Print a diagnostic payload to stdout instead of POSTing it.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Cloudformation,
    Terraform,
}

pub async fn register_command(
    args: RegisterArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    match args.import {
        ImportKind::Cloudformation => register_cloudformation(args, embedded_config).await,
        ImportKind::Terraform => register_terraform(args, embedded_config).await,
    }
}

async fn register_cloudformation(
    args: RegisterArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    let stack_name = args.stack_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "--stack-name is required for --import cloudformation".to_string(),
        })
    })?;

    let deployment_name = args.name.clone().unwrap_or_else(|| stack_name.clone());

    let input_values = collect_registration_inputs(
        &args.input_values,
        &args.json_input_values,
        &args.secret_input_files,
    )?;
    let target = resolve_registration_target(
        args.manager_url.as_deref(),
        args.base_url.as_ref(),
        embedded_config,
    )?;

    let outputs = fetch_cloudformation_outputs(&args.region, &stack_name).await?;
    // A stack created through the console's quick-create registers through its custom resource
    // and has no registration outputs to send.
    if outputs.platform.is_none() {
        let template = fetch_cloudformation_template(&args.region, &stack_name).await?;
        if let Some(registered) = registered_by_custom_resource(&stack_name, &template)? {
            println!("{registered}");
            return Ok(());
        }
    }
    let request = build_import_request(
        &outputs,
        ImportSourceKind::CloudFormation,
        &deployment_name,
        &stack_name,
        input_values,
    )?;
    send_registration(&args, &target, &request, &format!("stack '{stack_name}'")).await
}

async fn register_terraform(
    args: RegisterArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    let dir = args.dir.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "--dir is required for --import terraform".to_string(),
        })
    })?;
    // The module's outputs carry the deployer's input answers, as its registration resource
    // sent them; a second source could disagree with what the module was applied with.
    if !args.input_values.is_empty() || !args.json_input_values.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "--input and --input-json don't apply to --import terraform: the module's deployment_input_values output carries the inputs it was applied with".to_string(),
        }));
    }
    collect_registration_inputs(&[], &[], &args.secret_input_files)?;
    let target = resolve_registration_target(
        args.manager_url.as_deref(),
        args.base_url.as_ref(),
        embedded_config,
    )?;

    let outputs = read_terraform_outputs(&args.terraform_command, &dir)?;
    let deployment_name = match (&args.name, &outputs.deployment_name) {
        (Some(flag), Some(output)) if flag != output => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "--name '{flag}' differs from the module's deployment_name output '{output}'"
                ),
            }));
        }
        (Some(name), _) | (None, Some(name)) => name.clone(),
        (None, None) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "the module has no deployment_name output; pass --name".to_string(),
            }));
        }
    };
    let input_values = match &outputs.input_values {
        None => HashMap::new(),
        Some(JsonValue::Object(values)) => values.clone().into_iter().collect(),
        Some(_) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "deployment_input_values output is not a JSON object".to_string(),
            }));
        }
    };
    let source = format!("module in {}", dir.display());
    let request = build_import_request(
        &outputs,
        ImportSourceKind::Terraform,
        &deployment_name,
        &source,
        input_values,
    )?;
    send_registration(&args, &target, &request, &source).await
}

/// Print the request (`--dry-run`) or send it where the deployment registers.
async fn send_registration(
    args: &RegisterArgs,
    target: &RegistrationTarget,
    request: &StackImportRequest,
    source: &str,
) -> Result<()> {
    if args.dry_run {
        let body = serialize_registration_request(target, request)?;
        let json = serde_json::to_string_pretty(&body)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize stack registration request".to_string(),
                reason: "Failed to serialize stack registration request".to_string(),
            })?;
        println!("{json}");
        return Ok(());
    }

    let request_body = serialize_registration_request(target, request)?;
    let (url, operation, destination) = match target {
        RegistrationTarget::Platform(base_url) => (
            format!("{base_url}/v1/deployments/import"),
            "POST /v1/deployments/import",
            "Platform",
        ),
        RegistrationTarget::StandaloneManager(manager_url) => (
            format!("{manager_url}/v1/stack/import"),
            "POST /v1/stack/import",
            "standalone manager",
        ),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "build manager HTTP client".to_string(),
            url: url.clone(),
            reason: "Failed to build HTTP client".to_string(),
        })?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", args.token))
        .json(&request_body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: operation.to_string(),
            url: url.clone(),
            reason: format!("{destination} request failed"),
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: operation.to_string(),
            url,
            reason: format!("{destination} returned {status}: {body}"),
        }));
    }

    println!("Registered {source} through {destination} at {url}");
    println!("{body}");
    Ok(())
}

enum RegistrationTarget {
    Platform(String),
    StandaloneManager(String),
}

fn resolve_registration_target(
    manager_url: Option<&str>,
    base_url: Option<&String>,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<RegistrationTarget> {
    match (manager_url, base_url) {
        (Some(manager_url), _) => Ok(RegistrationTarget::StandaloneManager(
            manager_url.trim_end_matches('/').to_string(),
        )),
        (None, Some(base_url)) => Ok(RegistrationTarget::Platform(
            base_url.trim_end_matches('/').to_string(),
        )),
        (None, None) => Ok(RegistrationTarget::Platform(
            resolve_base_url_option(None, embedded_config)
                .trim_end_matches('/')
                .to_string(),
        )),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardImportRequest<'a> {
    mode: &'static str,
    source: ForwardImportSource<'a>,
    input_values: &'a HashMap<String, JsonValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardImportSource<'a> {
    deployment_name: &'a str,
    resource_prefix: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_kind: Option<ImportSourceKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup_metadata: Option<&'a JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_id: Option<&'a str>,
    platform: Platform,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_platform: Option<Platform>,
    region: &'a str,
    setup_target: &'a str,
    setup_import_format_version: u32,
    setup_fingerprint: &'a str,
    setup_fingerprint_version: u32,
    stack_settings: &'a StackSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    management_config: Option<&'a ManagementConfig>,
    resources: &'a [ImportedResource],
}

fn serialize_registration_request(
    target: &RegistrationTarget,
    request: &StackImportRequest,
) -> Result<JsonValue> {
    match target {
        RegistrationTarget::Platform(_) => serde_json::to_value(ForwardImportRequest {
            mode: "forward",
            source: ForwardImportSource {
                deployment_name: &request.deployment_name,
                resource_prefix: &request.resource_prefix,
                source_kind: request.source_kind,
                setup_metadata: request.setup_metadata.as_ref(),
                release_id: request.release_id.as_deref(),
                platform: request.platform,
                base_platform: request.base_platform,
                region: &request.region,
                setup_target: &request.setup_target,
                setup_import_format_version: request.setup_import_format_version,
                setup_fingerprint: &request.setup_fingerprint,
                setup_fingerprint_version: request.setup_fingerprint_version,
                stack_settings: &request.stack_settings,
                management_config: request.management_config.as_ref(),
                resources: &request.resources,
            },
            input_values: &request.input_values,
        }),
        RegistrationTarget::StandaloneManager(_) => serde_json::to_value(request),
    }
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "serialize stack registration request".to_string(),
        reason: "Failed to serialize stack registration request".to_string(),
    })
}

fn collect_registration_inputs(
    input_values: &[String],
    json_input_values: &[String],
    secret_input_files: &[String],
) -> Result<HashMap<String, JsonValue>> {
    // No deployer secret value may reach the platform: refuse before any
    // secret file is read.
    if let Some(input) = secret_input_files.first() {
        let (id, _) = parse_input_assignment(input, "--secret-input-file")?;
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "secret-input-file".to_string(),
            message: deployer_secret_value_refusal(&id),
        }));
    }

    let mut values = HashMap::new();
    for input in input_values {
        let (id, value) = parse_input_assignment(input, "--input")?;
        insert_registration_input(&mut values, id, JsonValue::String(value))?;
    }
    for input in json_input_values {
        let (id, raw_value) = parse_input_assignment(input, "--input-json")?;
        let value = serde_json::from_str(&raw_value)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: format!("parse --input-json value for '{id}'"),
                reason: format!("Stack input '{id}' is not valid JSON"),
            })?;
        if !is_supported_stack_input_value(&value) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input-json".to_string(),
                message: format!(
                    "Stack input '{id}' must be a string, number, boolean, or array of strings"
                ),
            }));
        }
        insert_registration_input(&mut values, id, value)?;
    }

    Ok(values)
}

fn is_supported_stack_input_value(value: &JsonValue) -> bool {
    matches!(
        value,
        JsonValue::String(_) | JsonValue::Number(_) | JsonValue::Bool(_)
    ) || matches!(value, JsonValue::Array(values) if values.iter().all(JsonValue::is_string))
}

fn parse_input_assignment(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: use id=value"),
        }));
    };
    let id = id.trim();
    if id.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id is required"),
        }));
    }
    Ok((id.to_string(), value.to_string()))
}

fn insert_registration_input(
    values: &mut HashMap<String, JsonValue>,
    id: String,
    value: JsonValue,
) -> Result<()> {
    if values.insert(id.clone(), value).is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "input".to_string(),
            message: format!("Stack input '{id}' was provided more than once"),
        }));
    }
    Ok(())
}

/// Resolved setup outputs needed to assemble a `StackImportRequest`. Mirrors
/// the CloudFormation Stack Outputs `alien-cloudformation` emits for
/// `RegistrationMode::OutputsFallback` / `Both`, and the Terraform module
/// outputs of the same names in snake case (`DeploymentPlatform` is
/// `deployment_platform`).
#[derive(Debug, Default)]
struct SetupOutputs {
    source_kind: Option<String>,
    platform: Option<String>,
    base_platform: Option<String>,
    region: Option<String>,
    resource_prefix: Option<String>,
    setup_target: Option<String>,
    setup_import_format_version: Option<u32>,
    setup_fingerprint: Option<String>,
    setup_fingerprint_version: Option<u32>,
    management_config: Option<JsonValue>,
    stack_settings: Option<JsonValue>,
    /// Resources may be split across `DeploymentResources0`..`DeploymentResourcesN`
    /// when chunking kicks in, or a single `DeploymentResources` output when
    /// the payload fits.
    resources_chunks: Vec<JsonValue>,
    /// Terraform only: the deployment name the module was applied with.
    deployment_name: Option<String>,
    /// Terraform only: the deployer's input answers the module was applied with.
    input_values: Option<JsonValue>,
}

async fn fetch_cloudformation_outputs(region: &str, stack_name: &str) -> Result<SetupOutputs> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_string()))
        .load()
        .await;
    let client = aws_sdk_cloudformation::Client::new(&config);

    let response = client
        .describe_stacks()
        .stack_name(stack_name)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "DescribeStacks".to_string(),
            url: format!("aws://cloudformation/{region}/{stack_name}"),
            reason: "DescribeStacks request failed".to_string(),
        })?;

    let stack = response
        .stacks
        .as_ref()
        .and_then(|stacks| stacks.first())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "CloudFormation stack '{stack_name}' not found in region {region}"
                ),
            })
        })?;

    let pairs = stack
        .outputs
        .iter()
        .flatten()
        .filter_map(|output| Some((output.output_key()?, output.output_value()?)));
    collect_outputs(pairs)
}

async fn fetch_cloudformation_template(region: &str, stack_name: &str) -> Result<String> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_string()))
        .load()
        .await;
    aws_sdk_cloudformation::Client::new(&config)
        .get_template()
        .stack_name(stack_name)
        .template_stage(aws_sdk_cloudformation::types::TemplateStage::Original)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "GetTemplate".to_string(),
            url: format!("aws://cloudformation/{region}/{stack_name}"),
            reason: "GetTemplate request failed".to_string(),
        })?
        .template_body()
        .map(str::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("CloudFormation stack '{stack_name}' returned no template"),
            })
        })
}

/// Whether stack `stack_name`, created from `template`, registers through its own custom
/// resource: `Some` with what to tell the user when there is nothing for `register` to do,
/// `None` when it has no such resource. A protected template's resource carries
/// `SetupRevision`, so every setup update ran it again; an older one runs only when what it
/// reports changes, and `register` can't stand in for it (the resources it reports are resolved
/// inside the stack).
fn registered_by_custom_resource(stack_name: &str, template: &str) -> Result<Option<String>> {
    let template: serde_yaml::Value = serde_yaml::from_str(template).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("the template of stack '{stack_name}' can't be read"),
        },
    )?;
    let registration = &template["Resources"][REGISTRATION_RESOURCE];
    if registration["Type"].as_str() != Some("AWS::CloudFormation::CustomResource") {
        return Ok(None);
    }
    if registration["Properties"][SETUP_REVISION].is_null() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Stack '{stack_name}' registers through its {} custom resource, which runs again only when what it reports changes, and it has no registration outputs for this command to send. Apply a setup update rendered by a newer release; its registration runs on every setup update",
                REGISTRATION_RESOURCE
            ),
        }));
    }
    Ok(Some(format!(
        "✓ Stack '{stack_name}' registers through its {} custom resource, which ran again with this setup update. Nothing to register.",
        REGISTRATION_RESOURCE
    )))
}

/// Collect outputs keyed by their CloudFormation names.
fn collect_outputs<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<SetupOutputs> {
    let mut outputs = SetupOutputs::default();
    let mut chunked: Vec<(usize, JsonValue)> = Vec::new();
    for (key, value) in pairs {
        match key {
            "DeploymentSourceKind" => outputs.source_kind = Some(value.to_string()),
            "DeploymentResourcePrefix" => {
                outputs.resource_prefix = Some(value.to_string());
            }
            "DeploymentPlatform" => outputs.platform = Some(value.to_string()),
            "DeploymentBasePlatform" => outputs.base_platform = Some(value.to_string()),
            "DeploymentRegion" => outputs.region = Some(value.to_string()),
            "DeploymentSetupTarget" => outputs.setup_target = Some(value.to_string()),
            "DeploymentSetupImportFormatVersion" => {
                let version = value.parse().map_err(|reason| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!(
                        "DeploymentSetupImportFormatVersion output '{value}' is invalid: {reason}"
                    ),
                    })
                })?;
                outputs.setup_import_format_version = Some(version);
            }
            "DeploymentSetupFingerprint" => outputs.setup_fingerprint = Some(value.to_string()),
            "DeploymentSetupFingerprintVersion" => {
                let version = value.parse().map_err(|reason| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!(
                        "DeploymentSetupFingerprintVersion output '{value}' is invalid: {reason}"
                    ),
                    })
                })?;
                outputs.setup_fingerprint_version = Some(version);
            }
            "DeploymentManagementConfig" => {
                outputs.management_config = Some(parse_json_output(key, value)?);
            }
            "DeploymentStackSettings" => {
                outputs.stack_settings = Some(parse_json_output(key, value)?);
            }
            "DeploymentResources" => {
                chunked.push((0, parse_json_output(key, value)?));
            }
            other if other.starts_with("DeploymentResources") => {
                let suffix = &other["DeploymentResources".len()..];
                let index: usize = suffix.parse().unwrap_or(usize::MAX);
                chunked.push((index, parse_json_output(other, value)?));
            }
            "DeploymentName" => outputs.deployment_name = Some(value.to_string()),
            "DeploymentInputValues" => {
                outputs.input_values = Some(parse_json_output(key, value)?);
            }
            _ => {}
        }
    }

    chunked.sort_by_key(|(index, _value)| *index);
    outputs.resources_chunks = chunked.into_iter().map(|(_index, value)| value).collect();
    Ok(outputs)
}

/// Read the outputs of the Terraform root module in `dir` with `terraform output -json`.
fn read_terraform_outputs(command: &str, dir: &std::path::Path) -> Result<SetupOutputs> {
    let output = std::process::Command::new(command)
        .arg(format!("-chdir={}", dir.display()))
        .args(["output", "-json"])
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "could not run `{command} output -json` in {}",
                dir.display()
            ),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "`{command} output -json` in {} failed ({}): {}",
                dir.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    terraform_outputs(&output.stdout)
}

/// Parse `terraform output -json` (`{"name": {"value": ..., "type": ..., "sensitive": ...}}`).
/// Outputs keep their CloudFormation names in snake case, so both sources share one reader.
fn terraform_outputs(json: &[u8]) -> Result<SetupOutputs> {
    #[derive(serde::Deserialize)]
    struct TerraformOutput {
        value: JsonValue,
    }
    let outputs: std::collections::BTreeMap<String, TerraformOutput> = serde_json::from_slice(json)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse terraform output -json".to_string(),
            reason: "terraform output -json printed something other than its JSON map".to_string(),
        })?;
    if !outputs.contains_key("deployment_resource_prefix") {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "the Terraform outputs have no deployment_resource_prefix; run this in the directory `terraform apply` ran in, with the setup module as the root module".to_string(),
        }));
    }
    let pairs = outputs
        .into_iter()
        .filter(|(name, _)| name.starts_with("deployment_"))
        .map(|(name, output)| {
            let value = match output.value {
                JsonValue::String(text) => text,
                other => other.to_string(),
            };
            (cloudformation_output_name(&name), value)
        })
        .collect::<Vec<_>>();
    collect_outputs(
        pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    )
}

/// `deployment_setup_target` -> `DeploymentSetupTarget`.
fn cloudformation_output_name(terraform_name: &str) -> String {
    terraform_name
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// The name an output has in `source`: `DeploymentPlatform` in CloudFormation,
/// `deployment_platform` in Terraform.
fn output_name(source: ImportSourceKind, cloudformation_name: &str) -> String {
    match source {
        ImportSourceKind::Terraform => {
            let mut name = String::new();
            for (index, ch) in cloudformation_name.chars().enumerate() {
                if ch.is_ascii_uppercase() && index > 0 {
                    name.push('_');
                }
                name.push(ch.to_ascii_lowercase());
            }
            name
        }
        ImportSourceKind::CloudFormation | ImportSourceKind::Helm => {
            cloudformation_name.to_string()
        }
    }
}

fn parse_json_output(key: &str, value: &str) -> Result<JsonValue> {
    serde_json::from_str(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: format!("parse CFN output {key}"),
            reason: format!("Output '{key}' is not valid JSON"),
        })
}

/// The import request a setup's outputs describe. `source` names the setup in errors.
fn build_import_request(
    outputs: &SetupOutputs,
    source_kind: ImportSourceKind,
    deployment_name: &str,
    source: &str,
    input_values: HashMap<String, JsonValue>,
) -> Result<StackImportRequest> {
    let name = |cloudformation_name: &str| output_name(source_kind, cloudformation_name);
    let missing = |cloudformation_name: &str| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("output {} not found in {source}", name(cloudformation_name)),
        })
    };
    let expected_kind = match source_kind {
        ImportSourceKind::CloudFormation => "cloudformation",
        ImportSourceKind::Terraform => "terraform",
        ImportSourceKind::Helm => "helm",
    };
    if let Some(other) = outputs
        .source_kind
        .as_deref()
        .filter(|kind| *kind != expected_kind)
    {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{} output '{other}' is not '{expected_kind}'",
                name("DeploymentSourceKind")
            ),
        }));
    }

    let platform: Platform = match outputs.platform.as_deref() {
        Some(p) => p.parse().map_err(|reason| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "{} output '{p}' is invalid: {reason}",
                    name("DeploymentPlatform")
                ),
            })
        })?,
        None => return Err(missing("DeploymentPlatform")),
    };
    let base_platform = outputs
        .base_platform
        .as_deref()
        .map(|p| {
            p.parse().map_err(|reason| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "{} output '{p}' is invalid: {reason}",
                        name("DeploymentBasePlatform")
                    ),
                })
            })
        })
        .transpose()?;

    let region = outputs
        .region
        .clone()
        .ok_or_else(|| missing("DeploymentRegion"))?;
    let resource_prefix = outputs
        .resource_prefix
        .clone()
        .ok_or_else(|| missing("DeploymentResourcePrefix"))?;
    let setup_target = outputs
        .setup_target
        .clone()
        .ok_or_else(|| missing("DeploymentSetupTarget"))?;
    let setup_fingerprint = outputs
        .setup_fingerprint
        .clone()
        .ok_or_else(|| missing("DeploymentSetupFingerprint"))?;
    let setup_fingerprint_version = outputs
        .setup_fingerprint_version
        .ok_or_else(|| missing("DeploymentSetupFingerprintVersion"))?;

    let management_config_value = outputs
        .management_config
        .clone()
        .ok_or_else(|| missing("DeploymentManagementConfig"))?;
    let management_config: ManagementConfig = serde_json::from_value(management_config_value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialize ManagementConfig".to_string(),
            reason: "DeploymentManagementConfig has unexpected shape".to_string(),
        })?;

    let mut stack_settings_value = outputs
        .stack_settings
        .clone()
        .ok_or_else(|| missing("DeploymentStackSettings"))?;
    if source_kind == ImportSourceKind::CloudFormation {
        normalize_cloudformation_stack_settings(&mut stack_settings_value)?;
    }
    let stack_settings: StackSettings = serde_json::from_value(stack_settings_value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialize StackSettings".to_string(),
            reason: "DeploymentStackSettings has unexpected shape".to_string(),
        })?;

    let mut resources: Vec<ImportedResource> = Vec::new();
    for chunk in &outputs.resources_chunks {
        let JsonValue::Array(items) = chunk else {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "DeploymentResources chunk is not a JSON array".to_string(),
            }));
        };
        for item in items {
            let mut imported: ImportedResourceWire = serde_json::from_value(item.clone())
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialize ImportedResource".to_string(),
                    reason: "DeploymentResources item has unexpected shape".to_string(),
                })?;
            if source_kind == ImportSourceKind::CloudFormation {
                normalize_cloudformation_import_data(
                    &imported.resource_type,
                    &mut imported.import_data,
                )?;
            }
            resources.push(ImportedResource {
                id: imported.id,
                resource_type: ResourceType::from(imported.resource_type),
                import_data: imported.import_data,
            });
        }
    }

    let setup_import_format_version = outputs
        .setup_import_format_version
        .ok_or_else(|| missing("DeploymentSetupImportFormatVersion"))?;
    // A module reports its setup only when it was rendered for registration; the import
    // can't name a release without them.
    if source_kind == ImportSourceKind::Terraform
        && (setup_target.is_empty() || setup_fingerprint.is_empty())
    {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{source} reports no setup target or fingerprint; only modules rendered for registration can be registered"
            ),
        }));
    }

    Ok(StackImportRequest {
        setup_import_format_version,
        deployment_group_token: String::new(),
        deployment_name: deployment_name.to_string(),
        resource_prefix,
        source_kind: Some(source_kind),
        setup_metadata: None,
        release_id: None,
        platform,
        base_platform,
        region,
        setup_target,
        setup_fingerprint,
        setup_fingerprint_version,
        stack_settings,
        management_config: Some(management_config),
        input_values,
        resources,
    })
}

/// CloudFormation resolves `Number` parameter references inside
/// `Fn::ToJsonString` as JSON strings. Convert only the numeric fields the
/// generated StackSettings contract sources from those parameters.
fn normalize_cloudformation_stack_settings(value: &mut JsonValue) -> Result<()> {
    normalize_u32_string(value, "/network/availability_zones")?;

    let Some(pools) = value
        .pointer_mut("/compute/pools")
        .and_then(JsonValue::as_object_mut)
    else {
        return Ok(());
    };
    for (pool_id, pool) in pools {
        for field in ["machines", "min", "max"] {
            let Some(field_value) = pool.get_mut(field) else {
                continue;
            };
            normalize_u32_value(field_value, &format!("compute.pools.{pool_id}.{field}"))?;
        }
    }
    Ok(())
}

fn normalize_u32_string(value: &mut JsonValue, pointer: &str) -> Result<()> {
    if let Some(field) = value.pointer_mut(pointer) {
        normalize_u32_value(field, pointer.trim_start_matches('/'))?;
    }
    Ok(())
}

fn normalize_u32_value(value: &mut JsonValue, field: &str) -> Result<()> {
    let JsonValue::String(text) = value else {
        return Ok(());
    };
    let number = text.parse::<u32>().map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "DeploymentStackSettings field '{field}' is not a valid unsigned integer"
            ),
        })
    })?;
    *value = JsonValue::from(number);
    Ok(())
}

/// `AWS::NoValue` becomes `null` inside an `Fn::ToJsonString` list. Generated
/// network lists gate only their trailing availability-zone entries, so
/// trim trailing nulls while rejecting an interior null as malformed output.
fn normalize_cloudformation_import_data(
    resource_type: &str,
    import_data: &mut JsonValue,
) -> Result<()> {
    if resource_type != "network" {
        return Ok(());
    }
    for field in ["publicSubnetIds", "privateSubnetIds", "availabilityZones"] {
        let Some(values) = import_data.get_mut(field).and_then(JsonValue::as_array_mut) else {
            continue;
        };
        while values.last().is_some_and(JsonValue::is_null) {
            values.pop();
        }
        if values.iter().any(JsonValue::is_null) {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "CloudFormation network import field '{field}' contains a non-trailing null"
                ),
            }));
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportedResourceWire {
    id: String,
    #[serde(rename = "type")]
    resource_type: String,
    import_data: JsonValue,
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use alien_core::AwsManagementConfig;

    #[test]
    fn the_registration_names_are_the_generators() {
        assert_eq!(
            REGISTRATION_RESOURCE,
            alien_cloudformation::REGISTRATION_RESOURCE
        );
        assert_eq!(SETUP_REVISION, alien_cloudformation::SETUP_REVISION);
    }

    /// A quick-created stack registers through its custom resource and has no registration
    /// outputs. A protected template's resource runs again on every setup update, so register
    /// has nothing to do; an older one can't be stood in for, and register says how to get
    /// there; a stack with neither still fails on the missing output.
    #[test]
    fn a_stack_registered_by_its_custom_resource_is_told_apart() {
        let template = |properties: &str| {
            format!(
                "Resources:\n  DeploymentRegistration:\n    Type: AWS::CloudFormation::CustomResource\n    Properties:\n      ServiceToken: arn:aws:lambda:us-east-1:1:function:r\n{properties}"
            )
        };
        let current =
            registered_by_custom_resource("acme", &template("      SetupRevision: sha256:abcd\n"))
                .unwrap()
                .expect("nothing to register");
        assert!(
            current.contains("ran again with this setup update"),
            "{current}"
        );

        let older = registered_by_custom_resource("acme", &template("")).unwrap_err();
        assert!(
            older
                .message
                .contains("runs again only when what it reports changes"),
            "{}",
            older.message
        );

        assert_eq!(
            registered_by_custom_resource(
                "acme",
                "Resources:\n  Bucket:\n    Type: AWS::S3::Bucket\n"
            )
            .unwrap(),
            None
        );
    }

    fn base_outputs() -> SetupOutputs {
        SetupOutputs {
            source_kind: Some("cloudformation".to_string()),
            platform: Some("kubernetes".to_string()),
            base_platform: Some("aws".to_string()),
            region: Some("us-east-1".to_string()),
            resource_prefix: Some("e2e-cfn-eks".to_string()),
            setup_target: Some("eks".to_string()),
            setup_import_format_version: Some(1),
            setup_fingerprint: Some("test".to_string()),
            setup_fingerprint_version: Some(1),
            management_config: Some(
                serde_json::to_value(ManagementConfig::Aws(AwsManagementConfig {
                    managing_role_arn: "arn:aws:iam::123456789012:role/manager".to_string(),
                }))
                .expect("management config"),
            ),
            stack_settings: Some(
                serde_json::to_value(StackSettings::default()).expect("stack settings"),
            ),
            resources_chunks: vec![JsonValue::Array(Vec::new())],
            deployment_name: None,
            input_values: None,
        }
    }

    #[test]
    fn cloudformation_import_request_preserves_base_platform() {
        let request = build_import_request(
            &base_outputs(),
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect("request");

        assert_eq!(request.platform, Platform::Kubernetes);
        assert_eq!(request.base_platform, Some(Platform::Aws));
        assert_eq!(request.setup_target, "eks");
    }

    /// The exact `DeploymentResources` shape read back from a live stack that was
    /// deployed with a gated `Kv` declined beside an ungated `Queue`. Captured
    /// from `describe-stacks` rather than written by hand, because this value only
    /// exists after CloudFormation resolves the conditions; the account id and
    /// stack name were normalized after capture.
    const DECLINED_RESOURCE_PAYLOAD: &str = r#"[{"id":"jobs","type":"queue","importData":{"queueName":"example-gate-Jobs-Xy2Abc3dEfGh","queueUrl":"https://sqs.us-east-1.amazonaws.com/123456789012/example-gate-Jobs-Xy2Abc3dEfGh","queueArn":"arn:aws:sqs:us-east-1:123456789012:example-gate-Jobs-Xy2Abc3dEfGh"}}]"#;

    fn outputs_with_resources(payload: &str) -> SetupOutputs {
        SetupOutputs {
            resources_chunks: vec![
                parse_json_output("DeploymentResources", payload).expect("payload is JSON")
            ],
            ..base_outputs()
        }
    }

    #[test]
    fn a_declined_resource_is_absent_from_the_import_request() {
        let request = build_import_request(
            &outputs_with_resources(DECLINED_RESOURCE_PAYLOAD),
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect("a payload with a declined resource omitted should import");

        assert_eq!(request.resources.len(), 1);
        assert_eq!(request.resources[0].id, "jobs");
        assert_eq!(
            request.resources[0].resource_type,
            ResourceType::from("queue")
        );
    }

    /// The reason the payload has to be correct at the source: there is no
    /// skip-on-null anywhere in this path, so a declined resource left behind as
    /// a null fails the whole import. Making this pass by tolerating nulls would
    /// hide the next producer that emits one.
    #[test]
    fn a_null_entry_fails_the_import_rather_than_being_skipped() {
        let error = build_import_request(
            &outputs_with_resources(r#"[null,{"id":"jobs","type":"queue","importData":{}}]"#),
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect_err("a null entry must not be silently dropped");

        assert!(
            format!("{error:?}").contains("deserialize ImportedResource"),
            "expected the typed importer to reject the null: {error:?}"
        );
    }

    #[test]
    fn registration_inputs_preserve_explicit_types() {
        let collected = collect_registration_inputs(
            &["region=us-east-1".to_string()],
            &[
                "replicas=3".to_string(),
                "enabled=true".to_string(),
                "zones=[\"a\",\"b\"]".to_string(),
            ],
            &[],
        )
        .expect("typed inputs should be collected");

        assert_eq!(collected["region"], serde_json::json!("us-east-1"));
        assert_eq!(collected["replicas"], serde_json::json!(3));
        assert_eq!(collected["enabled"], serde_json::json!(true));
        assert_eq!(collected["zones"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn secret_input_files_are_refused_before_they_are_read() {
        let secret = "secret-that-must-not-leave-the-host";
        let mut file = tempfile::NamedTempFile::new().expect("secret fixture");
        writeln!(file, "{secret}").expect("write secret fixture");

        let error = collect_registration_inputs(
            &["region=us-east-1".to_string()],
            &[],
            &[format!("apiKey={}", file.path().display())],
        )
        .expect_err("a secret input file is refused");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(
            error.message.contains("'apiKey' is a deployer secret"),
            "{}",
            error.message
        );
        assert!(!format!("{error:?}").contains(secret));

        // Refused without touching the file system at all.
        let error = collect_registration_inputs(&[], &[], &["apiKey=/does/not/exist".to_string()])
            .expect_err("refused before the path is opened");
        assert!(
            error.message.contains("deployer secret"),
            "{}",
            error.message
        );
    }

    #[test]
    fn platform_forward_request_preserves_typed_inputs_without_manager_credentials() {
        let secret = "private-value";
        let inputs = HashMap::from([
            ("region".to_string(), serde_json::json!("us-east-1")),
            ("apiKey".to_string(), serde_json::json!(secret)),
        ]);
        let request = build_import_request(
            &base_outputs(),
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            inputs,
        )
        .expect("request");

        let body = serialize_registration_request(
            &RegistrationTarget::Platform("https://api.example.test".to_string()),
            &request,
        )
        .expect("serialize Platform forward import");

        assert_eq!(body["mode"], "forward");
        assert_eq!(body["source"]["deploymentName"], "app");
        assert_eq!(body["source"]["basePlatform"], "aws");
        assert_eq!(body["inputValues"]["apiKey"], secret);
        assert_eq!(body["inputValues"]["region"], "us-east-1");
        assert!(body.get("deploymentGroupToken").is_none());
        assert!(body["source"].get("deploymentGroupToken").is_none());
    }

    #[test]
    fn manager_url_wins_over_a_base_url() {
        let base_url = "https://api.example.test".to_string();
        let target = resolve_registration_target(
            Some("https://manager.example.test"),
            Some(&base_url),
            None,
        )
        .expect("manager selection should remain explicit");

        let RegistrationTarget::StandaloneManager(url) = target else {
            panic!("manager URL must take precedence over a base URL");
        };
        assert_eq!(url, "https://manager.example.test");
    }

    #[test]
    fn registration_inputs_reject_json_outside_the_platform_contract() {
        for unsupported in ["null", "{}", "[1,2]", "[\"a\",2]"] {
            let error = collect_registration_inputs(&[], &[format!("value={unsupported}")], &[])
                .err()
                .expect("unsupported JSON must fail before registration");

            assert_eq!(error.code, "VALIDATION_ERROR");
            assert!(error.message.contains("array of strings"));
        }
    }

    #[test]
    fn duplicate_registration_input_is_rejected_across_sources() {
        let result = collect_registration_inputs(
            &["replicas=3".to_string()],
            &["replicas=3".to_string()],
            &[],
        );
        let error = match result {
            Ok(_) => panic!("duplicates must not depend on argument ordering"),
            Err(error) => error,
        };

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("provided more than once"));
    }

    #[test]
    fn resolved_cloudformation_outputs_normalize_typed_settings_and_subnet_lists() {
        let mut outputs = base_outputs();
        outputs.stack_settings = Some(serde_json::json!({
            "network": {
                "type": "create",
                "cidr": "10.20.0.0/16",
                "availability_zones": "2"
            },
            "compute": {
                "pools": {
                    "general": {
                        "mode": "autoscale",
                        "min": "1",
                        "max": "3",
                        "machine": "m7g.large"
                    },
                    "jobs": {
                        "mode": "fixed",
                        "machines": "2",
                        "machine": "m7g.xlarge"
                    }
                }
            }
        }));
        outputs.resources_chunks = vec![serde_json::json!([{
            "id": "network",
            "type": "network",
            "importData": {
                "vpcId": "vpc-123",
                "publicSubnetIds": ["subnet-public-a", "subnet-public-b", null],
                "privateSubnetIds": ["subnet-private-a", "subnet-private-b", null],
                "availabilityZones": ["us-east-1a", "us-east-1b", null]
            }
        }])];

        let request = build_import_request(
            &outputs,
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect("resolved CloudFormation outputs should satisfy the typed import contract");
        let settings = serde_json::to_value(&request.stack_settings).expect("stack settings");
        assert_eq!(settings["network"]["availability_zones"], 2);
        assert_eq!(settings["compute"]["pools"]["general"]["min"], 1);
        assert_eq!(settings["compute"]["pools"]["general"]["max"], 3);
        assert_eq!(settings["compute"]["pools"]["jobs"]["machines"], 2);
        assert_eq!(
            request.resources[0].import_data["publicSubnetIds"],
            serde_json::json!(["subnet-public-a", "subnet-public-b"])
        );
        assert_eq!(
            request.resources[0].import_data["privateSubnetIds"],
            serde_json::json!(["subnet-private-a", "subnet-private-b"])
        );
        assert_eq!(
            request.resources[0].import_data["availabilityZones"],
            serde_json::json!(["us-east-1a", "us-east-1b"])
        );
    }

    #[test]
    fn malformed_cloudformation_numeric_setting_still_fails() {
        let mut outputs = base_outputs();
        outputs.stack_settings = Some(serde_json::json!({
            "network": {"type": "create", "availability_zones": "two"}
        }));

        let error = build_import_request(
            &outputs,
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect_err("non-numeric text must not be coerced");
        assert!(
            format!("{error:?}").contains("network/availability_zones"),
            "the error should identify the malformed field: {error:?}"
        );
    }

    #[test]
    fn interior_network_null_still_fails() {
        let mut outputs = base_outputs();
        outputs.resources_chunks = vec![serde_json::json!([{
            "id": "network",
            "type": "network",
            "importData": {
                "publicSubnetIds": ["subnet-public-a", null, "subnet-public-c"],
                "privateSubnetIds": ["subnet-private-a"]
            }
        }])];

        let error = build_import_request(
            &outputs,
            ImportSourceKind::CloudFormation,
            "app",
            "stack",
            HashMap::new(),
        )
        .expect_err("an interior null is not a declined trailing AZ");
        assert!(
            format!("{error:?}").contains("non-trailing null"),
            "the malformed array should fail explicitly: {error:?}"
        );
    }

    mod terraform {
        //! `register --import terraform` against each protected module as rendered for a
        //! release: the outputs it declares, with the values `terraform output -json`
        //! prints for them, read through a stand-in `terraform` command.

        use std::os::unix::fs::PermissionsExt as _;

        use super::super::*;
        use crate::commands::verify_terraform::tests::{module, stack};
        use crate::commands::verify_terraform::ModuleCloud;

        /// What a module carries besides the providers the clouds publish.
        const PUBLIC_PROVIDERS: &[&str] = &["hashicorp/", "Azure/azapi"];

        fn module_files(bytes: &[u8]) -> std::collections::BTreeMap<String, String> {
            serde_json::from_slice(bytes).expect("module file map")
        }

        /// Every provider source the module requires.
        fn provider_sources(files: &std::collections::BTreeMap<String, String>) -> Vec<String> {
            let versions: hcl::Body = hcl::from_str(&files["versions.tf"]).expect("versions.tf");
            let mut sources = Vec::new();
            for terraform in versions.blocks().filter(|b| b.identifier() == "terraform") {
                for providers in terraform
                    .body()
                    .blocks()
                    .filter(|b| b.identifier() == "required_providers")
                {
                    for attribute in providers.body().attributes() {
                        let hcl::Expression::Object(object) = attribute.expr() else {
                            panic!("provider {} is not an object", attribute.key());
                        };
                        let source = object
                            .iter()
                            .find(|(key, _)| key.to_string().trim_matches('"') == "source")
                            .map(|(_, value)| value.to_string())
                            .unwrap_or_else(|| {
                                panic!("provider {} has no source", attribute.key())
                            });
                        sources.push(source.trim_matches('"').to_string());
                    }
                }
            }
            sources
        }

        /// Output name to its expression, from `outputs.tf`.
        fn declared_outputs(
            files: &std::collections::BTreeMap<String, String>,
        ) -> std::collections::BTreeMap<String, hcl::Expression> {
            let outputs: hcl::Body = hcl::from_str(&files["outputs.tf"]).expect("outputs.tf");
            outputs
                .blocks()
                .filter(|block| block.identifier() == "output")
                .map(|block| {
                    let name = block.labels()[0].as_str().to_string();
                    let value = block
                        .body()
                        .attributes()
                        .find(|attribute| attribute.key() == "value")
                        .expect("output value")
                        .expr()
                        .clone();
                    (name, value)
                })
                .collect()
        }

        fn literal(expression: &hcl::Expression) -> JsonValue {
            match expression {
                hcl::Expression::String(text) => JsonValue::String(text.clone()),
                hcl::Expression::Number(number) => {
                    serde_json::from_str(&number.to_string()).expect("number")
                }
                other => panic!("expected a literal, got {other:?}"),
            }
        }

        /// The signing settings the module merges last into the settings it reports.
        fn reported_signing(files: &std::collections::BTreeMap<String, String>) -> JsonValue {
            let locals: hcl::Body = hcl::from_str(&files["locals.tf"]).expect("locals.tf");
            let settings = locals
                .blocks()
                .filter(|block| block.identifier() == "locals")
                .flat_map(|block| block.body().attributes())
                .find(|attribute| attribute.key() == "deployment_settings")
                .expect("deployment_settings local")
                .expr()
                .clone();
            let hcl::Expression::FuncCall(call) = settings else {
                panic!("deployment_settings reports no signing settings: {settings}");
            };
            assert_eq!(call.name.to_string(), "merge");
            let object = |expression: &hcl::Expression| -> Vec<(String, hcl::Expression)> {
                let hcl::Expression::Object(object) = expression else {
                    panic!("expected an object, got {expression}");
                };
                object
                    .iter()
                    .map(|(key, value)| {
                        (key.to_string().trim_matches('"').to_string(), value.clone())
                    })
                    .collect()
            };
            let last = call.args.last().expect("merge arguments");
            let (_, signing) = object(last)
                .into_iter()
                .find(|(key, _)| key == "signing")
                .unwrap_or_else(|| panic!("the last settings merged are not signing: {last}"));
            JsonValue::Object(
                object(&signing)
                    .into_iter()
                    .map(|(key, value)| (key, literal(&value)))
                    .collect(),
            )
        }

        /// What `terraform output -json` prints after apply: the module's literals as
        /// written, the computed values as each cloud resolves them.
        fn recorded_outputs(
            cloud: ModuleCloud,
            declared: &std::collections::BTreeMap<String, hcl::Expression>,
            signing: JsonValue,
        ) -> JsonValue {
            let settings = serde_json::json!({ "setupManagement": "manual", "signing": signing });
            let (region, management, resources) = match cloud {
                ModuleCloud::Aws => (
                    "us-east-1",
                    serde_json::json!({ "platform": "aws", "managingRoleArn": "arn:aws:iam::111122223333:role/manager" }),
                    serde_json::json!([{ "id": "assets", "type": "storage", "importData": { "bucketName": "acme-prod-assets" } }]),
                ),
                ModuleCloud::Gcp => (
                    "us-central1",
                    serde_json::json!({ "platform": "gcp", "serviceAccountEmail": "manager@example-project.iam.gserviceaccount.com" }),
                    serde_json::json!([{ "id": "assets", "type": "storage", "importData": { "bucketName": "acme-prod-assets" } }]),
                ),
                ModuleCloud::Azure => (
                    "eastus",
                    serde_json::json!({ "platform": "azure", "managingTenantId": "00000000-0000-0000-0000-000000000001", "oidcIssuer": "https://issuer.example", "oidcSubject": "manager" }),
                    serde_json::json!([{ "id": "assets", "type": "storage", "importData": { "storageAccountName": "acmeprodassets", "containerName": "assets" } }]),
                ),
            };
            let mut outputs = serde_json::Map::new();
            for (name, expression) in declared {
                let value = match name.as_str() {
                    "deployment_name" => JsonValue::from("acme-prod"),
                    "deployment_resource_prefix" => JsonValue::from("acme-prod"),
                    "deployment_platform" => JsonValue::from(cloud.platform()),
                    "deployment_region" => JsonValue::from(region),
                    "deployment_management_config" => JsonValue::from(management.to_string()),
                    "deployment_stack_settings" => JsonValue::from(settings.to_string()),
                    "deployment_resources" => JsonValue::from(resources.to_string()),
                    "deployment_signing_vault" => JsonValue::from("acme-prod-signin-a1b2c3"),
                    _ => match expression {
                        hcl::Expression::String(_) | hcl::Expression::Number(_) => {
                            literal(expression)
                        }
                        _ => JsonValue::from("computed"),
                    },
                };
                outputs.insert(
                    name.clone(),
                    serde_json::json!({ "sensitive": false, "type": "string", "value": value }),
                );
            }
            JsonValue::Object(outputs)
        }

        /// A `terraform` that prints `recorded` for `-chdir=<dir> output -json` only.
        fn stand_in_terraform(dir: &std::path::Path, recorded: &JsonValue) -> std::path::PathBuf {
            let recorded_path = dir.join("recorded.json");
            std::fs::write(&recorded_path, recorded.to_string()).unwrap();
            let script = dir.join("terraform");
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\n[ \"$1\" = \"-chdir={}\" ] && [ \"$2\" = output ] && [ \"$3\" = -json ] && [ $# -eq 3 ] || {{ echo \"unexpected: $*\" >&2; exit 3; }}\ncat '{}'\n",
                    dir.display(),
                    recorded_path.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            script
        }

        #[tokio::test]
        async fn each_protected_module_registers_from_its_outputs() {
            for cloud in [ModuleCloud::Aws, ModuleCloud::Gcp, ModuleCloud::Azure] {
                let files = module_files(&module(cloud).await.bytes);

                // No provider of the service that hosts the module: only the clouds' own, so
                // no registration resource either (a resource needs its provider).
                for source in provider_sources(&files) {
                    assert!(
                        PUBLIC_PROVIDERS
                            .iter()
                            .any(|public| source.starts_with(public)),
                        "{cloud:?} module requires provider {source}"
                    );
                }

                let declared = declared_outputs(&files);
                assert_eq!(
                    declared.contains_key("deployment_signing_vault"),
                    cloud == ModuleCloud::Azure,
                    "{cloud:?}: the signing vault output is Azure's only"
                );
                let dir = tempfile::tempdir().unwrap();
                let signing = reported_signing(&files);
                assert_eq!(
                    signing,
                    serde_json::json!({
                        "fingerprint": crate::commands::verify::tests::fingerprint(),
                        "channel": "production"
                    }),
                    "{cloud:?}: the module reports the signing settings it pins"
                );
                let terraform =
                    stand_in_terraform(dir.path(), &recorded_outputs(cloud, &declared, signing));
                let outputs = read_terraform_outputs(terraform.to_str().unwrap(), dir.path())
                    .expect("outputs read");
                let request = build_import_request(
                    &outputs,
                    ImportSourceKind::Terraform,
                    outputs
                        .deployment_name
                        .as_deref()
                        .expect("deployment_name output"),
                    "module",
                    match &outputs.input_values {
                        Some(JsonValue::Object(values)) => values.clone().into_iter().collect(),
                        _ => HashMap::new(),
                    },
                )
                .expect("request");

                let (fingerprint, version) = alien_cli_common::setup_fingerprint::setup_fingerprint(
                    &serde_json::to_value(stack()).unwrap(),
                    cloud.platform(),
                );
                assert_eq!(request.setup_target, cloud.platform(), "{cloud:?}");
                assert_eq!(request.setup_fingerprint, fingerprint, "{cloud:?}");
                assert_eq!(request.setup_fingerprint_version, version, "{cloud:?}");
                assert_eq!(
                    request.setup_import_format_version,
                    literal(&declared["deployment_setup_import_format_version"])
                        .as_u64()
                        .unwrap() as u32
                );
                assert_eq!(request.source_kind, Some(ImportSourceKind::Terraform));
                assert_eq!(request.release_id, None);
                assert_eq!(request.deployment_name, "acme-prod");
                assert_eq!(request.resource_prefix, "acme-prod");
                assert_eq!(request.platform.as_str(), cloud.platform());
                assert_eq!(request.base_platform, None);
                let signing = request.stack_settings.signing.as_ref().expect("protected");
                assert_eq!(signing.channel, "production");
                assert_eq!(
                    signing.fingerprint,
                    crate::commands::verify::tests::fingerprint()
                );
                assert_eq!(request.resources.len(), 1);
                assert_eq!(request.resources[0].id, "assets");
                assert!(request.input_values.is_empty());

                let body = serialize_registration_request(
                    &RegistrationTarget::Platform("https://api.example".to_string()),
                    &request,
                )
                .unwrap();
                let source = &body["source"];
                assert_eq!(body["mode"], "forward");
                assert_eq!(source["sourceKind"], "terraform");
                assert_eq!(source["setupTarget"], cloud.platform());
                assert_eq!(source["setupFingerprint"], fingerprint.as_str());
                assert!(source.get("releaseId").is_none(), "{cloud:?}: {body}");
            }
        }

        fn protected_outputs_dir() -> (tempfile::TempDir, JsonValue) {
            let mut declared = std::collections::BTreeMap::new();
            for name in [
                "deployment_name",
                "deployment_resource_prefix",
                "deployment_platform",
                "deployment_region",
                "deployment_management_config",
                "deployment_stack_settings",
                "deployment_resources",
            ] {
                declared.insert(name.to_string(), hcl::Expression::Null);
            }
            declared.insert(
                "deployment_setup_target".to_string(),
                hcl::Expression::String("aws".to_string()),
            );
            declared.insert(
                "deployment_setup_fingerprint".to_string(),
                hcl::Expression::String("abc".to_string()),
            );
            declared.insert(
                "deployment_setup_fingerprint_version".to_string(),
                hcl::Expression::Number(1.into()),
            );
            declared.insert(
                "deployment_setup_import_format_version".to_string(),
                hcl::Expression::Number(1.into()),
            );
            (
                tempfile::tempdir().unwrap(),
                recorded_outputs(
                    ModuleCloud::Aws,
                    &declared,
                    serde_json::json!({ "fingerprint": "SHA256:abc", "channel": "production" }),
                ),
            )
        }

        fn args(dir: &std::path::Path, terraform: &std::path::Path) -> RegisterArgs {
            RegisterArgs::parse_from([
                "register",
                "--import",
                "terraform",
                "--dir",
                dir.to_str().unwrap(),
                "--terraform-command",
                terraform.to_str().unwrap(),
                "--token",
                "dg_test",
                "--manager-url",
                "http://127.0.0.1:9",
                "--dry-run",
            ])
        }

        #[tokio::test]
        async fn a_dry_run_reads_the_module_and_sends_nothing() {
            let (dir, recorded) = protected_outputs_dir();
            let terraform = stand_in_terraform(dir.path(), &recorded);
            register_command(args(dir.path(), &terraform), None)
                .await
                .expect("dry run");
        }

        #[tokio::test]
        async fn inputs_on_the_command_line_are_refused() {
            let (dir, recorded) = protected_outputs_dir();
            let terraform = stand_in_terraform(dir.path(), &recorded);
            let mut args = args(dir.path(), &terraform);
            args.input_values = vec!["region=us-east-1".to_string()];
            let error = register_command(args, None).await.expect_err("refused");
            assert!(
                error.message.contains("deployment_input_values"),
                "{}",
                error.message
            );
        }

        #[tokio::test]
        async fn a_name_other_than_the_modules_is_refused() {
            let (dir, recorded) = protected_outputs_dir();
            let terraform = stand_in_terraform(dir.path(), &recorded);
            let mut args = args(dir.path(), &terraform);
            args.name = Some("someone-else".to_string());
            let error = register_command(args, None).await.expect_err("refused");
            assert!(
                error
                    .message
                    .contains("differs from the module's deployment_name"),
                "{}",
                error.message
            );
        }

        #[tokio::test]
        async fn a_module_without_a_setup_fingerprint_is_refused() {
            let (dir, mut recorded) = protected_outputs_dir();
            recorded["deployment_setup_fingerprint"]["value"] = JsonValue::from("");
            let terraform = stand_in_terraform(dir.path(), &recorded);
            let error = register_command(args(dir.path(), &terraform), None)
                .await
                .expect_err("refused");
            assert!(
                error
                    .message
                    .contains("reports no setup target or fingerprint"),
                "{}",
                error.message
            );
        }

        #[tokio::test]
        async fn outputs_of_another_root_module_are_refused() {
            let (dir, mut recorded) = protected_outputs_dir();
            recorded
                .as_object_mut()
                .unwrap()
                .remove("deployment_resource_prefix");
            let terraform = stand_in_terraform(dir.path(), &recorded);
            let error = register_command(args(dir.path(), &terraform), None)
                .await
                .expect_err("refused");
            assert!(
                error.message.contains("no deployment_resource_prefix"),
                "{}",
                error.message
            );
        }

        #[tokio::test]
        async fn a_failing_terraform_fails_the_registration() {
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("terraform");
            std::fs::write(&script, "#!/bin/sh\necho 'No outputs found' >&2\nexit 1\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            let error = register_command(args(dir.path(), &script), None)
                .await
                .expect_err("refused");
            assert!(
                error.message.contains("No outputs found"),
                "{}",
                error.message
            );
        }
    }
}
