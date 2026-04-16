use crate::config::load_configuration;
use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::output::print_json;
use crate::ui::{accent, command, contextual_heading, dim_label, success_line};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::events::AlienEvent;
use alien_core::BinaryTarget;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResourceDetail {
    pub name: String,
    pub resource_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildOutput {
    pub success: bool,
    pub platform: String,
    pub output_dir: String,
    pub stack_id: String,
    pub resources: Vec<String>,
    pub resource_details: Vec<BuildResourceDetail>,
    pub artifacts: HashMap<String, String>,
    pub build_time_seconds: f64,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build the Alien application locally",
    long_about = "Build the Alien application locally, creating OCI tarballs without pushing to a registry. Use `alien release` to push images and create a release.",
    after_help = "EXAMPLES:
    alien build --platform aws
    alien build --platforms aws,gcp
    alien build --platform aws --targets linux-arm64
    alien build --config ../my-app/alien.ts --output-dir ./build --platform aws
    alien build --platform aws --json"
)]
pub struct BuildArgs {
    /// Path to alien.ts/js/json file or directory containing it
    #[arg(short = 'c', long)]
    pub config: Option<String>,

    /// Output directory
    #[arg(short = 'o', long)]
    pub output_dir: Option<String>,

    /// Target platforms (comma-separated). Examples: aws, gcp, azure, aws,gcp
    #[arg(long = "platforms", alias = "platform", value_delimiter = ',')]
    pub platforms: Vec<String>,

    /// Allow experimental platforms (kubernetes, local)
    #[arg(long)]
    pub experimental: bool,

    /// Target OS/architecture combinations (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub targets: Option<Vec<String>>,

    /// AWS managing account ID
    #[arg(long)]
    pub aws_managing_account_id: Option<String>,

    /// Cache URL for build caching (for example s3://bucket/path)
    #[arg(long)]
    pub cache_url: Option<String>,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn build_command(args: BuildArgs) -> Result<()> {
    if args.platforms.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platforms".to_string(),
            message: "At least one platform is required. Use --platform <aws|gcp|azure> or --platforms aws,gcp".to_string(),
        }));
    }

    let outputs = build_task(&args).await?;
    if args.json {
        // Single platform → single object for backward compatibility
        if outputs.len() == 1 {
            print_json(&outputs[0])?;
        } else {
            print_json(&outputs)?;
        }
    } else {
        for output in &outputs {
            print_build_summary(output);
        }
    }
    Ok(())
}

pub async fn build_task(args: &BuildArgs) -> Result<Vec<BuildOutput>> {
    let start_time = std::time::Instant::now();
    let current_dir = get_current_dir()?;
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir.join(".alien").display().to_string());

    let display_stack_name = build_display_name(args.config.as_deref(), &current_dir);
    let platforms_label = args.platforms.join(", ");

    if !args.json {
        println!(
            "{}",
            contextual_heading(
                "Building",
                &display_stack_name,
                &[("for", &platforms_label)]
            )
        );
        println!("{} {}", dim_label("Output"), accent(&output_dir));
    }

    let targets = args
        .targets
        .as_ref()
        .map(|targets| targets.iter().map(|target| parse_target(target)).collect())
        .transpose()?;

    let config_path = args
        .config
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| current_dir.clone());

    let stack = load_configuration(config_path)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to load configuration".to_string(),
        })?;

    let mut outputs = Vec::new();

    for platform_str in &args.platforms {
        let platform_str = platform_str.to_ascii_lowercase();

        // Check for experimental platforms
        if let Ok(platform) = alien_core::Platform::from_str(&platform_str) {
            if platform.is_experimental() && !args.experimental {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: format!(
                        "Platform '{}' is experimental and not yet production-ready. Pass --experimental to use it anyway.",
                        platform_str
                    ),
                }));
            }
        }

        let target_platform = match platform_str.as_str() {
            "aws" => PlatformBuildSettings::Aws {
                managing_account_id: args.aws_managing_account_id.clone(),
            },
            "gcp" => PlatformBuildSettings::Gcp {},
            "azure" => PlatformBuildSettings::Azure {},
            "kubernetes" => PlatformBuildSettings::Kubernetes {},
            "local" => PlatformBuildSettings::Local {},
            _ => {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: format!(
                        "Unknown platform '{}'. Supported platforms: aws, gcp, azure",
                        platform_str
                    ),
                }))
            }
        };

        let settings = BuildSettings {
            output_directory: output_dir.clone(),
            platform: target_platform,
            targets: targets.clone(),
            cache_url: args.cache_url.clone(),
            override_base_image: None,
            debug_mode: false,
        };

        alien_build::build_stack(stack.clone(), &settings)
            .await
            .context(ErrorData::BuildFailed)?;

        let output = load_build_output(
            &platform_str,
            &output_dir,
            start_time.elapsed().as_secs_f64(),
        )?;
        outputs.push(output);
    }

    AlienEvent::Finished
        .emit_with_state(alien_core::EventState::Success)
        .await
        .ok();

    Ok(outputs)
}

