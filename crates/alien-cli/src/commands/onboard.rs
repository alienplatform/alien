use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_text};
use crate::ui::{accent, command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_core::{Stack, StackInputDefinition, StackInputKind, StackInputProvider};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use std::collections::HashMap;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Onboard a customer and generate a deployment link or token",
    long_about = "Create a deployment group for a customer and generate a deployment link (platform) or CLI command (standalone) to share with their admin."
)]
pub struct OnboardArgs {
    /// Customer name
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Maximum number of deployments for this customer
    #[arg(long, default_value = "100")]
    pub max_deployments: u64,

    /// Output in JSON format (for scripting)
    #[arg(long)]
    pub json: bool,

    /// Plain environment variables for deployments created from this link (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "env")]
    pub env_vars: Vec<String>,

    /// Secret environment variables for deployments created from this link (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "secret")]
    pub secret_vars: Vec<String>,

    /// Stack input value provided before creating the deployment link (id=value)
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value provided before creating the deployment link (id=value)
    #[arg(long = "secret-input")]
    pub secret_input_values: Vec<String>,
}

pub async fn onboard_task(args: OnboardArgs, ctx: ExecutionMode) -> Result<()> {
    let name = if let Some(ref name) = args.name {
        name.clone()
    } else if args.json || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Customer name is required in non-interactive mode. Pass `alien onboard <name>`."
                    .to_string(),
        }));
    } else {
        prompt_text("Customer name", None)?
    };

    match ctx {
        #[cfg(feature = "platform")]
        ExecutionMode::Platform { .. } => onboard_platform(args, ctx, name).await,
        _ => onboard_standalone(args, ctx, name).await,
    }
}

/// Platform mode: use Platform API directly to get deployment link.
#[cfg(feature = "platform")]
async fn onboard_platform(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_platform_api::SdkResultExt;

    let setup_environment_variables = platform_setup_environment_variables(
        &crate::parse_env_and_secret_vars(&args.env_vars, &args.secret_vars)?,
    )?;

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let client = ctx.sdk_client().await?;
    let developer_inputs = fetch_developer_stack_inputs(&client, &workspace, &project_id).await?;
    let stack_input_values = collect_stack_input_values(
        &developer_inputs,
        &args.input_values,
        &args.secret_input_values,
        args.json,
    )?;

    if !args.json {
        println!("{}", contextual_heading("Onboarding", &name, &[]));
        print_required_developer_inputs(&developer_inputs);
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Create deployment group", "Generate deployment link"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    // Create deployment group via Platform API
    let workspace_param =
        alien_platform_api::types::CreateDeploymentGroupWorkspace::try_from(workspace.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "workspace".to_string(),
                    message: format!("Invalid workspace: {}", e),
                })
            })?;

    let response = client
        .create_deployment_group()
        .workspace(&workspace_param)
        .body(alien_platform_api::types::CreateDeploymentGroupRequest {
            name: name.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: format!("{}", e),
                })
            })?,
            project: project_id.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "project".to_string(),
                    message: format!("{}", e),
                })
            })?,
            max_deployments: std::num::NonZeroU64::new(args.max_deployments as u64)
                .unwrap_or(std::num::NonZeroU64::new(100).unwrap()),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;

    let deployment_group_id = response.id.clone();

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Generating deployment link".to_string()));
    }

    // Create token via Platform API — returns deploymentLink
    let token_workspace_param =
        alien_platform_api::types::CreateDeploymentGroupTokenWorkspace::try_from(
            workspace.as_str(),
        )
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "workspace".to_string(),
                message: format!("Invalid workspace: {}", e),
            })
        })?;

    let dg_id_param = alien_platform_api::types::CreateDeploymentGroupTokenId::try_from(
        deployment_group_id.as_str(),
    )
    .map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message: format!("Invalid deployment group ID: {}", e),
        })
    })?;

    let token_response = client
        .create_deployment_group_token()
        .id(&dg_id_param)
        .workspace(&token_workspace_param)
        .body(
            alien_platform_api::types::CreateDeploymentGroupTokenRequest {
                description: None,
                expires_at: None,
                deployment_setup_config: platform_onboard_deployment_setup_config(
                    setup_environment_variables,
                ),
                input_values: Some(stack_input_values),
            },
        )
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to generate deployment link".to_string(),
            url: None,
        })?;

    let deployment_link = token_response.deployment_link.clone();

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "deploymentLink": deployment_link,
            "maxDeployments": args.max_deployments,
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment link ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!();
    println!(
        "{}",
        dim_label("Share this link with the customer's admin:")
    );
    println!("  {}", accent(&deployment_link));
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}

