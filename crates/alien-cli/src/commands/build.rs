use crate::config::load_configuration;
use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::output::print_json;
use crate::ui::{accent, command, contextual_heading, dim_label, success_line};
use alien_build::plan::{plan_runner_groups, stack_targets_native_host_binaries};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::events::AlienEvent;
use alien_core::{BinaryTarget, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;
use tracing::info;

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

#[derive(Clone)]
struct PlatformBuildPlan {
    platform: String,
    settings: BuildSettings,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build the Alien application locally",
    long_about = "Build the Alien application locally, creating OCI tarballs without pushing to a registry. Use `alien release` to push images and create a release.",
    args_conflicts_with_subcommands = true,
    after_help = "EXAMPLES:
    alien build --platform aws
    alien build --platforms aws,gcp
    alien build --platform aws --targets linux-arm64
    alien build --config ../my-app/alien.ts --output-dir ./build --platform aws
    alien build --platform aws --json
    alien build plan --json
    alien build merge --input ./build-artifacts --output .alien"
)]
pub struct BuildArgs {
    /// Subcommand: `plan` (compute native-runner groups) or `merge` (combine partial outputs).
    /// When omitted, runs a normal build.
    #[command(subcommand)]
    pub command: Option<BuildSubcommand>,

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

    /// Base cloud platform for Kubernetes builds. This keeps the output
    /// platform as Kubernetes while using the managed cluster's default
    /// architecture.
    #[arg(long)]
    pub base_platform: Option<String>,

    /// Cache URL for build caching (for example s3://bucket/path)
    #[arg(long)]
    pub cache_url: Option<String>,

