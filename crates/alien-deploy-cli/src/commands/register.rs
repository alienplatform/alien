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

    /// Deployment token authorizing the import.
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

    let mut stack_settings_value = outputs.stack_settings.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeploymentStackSettings output not found in stack".to_string(),
        })
    })?;
    normalize_cloudformation_stack_settings(&mut stack_settings_value)?;
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
            normalize_cloudformation_import_data(
                &imported.resource_type,
                &mut imported.import_data,
            )?;
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
        deployment_group_token: token.to_string(),
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
        input_values: Default::default(),
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
            build_import_request(&base_outputs(), "dg_token", "app", "stack").expect("request");

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
            "dg_token",
            "app",
            "stack",
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
            "dg_token",
            "app",
            "stack",
        )
        .expect_err("a null entry must not be silently dropped");

        assert!(
            format!("{error:?}").contains("deserialize ImportedResource"),
            "expected the typed importer to reject the null: {error:?}"
        );
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

        let request = build_import_request(&outputs, "dg_token", "app", "stack")
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

        let error = build_import_request(&outputs, "dg_token", "app", "stack")
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

        let error = build_import_request(&outputs, "dg_token", "app", "stack")
            .expect_err("an interior null is not a declined trailing AZ");
        assert!(
            format!("{error:?}").contains("non-trailing null"),
            "the malformed array should fail explicitly: {error:?}"
        );
    }
}
