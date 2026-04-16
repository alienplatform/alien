#!/usr/bin/env bash
#
# Build and publish @alienplatform/cli npm packages.
#
# Uses the Codex-style single-package pattern: platform variants are published
# as @alienplatform/cli@{VERSION}-{platform} and the main package at
# @alienplatform/cli@{VERSION} has optionalDependencies using npm aliases.
#
# Required env vars:
#   VERSION          - Release version (e.g., 1.3.2)
#   NODE_AUTH_TOKEN  - npm auth token
#
# Expected artifacts layout (from GitHub Actions download-artifact):
#   ./artifacts/binaries-x86_64-unknown-linux-musl/{alien,alien-deploy,...}
#   ./artifacts/binaries-aarch64-unknown-linux-musl/{alien,alien-deploy,...}
#   ./artifacts/binaries-aarch64-apple-darwin/{alien,alien-deploy,...}
#   ./artifacts/binaries-x86_64-pc-windows-msvc/{alien.exe,alien-deploy.exe,...}

set -euo pipefail

: "${VERSION:?VERSION is required}"
: "${NODE_AUTH_TOKEN:?NODE_AUTH_TOKEN is required}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# Platform definitions: npm_suffix target_triple os cpu binary_ext
PLATFORMS=(
  "linux-x64    x86_64-unknown-linux-musl    linux   x64   "
  "linux-arm64  aarch64-unknown-linux-musl   linux   arm64 "
  "darwin-arm64 aarch64-apple-darwin         darwin  arm64 "
  "win32-x64    x86_64-pc-windows-msvc       win32   x64   .exe"
)

echo "==> Building npm packages for @alienplatform/cli v${VERSION}"

# ── Step 1: Build platform-specific packages ─────────────────────────

for platform_def in "${PLATFORMS[@]}"; do
  read -r npm_suffix target os cpu ext <<< "$platform_def"

  echo ""
  echo "--- Building @alienplatform/cli@${VERSION}-${npm_suffix} ---"

  pkg_dir="${WORK_DIR}/cli-${npm_suffix}"
  vendor_dir="${pkg_dir}/vendor/${target}"
  mkdir -p "$vendor_dir"

  # Copy binaries
  src_dir="./artifacts/binaries-${target}"
  for binary in alien alien-deploy; do
    cp "${src_dir}/${binary}${ext}" "${vendor_dir}/${binary}${ext}"
    chmod +x "${vendor_dir}/${binary}${ext}"
  done

  # Generate package.json for platform package
  cat > "${pkg_dir}/package.json" << EOF
{
  "name": "@alienplatform/cli",
  "version": "${VERSION}-${npm_suffix}",
  "description": "Alien CLI binary for ${os}-${cpu}",
  "os": ["${os}"],
  "cpu": ["${cpu}"],
  "files": ["vendor"],
  "publishConfig": {
    "access": "public"
  }
}
EOF

  # Pack and publish with platform-specific tag
  (cd "$pkg_dir" && npm pack)
  npm publish "${pkg_dir}/"*.tgz --tag "${npm_suffix}" || echo "WARN: publish of ${npm_suffix} variant may have already been published"
done

# ── Step 2: Build the main package ───────────────────────────────────

echo ""
echo "--- Building @alienplatform/cli@${VERSION} (main) ---"

main_dir="${WORK_DIR}/cli-main"
mkdir -p "${main_dir}/bin"

# Copy JS shim
cp "${REPO_ROOT}/packages/alien-cli-npm/bin/alien.js" "${main_dir}/bin/alien.js"

# Generate package.json with injected optionalDependencies
cat > "${main_dir}/package.json" << EOF
{
  "name": "@alienplatform/cli",
  "version": "${VERSION}",
  "description": "Alien Developer Platform CLI",
  "license": "Apache-2.0",
  "bin": {
    "alien": "bin/alien.js"
  },
  "type": "module",
  "engines": {
    "node": ">=18"
  },
  "files": ["bin"],
  "repository": {
    "type": "git",
    "url": "https://github.com/alienplatform/alien.git",
    "directory": "packages/alien-cli-npm"
  },
  "publishConfig": {
    "access": "public"
  },
  "optionalDependencies": {
    "@alienplatform/cli-linux-x64": "npm:@alienplatform/cli@${VERSION}-linux-x64",
    "@alienplatform/cli-linux-arm64": "npm:@alienplatform/cli@${VERSION}-linux-arm64",
    "@alienplatform/cli-darwin-arm64": "npm:@alienplatform/cli@${VERSION}-darwin-arm64",
    "@alienplatform/cli-win32-x64": "npm:@alienplatform/cli@${VERSION}-win32-x64"
  }
}
EOF

(cd "$main_dir" && npm pack)
npm publish "${main_dir}/"*.tgz --tag latest || echo "WARN: main package may have already been published"

echo ""
echo "==> Done! Published @alienplatform/cli@${VERSION}"
