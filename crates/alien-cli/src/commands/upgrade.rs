use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use clap::Parser;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use semver::Version;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use tokio::io::AsyncWriteExt;

const DEFAULT_RELEASES_URL: &str = "https://releases.alien.dev";
const INSTALL_METHOD_ENV: &str = "ALIEN_INSTALL_METHOD";
const RELEASES_URL_ENV: &str = "ALIEN_RELEASES_URL";

#[derive(Parser, Debug, Clone)]
pub struct UpgradeArgs {
    /// Check what would be installed without changing anything
    #[arg(long)]
    pub dry_run: bool,

    /// Reinstall even when the stable version matches the current version
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    Npm,
    Homebrew,
    Standalone,
}

pub async fn upgrade_task(args: UpgradeArgs) -> Result<()> {
    let current_exe = env::current_exe()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: "Could not locate the running Alien executable".to_string(),
        })?;
    let method = detect_install_method(&current_exe);

    match method {
        InstallMethod::Npm => upgrade_with_package_manager(
            &args,
            "npm",
            &["install", "-g", "@alienplatform/cli@latest"],
        ),
        InstallMethod::Homebrew => {
            upgrade_with_package_manager(&args, "brew", &["upgrade", "alienplatform/tap/alien"])
        }
        InstallMethod::Standalone => upgrade_standalone(&args, &current_exe).await,
    }
}

fn detect_install_method(current_exe: &Path) -> InstallMethod {
    if env::var(INSTALL_METHOD_ENV).as_deref() == Ok("npm") {
        return InstallMethod::Npm;
    }

    let path = current_exe.to_string_lossy().replace('\\', "/");
    if path.contains("/Cellar/alien/") || path.contains("/Caskroom/alien/") {
        InstallMethod::Homebrew
    } else {
        InstallMethod::Standalone
    }
}

fn upgrade_with_package_manager(
    args: &UpgradeArgs,
    program: &str,
    command_args: &[&str],
) -> Result<()> {
    let rendered = format!("{program} {}", command_args.join(" "));
    if args.dry_run {
        println!("Would upgrade Alien with `{rendered}`");
        return Ok(());
    }

    println!("Upgrading Alien with `{rendered}`...");
    let status = Command::new(program)
        .args(command_args)
        .status()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not run `{rendered}`"),
        })?;
    if !status.success() {
        return Err(alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: format!("`{rendered}` exited with {status}"),
        }));
    }

    println!("Alien was upgraded successfully. Restart it to use the new version.");
    Ok(())
}

async fn upgrade_standalone(args: &UpgradeArgs, current_exe: &Path) -> Result<()> {
    let releases_url =
        env::var(RELEASES_URL_ENV).unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());
    let client = reqwest::Client::new();
    let stable_url = format!("{releases_url}/channels/stable");
    let stable = client
        .get(&stable_url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not fetch the stable channel from {stable_url}"),
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("The stable channel request failed: {stable_url}"),
        })?
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not read the stable channel response from {stable_url}"),
        })?;
    let stable = parse_release_version(stable.trim())?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "The current CLI version is invalid: {}",
                env!("CARGO_PKG_VERSION")
            ),
        })?;

    if !should_install(&stable, &current, args.force) {
        if stable < current {
            println!(
                "Alien v{current} is newer than the stable release (v{stable}); leaving it unchanged."
            );
        } else {
            println!("Alien is already up to date (v{current}).");
        }
        return Ok(());
    }

    let artifact_url = artifact_url(&releases_url, &stable)?;
    if args.dry_run {
        println!("Would upgrade Alien from v{current} to v{stable}");
        println!("  {artifact_url}");
        return Ok(());
    }

    println!("Upgrading Alien from v{current} to v{stable}...");
    let response = client
        .get(&artifact_url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not download {artifact_url}"),
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("The release download failed: {artifact_url}"),
        })?;
    let expected_checksum = checksum_header(response.headers())?;
    let temp_dir = tempfile::Builder::new()
        .prefix("alien-upgrade-")
        .tempdir()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: "Could not create a temporary upgrade directory".to_string(),
        })?;
    let staged_exe = temp_dir.path().join(executable_name());
    let mut staged_file = tokio::fs::File::create(&staged_exe)
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "Could not create the staged CLI at {}",
                staged_exe.display()
            ),
        })?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.into_alien_error().context(ErrorData::UpgradeFailed {
            message: format!("Could not read the release download from {artifact_url}"),
        })?;
        hasher.update(&chunk);
        staged_file
            .write_all(&chunk)
            .await
            .into_alien_error()
            .context(ErrorData::UpgradeFailed {
                message: format!("Could not write the staged CLI at {}", staged_exe.display()),
            })?;
    }
    make_executable(&staged_exe)?;
    staged_file
        .sync_all()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "Could not synchronize the staged CLI at {}",
                staged_exe.display()
            ),
        })?;
    let actual_checksum = hex::encode(hasher.finalize());
    verify_checksum_value(&actual_checksum, &expected_checksum)?;
    validate_download(&staged_exe, &stable)?;
    self_replace::self_replace(&staged_exe)
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: "Could not replace the current Alien executable; check that it is writable"
                .to_string(),
        })?;
    sync_replacement(current_exe)?;

    println!("Alien was upgraded successfully to v{stable}.");
    Ok(())
}