    /// Override the runtime base image used for source-built cloud containers.
    #[arg(long, env = "ALIEN_OVERRIDE_BASE_IMAGE", hide = true)]
    pub override_base_image: Option<String>,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum BuildSubcommand {
    /// Compute the native-runner build groups for the stack's supported platforms
    #[command(after_help = "EXAMPLES:
    alien build plan
    alien build plan --json")]
    Plan(BuildPlanArgs),
    /// Merge partial build outputs (one per native runner) into one `.alien` directory
    #[command(after_help = "EXAMPLES:
    alien build merge --input ./build-artifacts --output .alien")]
    Merge(BuildMergeArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct BuildPlanArgs {
    /// Path to alien.ts/js/json file or directory containing it
    #[arg(short = 'c', long)]
    pub config: Option<String>,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct BuildMergeArgs {
    /// Directory of downloaded partial outputs (each subdir holds `build/<platform>/`)
    #[arg(long)]
    pub input: String,

    /// Output directory for the merged `.alien` build
    #[arg(long)]
    pub output: String,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn build_command(args: BuildArgs) -> Result<()> {
    match &args.command {
        Some(BuildSubcommand::Plan(plan_args)) => return plan_command(plan_args).await,
        Some(BuildSubcommand::Merge(merge_args)) => return merge_command(merge_args),
        None => {}
    }

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

/// `alien build plan` — derive the native-runner groups from the stack's supported platforms.
async fn plan_command(args: &BuildPlanArgs) -> Result<()> {
    let current_dir = get_current_dir()?;
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

    // None means the stack supports every deployable platform.
    let supported: Vec<Platform> = match stack.supported_platforms() {
        Some(platforms) => platforms.to_vec(),
        None => Platform::DEPLOYABLE.to_vec(),
    };
    let groups = plan_runner_groups(&supported, stack_targets_native_host_binaries(&stack));

    if args.json {
        print_json(&groups)?;
    } else {
        println!(
            "{}",
            contextual_heading(
                "Build plan",
                stack.id(),
                &[("groups", &groups.len().to_string())]
            )
        );
        for group in &groups {
            println!(
                "  {} {} ({})",
                accent(&group.name),
                dim_label(&group.runner),
                group.platforms.join(", ")
            );
        }
    }
    Ok(())
}

/// `alien build merge` — combine partial native-runner outputs into one `.alien` directory.
fn merge_command(args: &BuildMergeArgs) -> Result<()> {
    let input = PathBuf::from(&args.input);
    let output = PathBuf::from(&args.output);
    let platforms =
        alien_build::merge::merge_build_outputs(&input, &output).context(ErrorData::BuildFailed)?;

    if args.json {
        print_json(&serde_json::json!({
            "success": true,
            "output": args.output,
            "platforms": platforms,
        }))?;
    } else {
        println!("{}", success_line("Merge complete."));
        println!("{} {}", dim_label("Output"), accent(&args.output));
        println!("{} {}", dim_label("Platforms"), platforms.join(", "));
    }
    Ok(())
}

pub async fn build_task(args: &BuildArgs) -> Result<Vec<BuildOutput>> {
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

    let has_kubernetes_platform = args
        .platforms
        .iter()
        .any(|platform| platform.eq_ignore_ascii_case(Platform::Kubernetes.as_str()));
    let kubernetes_base_platform =
        parse_kubernetes_base_platform(has_kubernetes_platform, args.base_platform.as_deref())?;

    let mut plans = Vec::new();
    for platform_str in &args.platforms {
        let platform_str = platform_str.to_ascii_lowercase();

        if let Ok(platform) = Platform::from_str(&platform_str) {
            // Check for experimental platforms
            if platform.is_experimental() && !args.experimental {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: format!(
                        "Platform '{}' is experimental and not yet production-ready. Pass --experimental to use it anyway.",
                        platform_str
                    ),
                }));
            }

            // Validate against stack's supported platforms
            if !stack.supports_platform(&platform) {
                let supported_list: Vec<&str> = stack
                    .supported_platforms()
                    .unwrap()
                    .iter()
                    .map(|p| p.as_str())
                    .collect();
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: format!(
                        "Platform '{}' is not supported by this stack. Declared platforms: [{}]",
                        platform_str,
                        supported_list.join(", ")
                    ),
                }));
            }
        }

        let parsed_platform = Platform::from_str(&platform_str).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: e,
            })
        })?;
        let base_platform = if parsed_platform == Platform::Kubernetes {
            kubernetes_base_platform
        } else {
            None
        };

        let target_platform = match platform_str.as_str() {
            "aws" => PlatformBuildSettings::Aws {
                managing_account_id: args.aws_managing_account_id.clone(),
            },
            "gcp" => PlatformBuildSettings::Gcp {},
            "azure" => PlatformBuildSettings::Azure {},
            "kubernetes" => PlatformBuildSettings::Kubernetes { base_platform },
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

        plans.push(PlatformBuildPlan {
            platform: platform_str,
            settings: BuildSettings {
                output_directory: output_dir.clone(),
                platform: target_platform,
                targets: targets.clone(),
                cache_url: args.cache_url.clone(),
                override_base_image: args.override_base_image.clone(),
                debug_mode: false,
            },
        });
    }

    let timings = build_platform_groups(stack, plans.clone()).await?;
    let mut outputs = Vec::new();
    for plan in &plans {
        let output = load_build_output(
            &plan.platform,
            &output_dir,
            *timings.get(&plan.platform).unwrap_or(&0.0),
        )?;
        outputs.push(output);
    }

    AlienEvent::Finished
        .emit_with_state(alien_core::EventState::Success)
        .await
        .ok();

    Ok(outputs)
}

async fn build_platform_groups(
    stack: alien_core::Stack,
    plans: Vec<PlatformBuildPlan>,
) -> Result<HashMap<String, f64>> {
    let groups = group_platform_builds(plans);
    if groups.len() > 1 {
        info!(
            "Building {} independent target group(s) in parallel",
            groups.len()
        );
    }

    let group_futures = groups.into_iter().map(|group| {
        let stack = stack.clone();
        async move {
            let mut timings = Vec::new();
            for plan in group {
                let platform_started = Instant::now();
                alien_build::build_stack(stack.clone(), &plan.settings)
                    .await
                    .context(ErrorData::BuildFailed)?;
                timings.push((plan.platform, platform_started.elapsed().as_secs_f64()));
            }
            Ok::<_, AlienError<ErrorData>>(timings)
        }
    });

    let grouped_timings = futures::future::try_join_all(group_futures).await?;
    Ok(grouped_timings.into_iter().flatten().collect())
}

