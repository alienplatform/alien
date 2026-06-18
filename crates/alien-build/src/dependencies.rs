use crate::command_output::{image_build_error_with_output, CapturedCommandOutput};
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tracing::{debug, info};

/// Detected package manager for a TypeScript/JavaScript project
#[derive(Debug, Clone, PartialEq)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Bun,
}

impl PackageManager {
    /// Get the command name for the package manager
    pub fn command(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Bun => "bun",
        }
    }

    /// Check if the package manager is available on the system
    pub fn is_available(&self) -> bool {
        which::which(self.command()).is_ok()
    }
}

struct DependencyInstallLock {
    path: PathBuf,
}

impl Drop for DependencyInstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

async fn acquire_dependency_install_lock(
    install_dir: &Path,
    pm_command: &str,
) -> Result<DependencyInstallLock> {
    let lock_path = install_dir.join(".alien-dependency-install.lock");
    let start = Instant::now();
    let timeout = Duration::from_secs(300);

    loop {
        match fs::create_dir(&lock_path) {
            Ok(()) => return Ok(DependencyInstallLock { path: lock_path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if start.elapsed() > timeout {
                    return Err(AlienError::new(ErrorData::ImageBuildFailed {
                        resource_name: "dependency-install".to_string(),
                        reason: format!(
                            "Timed out waiting for another {} install in {}",
                            pm_command,
                            install_dir.display()
                        ),
                        build_output: None,
                    }));
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(err) => {
                return Err(err)
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        resource_name: "dependency-install".to_string(),
                        reason: format!(
                            "Failed to create dependency install lock in {}",
                            install_dir.display()
                        ),
                        build_output: None,
                    });
            }
        }
    }
}

/// Check if a directory is a workspace root by looking for workspace configuration.
///
/// - pnpm: `pnpm-workspace.yaml`
/// - npm/bun: `"workspaces"` field in `package.json`
fn is_workspace_root(dir: &Path) -> bool {
    // pnpm uses a dedicated workspace config file
    if dir.join("pnpm-workspace.yaml").exists() {
        return true;
    }

    // npm and bun use a "workspaces" field in package.json
    if let Ok(contents) = std::fs::read_to_string(dir.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
            if json.get("workspaces").is_some() {
                return true;
            }
        }
    }

    false
}

/// Find the project/workspace root by looking for lock files.
///
/// Walks up the directory tree from src_dir and returns the nearest workspace
/// root that has both a `package.json` AND a lock file. Stops at workspace
/// boundaries (directories with workspace configuration) to avoid escaping
/// into a parent workspace.
///
/// This correctly handles:
/// - Standalone projects (lock file in src_dir)
/// - Workspace members (lock file in parent workspace root)
/// - Nested workspaces (stops at the nearest workspace root, not the highest)
///
/// Also returns the detected package manager based on the lock file found.
fn find_workspace_root(src_dir: &Path) -> (PathBuf, PackageManager) {
    // Canonicalize the path to handle relative paths like "./"
    let canonical_src_dir = src_dir
        .canonicalize()
        .unwrap_or_else(|_| src_dir.to_path_buf());
    let mut workspace_root = canonical_src_dir.clone();
    let mut current_dir: Option<&Path> = Some(&canonical_src_dir);
    let mut detected_pm = PackageManager::Npm; // Default to npm

    debug!(
        "Finding workspace root starting from: {} (canonical: {})",
        src_dir.display(),
        canonical_src_dir.display()
    );

    let mut found_match = false;

    while let Some(dir) = current_dir {
        debug!("Checking directory: {}", dir.display());

        // Only consider directories that have package.json (valid JS project roots)
        if dir.join("package.json").exists() {
            debug!("  Found package.json");

            // Only set workspace_root on the first match (nearest lock file wins)
            if !found_match {
                // Check for lock files in priority order
                if dir.join("bun.lock").exists() {
                    debug!("  Found bun.lock (first match)");
                    workspace_root = dir.to_path_buf();
                    detected_pm = PackageManager::Bun;
                    found_match = true;
                } else if dir.join("bun.lockb").exists() {
                    debug!("  Found bun.lockb (first match)");
                    workspace_root = dir.to_path_buf();
                    detected_pm = PackageManager::Bun;
                    found_match = true;
                } else if dir.join("pnpm-lock.yaml").exists() {
                    debug!("  Found pnpm-lock.yaml (first match)");
                    workspace_root = dir.to_path_buf();
                    detected_pm = PackageManager::Pnpm;
                    found_match = true;
                } else if dir.join("package-lock.json").exists() {
                    debug!("  Found package-lock.json (first match)");
                    workspace_root = dir.to_path_buf();
                    detected_pm = PackageManager::Npm;
                    found_match = true;
                } else {
                    debug!("  No lock file found");
                }
            }

            // If this directory is a workspace root (has workspace config + lock file),
            // stop here — don't traverse into a parent workspace.
            if is_workspace_root(dir) && dir != &canonical_src_dir {
                debug!(
                    "  Found workspace boundary at {}, stopping traversal",
                    dir.display()
                );
                break;
            }
        }
        current_dir = dir.parent();
    }

    debug!(
        "Workspace root: {}, Package manager: {:?}",
        workspace_root.display(),
        detected_pm
    );
    (workspace_root, detected_pm)
}

