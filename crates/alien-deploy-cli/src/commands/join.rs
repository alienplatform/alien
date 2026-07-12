//! Join and leave commands for customer-managed Linux machines.

use crate::commands::up::read_token_file;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Args;
use flate2::read::GzDecoder;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use service_manager::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const IP_FORWARDING_CONFIG: &str = "net.ipv4.ip_forward = 1\n";
const IP_FORWARDING_SYSCTL: &str = "net.ipv4.ip_forward=1";
const DEFAULT_REGISTRATION_TIMEOUT_SECONDS: u64 = 120;
const REGISTRATION_POLL_INTERVAL_MS: u64 = 1_000;
const WRAPPED_JOIN_TOKEN_PREFIX: &str = "aj1_";

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

    /// Control plane API URL for the machine service.
    #[arg(long)]
    pub control_plane_url: Option<String>,

    /// Cluster ID the host should join.
    #[arg(long)]
    pub cluster_id: Option<String>,

    /// Print the resolved join plan without installing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Override the install root for local tests.
    #[arg(long, hide = true, default_value = "/")]
    pub install_root: PathBuf,
}

#[derive(Args, Debug)]
pub struct LeaveArgs {
    /// Also remove durable machine identity and state.
    #[arg(long)]
    pub purge: bool,

    /// Print the leave plan without stopping or removing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Override the install root for local tests.
    #[arg(long, hide = true, default_value = "/")]
    pub install_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinPlan {
    token_source: TokenSource,
    capacity_group: String,
    zone: Option<String>,
    bundle_url: String,
    control_plane_url: String,
    cluster_id: String,
    arch: MachineArch,
}

#[derive(Debug, Clone)]
struct JoinRequest {
    token: String,
    plan: JoinPlan,
    install_root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WrappedJoinToken {
    join_token: String,
    control_plane_url: String,
    cluster_id: String,
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

impl MachineArch {
    fn manifest_name(self) -> &'static str {
        match self {
            MachineArch::X64 => "x64",
            MachineArch::Arm64 => "arm64",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleManifest {
    version: String,
    config: MachineBundleConfig,
    service: MachineBundleService,
    artifacts: Vec<MachineBundleArtifact>,
    #[serde(default)]
    registration: Option<MachineBundleRegistration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleService {
    label: String,
    executable: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    restart_policy: MachineBundleRestartPolicy,
    #[serde(default)]
    restart_delay_seconds: Option<u32>,
    #[serde(default)]
    systemd: Option<MachineBundleSystemdService>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MachineBundleRestartPolicy {
    Always,
    #[default]
    OnFailure,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleSystemdService {
    service_type: MachineBundleSystemdServiceType,
    #[serde(default)]
    notify_access: Option<MachineBundleSystemdNotifyAccess>,
    #[serde(default)]
    pid_file: Option<String>,
    #[serde(default)]
    timeout_start_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MachineBundleSystemdServiceType {
    Simple,
    Notify,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MachineBundleSystemdNotifyAccess {
    Main,
    Exec,
    All,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleConfig {
    path: String,
    join_token_file: String,
    #[serde(default)]
    machine_id_file: Option<String>,
    #[serde(default)]
    machine_token_file: Option<String>,
    entries: Vec<MachineBundleConfigEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleConfigEntry {
    key: String,
    source: MachineBundleConfigSource,
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MachineBundleConfigSource {
    Literal(String),
    ControlPlaneUrl,
    ClusterId,
    JoinTokenFile,
    MachineIdFile,
    MachineTokenFile,
    CapacityGroup,
    Zone,
    BundleVersion,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleArtifact {
    os: String,
    arch: String,
    url: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleRegistration {
    machine_id_file: String,
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineInstallState {
    bundle_version: String,
    #[serde(default)]
    bundle_url: Option<String>,
    service_label: String,
    executable_path: PathBuf,
    config_path: PathBuf,
    join_token_path: PathBuf,
    machine_id_path: Option<PathBuf>,
    machine_token_path: Option<PathBuf>,
    #[serde(default)]
    control_plane_url: Option<String>,
    #[serde(default)]
    cluster_id: Option<String>,
    machine_id: Option<String>,
}

#[derive(Debug)]
struct InstallPaths {
    bundle_dir: PathBuf,
    state_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct HostFacts<'a> {
    os: &'a str,
    arch: &'a str,
    systemd_runtime_dir: &'a Path,
    systemctl_available: bool,
    wireguard_available: bool,
}

pub async fn join_command(args: JoinArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    output::header("Joining machine");

    let request = build_join_request(&args, embedded_config, current_host_facts()?)?;

    if args.dry_run {
        print_join_plan(&request.plan)?;
        return Ok(());
    }

    install_join(request).await
}

pub async fn leave_command(args: LeaveArgs) -> Result<()> {
    output::header("Leaving machine");

    if args.dry_run {
        output::label_value("Purge state", if args.purge { "yes" } else { "no" });
        return Ok(());
    }

    uninstall_joined_machine(&args.install_root, args.purge).await
}

#[cfg(test)]
fn build_join_plan(
    args: &JoinArgs,
    embedded_config: Option<&DeployCliConfig>,
    host: HostFacts<'_>,
) -> Result<JoinPlan> {
    Ok(build_join_request(args, embedded_config, host)?.plan)
}

fn build_join_request(
    args: &JoinArgs,
    embedded_config: Option<&DeployCliConfig>,
    host: HostFacts<'_>,
) -> Result<JoinRequest> {
    let (token, token_source) = resolve_join_token(args)?;
    let wrapped_token = parse_wrapped_join_token(&token)?;
    let bundle_url = resolve_bundle_url(args, embedded_config)?;
    let control_plane_url = resolve_control_plane_url(args, wrapped_token.as_ref())?;
    let cluster_id = resolve_cluster_id(args, wrapped_token.as_ref())?;
    let arch = preflight_host(host)?;

    Ok(JoinRequest {
        token: wrapped_token
            .map(|wrapped| wrapped.join_token)
            .unwrap_or(token),
        install_root: args.install_root.clone(),
        plan: JoinPlan {
            token_source,
            capacity_group: normalize_non_empty("capacity-group", &args.capacity_group)?,
            zone: args
                .zone
                .as_deref()
                .map(|zone| normalize_non_empty("zone", zone))
                .transpose()?,
            bundle_url,
            control_plane_url,
            cluster_id,
            arch,
        },
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

fn parse_wrapped_join_token(token: &str) -> Result<Option<WrappedJoinToken>> {
    let Some(encoded) = token.strip_prefix(WRAPPED_JOIN_TOKEN_PREFIX) else {
        return Ok(None);
    };
    let json = URL_SAFE_NO_PAD.decode(encoded).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: format!("wrapped join token is not valid base64url: {e}"),
        })
    })?;
    let wrapped: WrappedJoinToken = serde_json::from_slice(&json).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: format!("wrapped join token is not valid JSON: {e}"),
        })
    })?;
    normalize_non_empty("token.joinToken", &wrapped.join_token)?;
    validate_control_plane_url(&wrapped.control_plane_url)?;
    normalize_non_empty("token.clusterId", &wrapped.cluster_id)?;
    Ok(Some(wrapped))
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

fn resolve_control_plane_url(
    args: &JoinArgs,
    wrapped_token: Option<&WrappedJoinToken>,
) -> Result<String> {
    let url = args
        .control_plane_url
        .as_deref()
        .map(|value| normalize_non_empty("control-plane-url", value))
        .transpose()?
        .or_else(|| wrapped_token.map(|token| token.control_plane_url.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "control-plane-url".to_string(),
                message:
                    "--control-plane-url is required when --token is not a wrapped Machines join token"
                        .to_string(),
            })
        })?;
    validate_control_plane_url(&url)
}

fn validate_control_plane_url(url: &str) -> Result<String> {
    let parsed = Url::parse(&url).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "control-plane-url".to_string(),
            message: format!("invalid URL: {e}"),
        })
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(url.to_string()),
        scheme => Err(AlienError::new(ErrorData::ValidationError {
            field: "control-plane-url".to_string(),
            message: format!("unsupported URL scheme '{scheme}'"),
        })),
    }
}

fn resolve_cluster_id(args: &JoinArgs, wrapped_token: Option<&WrappedJoinToken>) -> Result<String> {
    args.cluster_id
        .as_deref()
        .map(|value| normalize_non_empty("cluster-id", value))
        .transpose()?
        .or_else(|| wrapped_token.map(|token| token.cluster_id.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "cluster-id".to_string(),
                message:
                    "--cluster-id is required when --token is not a wrapped Machines join token"
                        .to_string(),
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

    if !host.wireguard_available {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "wireguard".to_string(),
            message:
                "kernel WireGuard support is required; load or install the wireguard kernel module"
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
        wireguard_available: wireguard_available(),
    })
}

fn wireguard_available() -> bool {
    Path::new("/sys/module/wireguard").exists()
        || Command::new("modprobe")
            .arg("-n")
            .arg("wireguard")
            .status()
            .is_ok_and(|status| status.success())
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

async fn install_join(request: JoinRequest) -> Result<()> {
    let paths = install_paths(&request.install_root);

    output::step(1, 6, "Resolving machine bundle");
    let manifest = download_manifest(&request.plan.bundle_url).await?;
    if let Some(state) = existing_joined_machine(&paths, &request, &manifest)? {
        output::success("Machine is already joined");
        output::info(&format!("  Service: {}", state.service_label));
        output::info(&format!("  Bundle:  {}", state.bundle_version));
        if let Some(machine_id) = state.machine_id {
            output::label_value("Machine", &machine_id);
        }
        return Ok(());
    }

    let artifact = select_bundle_artifact(&manifest, request.plan.arch)?;
    let artifact_url = resolve_artifact_url(&request.plan.bundle_url, &artifact.url)?;

    output::step(2, 6, "Downloading machine bundle");
    std::fs::create_dir_all(&paths.bundle_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: paths.bundle_dir.display().to_string(),
            reason: "Failed to create bundle directory".to_string(),
        })?;
    let archive_path = paths
        .bundle_dir
        .join(format!("machine-bundle-{}.tar.gz", manifest.version));
    download_verified_artifact(&artifact_url, &artifact.sha256, &archive_path).await?;

    output::step(3, 6, "Installing bundle files");
    let extracted_dir = paths.bundle_dir.join(&manifest.version);
    if extracted_dir.exists() {
        std::fs::remove_dir_all(&extracted_dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "remove".to_string(),
                file_path: extracted_dir.display().to_string(),
                reason: "Failed to replace existing bundle directory".to_string(),
            })?;
    }
    std::fs::create_dir_all(&extracted_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: extracted_dir.display().to_string(),
            reason: "Failed to create extracted bundle directory".to_string(),
        })?;
    extract_tar_gz(&archive_path, &extracted_dir)?;

    output::step(4, 6, "Writing machine configuration");
    let executable_path = safe_join(&extracted_dir, &manifest.service.executable)?;
    if !executable_path.is_file() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle.service.executable".to_string(),
            message: format!(
                "bundle executable '{}' was not found after extraction",
                manifest.service.executable
            ),
        }));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable_path, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: executable_path.display().to_string(),
                reason: "Failed to make bundle executable runnable".to_string(),
            })?;
    }