fn group_platform_builds(plans: Vec<PlatformBuildPlan>) -> Vec<Vec<PlatformBuildPlan>> {
    let mut groups: Vec<(String, Vec<PlatformBuildPlan>)> = Vec::new();

    for plan in plans {
        let key = build_target_group_key(&plan.settings);
        if let Some((_, group)) = groups.iter_mut().find(|(group_key, _)| *group_key == key) {
            group.push(plan);
        } else {
            groups.push((key, vec![plan]));
        }
    }

    groups.into_iter().map(|(_, group)| group).collect()
}

fn build_target_group_key(settings: &BuildSettings) -> String {
    settings
        .get_targets()
        .iter()
        .map(|target| target.runtime_platform_id())
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_kubernetes_base_platform(
    has_kubernetes_platform: bool,
    base_platform: Option<&str>,
) -> Result<Option<Platform>> {
    let Some(base_platform) = base_platform else {
        return Ok(None);
    };

    let parsed = Platform::from_str(base_platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: e,
        })
    })?;

    if !has_kubernetes_platform {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: "--base-platform is only supported when building --platform kubernetes"
                .to_string(),
        }));
    }

    match parsed {
        Platform::Aws | Platform::Gcp | Platform::Azure => Ok(Some(parsed)),
        Platform::Kubernetes | Platform::Local | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "base-platform".to_string(),
                message: "--base-platform must be one of: aws, gcp, azure".to_string(),
            }))
        }
    }
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
    let build_dir = PathBuf::from(output_dir).join("build").join(platform);
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
    fn parse_kubernetes_base_platform_accepts_clouds_for_kubernetes_builds() {
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("aws")).unwrap(),
            Some(Platform::Aws)
        );
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("gcp")).unwrap(),
            Some(Platform::Gcp)
        );
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("azure")).unwrap(),
            Some(Platform::Azure)
        );
    }

    #[test]
    fn parse_kubernetes_base_platform_rejects_builds_without_kubernetes() {
        assert!(parse_kubernetes_base_platform(false, Some("aws")).is_err());
    }

    #[test]
    fn parse_kubernetes_base_platform_rejects_non_cloud_platforms() {
        assert!(parse_kubernetes_base_platform(true, Some("kubernetes")).is_err());
        assert!(parse_kubernetes_base_platform(true, Some("local")).is_err());
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

        let output = load_build_output("aws", &output_dir.display().to_string(), 1.25).unwrap();

        assert_eq!(output.stack_id, "stack-123");
        assert!(output.resources.contains(&"test-storage".to_string()));
        assert!(output.artifacts.contains_key("test-storage"));
        assert!(!output.artifacts.contains_key("test-function"));
    }

    #[test]
    fn group_platform_builds_keeps_equivalent_targets_in_order() {
        let plans = vec![
            test_plan(
                "aws",
                PlatformBuildSettings::Aws {
                    managing_account_id: None,
                },
            ),
            test_plan("gcp", PlatformBuildSettings::Gcp {}),
            test_plan("azure", PlatformBuildSettings::Azure {}),
        ];

        let groups = group_platform_builds(plans);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0][0].platform, "aws");
        assert_eq!(
            groups[1]
                .iter()
                .map(|plan| plan.platform.as_str())
                .collect::<Vec<_>>(),
            vec!["gcp", "azure"]
        );
    }

    fn test_plan(platform: &str, platform_settings: PlatformBuildSettings) -> PlatformBuildPlan {
        PlatformBuildPlan {
            platform: platform.to_string(),
            settings: BuildSettings {
                output_directory: ".alien".to_string(),
                platform: platform_settings,
                targets: None,
                cache_url: None,
                override_base_image: None,
                debug_mode: false,
            },
        }
    }
}
