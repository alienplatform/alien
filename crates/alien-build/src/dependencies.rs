use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info};

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
    // Default to whatever is available; bun preferred so file: deps resolve correctly
    let mut detected_pm = if PackageManager::Bun.is_available() {
        PackageManager::Bun
    } else if PackageManager::Pnpm.is_available() {
        PackageManager::Pnpm
    } else {
        PackageManager::Npm
    };

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
pub async fn install_dependencies(src_dir: &Path) -> Result<()> {
    debug!(
        "Installing dependencies in directory: {}",
        src_dir.display()
    );

    if std::env::var("ALIEN_SKIP_DEPENDENCY_INSTALL").is_ok() {
        info!("Skipping dependency installation (ALIEN_SKIP_DEPENDENCY_INSTALL set)");
        return Ok(());
    }

    // Find workspace root and detect package manager
    let (install_dir, package_manager) = find_workspace_root(src_dir);

    if install_dir.join("node_modules").exists() {
        debug!("node_modules already exists, skipping install");
        return Ok(());
    }
    let pm_command = package_manager.command();

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
            function_name: "dependency-install".to_string(),
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
    // Use frozen lockfile for pnpm/bun only when a lockfile already exists, to avoid
    // re-resolving workspace-linked packages from the npm registry.
    let has_lockfile = install_dir.join("bun.lock").exists()
        || install_dir.join("bun.lockb").exists()
        || install_dir.join("pnpm-lock.yaml").exists()
        || install_dir.join("package-lock.json").exists();

    let mut cmd = Command::new(pm_command);
    cmd.arg("install");
    if has_lockfile && matches!(package_manager, PackageManager::Pnpm | PackageManager::Bun) {
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
            function_name: "dependency-install".to_string(),
            reason: format!("Failed to execute {} install", pm_command),
            build_output: None,
        })?;

    if !install_output.status.success() {
        let stderr = String::from_utf8_lossy(&install_output.stderr);
        let stdout = String::from_utf8_lossy(&install_output.stdout);
        error!(
            "Failed to install dependencies - stderr: {}, stdout: {}",
            stderr, stdout
        );
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            function_name: "dependency-install".to_string(),
            reason: format!("{} install failed", pm_command),
            build_output: Some(stderr.to_string()),
        }));
    }

    let install_stdout = String::from_utf8_lossy(&install_output.stdout);
    info!("{} install output: {}", pm_command, install_stdout);
    info!("Dependencies installed successfully");

    Ok(())
}
