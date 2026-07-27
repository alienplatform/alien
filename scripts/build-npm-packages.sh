#!/usr/bin/env bash
#
# Build, qualify, or publish @alienplatform/cli npm packages.
#
# Uses the Codex-style single-package pattern: platform variants are published
# as @alienplatform/cli@{VERSION}-{platform} and the main package at
# @alienplatform/cli@{VERSION} has optionalDependencies using npm aliases.
#
# Required env vars:
#   VERSION          - Release version (e.g., 1.3.2)
#   MODE              - qualify, publish, or publish-existing (default: publish)
#   OUTPUT_DIR        - qualified tarball directory (required outside publish)
#   NODE_AUTH_TOKEN   - npm auth token (publish modes only)
#
# Expected artifacts layout (from GitHub Actions download-artifact):
#   ./artifacts/binaries-x86_64-unknown-linux-musl/{alien,alien-deploy,...}
#   ./artifacts/binaries-aarch64-unknown-linux-musl/{alien,alien-deploy,...}
#   ./artifacts/binaries-aarch64-apple-darwin/{alien,alien-deploy,...}
#   ./artifacts/binaries-x86_64-pc-windows-msvc/{alien.exe,alien-deploy.exe,...}

set -euo pipefail

: "${VERSION:?VERSION is required}"
MODE="${MODE:-publish}"
if [[ "$MODE" != "qualify" ]]; then
  : "${NODE_AUTH_TOKEN:?NODE_AUTH_TOKEN is required}"
fi
if [[ "$MODE" != "publish" ]]; then
  : "${OUTPUT_DIR:?OUTPUT_DIR is required}"
  mkdir -p "$OUTPUT_DIR"
  OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

publish_if_missing() {
  local package_spec="$1"
  local tarball="$2"
  local tag="$3"

  local integrity
  integrity="sha512-$(openssl dgst -sha512 -binary "$tarball" | openssl base64 -A)"
  local published
  published="$(npm view "$package_spec" dist.integrity 2>/dev/null || true)"
  if [[ -n "$published" ]]; then
    if [[ "$published" != "$integrity" ]]; then
      echo "error: $package_spec exists with different contents" >&2
      exit 1
    fi
    echo "Skipping $package_spec; qualified contents are already published"
    return
  fi

  npm publish "$tarball" --tag "$tag"
}

qualify_or_publish() {
  local package_spec="$1"
  local tarball="$2"
  local tag="$3"

  if [[ "$MODE" == "qualify" ]]; then
    cp "$tarball" "$OUTPUT_DIR/"
  else
    publish_if_missing "$package_spec" "$tarball" "$tag"
  fi
}

if [[ "$MODE" == "publish-existing" ]]; then
  for npm_suffix in linux-x64 linux-arm64 darwin-arm64 win32-x64; do
    qualify_or_publish \
      "@alienplatform/cli@${VERSION}-${npm_suffix}" \
      "${OUTPUT_DIR}/alienplatform-cli-${VERSION}-${npm_suffix}.tgz" \
      "$npm_suffix"
  done
  qualify_or_publish \
    "@alienplatform/cli@${VERSION}" \
    "${OUTPUT_DIR}/alienplatform-cli-${VERSION}.tgz" \
    latest
  exit 0
fi

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
  qualify_or_publish "@alienplatform/cli@${VERSION}-${npm_suffix}" "${pkg_dir}/"*.tgz "${npm_suffix}"
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
qualify_or_publish "@alienplatform/cli@${VERSION}" "${main_dir}/"*.tgz latest

echo ""
echo "==> Done! ${MODE} @alienplatform/cli@${VERSION}"