#[cfg(feature = "platform")]
fn platform_onboard_deployment_setup_config(
    environment_variables: Vec<alien_platform_api::types::EnvironmentVariableConfig>,
) -> alien_platform_api::types::DeploymentSetupConfig {
    use alien_platform_api::types;

    types::DeploymentSetupConfig {
        metadata: types::DeploymentSetupMetadata(serde_json::Map::new()),
        policy: types::DeploymentSetupPolicy {
            allow_release_pinning: None,
            allowed_platforms: vec![
                types::DeploymentSetupPolicyAllowedPlatformsItem::Aws,
                types::DeploymentSetupPolicyAllowedPlatformsItem::Gcp,
                types::DeploymentSetupPolicyAllowedPlatformsItem::Azure,
                types::DeploymentSetupPolicyAllowedPlatformsItem::Kubernetes,
                types::DeploymentSetupPolicyAllowedPlatformsItem::Local,
            ],
            allowed_setup_methods: vec![
                types::DeploymentSetupMethod::Cloudformation,
                types::DeploymentSetupMethod::GoogleOauth,
                types::DeploymentSetupMethod::Terraform,
                types::DeploymentSetupMethod::Helm,
                types::DeploymentSetupMethod::Cli,
                types::DeploymentSetupMethod::Manual,
            ],
            stack_settings: Some(types::DeploymentSetupStackSettingsPolicy {
                allow_custom_registry: Some(true),
                allow_external_bindings: Some(true),
                allowed_deployment_models: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Push,
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Pull,
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Airgapped,
                ],
                allowed_heartbeats_modes: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem::On,
                    types::DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem::Off,
                ],
                allowed_network_modes: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::None,
                    types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Create,
                    types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Default,
                    types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Byo,
                ],
                allowed_telemetry_modes: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::Off,
                    types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::Auto,
                    types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::ApprovalRequired,
                ],
                allowed_updates_modes: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem::Auto,
                    types::DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem::ApprovalRequired,
                ],
                defaults: None,
            }),
        },
        environment_variables,
        input_values: None,
    }
}

#[cfg(feature = "platform")]
async fn fetch_developer_stack_inputs(
    client: &alien_platform_api::Client,
    workspace: &str,
    project_id: &str,
) -> Result<Vec<StackInputDefinition>> {
    use alien_platform_api::SdkResultExt;

    let releases = client
        .list_releases()
        .workspace(workspace)
        .project(project_id)
        .limit(std::num::NonZeroU64::new(1).expect("1 is non-zero"))
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to fetch active release stack inputs".to_string(),
            url: None,
        })?;

    let Some(release) = releases.items.first() else {
        return Ok(Vec::new());
    };

    let mut inputs_by_id = HashMap::new();
    for stack_value in [
        release.stack.aws.as_ref(),
        release.stack.gcp.as_ref(),
        release.stack.azure.as_ref(),
        release.stack.kubernetes.as_ref(),
        release.stack.local.as_ref(),
        release.stack.test.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let stack: Stack = serde_json::from_value(stack_value.clone()).map_err(|error| {
            AlienError::new(ErrorData::ValidationError {
                field: "release.stack".to_string(),
                message: format!("Failed to parse release stack input metadata: {error}"),
            })
        })?;
        for input in stack.inputs {
            if input.provided_by.contains(&StackInputProvider::Developer) {
                inputs_by_id.entry(input.id.clone()).or_insert(input);
            }
        }
    }

    let mut inputs = inputs_by_id.into_values().collect::<Vec<_>>();
    inputs.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
    Ok(inputs)
}

