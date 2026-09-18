//! Install script routes for alien-deploy-cli.
//!
//! `GET /install` returns a POSIX shell script for Linux/macOS.
//! `GET /install.ps1` returns a PowerShell script for Windows.
//!
//! Supports `?version=1.2.3` query parameter to pin a specific version
//! (defaults to the stable release channel).

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Router,
};
use http::header;
use serde::Deserialize;

use super::AppState;

#[derive(Deserialize)]
struct InstallParams {
    version: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/install", axum::routing::get(install_script))
        .route(
            "/install.ps1",
            axum::routing::get(install_script_powershell),
        )
}

/// Validate that a version string contains only safe characters.
/// Prevents shell injection when the version is interpolated into the install script.
fn is_valid_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Defense-in-depth: ensure the operator-supplied releases URL parses as a URL
/// and contains only characters safe to interpolate into a double-quoted bash
/// string. Server-side config is normally trusted, but interpolation into a
/// shell context warrants an explicit shape check.
fn is_safe_releases_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 {
        return false;
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return false;
    }
    !url.chars()
        .any(|c| matches!(c, '"' | '\\' | '$' | '`' | '\n' | '\r' | '\0') || c.is_control())
}

async fn install_script(
    State(state): State<AppState>,
    Query(params): Query<InstallParams>,
) -> Response {
    install_response(state.config.releases_url(), params, InstallerKind::Unix).await
}

async fn install_script_powershell(
    State(state): State<AppState>,
    Query(params): Query<InstallParams>,
) -> Response {
    install_response(
        state.config.releases_url(),
        params,
        InstallerKind::PowerShell,
    )
    .await
}

enum InstallerKind {
    Unix,
    PowerShell,
}

async fn install_response(
    releases_url: String,
    params: InstallParams,
    kind: InstallerKind,
) -> Response {
    // Server-side config, but interpolated into shell/PowerShell scripts. Reject
    // misconfigured values explicitly rather than producing malformed scripts.
    if !is_safe_releases_url(&releases_url) {
        tracing::error!(releases_url = %releases_url, "ALIEN_RELEASES_URL contains characters unsafe for bash interpolation");
        return (
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "Server misconfiguration: invalid ALIEN_RELEASES_URL.",
        )
            .into_response();
    }

    let version_path = match &params.version {
        Some(v) => {
            if !is_valid_version(v) {
                return (
                    http::StatusCode::BAD_REQUEST,
                    "Invalid version format. Only alphanumeric characters, dots, hyphens, and underscores are allowed.",
                )
                    .into_response();
            }
            format!("v{v}")
        }
        None => "stable".to_string(),
    };

    let script = match kind {
        InstallerKind::Unix => generate_install_script(&releases_url, &version_path),
        InstallerKind::PowerShell => {
            generate_powershell_install_script(&releases_url, &version_path)
        }
    };

    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        script,
    )
        .into_response()
}

fn generate_install_script(releases_url: &str, version_path: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu

RELEASES_URL="{releases_url}"
VERSION_PATH="{version_path}"

die() {{
  echo "$1" >&2
  exit 1
}}

if [ "$VERSION_PATH" = "stable" ]; then
  if command -v curl >/dev/null 2>&1; then
    VERSION_PATH="$(curl -fsSL "$RELEASES_URL/channels/stable")"
  elif command -v wget >/dev/null 2>&1; then
    VERSION_PATH="$(wget -q -O - "$RELEASES_URL/channels/stable")"
  else
    die "Either curl or wget is required"
  fi
  case "$VERSION_PATH" in
    v*) ;;
    *) die "Invalid stable channel version: $VERSION_PATH" ;;
  esac
  case "${{VERSION_PATH#v}}" in
    ""|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-]*)
      die "Invalid stable channel version: $VERSION_PATH"
      ;;
  esac
fi

UNAME_S="$(uname -s)"
case "$UNAME_S" in
  Linux*) OS=linux ;;
  Darwin*) OS=darwin ;;
  *) die "Unsupported OS: $UNAME_S" ;;
esac

UNAME_M="$(uname -m)"
case "$UNAME_M" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *) die "Unsupported architecture: $UNAME_M" ;;
esac

if [ "$OS" = "darwin" ] && [ "$ARCH" = "x86_64" ]; then
  if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
    ARCH=aarch64
  else
    die "Unsupported platform: darwin-x86_64. Use an Apple Silicon Mac or install with npm."
  fi
fi

URL="$RELEASES_URL/alien-deploy/$VERSION_PATH/$OS-$ARCH/alien-deploy"

echo "Installing alien-deploy ($OS-$ARCH)..."

TMPDIR="${{TMPDIR:-/tmp}}"
TMPFILE="$(mktemp "${{TMPDIR}}/alien-deploy-install.XXXXXX")"
trap 'rm -f "$TMPFILE"' EXIT INT TERM

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$URL" -o "$TMPFILE"
elif command -v wget >/dev/null 2>&1; then
  wget -q -O "$TMPFILE" "$URL"