fn sync_replacement(current_exe: &Path) -> Result<()> {
    fs::File::open(current_exe)
        .and_then(|file| file.sync_all())
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "Alien was replaced, but the installed executable could not be synchronized: {}",
                current_exe.display()
            ),
        })?;
    sync_replacement_directory(current_exe)
}

#[cfg(unix)]
fn sync_replacement_directory(current_exe: &Path) -> Result<()> {
    let directory = current_exe.parent().ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: format!(
                "Could not locate the installation directory for {}",
                current_exe.display()
            ),
        })
    })?;
    fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "Alien was replaced, but its installation directory could not be synchronized: {}",
                directory.display()
            ),
        })
}

#[cfg(not(unix))]
fn sync_replacement_directory(_current_exe: &Path) -> Result<()> {
    Ok(())
}

fn parse_release_version(value: &str) -> Result<Version> {
    let version = value.strip_prefix('v').ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: format!("The stable channel returned an invalid version: {value}"),
        })
    })?;
    Version::parse(version)
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("The stable channel returned an invalid version: {value}"),
        })
}

fn should_install(stable: &Version, current: &Version, force: bool) -> bool {
    stable > current || (stable == current && force)
}

fn artifact_url(releases_url: &str, version: &Version) -> Result<String> {
    let (os, arch) = platform()?;
    Ok(format!(
        "{releases_url}/alien/v{version}/{os}-{arch}/{}",
        executable_name()
    ))
}

fn platform() -> Result<(&'static str, &'static str)> {
    let os = match env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => return unsupported_platform(other, env::consts::ARCH),
    };
    let arch = match env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => return unsupported_platform(os, other),
    };
    if os == "darwin" && arch == "x86_64" {
        return unsupported_platform(os, arch);
    }
    if os == "windows" && arch != "x86_64" {
        return unsupported_platform(os, arch);
    }
    Ok((os, arch))
}

fn unsupported_platform<T>(os: &str, arch: &str) -> Result<T> {
    Err(alien_error::AlienError::new(ErrorData::UpgradeFailed {
        message: format!("No Alien CLI release is published for {os}-{arch}"),
    }))
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "alien.exe"
    } else {
        "alien"
    }
}

fn checksum_header(headers: &HeaderMap) -> Result<String> {
    headers
        .get("x-amz-meta-sha256")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::UpgradeFailed {
                message: "The release download did not include a valid SHA-256 checksum"
                    .to_string(),
            })
        })
}

#[cfg(test)]
fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = hex::encode(Sha256::digest(bytes));
    verify_checksum_value(&actual, expected)
}

fn verify_checksum_value(actual: &str, expected: &str) -> Result<()> {
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: format!("Release checksum mismatch: expected {expected}, got {actual}"),
        }))
    }
}

fn validate_download(path: &Path, expected: &Version) -> Result<()> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not run the downloaded CLI at {}", path.display()),
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let valid = output.status.success()
        && stdout
            .split_whitespace()
            .last()
            .and_then(|value| value.strip_prefix('v').or(Some(value)))
            .and_then(|value| Version::parse(value).ok())
            .as_ref()
            == Some(expected);
    if valid {
        Ok(())
    } else {
        Err(alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: format!(
                "The downloaded CLI failed validation for v{expected}: {}",
                stdout.trim()
            ),
        }))
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not make {} executable", path.display()),
        })
}

#[cfg(windows)]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_channel_requires_v_prefixed_semver() {
        assert_eq!(
            parse_release_version("v3.3.18").unwrap(),
            Version::new(3, 3, 18)
        );
        assert!(parse_release_version("latest").is_err());
    }

    #[test]
    fn checksum_verification_rejects_modified_download() {
        let checksum = hex::encode(Sha256::digest(b"release"));
        assert!(verify_checksum(b"release", &checksum).is_ok());
        assert!(verify_checksum(b"modified", &checksum).is_err());
    }

    #[test]
    fn force_reinstalls_current_version_without_downgrading() {
        let current = Version::new(3, 3, 18);
        assert!(should_install(&current, &current, true));
        assert!(!should_install(&Version::new(3, 3, 17), &current, true));
    }

    #[test]
    fn homebrew_install_is_detected_from_cellar_path() {
        assert_eq!(
            detect_install_method(Path::new("/opt/homebrew/Cellar/alien/3.3.18/bin/alien")),
            InstallMethod::Homebrew
        );
    }
}
