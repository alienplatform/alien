//! Join and leave commands for customer-managed Linux machines.

use crate::commands::up::read_token_file;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::AlienError;
use clap::Args;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct JoinArgs {
    /// Join token printed by the deployment portal.
    #[arg(long, conflicts_with = "token_file")]
    pub token: Option<String>,

    /// Read the join token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Capacity group this host should join.
    #[arg(long, default_value = "general")]
    pub capacity_group: String,

    /// Optional physical or logical zone label for this host.
    #[arg(long)]
    pub zone: Option<String>,

    /// Machine bootstrap bundle manifest URL. Packaged CLIs embed this.
    #[arg(long)]
    pub bundle_url: Option<String>,

    /// Print the resolved join plan without installing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct LeaveArgs {
    /// Also remove durable machine identity and state.
    #[arg(long)]
    pub purge: bool,

    /// Print the leave plan without stopping or removing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinPlan {
    token_source: TokenSource,
    capacity_group: String,
    zone: Option<String>,
    bundle_url: String,
    arch: MachineArch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TokenSource {
    Argument,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MachineArch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy)]
struct HostFacts<'a> {
    os: &'a str,
    arch: &'a str,
    systemd_runtime_dir: &'a Path,
    systemctl_available: bool,
}

pub async fn join_command(args: JoinArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    output::header("Joining machine");

    let plan = build_join_plan(&args, embedded_config, current_host_facts()?)?;

    if args.dry_run {
        print_join_plan(&plan)?;
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ValidationError {
        field: "join".to_string(),
        message: "join installer is not complete yet; rerun with --dry-run to validate token, host, and bundle configuration".to_string(),
    }))
}

pub async fn leave_command(args: LeaveArgs) -> Result<()> {
    output::header("Leaving machine");

    if args.dry_run {
        output::label_value("Purge state", if args.purge { "yes" } else { "no" });
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ValidationError {
        field: "leave".to_string(),
        message: "leave installer is not complete yet; rerun with --dry-run to validate command arguments".to_string(),
    }))
}

fn build_join_plan(
    args: &JoinArgs,
    embedded_config: Option<&DeployCliConfig>,
    host: HostFacts<'_>,
) -> Result<JoinPlan> {
    let (_token, token_source) = resolve_join_token(args)?;
    let bundle_url = resolve_bundle_url(args, embedded_config)?;
    let arch = preflight_host(host)?;

    Ok(JoinPlan {
        token_source,
        capacity_group: normalize_non_empty("capacity-group", &args.capacity_group)?,
        zone: args
            .zone
            .as_deref()
            .map(|zone| normalize_non_empty("zone", zone))
            .transpose()?,
        bundle_url,
        arch,
    })
}

fn resolve_join_token(args: &JoinArgs) -> Result<(String, TokenSource)> {
    if let Some(token) = &args.token {
        return Ok((normalize_non_empty("token", token)?, TokenSource::Argument));
    }

    if let Some(path) = &args.token_file {
        return Ok((read_token_file(path)?, TokenSource::File));
    }

    Err(AlienError::new(ErrorData::ValidationError {
        field: "token".to_string(),
        message: "--token or --token-file is required".to_string(),
    }))
}

fn resolve_bundle_url(
    args: &JoinArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<String> {
    args.bundle_url
        .as_deref()
        .or_else(|| embedded_config.and_then(|config| config.machine_bundle_url.as_deref()))
        .map(|url| normalize_non_empty("bundle-url", url))
        .transpose()?
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "bundle-url".to_string(),
                message: "--bundle-url is required when the CLI was not packaged with a machine bundle URL".to_string(),
            })
        })
}

fn normalize_non_empty(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: field.to_string(),
            message: "value must not be empty".to_string(),
        }));
    }
    Ok(value.to_string())
}

fn preflight_host(host: HostFacts<'_>) -> Result<MachineArch> {
    if host.os != "linux" {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "host".to_string(),
            message: "join is supported only on Linux hosts with systemd".to_string(),
        }));
    }

    if !host.systemd_runtime_dir.exists() || !host.systemctl_available {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "systemd".to_string(),
            message: "systemd is required; run this command on a Linux host booted with systemd"
                .to_string(),
        }));
    }

    match host.arch {
        "x86_64" | "amd64" => Ok(MachineArch::X64),
        "aarch64" | "arm64" => Ok(MachineArch::Arm64),
        arch => Err(AlienError::new(ErrorData::ValidationError {
            field: "arch".to_string(),
            message: format!("unsupported CPU architecture '{arch}'"),
        })),
    }
}