/// Install dependencies using the detected package manager.
///
/// Detects the package manager based on lock files:
/// - bun.lock/bun.lockb -> bun
/// - pnpm-lock.yaml -> pnpm
/// - package-lock.json -> npm
///
/// If the package is part of a workspace, install is run from the workspace root.
/// If no `package.json` exists, returns Ok(()) — the config loader handles
/// auto-installing `@alienplatform/core` via its cached modules mechanism.
pub async fn install_dependencies(src_dir: &Path) -> Result<()> {
    debug!(
        "Installing dependencies in directory: {}",
        src_dir.display()
    );

    if std::env::var("ALIEN_SKIP_DEPENDENCY_INSTALL").is_ok() {
        info!("Skipping dependency installation (ALIEN_SKIP_DEPENDENCY_INSTALL set)");
        return Ok(());
    }

    // If there's no package.json at all, skip — the config loader will use
    // its cached @alienplatform/core installation instead.
    let canonical = src_dir
        .canonicalize()
        .unwrap_or_else(|_| src_dir.to_path_buf());
    if !canonical.join("package.json").exists() {
        debug!(
            "No package.json found in {}, skipping dependency install",
            src_dir.display()
        );
        return Ok(());
    }

    // Find workspace root and detect package manager
    let (install_dir, package_manager) = find_workspace_root(src_dir);
    let pm_command = package_manager.command();

    let _install_lock = acquire_dependency_install_lock(&install_dir, pm_command).await?;

    if install_dir.join("node_modules").exists() {
        debug!("node_modules already exists, skipping install");
        return Ok(());
    }

    info!("Detected package manager: {:?}", package_manager);

    if install_dir != src_dir {
        info!(
            "Detected workspace setup, installing from workspace root: {}",
            install_dir.display()
        );
    }

    // Check if package manager is available
    if !package_manager.is_available() {
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: "dependency-install".to_string(),
            reason: format!(
                "{} is required for building this project. Please install it.",
                pm_command
            ),
            build_output: None,
        }));
    }

    info!(
        "Installing dependencies with {} from: {}",
        pm_command,
        install_dir.display()
    );
    // Use frozen lockfile for pnpm/bun to avoid re-resolving workspace-linked
    // packages from the npm registry.
    let mut cmd = Command::new(pm_command);
    cmd.arg("install");
    if matches!(package_manager, PackageManager::Pnpm | PackageManager::Bun) {
        cmd.arg("--frozen-lockfile");
    }
    let install_output = cmd
        .current_dir(&install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: "dependency-install".to_string(),
            reason: format!("Failed to execute {} install", pm_command),
            build_output: None,
        })?;

    if !install_output.status.success() {
        let captured = CapturedCommandOutput::from_output(&install_output).display();
        debug!(
            "Failed to install dependencies with {}. Build output:\n{}",
            pm_command, captured
        );
        return Err(image_build_error_with_output(
            "dependency-install",
            format!("{} install failed", pm_command),
            &install_output,
        ));
    }

    let install_stdout = String::from_utf8_lossy(&install_output.stdout);
    info!("{} install output: {}", pm_command, install_stdout);
    info!("Dependencies installed successfully");

    Ok(())
}
