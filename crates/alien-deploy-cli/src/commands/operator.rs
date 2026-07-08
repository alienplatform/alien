//! Operator service management commands.
//!
//! Install, start, stop, and uninstall the alien-operator as an OS service
//! (systemd on Linux, launchd on macOS, Windows Service on Windows).

use crate::error::{ErrorData, Result};
use crate::output;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Args, Subcommand};
use service_manager::*;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const SERVICE_LABEL: &str = "dev.alien.operator";

#[derive(Args)]
pub struct OperatorArgs {
    #[command(subcommand)]
    pub command: OperatorCommand,
}

#[derive(Subcommand)]
pub enum OperatorCommand {
    /// Install alien-operator as an OS service
    Install(InstallArgs),
    /// Start the alien-operator service
    Start,
    /// Stop the alien-operator service
    Stop,
    /// Show the operator service status
    Status,
    /// Uninstall the alien-operator service
    Uninstall,
}

#[derive(Args)]
pub struct InstallArgs {
    /// Path to the alien-operator binary. If omitted, searches PATH.
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// Manager URL for the operator to sync with
    #[arg(long)]
    pub sync_url: String,

    /// Sync authentication token
    #[arg(long)]
    pub sync_token: String,

    /// Deployment ID this service should manage.
    #[arg(long)]
    pub deployment_id: Option<String>,

    /// Human-readable deployment name this service should manage.
    #[arg(long)]
    pub operator_name: Option<String>,

    /// Target platform (aws, gcp, azure)
    #[arg(long, default_value = "local")]
    pub platform: String,

    /// Data directory for operator state
    #[arg(long)]
    pub data_dir: Option<String>,

    /// Encryption key (64-char hex). Generated if not provided.
    #[arg(long)]
    pub encryption_key: Option<String>,

    /// Generic public endpoint URLs for exposed resources.
    #[arg(skip)]
    pub public_endpoints: Option<alien_core::PublicEndpointUrls>,

    /// Enable Local runtime debug commands and shells.
    #[arg(long)]
    pub enable_local_debug: bool,

    /// Override the shell command used by Local runtime debug shells.
    #[arg(long)]
    pub local_debug_shell_command: Option<String>,
}

pub async fn operator_command(args: OperatorArgs) -> Result<()> {
    match args.command {
        OperatorCommand::Install(install_args) => install(install_args),
        OperatorCommand::Start => start(),
        OperatorCommand::Stop => stop(),
        OperatorCommand::Status => status(),
        OperatorCommand::Uninstall => uninstall(),
    }
}

fn get_manager() -> Result<Box<dyn ServiceManager>> {
    <dyn ServiceManager>::native()
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "No supported service manager found".to_string(),
        })
}

fn label() -> ServiceLabel {
    SERVICE_LABEL.parse().expect("valid service label")
}

/// Public entry point so `up.rs` can delegate service installation.
pub fn install_service(args: InstallArgs) -> Result<()> {
    install(args)
}

/// Stop the service when it is installed and running.
pub fn stop_service_if_running() -> Result<()> {
    let manager = get_manager()?;
    match manager
        .status(ServiceStatusCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to check service status".to_string(),
        })? {
        ServiceStatus::Running => stop(),
        ServiceStatus::Stopped(_) => {
            output::info("alien-operator service is already stopped");
            Ok(())
        }
        ServiceStatus::NotInstalled => {
            output::info("alien-operator service is not installed");
            Ok(())
        }
    }
}

/// Remove the service when it is installed.
pub fn uninstall_service_if_installed() -> Result<()> {
    let manager = get_manager()?;
    match manager
        .status(ServiceStatusCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to check service status".to_string(),
        })? {
        ServiceStatus::Running | ServiceStatus::Stopped(_) => uninstall(),
        ServiceStatus::NotInstalled => {
            output::info("alien-operator service is not installed");
            Ok(())
        }
    }
}

/// Generate an encryption key (public for reuse from up.rs).
pub fn generate_encryption_key_public() -> String {
    generate_encryption_key()
}

