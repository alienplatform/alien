use crate::config::load_configuration;
use crate::error::{ErrorData, Result};
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, StackSettings,
    StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, ValueEnum};
use std::fs;
use std::path::{Path, PathBuf};

/// Render setup artifacts for review.
#[derive(Parser, Debug, Clone)]
pub struct RenderArgs {
    /// Artifact format to render.
    #[arg(long, value_enum)]
    pub format: RenderFormat,

    /// Terraform target. Required for --format terraform.
    #[arg(long, value_enum)]
    pub target: Option<RenderTarget>,

    /// Path to alien.ts, alien.js, alien.json, or a directory containing one.
    #[arg(long)]
    pub stack: PathBuf,

    /// Output directory. Defaults to stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// CloudFormation registration behavior.
    #[arg(long, value_enum, default_value_t = RenderRegistrationMode::Auto)]
    pub registration_mode: RenderRegistrationMode,

    /// Lambda ARN for CloudFormation auto-registration.
    #[arg(long, env = "ALIEN_CFN_NOTIFICATION_LAMBDA_ARN")]
    pub notification_lambda_arn: Option<String>,

    /// Optional YAML or JSON StackSettings override.
    #[arg(long)]
    pub stack_settings: Option<PathBuf>,

    /// Emit CloudFormation JSON instead of YAML.
    #[arg(long)]
    pub json: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderFormat {
    Cloudformation,
    Terraform,
    Helm,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTarget {
    Aws,
    Gcp,
    Azure,
    Eks,
    Gke,
    Aks,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderRegistrationMode {
    Auto,
    Outputs,
    Both,
}

pub async fn render_task(args: RenderArgs) -> Result<()> {
    validate_args(&args)?;
    let stack = load_configuration(args.stack.clone()).await?;
    let stack_settings = load_stack_settings(args.stack_settings.as_deref())?;

    match args.format {
        RenderFormat::Cloudformation => {
            let stack =
                prepare_stack_for_render(stack, Platform::Aws, None, &stack_settings).await?;
            render_cloudformation(&stack, &stack_settings, &args)
        }
        RenderFormat::Terraform => {
            let target = args.target.expect("validated by validate_args");
            let terraform_target = terraform_target(target);
            let stack = prepare_stack_for_render(
                stack,
                terraform_target.deployment_platform(),
                terraform_target.base_platform(),
                &stack_settings,
            )
            .await?;
            render_terraform(&stack, &stack_settings, &args)
        }
        RenderFormat::Helm => {
            let stack =
                prepare_stack_for_render(stack, Platform::Kubernetes, None, &stack_settings)
                    .await?;
            render_helm(&stack, &stack_settings, &args)
        }
    }
}

async fn prepare_stack_for_render(
    stack: alien_core::Stack,
    platform: Platform,
    base_platform: Option<Platform>,
    stack_settings: &StackSettings,
) -> Result<alien_core::Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, platform)
        .await
        .map_err(preflight_error)?;

    let stack_state = StackState::new(platform);
    let config = DeploymentConfig {
        deployment_name: Some(stack.id().to_string()),
        stack_settings: stack_settings.clone(),
        management_config: None,
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        base_platform,
        public_endpoints: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    runner
        .apply_mutations(stack, &stack_state, &config)
        .await
        .map_err(preflight_error)
}

fn validate_args(args: &RenderArgs) -> Result<()> {
    match args.format {
        RenderFormat::Terraform if args.target.is_none() => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "--target".to_string(),
                message: "--target is required when --format terraform".to_string(),
            }))
        }
        RenderFormat::Cloudformation => Ok(()),
        RenderFormat::Terraform | RenderFormat::Helm => {
            if args.json {
                Err(AlienError::new(ErrorData::ValidationError {
                    field: "--json".to_string(),
                    message: "--json is only supported for --format cloudformation".to_string(),
                }))
            } else {
                Ok(())
            }
        }
    }
}

fn load_stack_settings(path: Option<&Path>) -> Result<StackSettings> {
    let Some(path) = path else {
        return Ok(StackSettings::default());
    };

    let contents =
        fs::read_to_string(path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to read stack settings".to_string(),
            })?;

    serde_yaml::from_str(&contents)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse stack settings".to_string(),
            reason: format!("Failed to parse {}", path.display()),
        })
}

fn render_cloudformation(
    stack: &alien_core::Stack,
    stack_settings: &StackSettings,
    args: &RenderArgs,
) -> Result<()> {
    let registry = alien_cloudformation::CfRegistry::built_in();
    let registration = registration_mode(args)?;
    let template = alien_cloudformation::generate_cloudformation_template(
        stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: stack_settings.clone(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "render-preview".to_string(),
            setup_fingerprint_version: 1,
            registration,
            description: None,
        },
    )
    .map_err(render_error)?;

    let (file_name, contents) = if args.json {
        (
            "template.json",
            serde_json::to_string_pretty(&template)
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "serialize CloudFormation template".to_string(),
                    reason: "Failed to serialize template JSON".to_string(),
                })?,
        )
    } else {
        (
            "template.yaml",
            alien_cloudformation::to_yaml(&template).map_err(render_error)?,
        )
    };

    write_single_or_stdout(args.output.as_deref(), file_name, &contents)
}

