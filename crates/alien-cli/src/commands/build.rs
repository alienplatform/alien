use crate::config::load_configuration;
use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::tui::{BuildResult, BuildUiComponent, BuildUiEvent, BuildUiProps, ErrorPrinter};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::BinaryTarget;
use alien_core::{events::AlienEvent, EventChange, EventHandler};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::mpsc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildJsonOutput {
    pub success: bool,
    pub platform: String,
    pub output_dir: String,
    pub stack_id: String,
    pub resources: Vec<String>,
    pub artifacts: HashMap<String, String>, // resource_id -> artifact_path
    pub build_time_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build the Alien application locally",
    long_about = "Build the Alien application locally, creating OCI tarballs without pushing to a registry. Use 'alien release' to push images and create a release.",
    after_help = "EXAMPLES:
    # Build for AWS (default platform)
    alien build

    # Build for specific platform
    alien build --platform aws
    alien build --platform gcp
    alien build --platform azure
    alien build --platform kubernetes
    alien build --platform local

    # Build with custom configuration file
    alien build --config alien.config.function.ts
    alien build --config ../my-app/alien.config.ts

    # Build with custom configuration directory
    alien build --config ../my-app/

    # Build with custom output directory
    alien build --output-dir ./build

    # Build for AWS with managing account ID
    alien build --platform aws --aws-managing-account-id 123456789012

    # Build with S3 cache
    alien build --cache-url s3://my-bucket/build-cache

    # Build with GCS cache
    alien build --cache-url gcs://my-bucket/build-cache

    # Build without TUI (console output only)
    alien build --no-tui

    # Build for specific targets (overrides platform defaults)
    alien build --targets linux-x64,linux-arm64
    alien build --targets linux-arm64  # Single target"
)]
pub struct BuildArgs {
    /// Path to alien.config.ts/js/json file or directory containing it
    /// If not specified, searches in current directory
    #[arg(short = 'c', long)]
    pub config: Option<String>,

    /// Output directory
    #[arg(short = 'o', long)]
    pub output_dir: Option<String>,

    /// Target platform
    #[arg(long, default_value = "aws")]
    pub platform: String,

    /// Target OS/architecture combinations (comma-separated)
    /// Available: windows-x64, linux-x64, linux-arm64, darwin-arm64
    /// If not specified, uses platform-specific defaults
    #[arg(long, value_delimiter = ',')]
    pub targets: Option<Vec<String>>,

    /// AWS managing account ID
    #[arg(long)]
    pub aws_managing_account_id: Option<String>,

    /// Cache URL for build caching (e.g., s3://bucket/path, gcs://bucket/path)
    #[arg(long)]
    pub cache_url: Option<String>,

    /// Disable TUI and use console output instead
    #[arg(long)]
    pub no_tui: bool,

    /// Output JSON instead of human-readable text (implies --no-tui)
    #[arg(long)]
    pub json: bool,
}

/// Main entry point for build command - handles TUI vs console mode
pub async fn build_command(args: BuildArgs) -> Result<()> {
    // JSON mode: output JSON to stdout, errors to stderr
    if args.json {
        match build_task_json(args.clone()).await {
            Ok(output) => {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
                if !output.success {
                    std::process::exit(1);
                }
                Ok(())
            }
            Err(error) => {
                let output = BuildJsonOutput {
                    success: false,
                    platform: args.platform.clone(),
                    output_dir: String::new(),
                    stack_id: String::new(),
                    resources: vec![],
                    artifacts: HashMap::new(),
                    build_time_seconds: 0.0,
                    error: Some(format!("{}", error)),
                };
                eprintln!("{}", serde_json::to_string_pretty(&output).unwrap());
                std::process::exit(1);
            }
        }
    }
    // Use TUI only if no_tui is false and we're in a TTY environment
    else if !args.no_tui && std::io::stderr().is_terminal() && std::io::stdout().is_terminal() {
        match run_build_with_tui(args).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ =
                    ErrorPrinter::print_alien_error(&error.into_generic(), Some("BUILD FAILED"));
                std::process::exit(1);
            }
        }
    } else {
        match build_task(args).await {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ =
                    ErrorPrinter::print_alien_error(&error.into_generic(), Some("BUILD FAILED"));
                std::process::exit(1);
            }
        }
    }
}

/// Run build with TUI using the new BuildUiComponent
async fn run_build_with_tui(args: BuildArgs) -> Result<()> {
    let current_dir = get_current_dir()?;
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir.join(".alien").to_str().unwrap().to_string());

    // Create the BuildUiComponent with props
    let props = BuildUiProps {
        platform: args.platform.clone(),
        output_dir,
        on_result: None,
        on_cancel: None,
    };

    let mut ui_component = BuildUiComponent::new(props);

    // Start the component and get the event sender
    let ui_event_tx = ui_component
        .start()
        .context(ErrorData::TuiOperationFailed {
            message: "Failed to start UI component".to_string(),
        })?;

    // Set up alien event handler that forwards to UI component
    let event_handler = BuildEventHandler::new(ui_event_tx.clone());
    let bus = alien_core::EventBus::with_handlers(vec![std::sync::Arc::new(event_handler)]);

    // Run the build command in the background
    let build_ui_tx = ui_event_tx.clone();
    let build_handle = tokio::spawn(async move {
        let result = bus.run(|| async { build_task(args).await }).await;

        // Convert Result<BuildResult> to the UI event format
        let ui_result = match result {
            Ok(build_result) => Ok(build_result),
            Err(alien_error) => Err(alien_error),
        };

        let _ = build_ui_tx.send(BuildUiEvent::BuildFinished(ui_result));
    });

    // Run the UI component event loop (this blocks until completion)
    let ui_result = ui_component
        .run_event_loop()
        .context(ErrorData::TuiOperationFailed {
            message: "UI component failed".to_string(),
        });

    // Handle build task completion
    match build_handle.await {
        Ok(_) => {}                      // Build completed normally
        Err(e) if e.is_cancelled() => {} // Build was cancelled (user exit)
        Err(e) => {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: format!("Task join error: {}", e),
            }))
        }
    }

    // Return UI result (which includes any build errors)
    ui_result
}