fn load_build_output(
    platform: &str,
    output_dir: &str,
    build_time_seconds: f64,
) -> Result<BuildOutput> {
    let stack_file = PathBuf::from(output_dir)
        .join("build")
        .join(platform)
        .join("stack.json");

    let content = std::fs::read_to_string(&stack_file)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: stack_file.display().to_string(),
            reason: "Failed to read build output stack.json".to_string(),
        })?;

    let stack: alien_core::Stack =
        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialize".to_string(),
                reason: "Failed to parse build output stack.json".to_string(),
            })?;

    let resource_details: Vec<BuildResourceDetail> = stack
        .resources()
        .map(|(id, resource)| BuildResourceDetail {
            name: id.clone(),
            resource_type: resource.config.resource_type().to_string(),
        })
        .collect();
    let resources: Vec<String> = resource_details
        .iter()
        .map(|resource| resource.name.clone())
        .collect();
    let build_dir = PathBuf::from(output_dir)
        .join("build")
        .join(platform);
    let mut artifacts = HashMap::new();
    for resource_id in &resources {
        let artifact_path = build_dir.join(format!("{resource_id}.tar"));
        if artifact_path.exists() {
            artifacts.insert(resource_id.clone(), artifact_path.display().to_string());
        }
    }

    Ok(BuildOutput {
        success: true,
        platform: platform.to_string(),
        output_dir: output_dir.to_string(),
        stack_id: stack.id().to_string(),
        resources,
        resource_details,
        artifacts,
        build_time_seconds,
    })
}

fn parse_target(target_str: &str) -> Result<BinaryTarget> {
    match target_str.to_ascii_lowercase().as_str() {
        "windows-x64" => Ok(BinaryTarget::WindowsX64),
        "linux-x64" => Ok(BinaryTarget::LinuxX64),
        "linux-arm64" => Ok(BinaryTarget::LinuxArm64),
        "darwin-arm64" => Ok(BinaryTarget::DarwinArm64),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "targets".to_string(),
            message: format!(
                "Unknown target '{target_str}'. Supported targets: windows-x64, linux-x64, linux-arm64, darwin-arm64"
            ),
        })),
    }
}

fn build_display_name(config: Option<&str>, current_dir: &std::path::Path) -> String {
    let candidate = config
        .map(PathBuf::from)
        .and_then(|path| {
            if path.is_dir() {
                path.file_name().map(|name| name.to_owned())
            } else {
                path.parent()
                    .and_then(|parent| parent.file_name())
                    .map(|name| name.to_owned())
            }
        })
        .or_else(|| current_dir.file_name().map(|name| name.to_owned()));

    candidate
        .and_then(|name| name.to_str().map(|value| value.to_string()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "project".to_string())
}

fn print_build_summary(output: &BuildOutput) {
    println!("{}", success_line("Build complete."));
    println!("{} {}", dim_label("Stack"), output.stack_id);
    println!("{} {}", dim_label("Artifacts"), output.artifacts.len());
    println!(
        "{} {:.1}s",
        dim_label("Duration"),
        output.build_time_seconds
    );

    if !output.resources.is_empty() {
        println!("{}", dim_label("Resources"));
        if output.resource_details.is_empty() {
            for resource in &output.resources {
                println!("  - {resource}");
            }
        } else {
            for resource in &output.resource_details {
                println!("  - {} ({})", resource.name, resource.resource_type);
            }
        }
    }

    println!(
        "{} run {} to push images and create a release.",
        dim_label("Next"),
        command("alien release")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_sample_stack;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_target_accepts_known_values() {
        assert_eq!(
            parse_target("linux-arm64").unwrap(),
            BinaryTarget::LinuxArm64
        );
        assert_eq!(
            parse_target("WINDOWS-X64").unwrap(),
            BinaryTarget::WindowsX64
        );
    }

    #[test]
    fn parse_target_rejects_unknown_values() {
        let err = parse_target("solaris-sparc").unwrap_err();
        assert!(err.to_string().contains("Unknown target"));
    }

    #[test]
    fn load_build_output_collects_existing_artifacts() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join(".alien");
        let build_dir = output_dir.join("build").join("aws");
        fs::create_dir_all(&build_dir).unwrap();

        let stack = create_sample_stack("stack-123");
        fs::write(
            build_dir.join("stack.json"),
            serde_json::to_string_pretty(&stack).unwrap(),
        )
        .unwrap();
        fs::write(build_dir.join("test-storage.tar"), "tarball").unwrap();

        let output =
            load_build_output("aws", &output_dir.display().to_string(), 1.25).unwrap();

        assert_eq!(output.stack_id, "stack-123");
        assert!(output.resources.contains(&"test-storage".to_string()));
        assert!(output.artifacts.contains_key("test-storage"));
        assert!(!output.artifacts.contains_key("test-function"));
    }
}
