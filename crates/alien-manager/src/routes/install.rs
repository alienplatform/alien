//! Install script route — serves a bash installer for alien-deploy-cli.
//!
//! `GET /install` returns a bash script that detects OS/arch and downloads
//! the appropriate alien-deploy binary.

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Router,
};
use http::header;

use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/install", axum::routing::get(install_script))
}

async fn install_script(State(state): State<AppState>) -> Response {
    let releases_url = state.config.releases_url();

    let script = generate_install_script(&releases_url);

    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        script,
    )
        .into_response()
}

fn generate_install_script(releases_url: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

RELEASES_URL="{releases_url}"

# Detect OS
case "$(uname -s)" in
  Linux*)  OS=linux ;;
  Darwin*) OS=darwin ;;
  *)       echo "Unsupported OS: $(uname -s)"; exit 1 ;;
esac

# Detect architecture
case "$(uname -m)" in
  x86_64|amd64)  ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *)             echo "Unsupported architecture: $(uname -m)"; exit 1 ;;
esac

URL="${{RELEASES_URL}}/alien-deploy/latest/${{OS}}-${{ARCH}}/alien-deploy"

echo "Downloading alien-deploy (${{OS}}-${{ARCH}})..."
TMPDIR="${{TMPDIR:-/tmp}}"
curl -fsSL "${{URL}}" -o "${{TMPDIR}}/alien-deploy"
chmod +x "${{TMPDIR}}/alien-deploy"

# Install to /usr/local/bin (or ~/.local/bin if no sudo)
INSTALL_DIR="/usr/local/bin"
if [ ! -w "${{INSTALL_DIR}}" ]; then
  INSTALL_DIR="${{HOME}}/.local/bin"
  mkdir -p "${{INSTALL_DIR}}"
fi

mv "${{TMPDIR}}/alien-deploy" "${{INSTALL_DIR}}/alien-deploy"
chmod +x "${{INSTALL_DIR}}/alien-deploy"

echo "alien-deploy installed to ${{INSTALL_DIR}}/alien-deploy"

# Check if install dir is in PATH
case ":${{PATH}}:" in
  *":${{INSTALL_DIR}}:"*) ;;
  *) echo "Note: Add ${{INSTALL_DIR}} to your PATH:  export PATH=\"${{INSTALL_DIR}}:\$PATH\"" ;;
esac
"#,
        releases_url = releases_url
    )
}
