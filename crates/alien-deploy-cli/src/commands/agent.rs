//! Agent service management commands.
//!
//! Install, start, stop, and uninstall the alien-agent as an OS service
//! (systemd on Linux, launchd on macOS, Windows Service on Windows).

use crate::error::{ErrorData, Result};
use crate::output;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Args, Subcommand};
use service_manager::*;
use std::ffi::OsString;
use std::path::PathBuf;

const SERVICE_LABEL: &str = "dev.alien.agent";

#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Install alien-agent as an OS service
    Install(InstallArgs),
    /// Start the alien-agent service
    Start,
    /// Stop the alien-agent service
    Stop,
    /// Show the agent service status
    Status,
    /// Uninstall the alien-agent service
    Uninstall,
}

#[derive(Args)]
pub struct InstallArgs {
    /// Path to the alien-agent binary. If omitted, searches PATH.
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// Manager URL for the agent to sync with
    #[arg(long)]
    pub sync_url: String,

    /// Sync authentication token
    #[arg(long)]
    pub sync_token: String,

    /// Target platform (aws, gcp, azure)
    #[arg(long, default_value = "local")]
    pub platform: String,

    /// Data directory for agent state
    #[arg(long)]
    pub data_dir: Option<String>,

    /// Encryption key (64-char hex). Generated if not provided.
    #[arg(long)]
    pub encryption_key: Option<String>,
}

pub async fn agent_command(args: AgentArgs) -> Result<()> {
    match args.command {
        AgentCommand::Install(install_args) => install(install_args),
        AgentCommand::Start => start(),
        AgentCommand::Stop => stop(),
        AgentCommand::Status => status(),
        AgentCommand::Uninstall => uninstall(),
    }
}

