use crate::config::load_configuration;
use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::output::print_json;
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::BinaryTarget;
use alien_core::events::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildOutput {
    pub success: bool,
    pub platform: String,
    pub output_dir: String,
    pub stack_id: String,
    pub resources: Vec<String>,
    pub artifacts: HashMap<String, String>,
    pub build_time_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build the Alien application locally",
    long_about = "Build the Alien application locally, creating OCI tarballs without pushing to a registry. Use `alien release` to push images and create a release.",
    after_help = "EXAMPLES:
    alien build
    alien build --platform aws
    alien build --platform local --targets linux-arm64
    alien build --config ../my-app/alien.ts --output-dir ./build
    alien build --json"
)]
pub struct BuildArgs {
    /// Path to alien.ts/js/json file or directory containing it
    #[arg(short = 'c', long)]
    pub config: Option<String>,

    /// Output directory
    #[arg(short = 'o', long)]
    pub output_dir: Option<String>,

    /// Target platform
    #[arg(long, default_value = "aws")]
    pub platform: String,

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
    match build_task(&args).await {
        Ok(output) => {
            if args.json {
                print_json(&output)?;
            } else {
                print_build_summary(&output);
            }
            Ok(())
        }
        Err(error) if args.json => {
            print_json(&BuildOutput {
                success: false,
                platform: args.platform,
                output_dir: args.output_dir.unwrap_or_default(),
                stack_id: String::new(),
                resources: Vec::new(),
                artifacts: HashMap::new(),
                build_time_seconds: 0.0,
                error: Some(error.to_string()),
            })?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

pub async fn build_task(args: &BuildArgs) -> Result<BuildOutput> {
    let start_time = std::time::Instant::now();
    let current_dir = get_current_dir()?;
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir.join(".alien").display().to_string());

    println!("Building project...");
    println!("Platform: {}", args.platform);
    println!("Output directory: {output_dir}");

    let target_platform = match args.platform.to_ascii_lowercase().as_str() {
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
                message:
                    "Unknown platform. Supported platforms: aws, gcp, azure, kubernetes, local"
                        .to_string(),
            }))
        }
    };

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

    let settings = BuildSettings {
        output_directory: output_dir.clone(),
        platform: target_platform,
        targets,
        cache_url: args.cache_url.clone(),
        override_base_image: None,
        debug_mode: false,
    };

    alien_build::build_stack(stack, &settings)
        .await
        .context(ErrorData::BuildFailed)?;

    AlienEvent::Finished
        .emit_with_state(alien_core::EventState::Success)
        .await
        .ok();

    load_build_output(args, output_dir, start_time.elapsed().as_secs_f64())
}

fn load_build_output(args: &BuildArgs, output_dir: String, build_time_seconds: f64) -> Result<BuildOutput> {
    let stack_file = PathBuf::from(&output_dir)
        .join("build")
        .join(&args.platform)
        .join("stack.json");

    let content = std::fs::read_to_string(&stack_file)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: stack_file.display().to_string(),
            reason: "Failed to read build output stack.json".to_string(),
        })?;

    let stack: alien_core::Stack = serde_json::from_str(&content)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialize".to_string(),
            reason: "Failed to parse build output stack.json".to_string(),
        })?;

    let resources: Vec<String> = stack.resources().map(|(id, _)| id.clone()).collect();
    let build_dir = PathBuf::from(&output_dir).join("build").join(&args.platform);
    let mut artifacts = HashMap::new();
    for resource_id in &resources {
        let artifact_path = build_dir.join(format!("{resource_id}.tar"));
        if artifact_path.exists() {
            artifacts.insert(resource_id.clone(), artifact_path.display().to_string());
        }
    }

    Ok(BuildOutput {
        success: true,
        platform: args.platform.clone(),
        output_dir,
        stack_id: stack.id().to_string(),
        resources,
        artifacts,
        build_time_seconds,
        error: None,
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

fn print_build_summary(output: &BuildOutput) {
    println!("Build complete.");
    println!("Stack ID: {}", output.stack_id);
    println!("Artifacts: {}", output.artifacts.len());
    println!("Duration: {:.1}s", output.build_time_seconds);

    if !output.resources.is_empty() {
        println!("Resources:");
        for resource in &output.resources {
            println!("  - {resource}");
        }
    }

    println!("Next: run `alien release` to push images and create a release.");
}