#[cfg(feature = "platform")]
fn collect_stack_input_values(
    inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    json: bool,
) -> Result<alien_platform_api::types::StackInputValuesRequest> {
    use alien_platform_api::types;

    let mut raw_values = HashMap::<String, String>::new();
    for input in input_values {
        let (id, value) = parse_stack_input_arg(input, "--input")?;
        raw_values.insert(id, value);
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        raw_values.insert(id, value);
    }

    for id in raw_values.keys() {
        if !inputs.iter().any(|input| input.id == *id) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("Unknown or unavailable developer stack input '{id}'."),
            }));
        }
    }

    for input in inputs.iter().filter(|input| input.required) {
        if !raw_values.contains_key(&input.id) {
            if json || !can_prompt() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "input".to_string(),
                    message: format!(
                        "Missing developer input: {}. Pass {} {}=...",
                        input.label,
                        if matches!(input.kind, StackInputKind::Secret) {
                            "--secret-input"
                        } else {
                            "--input"
                        },
                        input.id
                    ),
                }));
            }
            let value = prompt_text(&input.label, input.placeholder.as_deref())?;
            raw_values.insert(input.id.clone(), value);
        }
    }

    let mut values = HashMap::<String, types::StackInputValueRequest>::new();
    for input in inputs {
        let Some(value) = raw_values.get(&input.id) else {
            continue;
        };
        values.insert(input.id.clone(), parse_stack_input_value(input, value)?);
    }

    Ok(types::StackInputValuesRequest(values))
}

#[cfg(feature = "platform")]
fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: '{input}'. Use id=value"),
        }));
    };
    if id.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id is required"),
        }));
    }
    Ok((id.trim().to_string(), value.to_string()))
}

#[cfg(feature = "platform")]
fn parse_stack_input_value(
    input: &StackInputDefinition,
    value: &str,
) -> Result<alien_platform_api::types::StackInputValueRequest> {
    use alien_platform_api::types;

    match input.kind {
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            validate_string_stack_input(input, value)?;
            Ok(types::StackInputValueRequest::Variant0(value.to_string()))
        }
        StackInputKind::Number => {
            let number = value.parse::<f64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a number.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant1(number))
        }
        StackInputKind::Integer => {
            let number = value.parse::<i64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a whole number.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant1(number as f64))
        }
        StackInputKind::Boolean => {
            let parsed = value.parse::<bool>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be true or false.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant2(parsed))
        }
        StackInputKind::StringList => {
            let values = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            Ok(types::StackInputValueRequest::Variant3(values))
        }
    }
}

#[cfg(feature = "platform")]
fn validate_string_stack_input(input: &StackInputDefinition, value: &str) -> Result<()> {
    if let Some(validation) = &input.validation {
        if let Some(values) = &validation.values {
            if !values.iter().any(|candidate| candidate == value) {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be one of: {}.", input.label, values.join(", ")),
                }));
            }
        }
        if let Some(min) = validation.min_length {
            if value.len() < min as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too short.", input.label),
                }));
            }
        }
        if let Some(max) = validation.max_length {
            if value.len() > max as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too long.", input.label),
                }));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "platform")]
fn print_required_developer_inputs(inputs: &[StackInputDefinition]) {
    let required = inputs
        .iter()
        .filter(|input| input.required)
        .collect::<Vec<_>>();
    if required.is_empty() {
        return;
    }

    println!("{}", dim_label("Required developer inputs"));
    for input in required {
        let kind = if matches!(input.kind, StackInputKind::Secret) {
            "secret"
        } else {
            "plain"
        };
        println!("  {}  required  {}", input.label, kind);
    }
    println!();
}

