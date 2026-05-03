//! Install script route — serves a bash installer for alien-deploy-cli.
//!
//! `GET /install` returns a bash script that detects OS/arch and downloads
//! the appropriate alien-deploy binary.
//!
//! Supports `?version=1.2.3` query parameter to pin a specific version
//! (defaults to `latest`).

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
    Router::new().route("/install", axum::routing::get(install_script))
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
    !url.chars().any(|c| {
        matches!(
            c,
            '"' | '\\' | '$' | '`' | '\n' | '\r' | '\0'
        ) || c.is_control()
    })
}

async fn install_script(
    State(state): State<AppState>,
    Query(params): Query<InstallParams>,
) -> Response {
    let releases_url = state.config.releases_url();
    // Server-side config, but interpolated into a bash here-doc — if an operator
    // ever sets a value with shell-special characters (`"`, `$`, backtick,
    // newline) the script breaks. Reject misconfigured values explicitly rather
    // than producing a malformed/dangerous script.
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
        None => "latest".to_string(),
    };

    let script = generate_install_script(&releases_url, &version_path);

    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        script,
    )
        .into_response()
}

fn generate_install_script(releases_url: &str, version_path: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

RELEASES_URL="{releases_url}"
VERSION_PATH="{version_path}"

# Detect OS
case "$(uname -s)" in
  Linux*)  OS=linux ;;
  Darwin*) OS=darwin ;;
  *)       echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

# Detect architecture
case "$(uname -m)" in
  x86_64|amd64)  ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *)             echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

URL="${{RELEASES_URL}}/alien-deploy/${{VERSION_PATH}}/${{OS}}-${{ARCH}}/alien-deploy"

echo "Installing alien-deploy (${{OS}}-${{ARCH}})..."

# Download to temp file
TMPDIR="${{TMPDIR:-/tmp}}"
TMPFILE="$(mktemp "${{TMPDIR}}/alien-deploy-install.XXXXXX")"
trap 'rm -f "${{TMPFILE}}"' EXIT

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${{URL}}" -o "${{TMPFILE}}"
elif command -v wget >/dev/null 2>&1; then
  wget -q -O "${{TMPFILE}}" "${{URL}}"
else
  echo "Either curl or wget is required" >&2
  exit 1
fi

chmod +x "${{TMPFILE}}"

# Install to /usr/local/bin (or ~/.local/bin if no sudo)
INSTALL_DIR="/usr/local/bin"
if [ ! -w "${{INSTALL_DIR}}" ]; then
  INSTALL_DIR="${{HOME}}/.local/bin"
  mkdir -p "${{INSTALL_DIR}}"
fi

mv "${{TMPFILE}}" "${{INSTALL_DIR}}/alien-deploy"
chmod +x "${{INSTALL_DIR}}/alien-deploy"

echo ""
echo "  alien-deploy installed to ${{INSTALL_DIR}}/alien-deploy"
echo ""

# Check if install dir is in PATH
case ":${{PATH}}:" in
  *":${{INSTALL_DIR}}:"*) ;;
  *)
    echo "  Add to your PATH:"
    echo "    export PATH=\"${{INSTALL_DIR}}:\$PATH\""
    echo ""
    ;;
esac
"#,
        releases_url = releases_url,
        version_path = version_path,
    )
}