    let config_path = write_machine_config(&paths, &request, &manifest)?;
    configure_ip_forwarding(&request.install_root)?;
    let join_token_path = rooted_manifest_path(
        &request.install_root,
        "bundle.config.joinTokenFile",
        &manifest.config.join_token_file,
    )?;
    let machine_id_path = manifest
        .config
        .machine_id_file
        .as_deref()
        .map(|path| {
            rooted_manifest_path(&request.install_root, "bundle.config.machineIdFile", path)
        })
        .transpose()?;
    let machine_token_path = manifest
        .config
        .machine_token_file
        .as_deref()
        .map(|path| {
            rooted_manifest_path(
                &request.install_root,
                "bundle.config.machineTokenFile",
                path,
            )
        })
        .transpose()?;
    let mut state = MachineInstallState {
        bundle_version: manifest.version.clone(),
        bundle_url: Some(request.plan.bundle_url.clone()),
        service_label: manifest.service.label.clone(),
        executable_path: executable_path.clone(),
        config_path: config_path.clone(),
        join_token_path,
        machine_id_path,
        machine_token_path,
        control_plane_url: Some(request.plan.control_plane_url.clone()),
        cluster_id: Some(request.plan.cluster_id.clone()),
        machine_id: None,
    };

    output::step(5, 6, "Installing machine service");
    install_machine_service(&manifest.service, &executable_path, &config_path)?;
    write_install_state(&paths, &state)?;

    output::step(6, 6, "Waiting for machine registration");
    if let Some(registration) = &manifest.registration {
        let machine_id = wait_for_registration(&request.install_root, registration).await?;
        state.machine_id = Some(machine_id.clone());
        write_install_state(&paths, &state)?;
        output::label_value("Machine", &machine_id);
    } else {
        output::info("Registration wait skipped; bundle manifest has no machine id file");
    }

    output::success("Machine service installed and started");
    output::info(&format!("  Service: {}", state.service_label));
    output::info(&format!("  Bundle:  {}", state.bundle_version));
    Ok(())
}

fn existing_joined_machine(
    paths: &InstallPaths,
    request: &JoinRequest,
    manifest: &MachineBundleManifest,
) -> Result<Option<MachineInstallState>> {
    let state_path = install_state_path(paths);
    if !state_path.exists() {
        return Ok(None);
    }

    let state = read_install_state(&state_path)?;
    if !install_state_matches_request(&state, request, manifest) {
        return Ok(None);
    }

    let Some(machine_id) = state.machine_id.as_deref() else {
        return Ok(None);
    };
    if machine_id.trim().is_empty() {
        return Ok(None);
    }

    if let Some(machine_id_path) = &state.machine_id_path {
        match read_secret_string(machine_id_path, "machine id") {
            Ok(stored_machine_id) if stored_machine_id == machine_id => {}
            _ => {
                return Ok(None);
            }
        }
    }

    if machine_service_status(&state.service_label)? != ServiceStatus::Running {
        return Ok(None);
    }

    Ok(Some(state))
}

fn install_state_matches_request(
    state: &MachineInstallState,
    request: &JoinRequest,
    manifest: &MachineBundleManifest,
) -> bool {
    state.bundle_version == manifest.version
        && state.bundle_url.as_deref() == Some(request.plan.bundle_url.as_str())
        && state.service_label == manifest.service.label
        && state.control_plane_url.as_deref() == Some(request.plan.control_plane_url.as_str())
        && state.cluster_id.as_deref() == Some(request.plan.cluster_id.as_str())
}

fn machine_service_status(service_label: &str) -> Result<ServiceStatus> {
    let manager = native_service_manager()?;
    let label = parse_service_label(service_label)?;
    manager
        .status(ServiceStatusCtx { label })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to inspect machine service status".to_string(),
        })
}

async fn download_manifest(url: &str) -> Result<MachineBundleManifest> {
    let response = reqwest::get(url)
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "GET".to_string(),
            url: url.to_string(),
            reason: "Failed to download machine bundle manifest".to_string(),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: "GET".to_string(),
            url: url.to_string(),
            reason: format!("server returned {}", response.status()),
        }));
    }

    response
        .json::<MachineBundleManifest>()
        .await
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse machine bundle manifest".to_string(),
            reason: "Invalid manifest JSON".to_string(),
        })
}

fn select_bundle_artifact(
    manifest: &MachineBundleManifest,
    arch: MachineArch,
) -> Result<&MachineBundleArtifact> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.os == "linux" && artifact.arch == arch.manifest_name())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "bundle.artifacts".to_string(),
                message: format!("manifest has no linux/{} artifact", arch.manifest_name()),
            })
        })
}

fn resolve_artifact_url(manifest_url: &str, artifact_url: &str) -> Result<String> {
    if Url::parse(artifact_url).is_ok() {
        return Ok(artifact_url.to_string());
    }

    let base = Url::parse(manifest_url).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "bundle-url".to_string(),
            message: format!("invalid manifest URL: {e}"),
        })
    })?;
    base.join(artifact_url)
        .map(|url| url.to_string())
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "bundle.artifacts.url".to_string(),
                message: format!("invalid artifact URL: {e}"),
            })
        })
}

