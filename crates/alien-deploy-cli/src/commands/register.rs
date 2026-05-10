//! Register an externally-provisioned stack with a manager.
//!
//! Today's only flow: read CloudFormation Stack Outputs via
//! `DescribeStacks` from the customer's local AWS credentials and POST
//! the resolved import payload to the manager's `/v1/stack/import`
//! endpoint.
//!
//! Future flows (Terraform `alien_deployment` provider, Helm boot path)
//! land alongside under the same subcommand surface keyed on
//! `--import <kind>`.

use crate::error::{ErrorData, Result};
use alien_core::{
    import::{ImportSourceKind, ImportedResource, StackImportRequest},
    ManagementConfig, Platform, ResourceType, StackSettings,
};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, ValueEnum};
use serde_json::Value as JsonValue;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Register an externally-provisioned stack with a manager",
    after_help = "EXAMPLES:
    # Register the outputs of an `alien render --format cloudformation` stack
    alien-deploy register \\
        --import cloudformation \\
        --stack-name acme-prod \\
        --region us-east-1 \\
        --manager-url https://manager.example.com \\
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

    /// Deployment name. Must be unique within the deployment group; the
    /// manager returns 409 on collision. Defaults to the source-specific
    /// natural name (CloudFormation: `--stack-name`).
    #[arg(long)]
    pub name: Option<String>,

    /// AWS region the CloudFormation stack lives in.
    #[arg(long, env = "AWS_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Manager URL the resolved payload is POSTed to.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: String,

    /// Deployment-group token authorizing the import.
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: String,

    /// Print the resolved payload to stdout instead of POSTing it.
    /// Useful for debugging or for piping into `curl`.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Cloudformation,
}

pub async fn register_command(args: RegisterArgs) -> Result<()> {
    match args.import {
        ImportKind::Cloudformation => register_cloudformation(args).await,
    }
}

async fn register_cloudformation(args: RegisterArgs) -> Result<()> {
    let stack_name = args.stack_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "--stack-name is required for --import cloudformation".to_string(),
        })
    })?;

    let deployment_name = args.name.clone().unwrap_or_else(|| stack_name.clone());

    let outputs = fetch_cloudformation_outputs(&args.region, &stack_name).await?;
    let request = build_import_request(&outputs, &args.token, &deployment_name, &stack_name)?;

    if args.dry_run {
        let json = serde_json::to_string_pretty(&request)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize stack import request".to_string(),
                reason: "Failed to serialize import request".to_string(),
            })?;
        println!("{json}");
        return Ok(());
    }

    let url = format!("{}/v1/stack/import", args.manager_url.trim_end_matches('/'));
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
        .json(&request)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "POST /v1/stack/import".to_string(),
            url: url.clone(),
            reason: "Manager request failed".to_string(),
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: "POST /v1/stack/import".to_string(),
            url,
            reason: format!("Manager returned {status}: {body}"),
        }));
    }

    println!("Imported stack '{stack_name}' into manager at {url}");
    println!("{body}");
    Ok(())
}

/// Resolved CloudFormation Stack Outputs needed to assemble a
/// `StackImportRequest`. Mirrors the keys emitted by
/// `alien-cloudformation`'s `RegistrationMode::OutputsFallback` /
/// `Both`.
#[derive(Debug, Default)]
struct CfnOutputs {
    source_kind: Option<String>,
    platform: Option<String>,
    region: Option<String>,
    stack_prefix: Option<String>,
    management_config: Option<JsonValue>,
    stack_settings: Option<JsonValue>,
    /// Resources may be split across `DeploymentResources0`..`DeploymentResourcesN`
    /// when chunking kicks in, or a single `DeploymentResources` output when
    /// the payload fits.
    resources_chunks: Vec<JsonValue>,
}

async fn fetch_cloudformation_outputs(region: &str, stack_name: &str) -> Result<CfnOutputs> {
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

    let mut outputs = CfnOutputs::default();
    let mut chunked: Vec<(usize, JsonValue)> = Vec::new();

    if let Some(stack_outputs) = &stack.outputs {
        for output in stack_outputs {
            let Some(key) = output.output_key() else {
                continue;
            };
            let Some(value) = output.output_value() else {
                continue;
            };
            match key {
                "DeploymentSourceKind" => outputs.source_kind = Some(value.to_string()),
                "DeploymentStackPrefix" => {
                    outputs.stack_prefix = Some(value.to_string());
                }
                "DeploymentPlatform" => outputs.platform = Some(value.to_string()),
                "DeploymentRegion" => outputs.region = Some(value.to_string()),
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
                _ => {}
            }
        }
    }

    chunked.sort_by_key(|(index, _value)| *index);
    outputs.resources_chunks = chunked.into_iter().map(|(_index, value)| value).collect();
    Ok(outputs)
}

fn parse_json_output(key: &str, value: &str) -> Result<JsonValue> {
    serde_json::from_str(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: format!("parse CFN output {key}"),
            reason: format!("Output '{key}' is not valid JSON"),
        })
}

fn build_import_request(
    outputs: &CfnOutputs,
    token: &str,
    deployment_name: &str,
    stack_name: &str,
) -> Result<StackImportRequest> {
    let source_kind: ImportSourceKind = match outputs.source_kind.as_deref() {
        Some("cloudformation") => ImportSourceKind::CloudFormation,
        Some(other) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!("DeploymentSourceKind output '{other}' is not 'cloudformation'"),
            }));
        }
        None => ImportSourceKind::CloudFormation,
    };

    let platform: Platform = match outputs.platform.as_deref() {
        Some(p) => p.parse().map_err(|reason| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("DeploymentPlatform output '{p}' is invalid: {reason}"),
            })
        })?,
        None => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "DeploymentPlatform output not found in stack".to_string(),
            }));
        }
    };

    let region = outputs.region.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentRegion output not found in stack".to_string(),
        })
    })?;
    let stack_prefix = outputs.stack_prefix.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("DeploymentStackPrefix output not found in stack '{stack_name}'"),
        })
    })?;

    let management_config_value = outputs.management_config.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentManagementConfig output not found in stack".to_string(),
        })
    })?;
    let management_config: ManagementConfig = serde_json::from_value(management_config_value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialize ManagementConfig".to_string(),
            reason: "DeploymentManagementConfig has unexpected shape".to_string(),
        })?;

    let stack_settings_value = outputs.stack_settings.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentStackSettings output not found in stack".to_string(),
        })
    })?;
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
            let imported: ImportedResourceWire = serde_json::from_value(item.clone())
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialize ImportedResource".to_string(),
                    reason: "DeploymentResources item has unexpected shape".to_string(),
                })?;
            resources.push(ImportedResource {
                id: imported.id,
                resource_type: ResourceType::from(imported.resource_type),
                import_data: imported.import_data,
            });
        }
    }

    Ok(StackImportRequest {
        deployment_group_token: token.to_string(),
        deployment_name: deployment_name.to_string(),
        stack_prefix,
        source_kind: Some(source_kind),
        release_id: None,
        platform,
        region,
        stack_settings,
        management_config,
        resources,
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportedResourceWire {
    id: String,
    #[serde(rename = "type")]
    resource_type: String,
    import_data: JsonValue,
}