/// Default data directory for the installed operator service.
pub fn default_service_data_dir() -> String {
    if cfg!(windows) {
        r"C:\ProgramData\alien-operator".to_string()
    } else {
        "/var/lib/alien-operator".to_string()
    }
}

fn install(args: InstallArgs) -> Result<()> {
    output::header("Installing alien-operator service");

    let binary_path = match args.binary {
        Some(p) => p,
        None => which_operator_binary()?,
    };

    if !binary_path.exists() {
        return Err(AlienError::new(ErrorData::OperatorServiceError {
            message: format!("Binary not found: {}", binary_path.display()),
        }));
    }

    let data_dir = args.data_dir.unwrap_or_else(default_service_data_dir);

    // Create data directory before writing secret files into it.
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        output::warn(&format!("Could not create data directory: {}", e));
    }

    // The operator rejects `--sync-token`/`--encryption-key` (argv leak via
    // `ps` / `/proc`). Persist secrets to 0o600 files in the service's
    // data directory and pass `--*-file` paths in the service args. The
    // service runs as the user that installed it (typically root on Linux,
    // the current user on macOS launchd), which owns these files.
    let sync_token_path = std::path::Path::new(&data_dir).join("sync-token");
    let encryption_key_path = std::path::Path::new(&data_dir).join("encryption-key");
    let encryption_key = resolve_encryption_key(args.encryption_key, &encryption_key_path)?;

    alien_core::file_utils::write_secret_file(&sync_token_path, args.sync_token.as_bytes())
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: format!(
                "Failed to write sync token to {}",
                sync_token_path.display()
            ),
        })?;
    alien_core::file_utils::write_secret_file(&encryption_key_path, encryption_key.as_bytes())
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: format!(
                "Failed to write encryption key to {}",
                encryption_key_path.display()
            ),
        })?;

    let public_endpoints_path = std::path::Path::new(&data_dir).join("public-endpoints.json");
    if let Some(public_endpoints) = &args.public_endpoints {
        let public_endpoints_json = serde_json::to_vec(public_endpoints)
            .into_alien_error()
            .context(ErrorData::OperatorServiceError {
                message: "Failed to serialize public endpoints".to_string(),
            })?;
        std::fs::write(&public_endpoints_path, public_endpoints_json)
            .into_alien_error()
            .context(ErrorData::OperatorServiceError {
                message: format!(
                    "Failed to write public endpoints to {}",
                    public_endpoints_path.display()
                ),
            })?;
    } else if let Err(e) = std::fs::remove_file(&public_endpoints_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            output::warn(&format!(
                "Could not remove stale public endpoints file {}: {}",
                public_endpoints_path.display(),
                e
            ));
        }
    }

    let mut service_args = vec![
        OsString::from("--service"),
        OsString::from("--platform"),
        OsString::from(&args.platform),
        OsString::from("--sync-url"),
        OsString::from(&args.sync_url),
        OsString::from("--sync-token-file"),
        OsString::from(&sync_token_path),
        OsString::from("--data-dir"),
        OsString::from(&data_dir),
        OsString::from("--encryption-key-file"),
        OsString::from(&encryption_key_path),
    ];
    if args.public_endpoints.is_some() {
        service_args.push(OsString::from("--public-endpoints-file"));
        service_args.push(OsString::from(&public_endpoints_path));
    }
    if args.enable_local_debug {
        service_args.push(OsString::from("--enable-local-debug"));
    }
    if let Some(shell_command) = &args.local_debug_shell_command {
        service_args.push(OsString::from("--local-debug-shell-command"));
        service_args.push(OsString::from(shell_command));
    }
    let service_args = if let Some(deployment_id) = &args.deployment_id {
        let mut service_args = service_args;
        service_args.push(OsString::from("--deployment-id"));
        service_args.push(OsString::from(deployment_id));
        service_args
    } else {
        service_args
    };
    let service_args = if let Some(operator_name) = &args.operator_name {
        let mut service_args = service_args;
        service_args.push(OsString::from("--operator-name"));
        service_args.push(OsString::from(operator_name));
        service_args
    } else {
        service_args
    };

    let manager = get_manager()?;

    output::step(1, 3, &format!("Registering service ({})", SERVICE_LABEL));

    manager
        .install(ServiceInstallCtx {
            label: label(),
            program: binary_path.clone(),
            args: service_args,
            contents: None,
            username: None,
            working_directory: None,
            environment: None,
            autostart: true,
            restart_policy: RestartPolicy::OnFailure {
                delay_secs: Some(5),
            },
        })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to install service".to_string(),
        })?;

    output::step(2, 3, "Stopping existing service");

    // `install` updates the service definition and secret files, but an already
    // running service keeps its old argv and open state until it is restarted.
    // Ignore stop errors: first install has nothing to stop.
    let _ = manager.stop(ServiceStopCtx { label: label() });

    output::step(3, 3, "Starting service");

    manager
        .start(ServiceStartCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to start service".to_string(),
        })?;

    output::success("alien-operator service installed and started");
    output::info(&format!("  Binary:     {}", binary_path.display()));
    output::info(&format!("  Data dir:   {}", data_dir));
    output::info(&format!("  Platform:   {}", args.platform));
    output::info(&format!("  Sync URL:   {}", args.sync_url));

    Ok(())
}

