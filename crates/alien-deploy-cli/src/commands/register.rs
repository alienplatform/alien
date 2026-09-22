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

use std::{collections::HashMap, path::PathBuf};

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
        --token dg_... \\
        --input region=us-east-1 \\
        --input-json replicas=3 \\
        --secret-input-file apiKey=/run/secrets/api-key"
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

    /// Deployment token authorizing the import.
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: String,

    /// String stack input for registration (id=value). Repeat for multiple inputs.
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Typed JSON stack input for registration (id=<json>). Repeat for multiple inputs.
    #[arg(long = "input-json")]
    pub json_input_values: Vec<String>,

    /// Secret stack input read from a file (id=path). Repeat for multiple inputs.
    /// A single trailing newline is removed from the file contents.
    #[arg(long = "secret-input-file")]
    pub secret_input_files: Vec<String>,

    /// Print a diagnostic payload to stdout instead of POSTing it.
    /// File-backed secret values are redacted, so the output is not reusable.
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
    let registration_inputs = collect_registration_inputs(
        &args.input_values,
        &args.json_input_values,
        &args.secret_input_files,
    )?;
    let request = build_import_request(
        &outputs,
        &deployment_name,
        &stack_name,
        registration_inputs.values,
    )?;

    if args.dry_run {
        let redacted_request = redact_secret_inputs(&request, &registration_inputs.secret_ids);
        let json = serde_json::to_string_pretty(&redacted_request)
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

    println!("Registered stack '{stack_name}' with the manager at {url}");
    println!("{body}");
    Ok(())
}

struct RegistrationInputs {
    values: HashMap<String, JsonValue>,
    secret_ids: Vec<String>,
}

fn redact_secret_inputs(request: &StackImportRequest, secret_ids: &[String]) -> StackImportRequest {
    let mut redacted = request.clone();
    for input_id in secret_ids {
        if let Some(value) = redacted.input_values.get_mut(input_id) {
            *value = JsonValue::String("[REDACTED]".to_string());
        }
    }
    redacted
}

fn collect_registration_inputs(
    input_values: &[String],
    json_input_values: &[String],
    secret_input_files: &[String],
) -> Result<RegistrationInputs> {
    let mut values = HashMap::new();
    let mut secret_ids = Vec::new();

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
        insert_registration_input(&mut values, id, value)?;
    }
    for input in secret_input_files {
        let (id, path) = parse_input_assignment(input, "--secret-input-file")?;
        let mut value = std::fs::read_to_string(PathBuf::from(&path))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read secret input file".to_string(),
                file_path: path,
                reason: format!("Could not load value for stack input '{id}'"),
            })?;
        if value.ends_with('\n') {
            value.pop();
            if value.ends_with('\r') {
                value.pop();
            }
        }
        insert_registration_input(&mut values, id.clone(), JsonValue::String(value))?;
        secret_ids.push(id);
    }

    Ok(RegistrationInputs { values, secret_ids })
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

/// Resolved CloudFormation Stack Outputs needed to assemble a
/// `StackImportRequest`. Mirrors the keys emitted by
/// `alien-cloudformation`'s `RegistrationMode::OutputsFallback` /
/// `Both`.
#[derive(Debug, Default)]
struct CfnOutputs {
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
    deployment_name: &str,
    stack_name: &str,
    input_values: HashMap<String, JsonValue>,
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
    let base_platform = outputs
        .base_platform
        .as_deref()
        .map(|p| {
            p.parse().map_err(|reason| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("DeploymentBasePlatform output '{p}' is invalid: {reason}"),
                })
            })
        })
        .transpose()?;

    let region = outputs.region.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentRegion output not found in stack".to_string(),
        })
    })?;
    let resource_prefix = outputs.resource_prefix.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("DeploymentResourcePrefix output not found in stack '{stack_name}'"),
        })
    })?;
    let setup_target = outputs.setup_target.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentSetupTarget output not found in stack".to_string(),
        })
    })?;
    let setup_fingerprint = outputs.setup_fingerprint.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentSetupFingerprint output not found in stack".to_string(),
        })
    })?;
    let setup_fingerprint_version = outputs.setup_fingerprint_version.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentSetupFingerprintVersion output not found in stack".to_string(),
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

    let setup_import_format_version = outputs.setup_import_format_version.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "CloudFormation output DeploymentSetupImportFormatVersion is required"
                .to_string(),
        })
    })?;

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

    fn base_outputs() -> CfnOutputs {
        CfnOutputs {
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
        }
    }

    #[test]
    fn cloudformation_import_request_preserves_base_platform() {
        let request =
            build_import_request(&base_outputs(), "app", "stack", HashMap::new()).expect("request");

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

    fn outputs_with_resources(payload: &str) -> CfnOutputs {
        CfnOutputs {
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
    fn registration_inputs_preserve_explicit_types_and_secret_file_contents() {
        let secret = "secret-that-must-not-appear-in-argv";
        let mut file = tempfile::NamedTempFile::new().expect("secret fixture");
        writeln!(file, "{secret}").expect("write secret fixture");

        let secret_arg = format!("apiKey={}", file.path().display());
        assert!(!secret_arg.contains(secret));
        let collected = collect_registration_inputs(
            &["region=us-east-1".to_string()],
            &[
                "replicas=3".to_string(),
                "enabled=true".to_string(),
                "zones=[\"a\",\"b\"]".to_string(),
            ],
            &[secret_arg],
        )
        .expect("typed inputs should be collected");

        assert_eq!(collected.values["region"], serde_json::json!("us-east-1"));
        assert_eq!(collected.values["replicas"], serde_json::json!(3));
        assert_eq!(collected.values["enabled"], serde_json::json!(true));
        assert_eq!(collected.values["zones"], serde_json::json!(["a", "b"]));
        assert_eq!(collected.values["apiKey"], serde_json::json!(secret));
        assert_eq!(collected.secret_ids, vec!["apiKey"]);
    }

    #[test]
    fn dry_run_redacts_secret_inputs_without_changing_the_request() {
        let secret = "private-value";
        let inputs = HashMap::from([
            ("region".to_string(), serde_json::json!("us-east-1")),
            ("apiKey".to_string(), serde_json::json!(secret)),
        ]);
        let request =
            build_import_request(&base_outputs(), "app", "stack", inputs).expect("request");

        let redacted = redact_secret_inputs(&request, &["apiKey".to_string()]);
        let output = serde_json::to_string(&redacted).expect("serialize dry run");

        assert!(!output.contains(secret));
        assert!(output.contains("[REDACTED]"));
        assert_eq!(request.input_values["apiKey"], serde_json::json!(secret));
        assert_eq!(
            redacted.input_values["region"],
            serde_json::json!("us-east-1")
        );
        assert!(redacted.deployment_group_token.is_empty());
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
}