fn registration_mode(args: &RenderArgs) -> Result<alien_cloudformation::RegistrationMode> {
    match args.registration_mode {
        RenderRegistrationMode::Outputs => {
            Ok(alien_cloudformation::RegistrationMode::OutputsFallback)
        }
        RenderRegistrationMode::Auto => match &args.notification_lambda_arn {
            Some(lambda_arn) => Ok(alien_cloudformation::RegistrationMode::CustomResource {
                lambda_arn: lambda_arn.clone(),
                callback_url: None,
            }),
            None => Ok(alien_cloudformation::RegistrationMode::OutputsFallback),
        },
        RenderRegistrationMode::Both => {
            let lambda_arn = args.notification_lambda_arn.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "--notification-lambda-arn".to_string(),
                    message: "--registration-mode both requires a notification Lambda ARN"
                        .to_string(),
                })
            })?;
            Ok(alien_cloudformation::RegistrationMode::Both {
                lambda_arn,
                callback_url: None,
            })
        }
    }
}

fn render_terraform(
    stack: &alien_core::Stack,
    stack_settings: &StackSettings,
    args: &RenderArgs,
) -> Result<()> {
    let target = args.target.expect("validated by validate_args");
    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        stack,
        terraform_target(target),
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: stack_settings.clone(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .map_err(render_error)?;

    let files: Vec<(String, String)> = module
        .iter()
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();

    write_files_or_stdout(args.output.as_deref(), &files)
}

fn terraform_target(target: RenderTarget) -> alien_terraform::TerraformTarget {
    match target {
        RenderTarget::Aws => alien_terraform::TerraformTarget::Aws,
        RenderTarget::Gcp => alien_terraform::TerraformTarget::Gcp,
        RenderTarget::Azure => alien_terraform::TerraformTarget::Azure,
        RenderTarget::Eks => alien_terraform::TerraformTarget::Eks,
        RenderTarget::Gke => alien_terraform::TerraformTarget::Gke,
        RenderTarget::Aks => alien_terraform::TerraformTarget::Aks,
    }
}

fn render_helm(
    stack: &alien_core::Stack,
    stack_settings: &StackSettings,
    args: &RenderArgs,
) -> Result<()> {
    let registry = alien_helm::HelmRegistry::built_in();
    let chart = alien_helm::generate_helm_chart(
        stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings: stack_settings.clone(),
            chart_name: stack.id().to_string(),
        },
    )
    .map_err(render_error)?;

    let files = chart.files.into_iter().collect::<Vec<_>>();
    write_files_or_stdout(args.output.as_deref(), &files)
}

fn write_single_or_stdout(
    output_dir: Option<&Path>,
    file_name: &str,
    contents: &str,
) -> Result<()> {
    if let Some(output_dir) = output_dir {
        write_file(&output_dir.join(file_name), contents)
    } else {
        print!("{contents}");
        if !contents.ends_with('\n') {
            println!();
        }
        Ok(())
    }
}

fn write_files_or_stdout(output_dir: Option<&Path>, files: &[(String, String)]) -> Result<()> {
    if let Some(output_dir) = output_dir {
        for (path, contents) in files {
            write_file(&output_dir.join(path), contents)?;
        }
    } else {
        for (path, contents) in files {
            println!("# === {path} ===");
            print!("{contents}");
            if !contents.ends_with('\n') {
                println!();
            }
            println!();
        }
    }

    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "create directory".to_string(),
                file_path: parent.display().to_string(),
                reason: "Failed to create render output directory".to_string(),
            })?;
    }

    fs::write(path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write rendered artifact".to_string(),
        })
}

fn render_error(error: alien_error::AlienError<alien_core::ErrorData>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: error.to_string(),
    })
}

fn preflight_error(
    error: alien_error::AlienError<alien_preflights::error::ErrorData>,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AzureStorageAccount, ResourceLifecycle, Stack, Storage};

    #[tokio::test]
    async fn render_preflights_inject_azure_auxiliary_resources() {
        let stack = Stack::new("render-review".to_string())
            .add(
                Storage::new("assets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let stack =
            prepare_stack_for_render(stack, Platform::Azure, None, &StackSettings::default())
                .await
                .expect("render preflights should mutate stack");

        let has_storage_account = stack.resources().any(|(id, entry)| {
            id == "default-storage-account"
                && entry.config.downcast_ref::<AzureStorageAccount>().is_some()
        });

        assert!(
            has_storage_account,
            "Azure render should include preflight-injected storage account"
        );
    }
}