fn get_manager() -> Result<Box<dyn ServiceManager>> {
    <dyn ServiceManager>::native()
        .into_alien_error()
        .context(ErrorData::AgentServiceError {
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

/// Generate an encryption key (public for reuse from up.rs).
pub fn generate_encryption_key_public() -> String {
    generate_encryption_key()
}

fn install(args: InstallArgs) -> Result<()> {
    output::header("Installing alien-agent service");

    let binary_path = match args.binary {
        Some(p) => p,
        None => which_agent_binary()?,
    };

    if !binary_path.exists() {
        return Err(AlienError::new(ErrorData::AgentServiceError {
            message: format!("Binary not found: {}", binary_path.display()),
        }));
    }

    let encryption_key = args.encryption_key.unwrap_or_else(generate_encryption_key);

    let data_dir = args.data_dir.unwrap_or_else(|| {
        if cfg!(windows) {
            r"C:\ProgramData\alien-agent".to_string()
        } else {
            "/var/lib/alien-agent".to_string()
        }
    });

    // Create data directory before writing secret files into it.
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        output::warn(&format!("Could not create data directory: {}", e));
    }

    // The agent rejects `--sync-token`/`--encryption-key` (argv leak via
    // `ps` / `/proc`). Persist secrets to 0o600 files in the service's
    // data directory and pass `--*-file` paths in the service args. The
    // service runs as the user that installed it (typically root on Linux,
    // the current user on macOS launchd), which owns these files.
    let sync_token_path = std::path::Path::new(&data_dir).join("sync-token");
    let encryption_key_path = std::path::Path::new(&data_dir).join("encryption-key");

    alien_core::file_utils::write_secret_file(&sync_token_path, args.sync_token.as_bytes())
        .into_alien_error()
        .context(ErrorData::AgentServiceError {
            message: format!(
                "Failed to write sync token to {}",
                sync_token_path.display()
            ),
        })?;
    alien_core::file_utils::write_secret_file(
        &encryption_key_path,
        encryption_key.as_bytes(),
    )
    .into_alien_error()
    .context(ErrorData::AgentServiceError {
        message: format!(
            "Failed to write encryption key to {}",
            encryption_key_path.display()
        ),
    })?;

    let service_args = vec![
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

    let manager = get_manager()?;

    output::step(1, 2, &format!("Registering service ({})", SERVICE_LABEL));

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
        .context(ErrorData::AgentServiceError {
            message: "Failed to install service".to_string(),
        })?;

    output::step(2, 2, "Starting service");

    manager
        .start(ServiceStartCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::AgentServiceError {
            message: "Failed to start service".to_string(),
        })?;

    output::success("alien-agent service installed and started");
    output::info(&format!("  Binary:     {}", binary_path.display()));
    output::info(&format!("  Data dir:   {}", data_dir));
    output::info(&format!("  Platform:   {}", args.platform));
    output::info(&format!("  Sync URL:   {}", args.sync_url));

    Ok(())
}

fn start() -> Result<()> {
    let manager = get_manager()?;
    manager
        .start(ServiceStartCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::AgentServiceError {
            message: "Failed to start service".to_string(),
        })?;
    output::success("alien-agent service started");
    Ok(())
}

fn stop() -> Result<()> {
    let manager = get_manager()?;
    manager
        .stop(ServiceStopCtx { label: label() })
        .into_alien_error()
        .context(ErrorData::AgentServiceError {
            message: "Failed to stop service".to_string(),
        })?;
    output::success("alien-agent service stopped");
    Ok(())
}

fn status() -> Result<()> {
    output::header("alien-agent service status");

    // Try to query by starting — if already running this errors.
    // service-manager doesn't have a status() method, so we check the data dir.
    let data_dir = if cfg!(windows) {
        PathBuf::from(r"C:\ProgramData\alien-agent")
    } else {
        PathBuf::from("/var/lib/alien-agent")
    };

    let lock_path = data_dir.join("agent.lock");

    if lock_path.exists() {
        // Try to acquire the lock — if we can't, agent is running
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
        .context(ErrorData::AgentServiceError {
            message: "Failed to uninstall service".to_string(),
        })?;

    output::success("alien-agent service uninstalled");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn which_agent_binary() -> Result<PathBuf> {
    // 1. Check ALIEN_AGENT_BINARY env var (useful for local development)
    if let Ok(env_path) = std::env::var("ALIEN_AGENT_BINARY") {
        let p = PathBuf::from(&env_path);
        if p.exists() {
            return Ok(p);
        }
        return Err(AlienError::new(ErrorData::AgentServiceError {
            message: format!(
                "ALIEN_AGENT_BINARY set to '{}' but file not found",
                env_path
            ),
        }));
    }

    // 2. Look for alien-agent in PATH
    let candidates = if cfg!(windows) {
        vec!["alien-agent.exe"]
    } else {
        vec!["alien-agent"]
    };

    for name in &candidates {
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }
    }

    // 3. Check local build artifacts (for development from repo root)
    for profile in &["release", "debug"] {
        let local = PathBuf::from(format!("target/{}/alien-agent", profile));
        if local.exists() {
            return Ok(std::fs::canonicalize(&local).unwrap_or(local));
        }
    }

    // 4. Check ~/.alien/bin/ (where auto-download places the binary)
    if let Some(home) = dirs::home_dir() {
        let alien_bin = home.join(".alien").join("bin").join("alien-agent");
        if alien_bin.exists() {
            return Ok(alien_bin);
        }
    }

    // 5. Check common install locations
    let common_paths = if cfg!(windows) {
        vec![r"C:\Program Files\alien\alien-agent.exe"]
    } else {
        vec!["/usr/local/bin/alien-agent", "/usr/bin/alien-agent"]
    };

    for path in &common_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(AlienError::new(ErrorData::AgentServiceError {
        message: "alien-agent binary not found. Set ALIEN_AGENT_BINARY=/path/to/alien-agent, \
                  build with 'cargo build -p alien-agent', or install it first."
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
        // We got the lock — nobody else holds it, so agent is NOT running
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        false
    } else {
        // Couldn't get lock — agent IS running
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
    fn test_service_label_parse() {
        let lbl = label();
        assert_eq!(lbl.to_string(), SERVICE_LABEL);
    }
}