else
  die "Either curl or wget is required"
fi

chmod +x "$TMPFILE"

if [ -n "${{INSTALL_DIR:-}}" ]; then
  INSTALL_DIR="${{INSTALL_DIR%/}}"
elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="$HOME/.local/bin"
fi

mkdir -p "$INSTALL_DIR"
mv "$TMPFILE" "$INSTALL_DIR/alien-deploy"
chmod +x "$INSTALL_DIR/alien-deploy"

if [ "$#" -gt 0 ]; then
  exec "$INSTALL_DIR/alien-deploy" "$@"
fi

echo ""
echo "  alien-deploy installed to $INSTALL_DIR/alien-deploy"
echo ""

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo "  Add to your PATH:"
    echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
    echo ""
    ;;
esac
"#,
        releases_url = releases_url,
        version_path = version_path,
    )
}

fn generate_powershell_install_script(releases_url: &str, version_path: &str) -> String {
    format!(
        r#"Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$VersionPath = "{version_path}"
if ($VersionPath -eq "stable") {{
  $VersionPath = (Invoke-RestMethod -Uri "{releases_url}/channels/stable" -ErrorAction Stop).Trim()
  if ($VersionPath -notmatch '^v[A-Za-z0-9._-]+$') {{
    throw "Invalid stable channel version: $VersionPath"
  }}
}}

if (-not [Environment]::Is64BitOperatingSystem) {{
  throw "alien-deploy requires 64-bit Windows."
}}

$Platform = "windows-x86_64"
$FileName = "alien-deploy.exe"
$Url = "{releases_url}/alien-deploy/$VersionPath/$Platform/$FileName"
$InstallDir = if ($env:INSTALL_DIR) {{
  $env:INSTALL_DIR
}} else {{
  Join-Path $env:LOCALAPPDATA "Programs\Alien\bin"
}}

Write-Host "Installing $FileName ($Platform)..." -ForegroundColor Cyan

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$BinaryPath = Join-Path $InstallDir $FileName
try {{
  Invoke-WebRequest -Uri $Url -OutFile $BinaryPath -ErrorAction Stop
}} catch {{
  if (Test-Path $BinaryPath) {{
    Remove-Item -Force $BinaryPath
  }}
  throw "Failed to download $($Url): $_"
}}

$env:PATH = "$InstallDir;$env:PATH"

$UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
if (-not $UserPath) {{
  $UserPath = ""
}}

if (((";" + $UserPath + ";").ToLowerInvariant()) -notlike (("*;" + $InstallDir + ";*").ToLowerInvariant())) {{
  $NewUserPath = if ($UserPath.Length -eq 0) {{ $InstallDir }} else {{ "$UserPath;$InstallDir" }}
  [Environment]::SetEnvironmentVariable("Path", $NewUserPath, [EnvironmentVariableTarget]::User)
}}

if ($args.Count -gt 0) {{
  & $BinaryPath @args
  exit $LASTEXITCODE
}}

Write-Host ""
Write-Host "$FileName installed to $BinaryPath" -ForegroundColor Green
Write-Host ""
Write-Host "The install directory was added to PATH for this session and future terminals."
Write-Host ""
"#,
        releases_url = releases_url,
        version_path = version_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn unix_installer_parses_with_system_sh() {
        let script = generate_install_script("https://releases.example.com", "stable");
        let mut child = Command::new("sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start sh");

        child
            .stdin
            .as_mut()
            .expect("sh stdin should be piped")
            .write_all(script.as_bytes())
            .expect("failed to write installer script to sh");

        let output = child.wait_with_output().expect("failed to wait for sh");
        assert!(
            output.status.success(),
            "generated installer did not parse as sh: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn powershell_installer_parses_with_pwsh_when_available() {
        let script = generate_powershell_install_script("https://releases.example.com", "v1.2.3");
        let mut child = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("$script = [Console]::In.ReadToEnd(); [scriptblock]::Create($script) | Out-Null")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("failed to start pwsh: {error}"),
        };

        child
            .stdin
            .as_mut()
            .expect("pwsh stdin should be piped")
            .write_all(script.as_bytes())
            .expect("failed to write installer script to pwsh");

        let output = child.wait_with_output().expect("failed to wait for pwsh");
        assert!(
            output.status.success(),
            "generated installer did not parse as PowerShell: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn rejects_unsafe_release_urls() {
        assert!(is_safe_releases_url("https://releases.example.com"));
        assert!(!is_safe_releases_url("https://example.com/`touch nope`"));
        assert!(!is_safe_releases_url("https://example.com/$HOME"));
    }

    // Execute real interpreters against an HTTP fixture: text matching cannot
    // catch quoting, channel resolution, or download failures.
    async fn run_installer_case(
        kind: InstallerKind,
        version: Option<&str>,
        pointer: &str,
        status: u16,
    ) {
        let powershell = matches!(kind, InstallerKind::PowerShell);
        if powershell && Command::new("pwsh").arg("--version").output().is_err() {
            eprintln!("PowerShell execution requires pwsh; skipped on this host");
            return;
        }
        let server = httpmock::MockServer::start();
        let channel = server.mock(|when, then| {
            when.path("/channels/stable");
            then.status(status).body(pointer);
        });
        let binary = "#!/bin/sh\nprintf 'arg=<%s>\\n' \"$@\"\nexit 7\n";
        let binary_path = if powershell {
            "/alien-deploy/v1.2.3/windows-x86_64/alien-deploy.exe".to_owned()
        } else {
            let os = if cfg!(target_os = "macos") {
                "darwin"
            } else {
                "linux"
            };
            format!(
                "/alien-deploy/v1.2.3/{os}-{}/alien-deploy",
                std::env::consts::ARCH
            )
        };
        let download = server.mock(|when, then| {
            when.path(&binary_path);
            then.status(200).body(binary);
        });
        let response = install_response(
            server.base_url(),
            InstallParams {
                version: version.map(str::to_owned),
            },
            kind,
        )
        .await;
        assert_eq!(response.status(), http::StatusCode::OK);
        let script = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let valid = version.is_some() || (status == 200 && pointer == "v1.2.3\n");
        let installed = dir.path().join(if powershell {
            "alien-deploy.exe"
        } else {
            "alien-deploy"
        });
        #[cfg(unix)]
        if powershell && valid {
            use std::os::unix::fs::PermissionsExt;
            // Windows does not need an executable bit. On Unix, pre-create an
            // executable destination so pwsh can invoke the downloaded fixture.
            std::fs::write(&installed, "").unwrap();
            std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let script_path = dir.path().join(if powershell {
            "install.ps1"
        } else {
            "install.sh"
        });
        std::fs::write(&script_path, script).unwrap();
        let mut command = Command::new(if powershell { "pwsh" } else { "sh" });
        if powershell {
            command.args(["-NoProfile", "-NonInteractive", "-File"]);
        }
        command.arg(script_path).env("INSTALL_DIR", dir.path());
        let forward_arguments = !powershell || cfg!(unix);
        if forward_arguments {
            command.args(["deploy", "space in argument", "--no-browser"]);
        }
        let output = command.output().unwrap();
        channel.assert_hits(usize::from(version.is_none()));
        download.assert_hits(usize::from(valid));
        if valid {
            assert_eq!(std::fs::read_to_string(installed).unwrap(), binary);
            if !forward_arguments {
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                assert_eq!(output.status.code(), Some(7));
                assert!(String::from_utf8_lossy(&output.stdout)
                    .ends_with("arg=<deploy>\narg=<space in argument>\narg=<--no-browser>\n"));
            }
        } else {
            assert!(!output.status.success());
            assert!(!installed.exists());
        }
    }

    #[tokio::test]
    async fn unix_installer_resolves_stable_and_forwards_arguments_and_exit_status() {
        run_installer_case(InstallerKind::Unix, None, "v1.2.3\n", 200).await;
    }

    #[tokio::test]
    async fn explicit_unix_version_does_not_fetch_channel() {
        run_installer_case(InstallerKind::Unix, Some("1.2.3"), "", 404).await;
    }

    #[tokio::test]
    async fn unix_installer_rejects_invalid_or_unavailable_channel() {
        for pointer in [
            "",
            "v",
            "latest",
            "v1.2.3/../../latest",
            "v1.2.3\necho unsafe",
        ] {
            run_installer_case(InstallerKind::Unix, None, pointer, 200).await;
        }
        run_installer_case(InstallerKind::Unix, None, "missing", 404).await;
    }

    #[tokio::test]
    async fn powershell_installer_resolves_stable_when_pwsh_available() {
        run_installer_case(InstallerKind::PowerShell, None, "v1.2.3\n", 200).await;
    }

    #[tokio::test]
    async fn explicit_powershell_version_does_not_fetch_channel_when_pwsh_available() {
        run_installer_case(InstallerKind::PowerShell, Some("1.2.3"), "", 404).await;
    }

    #[tokio::test]
    async fn powershell_installer_rejects_invalid_or_unavailable_channel_when_pwsh_available() {
        for pointer in [
            "",
            "v",
            "latest",
            "v1.2.3/../../latest",
            "v1.2.3\necho unsafe",
        ] {
            run_installer_case(InstallerKind::PowerShell, None, pointer, 200).await;
        }
        run_installer_case(InstallerKind::PowerShell, None, "missing", 404).await;
    }
}