fn current_host_facts() -> Result<HostFacts<'static>> {
    Ok(HostFacts {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        systemd_runtime_dir: Path::new("/run/systemd/system"),
        systemctl_available: which::which("systemctl").is_ok(),
    })
}

fn print_join_plan(plan: &JoinPlan) -> Result<()> {
    let json = serde_json::to_string_pretty(plan).map_err(|e| {
        AlienError::new(ErrorData::JsonError {
            operation: "serialize join plan".to_string(),
            reason: e.to_string(),
        })
    })?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn linux_host(systemd_runtime_dir: &Path) -> HostFacts<'_> {
        HostFacts {
            os: "linux",
            arch: "x86_64",
            systemd_runtime_dir,
            systemctl_available: true,
        }
    }

    #[test]
    fn join_plan_uses_embedded_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            token: Some(" jt_secret ".to_string()),
            token_file: None,
            capacity_group: " gpu ".to_string(),
            zone: Some(" rack-1 ".to_string()),
            bundle_url: None,
            dry_run: true,
        };
        let embedded = DeployCliConfig {
            token: None,
            deployment_group_id: None,
            default_platform: None,
            api_base_url: None,
            agent_binary_url: None,
            machine_bundle_url: Some(
                "https://packages.example.com/machines/manifest.json".to_string(),
            ),
            token_env_var: None,
            name: None,
            display_name: None,
        };

        let plan =
            build_join_plan(&args, Some(&embedded), linux_host(dir.path())).expect("join plan");

        assert_eq!(plan.token_source, TokenSource::Argument);
        assert_eq!(plan.capacity_group, "gpu");
        assert_eq!(plan.zone.as_deref(), Some("rack-1"));
        assert_eq!(
            plan.bundle_url,
            "https://packages.example.com/machines/manifest.json"
        );
        assert_eq!(plan.arch, MachineArch::X64);
    }

    #[test]
    fn join_plan_prefers_explicit_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            token: Some("jt_secret".to_string()),
            token_file: None,
            capacity_group: "general".to_string(),
            zone: None,
            bundle_url: Some("https://override.example.com/manifest.json".to_string()),
            dry_run: true,
        };
        let embedded = DeployCliConfig {
            token: None,
            deployment_group_id: None,
            default_platform: None,
            api_base_url: None,
            agent_binary_url: None,
            machine_bundle_url: Some(
                "https://packages.example.com/machines/manifest.json".to_string(),
            ),
            token_env_var: None,
            name: None,
            display_name: None,
        };

        let plan =
            build_join_plan(&args, Some(&embedded), linux_host(dir.path())).expect("join plan");

        assert_eq!(
            plan.bundle_url,
            "https://override.example.com/manifest.json"
        );
    }

    #[test]
    fn join_plan_reads_token_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut token_file = tempfile::NamedTempFile::new().expect("token file");
        token_file
            .write_all(b" jt_file_secret\n")
            .expect("write token");
        let args = JoinArgs {
            token: None,
            token_file: Some(token_file.path().to_path_buf()),
            capacity_group: "general".to_string(),
            zone: None,
            bundle_url: Some("https://packages.example.com/machines/manifest.json".to_string()),
            dry_run: true,
        };

        let plan = build_join_plan(&args, None, linux_host(dir.path())).expect("join plan");

        assert_eq!(plan.token_source, TokenSource::File);
    }

    #[test]
    fn join_plan_requires_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            token: Some("jt_secret".to_string()),
            token_file: None,
            capacity_group: "general".to_string(),
            zone: None,
            bundle_url: None,
            dry_run: true,
        };

        let error = build_join_plan(&args, None, linux_host(dir.path()))
            .expect_err("bundle URL should be required");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn preflight_rejects_non_linux_hosts() {
        let host = HostFacts {
            os: "macos",
            arch: "aarch64",
            systemd_runtime_dir: Path::new("/unused"),
            systemctl_available: true,
        };

        let error = preflight_host(host).expect_err("macOS should be rejected");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn preflight_maps_arm64() {
        let dir = tempfile::tempdir().expect("temp dir");
        let host = HostFacts {
            os: "linux",
            arch: "aarch64",
            systemd_runtime_dir: dir.path(),
            systemctl_available: true,
        };

        assert_eq!(preflight_host(host).expect("supported"), MachineArch::Arm64);
    }
}