#[cfg(feature = "platform")]
fn platform_setup_environment_variables(
    variables: &[super::CliEnvVar],
) -> Result<Vec<alien_platform_api::types::EnvironmentVariableConfig>> {
    use alien_platform_api::types;

    variables
        .iter()
        .map(|variable| {
            let target_resources = variable
                .target_resources
                .as_ref()
                .map(|targets| {
                    targets
                        .iter()
                        .map(|target| {
                            types::EnvironmentVariableConfigTargetResourcesItem::try_from(
                                target.clone(),
                            )
                            .into_alien_error()
                            .context(ErrorData::ValidationError {
                                field: if variable.is_secret {
                                    "secret".to_string()
                                } else {
                                    "env".to_string()
                                },
                                message: format!(
                                    "Invalid target resource pattern in {}: '{}'. Must match pattern ^[a-zA-Z0-9_-]+(\\*)?$",
                                    if variable.is_secret { "--secret" } else { "--env" },
                                    target
                                ),
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?;

            Ok(types::EnvironmentVariableConfig {
                name: types::EnvironmentVariableConfigName::try_from(variable.name.clone())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: if variable.is_secret {
                            "secret".to_string()
                        } else {
                            "env".to_string()
                        },
                        message: format!(
                            "Invalid variable name in {}: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$",
                            if variable.is_secret { "--secret" } else { "--env" },
                            variable.name
                        ),
                    })?,
                value: types::EnvironmentVariableConfigValue::try_from(variable.value.clone())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: if variable.is_secret {
                            "secret".to_string()
                        } else {
                            "env".to_string()
                        },
                        message: format!(
                            "Invalid variable value for {} '{}'. Must not exceed 10000 characters",
                            if variable.is_secret { "--secret" } else { "--env" },
                            variable.name
                        ),
                    })?,
                type_: if variable.is_secret {
                    types::EnvironmentVariableType::Secret
                } else {
                    types::EnvironmentVariableType::Plain
                },
                target_resources,
            })
        })
        .collect()
}

/// Standalone/Dev mode: use manager API, show CLI command.
async fn onboard_standalone(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_manager_api::types::CreateDeploymentGroupRequest;
    use alien_manager_api::SdkResultExt;

    if !args.env_vars.is_empty() || !args.secret_vars.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "`alien onboard --env/--secret` is only supported in platform mode because standalone deployment-group tokens do not carry setup config.".to_string(),
        }));
    }

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;

    // Resolve manager (known in Standalone/Dev mode)
    let mgr = ctx.resolve_manager(&project_id, "local").await?;

    if !args.json {
        println!("{}", contextual_heading("Onboarding", &name, &[]));
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Create deployment group", "Generate deployment token"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    let response = mgr
        .client
        .create_deployment_group()
        .body(CreateDeploymentGroupRequest {
            name: name.clone(),
            max_deployments: Some(args.max_deployments as i64),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;

    let deployment_group_id = response.id.clone();

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Creating deployment token".to_string()));
    }

    let token_response = mgr
        .client
        .create_deployment_group_token()
        .id(&deployment_group_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group token".to_string(),
            url: None,
        })?;

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "token": token_response.token,
            "maxDeployments": args.max_deployments,
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment token ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!("{} {}", dim_label("Token"), accent(&token_response.token));
    println!();
    println!("{}", dim_label("Share with the customer's admin:"));
    println!(
        "  curl -fsSL {}/install | sh",
        mgr.manager_url.trim_end_matches('/')
    );
    println!("  export PATH=\"$HOME/.local/bin:$PATH\"");
    println!("  alien-deploy deploy \\");
    println!("    --token {} \\", token_response.token);
    println!("    --name <deployment-name> \\");
    println!("    --platform <aws|gcp|azure> \\");
    println!(
        "    --manager-url {}",
        mgr.manager_url.trim_end_matches('/')
    );
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;

    fn input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            provided_by: vec![StackInputProvider::Developer],
            required,
            label: "Control plane API key".to_string(),
            description: "API key issued by the control plane.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            setup_methods: None,
            validation: None,
            env: vec![],
        }
    }

    #[test]
    fn parse_stack_input_arg_requires_id_value() {
        let parsed = parse_stack_input_arg("controlPlaneApiKey=secret", "--secret-input")
            .expect("valid input should parse");
        assert_eq!(
            parsed,
            ("controlPlaneApiKey".to_string(), "secret".to_string())
        );

        let err = parse_stack_input_arg("controlPlaneApiKey", "--secret-input")
            .expect_err("missing equals should fail");
        assert!(err.to_string().contains("Invalid --secret-input format"));
    }

    #[test]
    fn collect_stack_input_values_rejects_missing_required_in_json_mode() {
        let err = collect_stack_input_values(
            &[input("controlPlaneApiKey", StackInputKind::Secret, true)],
            &[],
            &[],
            true,
        )
        .expect_err("missing required input should fail");

        assert!(err.to_string().contains("Missing developer input"));
        assert!(err
            .to_string()
            .contains("--secret-input controlPlaneApiKey=..."));
    }

    #[test]
    fn collect_stack_input_values_parses_typed_values() {
        let values = collect_stack_input_values(
            &[
                input("region", StackInputKind::String, true),
                input("replicas", StackInputKind::Integer, true),
                input("enabled", StackInputKind::Boolean, true),
            ],
            &[
                "region=us-east-1".to_string(),
                "replicas=3".to_string(),
                "enabled=true".to_string(),
            ],
            &[],
            true,
        )
        .expect("typed values should parse");

        assert_eq!(values.len(), 3);
    }
}