async fn download_verified_artifact(url: &str, expected_sha256: &str, path: &Path) -> Result<()> {
    let response = reqwest::get(url)
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "GET".to_string(),
            url: url.to_string(),
            reason: "Failed to download machine bundle artifact".to_string(),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: "GET".to_string(),
            url: url.to_string(),
            reason: format!("server returned {}", response.status()),
        }));
    }

    let bytes = response
        .bytes()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "read".to_string(),
            url: url.to_string(),
            reason: "Failed to read machine bundle artifact".to_string(),
        })?;
    verify_sha256(&bytes, expected_sha256)?;
    std::fs::write(path, &bytes)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write machine bundle artifact".to_string(),
        })?;
    Ok(())
}

fn verify_sha256(bytes: &[u8], expected_sha256: &str) -> Result<()> {
    let expected = expected_sha256
        .strip_prefix("sha256:")
        .unwrap_or(expected_sha256)
        .trim()
        .to_ascii_lowercase();
    let actual = hex::encode(Sha256::digest(bytes));

    if expected != actual {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle.artifacts.sha256".to_string(),
            message: format!("checksum mismatch: expected {expected}, got {actual}"),
        }));
    }

    Ok(())
}

fn extract_tar_gz(archive_path: &Path, destination: &Path) -> Result<()> {
    let file =
        File::open(archive_path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "open".to_string(),
                file_path: archive_path.display().to_string(),
                reason: "Failed to open machine bundle archive".to_string(),
            })?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: archive_path.display().to_string(),
            reason: "Failed to read machine bundle archive entries".to_string(),
        })?;

    for entry in entries {
        let mut entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: archive_path.display().to_string(),
                reason: "Failed to read machine bundle archive entry".to_string(),
            })?;
        let unpacked = entry.unpack_in(destination).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "extract".to_string(),
                file_path: archive_path.display().to_string(),
                reason: "Failed to extract machine bundle archive".to_string(),
            },
        )?;
        if !unpacked {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "bundle.artifact".to_string(),
                message: "archive contains a path outside the install directory".to_string(),
            }));
        }
    }

    Ok(())
}

fn write_machine_config(
    paths: &InstallPaths,
    request: &JoinRequest,
    manifest: &MachineBundleManifest,
) -> Result<PathBuf> {
    std::fs::create_dir_all(&paths.state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: paths.state_dir.display().to_string(),
            reason: "Failed to create machine state directory".to_string(),
        })?;

    let config_path = rooted_manifest_path(
        &request.install_root,
        "bundle.config.path",
        &manifest.config.path,
    )?;
    let config_parent = config_path.parent().ok_or_else(|| {
        AlienError::new(ErrorData::FileOperationFailed {
            operation: "resolve".to_string(),
            file_path: config_path.display().to_string(),
            reason: "Failed to resolve machine config directory".to_string(),
        })
    })?;
    std::fs::create_dir_all(config_parent)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: config_parent.display().to_string(),
            reason: "Failed to create machine config directory".to_string(),
        })?;

    let token_path = rooted_manifest_path(
        &request.install_root,
        "bundle.config.joinTokenFile",
        &manifest.config.join_token_file,
    )?;
    let token_parent = token_path.parent().ok_or_else(|| {
        AlienError::new(ErrorData::FileOperationFailed {
            operation: "resolve".to_string(),
            file_path: token_path.display().to_string(),
            reason: "Failed to resolve join token directory".to_string(),
        })
    })?;
    std::fs::create_dir_all(token_parent)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: token_parent.display().to_string(),
            reason: "Failed to create join token directory".to_string(),
        })?;
    write_secret_file(&token_path, &request.token)?;

    let mut config = toml::Table::new();
    for entry in &manifest.config.entries {
        match resolve_config_entry_value(entry, request, manifest, &token_path)? {
            Some(value) => {
                config.insert(entry.key.clone(), value.into());
            }
            None if entry.optional => {}
            None => {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: format!("bundle.config.entries.{}", entry.key),
                    message: "required value is missing".to_string(),
                }));
            }
        }
    }
    let config_text =
        toml::to_string(&config)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize machine config".to_string(),
                reason: "Failed to serialize machine configuration as TOML".to_string(),
            })?;
    write_secret_file(&config_path, &config_text)?;
    Ok(config_path)
}

fn resolve_config_entry_value(
    entry: &MachineBundleConfigEntry,
    request: &JoinRequest,
    manifest: &MachineBundleManifest,
    join_token_path: &Path,
) -> Result<Option<String>> {
    match &entry.source {
        MachineBundleConfigSource::Literal(value) => Ok(Some(value.clone())),
        MachineBundleConfigSource::ControlPlaneUrl => {
            Ok(Some(request.plan.control_plane_url.clone()))
        }
        MachineBundleConfigSource::ClusterId => Ok(Some(request.plan.cluster_id.clone())),
        MachineBundleConfigSource::JoinTokenFile => Ok(Some(join_token_path.display().to_string())),
        MachineBundleConfigSource::MachineIdFile => manifest
            .config
            .machine_id_file
            .as_deref()
            .map(|path| {
                rooted_manifest_path(&request.install_root, "bundle.config.machineIdFile", path)
            })
            .transpose()
            .map(|path| path.map(|path| path.display().to_string())),
        MachineBundleConfigSource::MachineTokenFile => manifest
            .config
            .machine_token_file
            .as_deref()
            .map(|path| {
                rooted_manifest_path(
                    &request.install_root,
                    "bundle.config.machineTokenFile",
                    path,
                )
            })
            .transpose()
            .map(|path| path.map(|path| path.display().to_string())),
        MachineBundleConfigSource::CapacityGroup => Ok(Some(request.plan.capacity_group.clone())),
        MachineBundleConfigSource::Zone => Ok(request.plan.zone.clone()),
        MachineBundleConfigSource::BundleVersion => Ok(Some(manifest.version.clone())),
    }
}

fn configure_ip_forwarding(install_root: &Path) -> Result<()> {
    let config_path = rooted_path(install_root, "etc/sysctl.d/99-alien-machine.conf");
    let parent = config_path.parent().ok_or_else(|| {
        AlienError::new(ErrorData::FileOperationFailed {
            operation: "resolve".to_string(),
            file_path: config_path.display().to_string(),
            reason: "Failed to resolve sysctl configuration directory".to_string(),
        })
    })?;
    std::fs::create_dir_all(parent)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: parent.display().to_string(),
            reason: "Failed to create sysctl configuration directory".to_string(),
        })?;
    std::fs::write(&config_path, IP_FORWARDING_CONFIG)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: config_path.display().to_string(),
            reason: "Failed to persist IPv4 forwarding configuration".to_string(),
        })?;

    if install_root == Path::new("/") {
        apply_ip_forwarding_now()?;
    }

    Ok(())
}

async fn wait_for_registration(
    install_root: &Path,
    registration: &MachineBundleRegistration,
) -> Result<String> {
    let path = registration_file_path(install_root, registration)?;
    let timeout = Duration::from_secs(
        registration
            .timeout_seconds
            .unwrap_or(DEFAULT_REGISTRATION_TIMEOUT_SECONDS),
    );
    wait_for_machine_id_file(
        &path,
        timeout,
        Duration::from_millis(REGISTRATION_POLL_INTERVAL_MS),
    )
    .await
}

fn registration_file_path(
    install_root: &Path,
    registration: &MachineBundleRegistration,
) -> Result<PathBuf> {
    let relative = safe_relative_path(
        "bundle.registration.machineIdFile",
        &normalize_non_empty(
            "bundle.registration.machineIdFile",
            &registration.machine_id_file,
        )?,
    )?;
    Ok(if install_root == Path::new("/") {
        PathBuf::from("/").join(relative)
    } else {
        install_root.join(relative)
    })
}

async fn wait_for_machine_id_file(
    path: &Path,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<String> {
    let started = Instant::now();

    loop {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let machine_id = contents.trim();
                if !machine_id.is_empty() {
                    return Ok(machine_id.to_string());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AlienError::new(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: path.display().to_string(),
                    reason: error.to_string(),
                }));
            }
        }

        if started.elapsed() >= timeout {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Timed out waiting for machine registration at {}",
                    path.display()
                ),
            }));
        }

        tokio::time::sleep(poll_interval).await;
    }
}

