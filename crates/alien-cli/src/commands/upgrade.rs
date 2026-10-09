use crate::{
    error::{ErrorData, Result},
    CLI_VERSION,
};
use alien_error::{Context, IntoAlienError};
use clap::Parser;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use semver::Version;
use sha2::{Digest, Sha256};
use std::env;
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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

    /// Install the latest canary build (standalone installations only)
    #[arg(long)]
    pub canary: bool,

    /// Reinstall even when the selected version matches the current version
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseChannel {
    Stable,
    Canary,
}

impl ReleaseChannel {
    fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Canary => "canary",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CliVersion {
    Stable(Version),
    Canary { base: Version, revision: String },
}

impl CliVersion {
    fn parse(value: &str) -> Result<Self> {
        let invalid = || ErrorData::UpgradeFailed {
            message: format!("Invalid CLI version: {value}"),
        };
        if let Some((base, revision)) = value.rsplit_once('-').filter(|(_, revision)| {
            revision.len() == 8
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            let base = Version::parse(base).into_alien_error().context(invalid())?;
            if !base.pre.is_empty() || !base.build.is_empty() {
                return Err(alien_error::AlienError::new(invalid()));
            }
            Ok(Self::Canary {
                base,
                revision: revision.to_owned(),
            })
        } else {
            Version::parse(value)
                .map(Self::Stable)
                .into_alien_error()
                .context(invalid())
        }
    }

    fn channel(&self) -> ReleaseChannel {
        match self {
            Self::Stable(_) => ReleaseChannel::Stable,
            Self::Canary { .. } => ReleaseChannel::Canary,
        }
    }
}

impl fmt::Display for CliVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable(version) => write!(f, "{version}"),
            Self::Canary { base, revision } => write!(f, "{base}-{revision}"),
        }
    }
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
    if args.canary && method != InstallMethod::Standalone {
        return Err(alien_error::AlienError::new(ErrorData::UpgradeFailed {
            message: "Canary builds require a standalone installation. Install Alien from https://alien.dev/install, then run `alien update --canary`.".to_string(),
        }));
    }

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
    let channel = if args.canary {
        ReleaseChannel::Canary
    } else {
        ReleaseChannel::Stable
    };
    let channel_name = channel.name();
    let channel_url = format!("{releases_url}/channels/{channel_name}");
    let release = client
        .get(&channel_url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("Could not fetch the {channel_name} channel from {channel_url}"),
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!("The {channel_name} channel request failed: {channel_url}"),
        })?
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: format!(
                "Could not read the {channel_name} channel response from {channel_url}"
            ),
        })?;
    let release = parse_release_version(release.trim(), channel)?;
    let current = CliVersion::parse(CLI_VERSION)?;

    if !should_install(&release, &current, args.force) {
        if release != current {
            println!(
                "Alien v{current} is newer than the stable release (v{release}); leaving it unchanged."
            );
        } else {
            println!("Alien is already up to date (v{current}).");
        }
        return Ok(());
    }

    let artifact_url = artifact_url(&releases_url, &release)?;
    if args.dry_run {
        println!("Would upgrade Alien from v{current} to v{release}");
        println!("  {artifact_url}");
        return Ok(());
    }

    println!("Upgrading Alien from v{current} to v{release}...");
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
    drop(staged_file);
    let actual_checksum = hex::encode(hasher.finalize());
    verify_checksum_value(&actual_checksum, &expected_checksum)?;
    validate_download(&staged_exe, &release)?;
    self_replace::self_replace(&staged_exe)
        .into_alien_error()
        .context(ErrorData::UpgradeFailed {
            message: "Could not replace the current Alien executable; check that it is writable"
                .to_string(),
        })?;
    if let Err(error) = sync_replacement(current_exe) {
        eprintln!(
            "Warning: Alien was replaced, but the change could not be fully synchronized to disk: {error}"
        );
    }

    println!("Alien was upgraded successfully to v{release}.");
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

fn parse_release_version(value: &str, channel: ReleaseChannel) -> Result<CliVersion> {
    let invalid = || ErrorData::UpgradeFailed {
        message: format!(
            "The {} channel returned an invalid version: {value}",
            channel.name()
        ),
    };
    let version = value
        .strip_prefix('v')
        .ok_or_else(|| alien_error::AlienError::new(invalid()))?;
    let version = CliVersion::parse(version).context(invalid())?;
    if version.channel() != channel {
        return Err(alien_error::AlienError::new(invalid()));
    }
    Ok(version)
}

fn should_install(release: &CliVersion, current: &CliVersion, force: bool) -> bool {
    match (release, current) {
        (CliVersion::Stable(release), CliVersion::Stable(current)) => {
            release > current || (release == current && force)
        }
        _ => release != current || force,
    }
}

fn artifact_url(releases_url: &str, version: &CliVersion) -> Result<String> {
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

fn validate_download(path: &Path, expected: &CliVersion) -> Result<()> {
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
            .and_then(|value| CliVersion::parse(value).ok())
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
            parse_release_version("v3.3.18", ReleaseChannel::Stable).unwrap(),
            CliVersion::Stable(Version::new(3, 3, 18))
        );
        assert!(parse_release_version("latest", ReleaseChannel::Stable).is_err());
    }

    #[test]
    fn canary_versions_preserve_numeric_revisions_with_leading_zeros() {
        let value = "v3.3.30-00000001";
        let version = parse_release_version(value, ReleaseChannel::Canary).unwrap();
        assert_eq!(format!("v{version}"), value);
        assert!(parse_release_version(value, ReleaseChannel::Stable).is_err());
        assert!(parse_release_version("v3.3.30", ReleaseChannel::Canary).is_err());
        for invalid in [
            "v3.3.30-0000001",
            "v3.3.30-000000001",
            "v3.3.30-ABCDEF12",
            "v3.3.30-abcdefg1",
            "v3.3.30-00000001+extra",
            "v3.3.30-rc.1-abcdef12",
        ] {
            assert!(
                parse_release_version(invalid, ReleaseChannel::Canary).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn changing_canaries_uses_identity_instead_of_revision_order() {
        let current = CliVersion::parse("3.3.31-ffffffff").unwrap();
        let next = CliVersion::parse("3.3.31-00000001").unwrap();
        assert!(should_install(&next, &current, false));
        assert!(!should_install(&next, &next, false));
        assert!(should_install(&next, &next, true));
    }

    #[test]
    fn stable_update_leaves_canary_even_when_stable_base_is_lower() {
        let current = CliVersion::parse("3.3.31-ffffffff").unwrap();
        let stable = CliVersion::Stable(Version::new(3, 3, 30));
        assert!(should_install(&stable, &current, false));
    }

    #[test]
    fn checksum_verification_rejects_modified_download() {
        let checksum = hex::encode(Sha256::digest(b"release"));
        assert!(verify_checksum(b"release", &checksum).is_ok());
        assert!(verify_checksum(b"modified", &checksum).is_err());
    }

    #[test]
    fn force_reinstalls_current_version_without_downgrading() {
        let current = CliVersion::Stable(Version::new(3, 3, 18));
        assert!(should_install(&current, &current, true));
        assert!(!should_install(
            &CliVersion::Stable(Version::new(3, 3, 17)),
            &current,
            true
        ));
    }

    #[test]
    fn homebrew_install_is_detected_from_cellar_path() {
        assert_eq!(
            detect_install_method(Path::new("/opt/homebrew/Cellar/alien/3.3.18/bin/alien")),
            InstallMethod::Homebrew
        );
    }
}
