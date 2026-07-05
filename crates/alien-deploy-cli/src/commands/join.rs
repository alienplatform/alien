//! Join and leave commands for customer-managed Linux machines.

use crate::commands::up::read_token_file;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Args;
use flate2::read::GzDecoder;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use service_manager::*;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::File;
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
    arch: MachineArch,
}

#[derive(Debug, Clone)]
struct JoinRequest {
    token: String,
    plan: JoinPlan,
    install_root: PathBuf,
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
    service: MachineBundleService,
    artifacts: Vec<MachineBundleArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleService {
    label: String,
    executable: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineBundleArtifact {
    os: String,
    arch: String,
    url: String,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineInstallState {
    bundle_version: String,
    service_label: String,
    executable_path: PathBuf,
    config_path: PathBuf,
}

#[derive(Debug)]
struct InstallPaths {
    bundle_dir: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
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

    uninstall_joined_machine(&args.install_root, args.purge)
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
    let bundle_url = resolve_bundle_url(args, embedded_config)?;
    let arch = preflight_host(host)?;

    Ok(JoinRequest {
        token,
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

async fn install_join(request: JoinRequest) -> Result<()> {
    let paths = install_paths(&request.install_root);

    output::step(1, 5, "Resolving machine bundle");
    let manifest = download_manifest(&request.plan.bundle_url).await?;
    let artifact = select_bundle_artifact(&manifest, request.plan.arch)?;
    let artifact_url = resolve_artifact_url(&request.plan.bundle_url, &artifact.url)?;

    output::step(2, 5, "Downloading machine bundle");
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

    output::step(3, 5, "Installing bundle files");
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

    output::step(4, 5, "Writing machine configuration");
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
    let state = MachineInstallState {
        bundle_version: manifest.version.clone(),
        service_label: manifest.service.label.clone(),
        executable_path: executable_path.clone(),
        config_path: config_path.clone(),
    };

    output::step(5, 5, "Installing machine service");
    install_machine_service(&manifest.service, &executable_path, &config_path)?;
    write_install_state(&paths, &state)?;

    output::success("Machine service installed and started");
    output::info(&format!("  Service: {}", state.service_label));
    output::info(&format!("  Bundle:  {}", state.bundle_version));
    Ok(())
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
    std::fs::create_dir_all(&paths.config_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: paths.config_dir.display().to_string(),
            reason: "Failed to create machine config directory".to_string(),
        })?;
    std::fs::create_dir_all(&paths.state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: paths.state_dir.display().to_string(),
            reason: "Failed to create machine state directory".to_string(),
        })?;

    let token_path = paths.config_dir.join("join-token");
    write_secret_file(&token_path, &request.token)?;

    let config_path = paths.config_dir.join("machine.toml");
    let mut config = toml::Table::new();
    config.insert(
        "join_token_file".to_string(),
        token_path.display().to_string().into(),
    );
    config.insert(
        "capacity_group".to_string(),
        request.plan.capacity_group.clone().into(),
    );
    if let Some(zone) = &request.plan.zone {
        config.insert("zone".to_string(), zone.clone().into());
    }
    config.insert(
        "bundle_version".to_string(),
        manifest.version.clone().into(),
    );
    write_secret_file(&config_path, &toml::Value::Table(config).to_string())?;
    Ok(config_path)
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

    manager
        .install(ServiceInstallCtx {
            label: label.clone(),
            program: executable_path.to_path_buf(),
            args: rendered_args,
            contents: None,
            username: None,
            working_directory: executable_path.parent().map(Path::to_path_buf),
            environment: None,
            autostart: true,
            restart_policy: RestartPolicy::OnFailure {
                delay_secs: Some(5),
            },
        })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to install machine service".to_string(),
        })?;

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

fn uninstall_joined_machine(install_root: &Path, purge: bool) -> Result<()> {
    let paths = install_paths(install_root);
    let state_path = install_state_path(&paths);
    let state = read_install_state(&state_path)?;
    let manager = native_service_manager()?;
    let label = parse_service_label(&state.service_label)?;

    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    manager
        .uninstall(ServiceUninstallCtx { label })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to uninstall machine service".to_string(),
        })?;

    remove_dir_if_exists(&paths.config_dir)?;
    let _ = std::fs::remove_file(&state_path);
    if purge {
        remove_dir_if_exists(&paths.state_dir)?;
        remove_dir_if_exists(&paths.bundle_dir)?;
    }

    output::success("Machine service uninstalled");
    Ok(())
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
        config_dir: rooted_path(root, "etc/alien/machine"),
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
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle.service.executable".to_string(),
            message: "path must be relative and stay inside the bundle".to_string(),
        }));
    }
    Ok(base.join(relative_path))
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
            install_root: PathBuf::from("/"),
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
            install_root: PathBuf::from("/"),
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
            install_root: PathBuf::from("/"),
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
            install_root: PathBuf::from("/"),
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

    #[test]
    fn bundle_artifact_selection_uses_linux_arch() {
        let manifest = MachineBundleManifest {
            version: "2026-07-05".to_string(),
            service: MachineBundleService {
                label: "dev.alien.machine".to_string(),
                executable: "bin/machine".to_string(),
                args: vec!["--config".to_string(), "{config_path}".to_string()],
            },
            artifacts: vec![
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
            ],
        };

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
    fn machine_config_writes_secret_files_under_install_root() {
        let root = tempfile::tempdir().expect("install root");
        let args = JoinArgs {
            token: Some("jt_secret".to_string()),
            token_file: None,
            capacity_group: "general".to_string(),
            zone: Some("rack-1".to_string()),
            bundle_url: Some("https://packages.example.com/manifest.json".to_string()),
            dry_run: false,
            install_root: root.path().to_path_buf(),
        };
        let request = build_join_request(&args, None, linux_host(root.path())).expect("request");
        let manifest = MachineBundleManifest {
            version: "2026-07-05".to_string(),
            service: MachineBundleService {
                label: "dev.alien.machine".to_string(),
                executable: "bin/machine".to_string(),
                args: vec![],
            },
            artifacts: vec![],
        };
        let paths = install_paths(root.path());

        let config_path = write_machine_config(&paths, &request, &manifest).expect("write config");

        let config = std::fs::read_to_string(config_path).expect("config");
        assert!(config.contains("capacity_group = \"general\""));
        assert!(config.contains("zone = \"rack-1\""));
        assert_eq!(
            std::fs::read_to_string(root.path().join("etc/alien/machine/join-token"))
                .expect("token"),
            "jt_secret"
        );
    }
}