fn resolve_encryption_key(
    explicit_key: Option<String>,
    encryption_key_path: &Path,
) -> Result<String> {
    if let Some(key) = explicit_key {
        return Ok(key);
    }

    match std::fs::read_to_string(encryption_key_path) {
        Ok(key) => {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Ok(key);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            output::warn(&format!(
                "Could not read existing encryption key from {}: {}",
                encryption_key_path.display(),
                e
            ));
        }
    }

    Ok(generate_encryption_key())
}

fn start() -> Result<()> {
    let manager = get_manager()?;
    manager
        .start(ServiceStartCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to start service".to_string(),
        })?;
    output::success("alien-operator service started");
    Ok(())
}

fn stop() -> Result<()> {
    let manager = get_manager()?;
    manager
        .stop(ServiceStopCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to stop service".to_string(),
        })?;
    output::success("alien-operator service stopped");
    Ok(())
}

fn status() -> Result<()> {
    output::header("alien-operator service status");

    // Try to query by starting — if already running this errors.
    // service-manager doesn't have a status() method, so we check the data dir.
    let data_dir = if cfg!(windows) {
        PathBuf::from(r"C:\ProgramData\operator")
    } else {
        PathBuf::from("/var/lib/operator")
    };

    let lock_path = data_dir.join("operator.lock");

    if lock_path.exists() {
        // Try to acquire the lock — if we can't, operator is running
        match try_check_running(&lock_path) {
            true => output::info("  Status: running"),
            false => output::info("  Status: stopped (lock file exists but not held)"),
        }
    } else {
        output::info("  Status: not installed or never started");
    }

    // Show panic log if present
    let panic_log = data_dir.join("panic.log");
    if panic_log.exists() {
        if let Ok(content) = std::fs::read_to_string(&panic_log) {
            let lines: Vec<&str> = content.lines().collect();
            if let Some(last) = lines.last() {
                output::warn(&format!("  Last panic: {}", last));
            }
        }
    }

    Ok(())
}