fn apply_ip_forwarding_now() -> Result<()> {
    let output = Command::new("sysctl")
        .arg("-w")
        .arg(IP_FORWARDING_SYSCTL)
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to run sysctl to enable IPv4 forwarding".to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "sysctl failed while enabling IPv4 forwarding: {}",
                if stderr.is_empty() {
                    output.status.to_string()
                } else {
                    stderr
                }
            ),
        }));
    }

    Ok(())
}

fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write machine configuration".to_string(),
        })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to restrict machine configuration permissions".to_string(),
            })?;
    }

    Ok(())
}

fn install_machine_service(
    service: &MachineBundleService,
    executable_path: &Path,
    config_path: &Path,
) -> Result<()> {
    let manager = native_service_manager()?;
    let label = parse_service_label(&service.label)?;
    let rendered_args = service
        .args
        .iter()
        .map(|arg| OsString::from(arg.replace("{config_path}", &config_path.display().to_string())))
        .collect();
    let environment = if service.environment.is_empty() {
        None
    } else {
        Some(
            service
                .environment
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        value.replace("{config_path}", &config_path.display().to_string()),
                    )
                })
                .collect(),
        )
    };

    let restart_policy = match service.restart_policy {
        MachineBundleRestartPolicy::Always => RestartPolicy::Always {
            delay_secs: service.restart_delay_seconds,
        },
        MachineBundleRestartPolicy::OnFailure => RestartPolicy::OnFailure {
            delay_secs: service.restart_delay_seconds.or(Some(5)),
        },
    };
    let contents = service
        .systemd
        .as_ref()
        .map(|systemd| {
            render_systemd_machine_service(
                service,
                systemd,
                executable_path,
                config_path,
                executable_path.parent(),
            )
        })
        .transpose()?;

    manager
        .install(ServiceInstallCtx {
            label: label.clone(),
            program: executable_path.to_path_buf(),
            args: rendered_args,
            contents,
            username: None,
            working_directory: executable_path.parent().map(Path::to_path_buf),
            environment,
            autostart: true,
            restart_policy,
        })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to install machine service".to_string(),
        })?;

    if service.systemd.is_some() {
        reload_systemd_units()?;
    }

    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });

    manager
        .start(ServiceStartCtx { label })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to start machine service".to_string(),
        })?;

    Ok(())
}

fn render_systemd_machine_service(
    service: &MachineBundleService,
    systemd: &MachineBundleSystemdService,
    executable_path: &Path,
    config_path: &Path,
    working_directory: Option<&Path>,
) -> Result<String> {
    if matches!(
        systemd.service_type,
        MachineBundleSystemdServiceType::Notify
    ) && !matches!(
        systemd.notify_access,
        Some(MachineBundleSystemdNotifyAccess::All)
    ) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle.service.systemd.notifyAccess".to_string(),
            message: "notify machine services must use notifyAccess=all so handoff children can promote themselves".to_string(),
        }));
    }

    let rendered_args = service
        .args
        .iter()
        .map(|arg| arg.replace("{config_path}", &config_path.display().to_string()));
    let mut exec_start = systemd_quote(executable_path.as_os_str().to_string_lossy().as_ref());
    for arg in rendered_args {
        exec_start.push(' ');
        exec_start.push_str(&systemd_quote(&arg));
    }

    let mut unit = format!(
        "[Unit]\nDescription={}\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType={}\n",
        service.label,
        match systemd.service_type {
            MachineBundleSystemdServiceType::Simple => "simple",
            MachineBundleSystemdServiceType::Notify => "notify",
        }
    );
    if let Some(notify_access) = systemd.notify_access {
        unit.push_str(&format!(
            "NotifyAccess={}\n",
            match notify_access {
                MachineBundleSystemdNotifyAccess::Main => "main",
                MachineBundleSystemdNotifyAccess::Exec => "exec",
                MachineBundleSystemdNotifyAccess::All => "all",
            }
        ));
    }
    if let Some(pid_file) = &systemd.pid_file {
        unit.push_str(&format!("PIDFile={}\n", systemd_escape_value(pid_file)));
    }
    if let Some(timeout) = systemd.timeout_start_seconds {
        unit.push_str(&format!("TimeoutStartSec={timeout}\n"));
    }
    if let Some(working_directory) = working_directory {
        unit.push_str(&format!(
            "WorkingDirectory={}\n",
            systemd_escape_value(&working_directory.display().to_string())
        ));
    }
    for (key, value) in &service.environment {
        let value = value.replace("{config_path}", &config_path.display().to_string());
        unit.push_str(&format!(
            "Environment={}\n",
            systemd_quote(&format!("{key}={value}"))
        ));
    }
    unit.push_str(&format!("ExecStart={exec_start}\n"));
    unit.push_str(match service.restart_policy {
        MachineBundleRestartPolicy::Always => "Restart=always\n",
        MachineBundleRestartPolicy::OnFailure => "Restart=on-failure\n",
    });
    if let Some(delay) = service.restart_delay_seconds {
        unit.push_str(&format!("RestartSec={delay}\n"));
    }
    unit.push_str("\n[Install]\nWantedBy=multi-user.target\n");
    Ok(unit)
}

fn systemd_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    )
}

fn systemd_escape_value(value: &str) -> String {
    value
        .replace('%', "%%")
        .replace(char::is_whitespace, "\\x20")
}

fn reload_systemd_units() -> Result<()> {
    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to reload systemd after installing machine service".to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::OperatorServiceError {
        message: format!(
            "systemctl daemon-reload failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }))
}

async fn uninstall_joined_machine(install_root: &Path, purge: bool) -> Result<()> {
    let paths = install_paths(install_root);
    let state_path = install_state_path(&paths);
    let state = read_install_state(&state_path)?;
    let manager = native_service_manager()?;
    let label = parse_service_label(&state.service_label)?;

    match request_machine_drain(&state).await {
        Ok(true) => output::info("Drain requested for machine"),
        Ok(false) => output::info(
            "Drain skipped; installed machine state is missing control-plane credentials",
        ),
        Err(error) => output::warn(&format!(
            "Could not request control-plane drain before uninstall: {error}"
        )),
    }

    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    manager
        .uninstall(ServiceUninstallCtx { label })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to uninstall machine service".to_string(),
        })?;

    match request_machine_leave(&state).await {
        Ok(true) => output::info("Machine removed from control plane"),
        Ok(false) => output::info(
            "Control-plane leave skipped; installed machine state is missing credentials",
        ),
        Err(error) => output::warn(&format!(
            "Could not remove machine from control plane before uninstall: {error}"
        )),
    }

    remove_file_if_exists(&state.join_token_path)?;
    if let Some(machine_token_path) = &state.machine_token_path {
        remove_file_if_exists(machine_token_path)?;
    }
    remove_file_if_exists(&state.config_path)?;
    if let Some(config_parent) = state.config_path.parent() {
        remove_dir_if_empty(config_parent)?;
    }
    let _ = std::fs::remove_file(&state_path);
    if purge {
        if let Some(machine_id_path) = &state.machine_id_path {
            if let Some(machine_state_dir) = machine_id_path.parent() {
                remove_dir_if_exists(machine_state_dir)?;
            }
        }
        remove_dir_if_exists(&paths.state_dir)?;
        remove_dir_if_exists(&paths.bundle_dir)?;
    }

    output::success("Machine service uninstalled");
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineDrainRequest<'a> {
    cluster_id: &'a str,
    machine_id: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineLeaveRequest<'a> {
    cluster_id: &'a str,
    machine_id: &'a str,
}

async fn request_machine_drain(state: &MachineInstallState) -> Result<bool> {
    let (Some(control_plane_url), Some(cluster_id), Some(machine_id), Some(machine_token_path)) = (
        state.control_plane_url.as_deref(),
        state.cluster_id.as_deref(),
        state.machine_id.as_deref(),
        state.machine_token_path.as_deref(),
    ) else {
        return Ok(false);
    };

    let machine_token = read_secret_string(machine_token_path, "machine token")?;
    let drain_url = control_plane_endpoint(control_plane_url, "drain")?;
    let response = reqwest::Client::new()
        .post(drain_url.clone())
        .header("Authorization", format!("Machine {machine_token}"))
        .json(&MachineDrainRequest {
            cluster_id,
            machine_id,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "POST".to_string(),
            url: drain_url.to_string(),
            reason: "Failed to request machine drain".to_string(),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: "POST".to_string(),
            url: drain_url.to_string(),
            reason: format!("server returned {}", response.status()),
        }));
    }

    Ok(true)
}

