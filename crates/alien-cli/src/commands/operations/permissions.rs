//! Offline compilation of operation permissions with explicit installer limits.

use std::path::Path;

use alien_error::{Context, ContextError, IntoAlienError};
use alien_permissions::operations::{
    self, AwsResourceCeilings, DeclaredPlugin, GcpResourceCeilings, KubernetesMode, PluginOrigin,
};
use clap::{Args, ValueEnum};
use serde_json::json;

use super::check::parse_manifest_for_cli;
use crate::error::{ErrorData, Result};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum Cloud {
    Aws,
    Gcp,
    Kubernetes,
    Azure,
}

#[derive(ValueEnum, Debug, Clone, Copy, Default)]
pub enum Permission {
    #[default]
    Diagnostics,
    Remediation,
}

#[derive(Args, Debug, Clone, Default)]
pub struct PermissionsOptions {
    /// Kubernetes installation permission mode.
    #[arg(long, value_enum, default_value = "diagnostics")]
    pub permission: Permission,
    /// Exact S3 bucket ARN allowed by the installation. May be repeated.
    #[arg(long)]
    pub s3_bucket_arn: Vec<String>,
    /// Exact SQS queue ARN allowed by the installation. May be repeated.
    #[arg(long)]
    pub sqs_queue_arn: Vec<String>,
    /// Exact Cloud Storage bucket name allowed by the installation. May be repeated.
    #[arg(long)]
    pub gcs_bucket: Vec<String>,
    /// Compile reviewed built-in source definitions. Custom plugins must use named AWS capabilities.
    #[arg(long)]
    pub builtin: bool,
}

pub fn permissions_task(
    directory: Option<&str>,
    cloud: Cloud,
    options: &PermissionsOptions,
    json_output: bool,
) -> Result<()> {
    let path =
        Path::new(directory.unwrap_or(".")).join(alien_operations_sdk::manifest::MANIFEST_FILENAME);
    let bytes = std::fs::read(&path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read '{}'", path.display()),
        })?;
    let manifest = parse_manifest_for_cli(&bytes).context(ErrorData::ConfigurationError {
        message: format!("'{}' is not a valid plugin manifest", path.display()),
    })?;
    let plugin: DeclaredPlugin =
        serde_json::from_value(serde_json::to_value(&manifest).into_alien_error().context(
            ErrorData::ConfigurationError {
                message: "could not serialize plugin permission declarations".into(),
            },
        )?)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not read plugin permission declarations".into(),
        })?;
    let origin = if options.builtin {
        PluginOrigin::Builtin
    } else {
        PluginOrigin::Custom
    };
    let catalog = operations::catalog::resolve_plugin(plugin, origin).map_err(|error| {
        let message = format!("plugin '{}': {}", manifest.name, error.message);
        error.context(ErrorData::ConfigurationError { message })
    })?;
    let plugins = [catalog];
    let (cloud_name, permissions) = match cloud {
        Cloud::Aws => (
            "aws",
            serde_json::to_value(
                operations::aws::compile(
                    &operations::aws::collect(&plugins),
                    &AwsResourceCeilings {
                        s3_bucket_arns: options.s3_bucket_arn.clone(),
                        sqs_queue_arns: options.sqs_queue_arn.clone(),
                    },
                )
                .map_err(|error| {
                    let message = format!("AWS operation permissions: {}", error.message);
                    error.context(ErrorData::ConfigurationError { message })
                })?,
            ),
        ),
        Cloud::Gcp => (
            "gcp",
            serde_json::to_value(
                operations::gcp::compile(
                    &operations::gcp::collect(&plugins),
                    &GcpResourceCeilings {
                        gcs_bucket_names: options.gcs_bucket.clone(),
                    },
                )
                .map_err(|error| {
                    let message = format!("Google Cloud operation permissions: {}", error.message);
                    error.context(ErrorData::ConfigurationError { message })
                })?,
            ),
        ),
        Cloud::Kubernetes => (
            "kubernetes",
            serde_json::to_value(
                operations::kubernetes::compile(
                    &operations::kubernetes::collect(&plugins),
                    match options.permission {
                        Permission::Diagnostics => KubernetesMode::Diagnostics,
                        Permission::Remediation => KubernetesMode::Remediation,
                    },
                )
                .map_err(|error| {
                    let message = format!("Kubernetes operation permissions: {}", error.message);
                    error.context(ErrorData::ConfigurationError { message })
                })?,
            ),
        ),
        Cloud::Azure => {
            return Err(alien_error::AlienError::new(
                ErrorData::ConfigurationError {
                    message:
                        "Azure resource operations have no reviewed permission capabilities yet"
                            .into(),
                },
            ))
        }
    };
    let permissions = permissions
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not serialize operation permissions".into(),
        })?;
    let output = json!({"plugin": manifest.name, "cloud": cloud_name, "permissions": permissions});
    if json_output {
        crate::output::print_json(&output)?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&output)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "could not format operation permissions".into()
                })?
        );
    }
    Ok(())
}