fn uninstall() -> Result<()> {
    let manager = get_manager()?;

    // Stop first (ignore errors if not running)
    let _ = manager.stop(ServiceStopCtx { label: label() });

    manager
        .uninstall(ServiceUninstallCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::OperatorServiceError {
            message: "Failed to uninstall service".to_string(),
        })?;

    output::success("alien-operator service uninstalled");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn which_operator_binary() -> Result<PathBuf> {
    // 1. Check ALIEN_OPERATOR_BINARY env var (useful for local development)
    if let Ok(env_path) = std::env::var("ALIEN_OPERATOR_BINARY") {
        let p = PathBuf::from(&env_path);
        if p.exists() {
            return Ok(p);
        }
        return Err(AlienError::new(ErrorData::OperatorServiceError {
            message: format!(
                "ALIEN_OPERATOR_BINARY set to '{}' but file not found",
                env_path
            ),
        }));
    }

    // 2. Look for alien-operator in PATH
    let candidates = if cfg!(windows) {
        vec!["alien-operator.exe"]
    } else {
        vec!["alien-operator"]
    };

    for name in &candidates {
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }
    }

    // 3. Check local build artifacts (for development from repo root)
    for profile in &["release", "debug"] {
        let local = PathBuf::from(format!("target/{}/alien-operator", profile));
        if local.exists() {
            return Ok(std::fs::canonicalize(&local).unwrap_or(local));
        }
    }

    // 4. Check ~/.alien/bin/ (where auto-download places the binary)
    if let Some(home) = dirs::home_dir() {
        let alien_bin = home.join(".alien").join("bin").join("alien-operator");
        if alien_bin.exists() {
            return Ok(alien_bin);
        }
    }

    // 5. Check common install locations
    let common_paths = if cfg!(windows) {
        vec![r"C:\Program Files\alien\alien-operator.exe"]
    } else {
        vec!["/usr/local/bin/alien-operator", "/usr/bin/alien-operator"]
    };

    for path in &common_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(AlienError::new(ErrorData::OperatorServiceError {
        message:
            "alien-operator binary not found. Set ALIEN_OPERATOR_BINARY=/path/to/alien-operator, \
                  build with 'cargo build -p alien-operator', or install it first."
                .to_string(),
    }))
}

fn generate_encryption_key() -> String {
    use std::fmt::Write;
    let mut key = String::with_capacity(64);
    for _ in 0..32 {
        let byte: u8 = rand_byte();
        write!(&mut key, "{:02x}", byte).unwrap();
    }
    key
}

fn rand_byte() -> u8 {
    // Use getrandom for cryptographic randomness
    let mut buf = [0u8; 1];
    getrandom::getrandom(&mut buf).expect("failed to generate random bytes");
    buf[0]
}

#[cfg(unix)]
fn try_check_running(lock_path: &PathBuf) -> bool {
    use std::os::unix::io::AsRawFd;

    let Ok(file) = std::fs::File::open(lock_path) else {
        return false;
    };
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        // We got the lock — nobody else holds it, so operator is NOT running
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        false
    } else {
        // Couldn't get lock — operator IS running
        true
    }
}

#[cfg(windows)]
fn try_check_running(lock_path: &PathBuf) -> bool {
    use fs2::FileExt;

    let Ok(file) = std::fs::File::open(lock_path) else {
        return false;
    };
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_encryption_key_length() {
        let key = generate_encryption_key();
        assert_eq!(key.len(), 64, "encryption key should be 64 hex chars");
    }

    #[test]
    fn test_generate_encryption_key_is_hex() {
        let key = generate_encryption_key();
        assert!(
            key.chars().all(|c| c.is_ascii_hexdigit()),
            "encryption key should contain only hex chars"
        );
    }

    #[test]
    fn test_generate_encryption_key_unique() {
        let key1 = generate_encryption_key();
        let key2 = generate_encryption_key();
        assert_ne!(key1, key2, "two generated keys should differ");
    }

    #[test]
    fn test_resolve_encryption_key_reuses_existing_file() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let key_path = dir.path().join("encryption-key");
        let existing_key = "a".repeat(64);
        std::fs::write(&key_path, format!("{existing_key}\n")).expect("key file should be written");

        let resolved =
            resolve_encryption_key(None, &key_path).expect("key should resolve from file");

        assert_eq!(resolved, existing_key);
    }

    #[test]
    fn test_resolve_encryption_key_prefers_explicit_key() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let key_path = dir.path().join("encryption-key");
        std::fs::write(&key_path, "a".repeat(64)).expect("key file should be written");
        let explicit_key = "b".repeat(64);

        let resolved = resolve_encryption_key(Some(explicit_key.clone()), &key_path)
            .expect("explicit key should resolve");

        assert_eq!(resolved, explicit_key);
    }

    #[test]
    fn test_service_label_parse() {
        let lbl = label();
        assert_eq!(lbl.to_string(), SERVICE_LABEL);
    }
}