/// Event handler for alien events that forwards to BuildUiComponent
struct BuildEventHandler {
    tx: mpsc::Sender<BuildUiEvent>,
}

impl BuildEventHandler {
    fn new(tx: mpsc::Sender<BuildUiEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventHandler for BuildEventHandler {
    async fn on_event_change(&self, change: EventChange) -> alien_core::Result<()> {
        let _ = self.tx.send(BuildUiEvent::AlienEventChange(change));
        Ok(())
    }
}

/// Parse target string to BinaryTarget
fn parse_target(target_str: &str) -> Result<BinaryTarget> {
    match target_str.to_lowercase().as_str() {
        "windows-x64" => Ok(BinaryTarget::WindowsX64),
        "linux-x64" => Ok(BinaryTarget::LinuxX64),
        "linux-arm64" => Ok(BinaryTarget::LinuxArm64),
        "darwin-arm64" => Ok(BinaryTarget::DarwinArm64),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "targets".to_string(),
            message: format!(
                "Unknown target: '{}'. Supported targets are: windows-x64, linux-x64, linux-arm64, darwin-arm64",
                target_str
            ),
        })),
    }
}

/// Core build logic without TUI
pub async fn build_task(args: BuildArgs) -> Result<BuildResult> {
    info!("Starting build command");
    let current_dir = get_current_dir()?;
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir.join(".alien").to_str().unwrap().to_string());

    let target_platform = match args.platform.to_lowercase().as_str() {
        "aws" => PlatformBuildSettings::Aws {
            managing_account_id: args.aws_managing_account_id.clone(),
        },
        "gcp" => PlatformBuildSettings::Gcp {},
        "azure" => PlatformBuildSettings::Azure {},
        "kubernetes" => PlatformBuildSettings::Kubernetes {},
        "local" => PlatformBuildSettings::Local {},
        _ => return Err(AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: format!("Unknown platform: '{}'. Supported platforms are: aws, gcp, azure, kubernetes, local", args.platform),
        })),
    };

    // Parse target architectures if specified
    let targets = if let Some(target_strs) = &args.targets {
        let mut parsed_targets = Vec::new();
        for target_str in target_strs {
            parsed_targets.push(parse_target(target_str)?);
        }
        Some(parsed_targets)
    } else {
        None // Use platform defaults
    };

    let settings = BuildSettings {
        output_directory: output_dir,
        platform: target_platform,
        targets,
        cache_url: args.cache_url.clone(),
        override_base_image: None,
        debug_mode: false, // Release builds for production
    };

    // Use specified config path or default to current directory
    let config_path = args
        .config
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| current_dir.clone());

    let stack = load_configuration(config_path)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to load configuration".to_string(),
        })?;

    alien_build::build_stack(stack, &settings)
        .await
        .context(ErrorData::BuildFailed)?;

    AlienEvent::Finished
        .emit_with_state(alien_core::EventState::Success)
        .await
        .ok();

    Ok(BuildResult::Success)
}

/// Build task with JSON output
async fn build_task_json(args: BuildArgs) -> Result<BuildJsonOutput> {
    use std::path::PathBuf;

    let start_time = std::time::Instant::now();

    let current_dir = get_current_dir()?;
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir.join(".alien").to_str().unwrap().to_string());

    // Run build (reuse existing logic from build_task)
    let result = build_task(args.clone()).await;

    let build_time = start_time.elapsed().as_secs_f64();

    match result {
        Ok(_) => {
            // Read built stack to get resources
            let stack_file = PathBuf::from(&output_dir)
                .join("build")
                .join(&args.platform)
                .join("stack.json");

            let stack: alien_core::Stack = if stack_file.exists() {
                let content = std::fs::read_to_string(&stack_file)
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "read".to_string(),
                        file_path: stack_file.display().to_string(),
                        reason: "Failed to read stack file".to_string(),
                    })?;
                serde_json::from_str(&content).into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "parse".to_string(),
                        file_path: stack_file.display().to_string(),
                        reason: "Failed to parse stack JSON".to_string(),
                    },
                )?
            } else {
                return Err(AlienError::new(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: stack_file.display().to_string(),
                    reason: "Built stack not found".to_string(),
                }));
            };

            let resources: Vec<String> = stack
                .resources()
                .map(|(id, _entry): (&String, &alien_core::ResourceEntry)| id.clone())
                .collect();

            // Collect artifacts
            let mut artifacts: HashMap<String, String> = HashMap::new();
            let build_dir = PathBuf::from(&output_dir)
                .join("build")
                .join(&args.platform);
            for resource_id in &resources {
                let artifact_path = build_dir.join(format!("{}.tar", resource_id));
                if artifact_path.exists() {
                    artifacts.insert(resource_id.to_string(), artifact_path.display().to_string());
                }
            }

            Ok(BuildJsonOutput {
                success: true,
                platform: args.platform,
                output_dir,
                stack_id: stack.id().to_string(),
                resources,
                artifacts,
                build_time_seconds: build_time,
                error: None,
            })
        }
        Err(e) => Ok(BuildJsonOutput {
            success: false,
            platform: args.platform,
            output_dir,
            stack_id: String::new(),
            resources: vec![],
            artifacts: HashMap::new(),
            build_time_seconds: build_time,
            error: Some(format!("{}", e)),
        }),
    }
}