async fn request_machine_leave(state: &MachineInstallState) -> Result<bool> {
    let (Some(control_plane_url), Some(cluster_id), Some(machine_id), Some(machine_token_path)) = (
        state.control_plane_url.as_deref(),
        state.cluster_id.as_deref(),
        state.machine_id.as_deref(),
        state.machine_token_path.as_deref(),
    ) else {
        return Ok(false);
    };

    let machine_token = read_secret_string(machine_token_path, "machine token")?;
    let leave_url = control_plane_endpoint(control_plane_url, "leave")?;
    let response = reqwest::Client::new()
        .post(leave_url.clone())
        .header("Authorization", format!("Machine {machine_token}"))
        .json(&MachineLeaveRequest {
            cluster_id,
            machine_id,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpError {
            operation: "POST".to_string(),
            url: leave_url.to_string(),
            reason: "Failed to remove machine from control plane".to_string(),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::HttpError {
            operation: "POST".to_string(),
            url: leave_url.to_string(),
            reason: format!("server returned {}", response.status()),
        }));
    }

    Ok(true)
}

fn control_plane_endpoint(base_url: &str, relative_path: &str) -> Result<Url> {
    let mut base = Url::parse(base_url).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "control-plane-url".to_string(),
            message: format!("invalid URL: {e}"),
        })
    })?;
    let normalized_path = format!("{}/", base.path().trim_end_matches('/'));
    base.set_path(&normalized_path);
    base.join(relative_path.trim_start_matches('/'))
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "control-plane-url".to_string(),
                message: format!("invalid drain endpoint: {e}"),
            })
        })
}

fn read_secret_string(path: &Path, label: &str) -> Result<String> {
    let value = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: format!("Failed to read {label}"),
        },
    )?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: label.to_string(),
            message: "value is empty".to_string(),
        }));
    }
    Ok(value)
}

fn native_service_manager() -> Result<Box<dyn ServiceManager>> {
    <dyn ServiceManager>::native()
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "No supported service manager found".to_string(),
        })
}

fn parse_service_label(label: &str) -> Result<ServiceLabel> {
    label.parse().map_err(|_| {
        AlienError::new(ErrorData::ValidationError {
            field: "bundle.service.label".to_string(),
            message: format!("'{label}' is not a valid service label"),
        })
    })
}

fn write_install_state(paths: &InstallPaths, state: &MachineInstallState) -> Result<()> {
    std::fs::create_dir_all(&paths.state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: paths.state_dir.display().to_string(),
            reason: "Failed to create machine state directory".to_string(),
        })?;
    let state_path = install_state_path(paths);
    let json = serde_json::to_vec_pretty(state)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize machine install state".to_string(),
            reason: "Invalid install state".to_string(),
        })?;
    std::fs::write(&state_path, json)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: state_path.display().to_string(),
            reason: "Failed to write machine install state".to_string(),
        })
}

fn read_install_state(path: &Path) -> Result<MachineInstallState> {
    let bytes = std::fs::read(path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Machine is not joined on this host".to_string(),
        })?;
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse machine install state".to_string(),
            reason: "Invalid install state JSON".to_string(),
        })
}

fn install_state_path(paths: &InstallPaths) -> PathBuf {
    paths.state_dir.join("install-state.json")
}

fn install_paths(root: &Path) -> InstallPaths {
    InstallPaths {
        bundle_dir: rooted_path(root, "opt/alien/machine-bundle"),
        state_dir: rooted_path(root, "var/lib/alien/machine"),
    }
}

fn rooted_path(root: &Path, relative: &str) -> PathBuf {
    if root == Path::new("/") {
        PathBuf::from("/").join(relative)
    } else {
        root.join(relative)
    }
}

fn safe_join(base: &Path, relative: &str) -> Result<PathBuf> {
    Ok(base.join(safe_relative_path("bundle.service.executable", relative)?))
}

fn rooted_manifest_path(root: &Path, field: &str, relative: &str) -> Result<PathBuf> {
    let relative = safe_relative_path(field, &normalize_non_empty(field, relative)?)?;
    Ok(if root == Path::new("/") {
        PathBuf::from("/").join(relative)
    } else {
        root.join(relative)
    })
}

fn safe_relative_path(field: &str, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: field.to_string(),
            message: "path must be relative and stay inside the install root".to_string(),
        }));
    }
    Ok(relative_path.to_path_buf())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AlienError::new(ErrorData::FileOperationFailed {
            operation: "remove".to_string(),
            file_path: path.display().to_string(),
            reason: e.to_string(),
        })),
    }
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::DirectoryNotEmpty =>
        {
            Ok(())
        }
        Err(e) => Err(AlienError::new(ErrorData::FileOperationFailed {
            operation: "remove".to_string(),
            file_path: path.display().to_string(),
            reason: e.to_string(),
        })),
    }
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AlienError::new(ErrorData::FileOperationFailed {
            operation: "remove".to_string(),
            file_path: path.display().to_string(),
            reason: e.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    fn linux_host(systemd_runtime_dir: &Path) -> HostFacts<'_> {
        HostFacts {
            os: "linux",
            arch: "x86_64",
            systemd_runtime_dir,
            systemctl_available: true,
            wireguard_available: true,
        }
    }

    fn test_join_args() -> JoinArgs {
        JoinArgs {
            token: Some("jt_secret".to_string()),
            token_file: None,
            capacity_group: "general".to_string(),
            zone: None,
            bundle_url: Some("https://packages.example.com/manifest.json".to_string()),
            control_plane_url: Some("https://control.example.com".to_string()),
            cluster_id: Some("cluster-123".to_string()),
            dry_run: false,
            install_root: PathBuf::from("/"),
        }
    }

    fn wrapped_join_token(join_token: &str, control_plane_url: &str, cluster_id: &str) -> String {
        let payload = serde_json::json!({
            "joinToken": join_token,
            "controlPlaneUrl": control_plane_url,
            "clusterId": cluster_id,
        });
        format!(
            "{WRAPPED_JOIN_TOKEN_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(payload.to_string())
        )
    }

    fn test_manifest() -> MachineBundleManifest {
        MachineBundleManifest {
            version: "2026-07-05".to_string(),
            config: MachineBundleConfig {
                path: "etc/machine-service/machine.toml".to_string(),
                join_token_file: "var/lib/machine-service/join-token".to_string(),
                machine_id_file: Some("var/lib/machine-service/machine-id".to_string()),
                machine_token_file: Some("var/lib/machine-service/machine-token".to_string()),
                entries: vec![
                    MachineBundleConfigEntry {
                        key: "mode".to_string(),
                        source: MachineBundleConfigSource::Literal("external".to_string()),
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "apiUrl".to_string(),
                        source: MachineBundleConfigSource::ControlPlaneUrl,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "clusterId".to_string(),
                        source: MachineBundleConfigSource::ClusterId,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "joinTokenFile".to_string(),
                        source: MachineBundleConfigSource::JoinTokenFile,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "machineIdFile".to_string(),
                        source: MachineBundleConfigSource::MachineIdFile,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "machineTokenFile".to_string(),
                        source: MachineBundleConfigSource::MachineTokenFile,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "capacityGroup".to_string(),
                        source: MachineBundleConfigSource::CapacityGroup,
                        optional: false,
                    },
                    MachineBundleConfigEntry {
                        key: "zone".to_string(),
                        source: MachineBundleConfigSource::Zone,
                        optional: true,
                    },
                    MachineBundleConfigEntry {
                        key: "bundleVersion".to_string(),
                        source: MachineBundleConfigSource::BundleVersion,
                        optional: false,
                    },
                ],
            },
            service: MachineBundleService {
                label: "dev.alien.machine".to_string(),
                executable: "bin/machine".to_string(),
                args: vec![],
                environment: BTreeMap::new(),
                restart_policy: MachineBundleRestartPolicy::OnFailure,
                restart_delay_seconds: None,
                systemd: None,
            },
            artifacts: vec![],
            registration: None,
        }
    }

    #[test]
    fn join_plan_uses_embedded_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            token: Some(" jt_secret ".to_string()),
            capacity_group: " gpu ".to_string(),
            zone: Some(" rack-1 ".to_string()),
            bundle_url: None,
            dry_run: true,
            ..test_join_args()
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
            install_script_url: None,
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
        assert_eq!(plan.control_plane_url, "https://control.example.com");
        assert_eq!(plan.cluster_id, "cluster-123");
        assert_eq!(plan.arch, MachineArch::X64);
    }

    #[test]
    fn join_request_uses_context_from_wrapped_join_token() {
        let dir = tempfile::tempdir().expect("temp dir");
        let wrapped = wrapped_join_token(
            "hj_raw_secret",
            "https://horizon.example.com",
            "cluster-from-token",
        );
        let args = JoinArgs {
            token: Some(wrapped),
            control_plane_url: None,
            cluster_id: None,
            dry_run: true,
            ..test_join_args()
        };

        let request = build_join_request(&args, None, linux_host(dir.path()))
            .expect("join request should resolve from wrapped token");

        assert_eq!(request.token, "hj_raw_secret");
        assert_eq!(
            request.plan.control_plane_url,
            "https://horizon.example.com"
        );
        assert_eq!(request.plan.cluster_id, "cluster-from-token");
    }

    #[test]
    fn explicit_context_overrides_wrapped_join_token_context() {
        let dir = tempfile::tempdir().expect("temp dir");
        let wrapped = wrapped_join_token(
            "hj_raw_secret",
            "https://horizon.example.com",
            "cluster-from-token",
        );
        let args = JoinArgs {
            token: Some(wrapped),
            control_plane_url: Some("https://override.example.com".to_string()),
            cluster_id: Some("cluster-override".to_string()),
            dry_run: true,
            ..test_join_args()
        };

        let request = build_join_request(&args, None, linux_host(dir.path()))
            .expect("join request should resolve");

        assert_eq!(request.token, "hj_raw_secret");
        assert_eq!(
            request.plan.control_plane_url,
            "https://override.example.com"
        );
        assert_eq!(request.plan.cluster_id, "cluster-override");
    }

    #[test]
    fn raw_join_token_requires_explicit_context() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            control_plane_url: None,
            cluster_id: None,
            dry_run: true,
            ..test_join_args()
        };

        let error = build_join_request(&args, None, linux_host(dir.path()))
            .expect_err("raw join token needs explicit context");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("--control-plane-url"));
    }

    #[test]
    fn malformed_wrapped_join_token_is_rejected() {
        let error = parse_wrapped_join_token("aj1_not-valid-base64!!!")
            .expect_err("wrapped token should be rejected");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn join_plan_prefers_explicit_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            bundle_url: Some("https://override.example.com/manifest.json".to_string()),
            dry_run: true,
            ..test_join_args()
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
            install_script_url: None,
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
            bundle_url: Some("https://packages.example.com/machines/manifest.json".to_string()),
            dry_run: true,
            ..test_join_args()
        };

        let plan = build_join_plan(&args, None, linux_host(dir.path())).expect("join plan");

        assert_eq!(plan.token_source, TokenSource::File);
    }

    #[test]
    fn join_request_reads_wrapped_token_file_context() {
        let dir = tempfile::tempdir().expect("temp dir");
        let wrapped = wrapped_join_token(
            "hj_file_secret",
            "https://horizon-from-file.example.com",
            "cluster-from-file",
        );
        let mut token_file = tempfile::NamedTempFile::new().expect("token file");
        token_file
            .write_all(format!(" {wrapped}\n").as_bytes())
            .expect("write token");
        let args = JoinArgs {
            token: None,
            token_file: Some(token_file.path().to_path_buf()),
            control_plane_url: None,
            cluster_id: None,
            dry_run: true,
            ..test_join_args()
        };

        let request = build_join_request(&args, None, linux_host(dir.path()))
            .expect("join request should resolve wrapped token from file");

        assert_eq!(request.token, "hj_file_secret");
        assert_eq!(request.plan.token_source, TokenSource::File);
        assert_eq!(
            request.plan.control_plane_url,
            "https://horizon-from-file.example.com"
        );
        assert_eq!(request.plan.cluster_id, "cluster-from-file");
    }

    #[test]
    fn join_plan_requires_bundle_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let args = JoinArgs {
            bundle_url: None,
            dry_run: true,
            ..test_join_args()
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
            wireguard_available: false,
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
            wireguard_available: true,
        };

        assert_eq!(preflight_host(host).expect("supported"), MachineArch::Arm64);
    }

    #[test]
    fn preflight_rejects_missing_wireguard() {
        let dir = tempfile::tempdir().expect("temp dir");
        let host = HostFacts {
            os: "linux",
            arch: "x86_64",
            systemd_runtime_dir: dir.path(),
            systemctl_available: true,
            wireguard_available: false,
        };

        let error = preflight_host(host).expect_err("wireguard should be required");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("WireGuard"));
    }

    #[test]
    fn bundle_artifact_selection_uses_linux_arch() {
        let mut manifest = test_manifest();
        manifest.service.args = vec!["--config".to_string(), "{config_path}".to_string()];
        manifest.artifacts = vec![
            MachineBundleArtifact {
                os: "linux".to_string(),
                arch: "arm64".to_string(),
                url: "linux-arm64.tar.gz".to_string(),
                sha256: "unused".to_string(),
            },
            MachineBundleArtifact {
                os: "linux".to_string(),
                arch: "x64".to_string(),
                url: "linux-x64.tar.gz".to_string(),
                sha256: "unused".to_string(),
            },
        ];

        let artifact = select_bundle_artifact(&manifest, MachineArch::X64).expect("artifact");

        assert_eq!(artifact.url, "linux-x64.tar.gz");
    }

    #[test]
    fn relative_artifact_url_resolves_against_manifest_url() {
        let url = resolve_artifact_url(
            "https://packages.example.com/machine-bundles/abc/manifest.json",
            "linux-x64.tar.gz",
        )
        .expect("url");

        assert_eq!(
            url,
            "https://packages.example.com/machine-bundles/abc/linux-x64.tar.gz"
        );
    }

    #[test]
    fn checksum_verification_rejects_mismatch() {
        let error = verify_sha256(b"bundle", "sha256:0000").expect_err("checksum mismatch");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn bundle_manifest_parses_config_sources() {
        let raw = r#"{
          "version": "2026-07-05",
          "config": {
            "path": "etc/machine-service/machine.toml",
            "joinTokenFile": "var/lib/machine-service/join-token",
            "machineIdFile": "var/lib/machine-service/machine-id",
            "machineTokenFile": "var/lib/machine-service/machine-token",
            "entries": [
              { "key": "mode", "source": { "literal": "external" } },
              { "key": "apiUrl", "source": "controlPlaneUrl" },
              { "key": "zone", "source": "zone", "optional": true }
            ]
          },
          "service": {
            "label": "dev.alien.machine",
            "executable": "bin/machine-entrypoint",
            "environment": {
              "MACHINE_CONFIG": "{config_path}"
            }
          },
          "artifacts": [
            {
              "os": "linux",
              "arch": "x64",
              "url": "machine-bundle.tar.gz",
              "sha256": "00"
            }
          ],
          "registration": {
            "machineIdFile": "var/lib/machine-service/machine-id"
          }
        }"#;

        let manifest: MachineBundleManifest =
            serde_json::from_str(raw).expect("manifest should parse");

        assert_eq!(manifest.config.entries.len(), 3);
        assert_eq!(
            manifest.service.environment.get("MACHINE_CONFIG"),
            Some(&"{config_path}".to_string())
        );
    }

    #[test]
    fn bundle_manifest_ignores_machine_image_catalog_fields() {
        let raw = r#"{
          "channel": "prod",
          "machineImageVersion": "1.1.5+abc.42",
          "horizondVersion": "1.1.5",
          "gitSha": "abc",
          "createdAt": "2026-07-12T00:00:00Z",
          "baseImage": { "name": "flatcar", "version": "stable-current" },
          "horizondArtifacts": {},
          "aws": { "amis": {} },
          "version": "1.1.5+abc.42",
          "config": {
            "path": "etc/horizond/horizond.toml",
            "joinTokenFile": "var/lib/horizond/join-token",
            "machineIdFile": "var/lib/horizond/machine-id",
            "machineTokenFile": "var/lib/horizond/machine-token",
            "entries": []
          },
          "service": {
            "label": "dev.alien.machine",
            "executable": "bin/machine-entrypoint"
          },
          "artifacts": [{
            "os": "linux",
            "arch": "x64",
            "url": "https://releases.example.com/machine-bundle.tar.gz",
            "sha256": "00"
          }]
        }"#;

        let manifest: MachineBundleManifest =
            serde_json::from_str(raw).expect("combined release manifest should parse");

        assert_eq!(manifest.version, "1.1.5+abc.42");
    }

    #[test]
    fn machine_config_writes_secret_files_under_install_root() {
        let root = tempfile::tempdir().expect("install root");
        let args = JoinArgs {
            zone: Some("rack-1".to_string()),
            install_root: root.path().to_path_buf(),
            ..test_join_args()
        };
        let request = build_join_request(&args, None, linux_host(root.path())).expect("request");
        let manifest = test_manifest();
        let paths = install_paths(root.path());

        let config_path = write_machine_config(&paths, &request, &manifest).expect("write config");

        let config = std::fs::read_to_string(config_path).expect("config");
        let parsed: toml::Table = toml::from_str(&config).expect("config should be valid TOML");
        assert_eq!(
            parsed.get("capacityGroup").and_then(toml::Value::as_str),
            Some("general")
        );
        assert_eq!(
            parsed.get("apiUrl").and_then(toml::Value::as_str),
            Some("https://control.example.com")
        );
        assert!(config.contains("capacityGroup = \"general\""));
        assert!(config.contains("zone = \"rack-1\""));
        assert!(config.contains("apiUrl = \"https://control.example.com\""));
        assert!(config.contains("clusterId = \"cluster-123\""));
        assert!(config.contains("joinTokenFile = "));
        assert_eq!(
            std::fs::read_to_string(root.path().join("var/lib/machine-service/join-token"))
                .expect("token"),
            "jt_secret"
        );
    }

    #[test]
    fn bundle_extraction_rejects_paths_outside_install_root() {
        let root = tempfile::tempdir().expect("install root");
        let archive_path = root.path().join("bundle.tar.gz");
        write_raw_test_archive(&archive_path, "../escape", b"nope");

        let error = extract_tar_gz(&archive_path, &root.path().join("extract"))
            .expect_err("archive traversal should fail");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(!root.path().join("escape").exists());
    }

    #[test]
    fn bundle_extraction_can_repair_after_partial_install() {
        let root = tempfile::tempdir().expect("install root");
        let archive_path = root.path().join("bundle.tar.gz");
        let destination = root.path().join("bundle");
        let executable = destination.join("bin/machine");
        std::fs::create_dir_all(executable.parent().expect("parent")).expect("partial dir");
        std::fs::write(&executable, b"partial").expect("partial executable");
        write_test_archive(&archive_path, &[("bin/machine", b"complete".as_slice())]);

        std::fs::remove_dir_all(&destination).expect("replace partial bundle");
        std::fs::create_dir_all(&destination).expect("recreate bundle dir");
        extract_tar_gz(&archive_path, &destination).expect("extract repaired bundle");

        assert_eq!(std::fs::read(&executable).expect("executable"), b"complete");
    }

    #[test]
    fn machine_config_rejoin_overwrites_stale_token_and_zone() {
        let root = tempfile::tempdir().expect("install root");
        let paths = install_paths(root.path());
        std::fs::create_dir_all(root.path().join("var/lib/machine-service")).expect("token dir");
        std::fs::create_dir_all(root.path().join("etc/machine-service")).expect("config dir");
        std::fs::write(
            root.path().join("var/lib/machine-service/join-token"),
            "stale",
        )
        .expect("stale token");
        std::fs::write(
            root.path().join("etc/machine-service/machine.toml"),
            "capacityGroup = \"old\"\nzone = \"old-zone\"\n",
        )
        .expect("stale config");
        let manifest = test_manifest();
        let args = JoinArgs {
            token: Some("jt_new".to_string()),
            capacity_group: "gpu".to_string(),
            zone: Some("rack-2".to_string()),
            install_root: root.path().to_path_buf(),
            ..test_join_args()
        };
        let request = build_join_request(&args, None, linux_host(root.path())).expect("request");

        let config_path =
            write_machine_config(&paths, &request, &manifest).expect("rewrite machine config");

        assert_eq!(
            std::fs::read_to_string(root.path().join("var/lib/machine-service/join-token"))
                .expect("token"),
            "jt_new"
        );
        let config = std::fs::read_to_string(config_path).expect("config");
        assert!(config.contains("capacityGroup = \"gpu\""));
        assert!(config.contains("zone = \"rack-2\""));
        assert!(!config.contains("old-zone"));
    }

    #[test]
    fn ip_forwarding_config_is_persisted_under_install_root() {
        let root = tempfile::tempdir().expect("install root");
        let config_path = root.path().join("etc/sysctl.d/99-alien-machine.conf");
        std::fs::create_dir_all(config_path.parent().expect("parent")).expect("sysctl dir");
        std::fs::write(&config_path, "net.ipv4.ip_forward = 0\n").expect("stale sysctl");

        configure_ip_forwarding(root.path()).expect("configure forwarding");

        assert_eq!(
            std::fs::read_to_string(config_path).expect("sysctl config"),
            IP_FORWARDING_CONFIG
        );
    }

    #[test]
    fn install_state_rejoin_overwrites_stale_state() {
        let root = tempfile::tempdir().expect("install root");
        let paths = install_paths(root.path());
        let first = MachineInstallState {
            bundle_version: "old".to_string(),
            bundle_url: Some("https://old.example.com/manifest.json".to_string()),
            service_label: "dev.alien.old".to_string(),
            executable_path: PathBuf::from("/old/bin"),
            config_path: PathBuf::from("/old/config.toml"),
            join_token_path: PathBuf::from("/old/join-token"),
            machine_id_path: Some(PathBuf::from("/old/machine-id")),
            machine_token_path: Some(PathBuf::from("/old/machine-token")),
            control_plane_url: Some("https://old-control.example.com".to_string()),
            cluster_id: Some("old-cluster".to_string()),
            machine_id: None,
        };
        let second = MachineInstallState {
            bundle_version: "new".to_string(),
            bundle_url: Some("https://packages.example.com/manifest.json".to_string()),
            service_label: "dev.alien.new".to_string(),
            executable_path: PathBuf::from("/new/bin"),
            config_path: PathBuf::from("/new/config.toml"),
            join_token_path: PathBuf::from("/new/join-token"),
            machine_id_path: Some(PathBuf::from("/new/machine-id")),
            machine_token_path: Some(PathBuf::from("/new/machine-token")),
            control_plane_url: Some("https://control.example.com".to_string()),
            cluster_id: Some("cluster-123".to_string()),
            machine_id: Some("machine-new".to_string()),
        };

        write_install_state(&paths, &first).expect("write first state");
        write_install_state(&paths, &second).expect("rewrite state");
        let stored = read_install_state(&install_state_path(&paths)).expect("read state");

        assert_eq!(stored.bundle_version, "new");
        assert_eq!(
            stored.bundle_url.as_deref(),
            Some("https://packages.example.com/manifest.json")
        );
        assert_eq!(stored.service_label, "dev.alien.new");
        assert_eq!(stored.executable_path, PathBuf::from("/new/bin"));
        assert_eq!(stored.config_path, PathBuf::from("/new/config.toml"));
        assert_eq!(stored.join_token_path, PathBuf::from("/new/join-token"));
        assert_eq!(
            stored.machine_token_path.as_deref(),
            Some(Path::new("/new/machine-token"))
        );
        assert_eq!(
            stored.control_plane_url.as_deref(),
            Some("https://control.example.com")
        );
        assert_eq!(stored.cluster_id.as_deref(), Some("cluster-123"));
        assert_eq!(stored.machine_id.as_deref(), Some("machine-new"));
    }

    #[test]
    fn install_state_match_requires_same_bundle_and_cluster() {
        let root = tempfile::tempdir().expect("install root");
        let manifest = test_manifest();
        let request = build_join_request(
            &JoinArgs {
                install_root: root.path().to_path_buf(),
                ..test_join_args()
            },
            None,
            linux_host(root.path()),
        )
        .expect("request");
        let state = MachineInstallState {
            bundle_version: manifest.version.clone(),
            bundle_url: Some(request.plan.bundle_url.clone()),
            service_label: manifest.service.label.clone(),
            executable_path: PathBuf::from("/bundle/bin/machine"),
            config_path: PathBuf::from("/etc/machine-service/machine.toml"),
            join_token_path: PathBuf::from("/var/lib/machine-service/join-token"),
            machine_id_path: Some(PathBuf::from("/var/lib/machine-service/machine-id")),
            machine_token_path: Some(PathBuf::from("/var/lib/machine-service/machine-token")),
            control_plane_url: Some(request.plan.control_plane_url.clone()),
            cluster_id: Some(request.plan.cluster_id.clone()),
            machine_id: Some("machine-123".to_string()),
        };

        assert!(install_state_matches_request(&state, &request, &manifest));

        let mut wrong_cluster = state.clone();
        wrong_cluster.cluster_id = Some("other-cluster".to_string());
        assert!(!install_state_matches_request(
            &wrong_cluster,
            &request,
            &manifest
        ));

        let mut old_state_without_bundle_url = state;
        old_state_without_bundle_url.bundle_url = None;
        assert!(!install_state_matches_request(
            &old_state_without_bundle_url,
            &request,
            &manifest
        ));
    }

    #[test]
    fn control_plane_endpoint_preserves_base_path() {
        let endpoint =
            control_plane_endpoint("https://control.example.com/api", "drain").expect("endpoint");
        assert_eq!(endpoint.as_str(), "https://control.example.com/api/drain");
        let endpoint =
            control_plane_endpoint("https://control.example.com/api", "leave").expect("endpoint");
        assert_eq!(endpoint.as_str(), "https://control.example.com/api/leave");
    }

    #[tokio::test]
    async fn registration_wait_reads_machine_id_file() {
        let root = tempfile::tempdir().expect("install root");
        let registration = MachineBundleRegistration {
            machine_id_file: "var/lib/alien/machine/machine-id".to_string(),
            timeout_seconds: Some(1),
        };
        let machine_id_path = registration_file_path(root.path(), &registration).expect("path");
        std::fs::create_dir_all(machine_id_path.parent().expect("parent")).expect("state dir");
        std::fs::write(&machine_id_path, " machine-123\n").expect("machine id");

        let machine_id = wait_for_registration(root.path(), &registration)
            .await
            .expect("registration");

        assert_eq!(machine_id, "machine-123");
    }

    #[tokio::test]
    async fn registration_wait_observes_delayed_machine_id_file() {
        let root = tempfile::tempdir().expect("install root");
        let machine_id_path = root.path().join("var/lib/alien/machine/machine-id");
        let writer_path = machine_id_path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            std::fs::create_dir_all(writer_path.parent().expect("parent")).expect("state dir");
            std::fs::write(writer_path, "machine-delayed").expect("machine id");
        });

        let machine_id = wait_for_machine_id_file(
            &machine_id_path,
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .await
        .expect("registration");

        assert_eq!(machine_id, "machine-delayed");
    }

    #[test]
    fn registration_file_path_must_stay_inside_install_root() {
        let root = tempfile::tempdir().expect("install root");
        let registration = MachineBundleRegistration {
            machine_id_file: "../machine-id".to_string(),
            timeout_seconds: Some(1),
        };

        let error = registration_file_path(root.path(), &registration)
            .expect_err("parent directory traversal should fail");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn service_executable_path_must_stay_inside_bundle() {
        let root = tempfile::tempdir().expect("install root");

        let parent_error = safe_join(root.path(), "../bin/machine")
            .expect_err("parent directory traversal should fail");
        assert_eq!(parent_error.code, "VALIDATION_ERROR");

        let absolute_error =
            safe_join(root.path(), "/bin/machine").expect_err("absolute executable should fail");
        assert_eq!(absolute_error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn notify_machine_service_renders_handoff_and_restart_contract() {
        let service = MachineBundleService {
            label: "dev.alien.machine".to_string(),
            executable: "bin/machine-entrypoint".to_string(),
            args: vec![],
            environment: BTreeMap::from([(
                "HORIZON_CONFIG".to_string(),
                "{config_path}".to_string(),
            )]),
            restart_policy: MachineBundleRestartPolicy::Always,
            restart_delay_seconds: Some(5),
            systemd: None,
        };
        let systemd = MachineBundleSystemdService {
            service_type: MachineBundleSystemdServiceType::Notify,
            notify_access: Some(MachineBundleSystemdNotifyAccess::All),
            pid_file: Some("/run/horizond.pid".to_string()),
            timeout_start_seconds: Some(600),
        };

        let unit = render_systemd_machine_service(
            &service,
            &systemd,
            Path::new("/opt/alien/machine-bundle/bin/machine-entrypoint"),
            Path::new("/etc/horizond/horizond.toml"),
            Some(Path::new("/opt/alien/machine-bundle/bin")),
        )
        .expect("notify service should render");

        assert!(unit.contains("Type=notify\n"));
        assert!(unit.contains("NotifyAccess=all\n"));
        assert!(unit.contains("PIDFile=/run/horizond.pid\n"));
        assert!(unit.contains("TimeoutStartSec=600\n"));
        assert!(unit.contains("Restart=always\nRestartSec=5\n"));
        assert!(unit.contains("Environment=\"HORIZON_CONFIG=/etc/horizond/horizond.toml\"\n"));
    }

    #[test]
    fn notify_machine_service_rejects_main_only_notifications() {
        let mut manifest = test_manifest();
        manifest.service.restart_policy = MachineBundleRestartPolicy::Always;
        let systemd = MachineBundleSystemdService {
            service_type: MachineBundleSystemdServiceType::Notify,
            notify_access: Some(MachineBundleSystemdNotifyAccess::Main),
            pid_file: Some("/run/horizond.pid".to_string()),
            timeout_start_seconds: Some(600),
        };

        let error = render_systemd_machine_service(
            &manifest.service,
            &systemd,
            Path::new("/opt/alien/machine"),
            Path::new("/etc/horizond.toml"),
            None,
        )
        .expect_err("handoff children require notify access all");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    fn write_test_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("archive file");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);

        for (name, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, *name, *contents)
                .expect("archive entry");
        }

        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
    }

    fn write_raw_test_archive(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = File::create(path).expect("archive file");
        let mut encoder = GzEncoder::new(file, Compression::default());
        let mut header = [0_u8; 512];
        let name = entry_name.as_bytes();
        header[..name.len()].copy_from_slice(name);
        write_tar_octal(&mut header[100..108], 0o644);
        write_tar_octal(&mut header[108..116], 0);
        write_tar_octal(&mut header[116..124], 0);
        write_tar_octal(&mut header[124..136], contents.len() as u64);
        write_tar_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        write_tar_checksum(&mut header[148..156], checksum);

        encoder.write_all(&header).expect("header");
        encoder.write_all(contents).expect("contents");
        let padding = (512 - (contents.len() % 512)) % 512;
        if padding > 0 {
            encoder.write_all(&vec![0_u8; padding]).expect("padding");
        }
        encoder.write_all(&[0_u8; 1024]).expect("end of archive");
        encoder.finish().expect("finish gzip");
    }

    fn write_tar_octal(field: &mut [u8], value: u64) {
        field.fill(0);
        let encoded = format!("{:0width$o}", value, width = field.len() - 1);
        field[..encoded.len()].copy_from_slice(encoded.as_bytes());
    }

    fn write_tar_checksum(field: &mut [u8], value: u32) {
        field.fill(0);
        let encoded = format!("{value:06o}");
        field[..encoded.len()].copy_from_slice(encoded.as_bytes());
        field[6] = 0;
        field[7] = b' ';
    }
}
