#!/usr/bin/env bash
set -euo pipefail

# Runs example-project tests against local source code, without making examples
# part of the root workspace and without relying on relative package paths in
# committed example manifests.
#
# Usage:
#   pnpm test:examples
#
# Strategy:
# 1. Build local CLI + local TS packages that examples consume.
# 2. Use the local package overrides committed in examples/pnpm-workspace.yaml.
# 3. Install + run example tests from examples/ as if examples were standalone.
# 4. Always restore the examples lockfile (trap cleanup), even on failures.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"
EXAMPLES_DIR="$ROOT_DIR/examples"
EXAMPLES_LOCK_FILE="$EXAMPLES_DIR/pnpm-lock.yaml"
EXAMPLES_LOCK_FILE_BACKUP="$(mktemp)"

cp "$EXAMPLES_LOCK_FILE" "$EXAMPLES_LOCK_FILE_BACKUP"
cleanup() {
  cp "$EXAMPLES_LOCK_FILE_BACKUP" "$EXAMPLES_LOCK_FILE"
  rm -f "$EXAMPLES_LOCK_FILE_BACKUP"
}
trap cleanup EXIT

# Use depot cargo when available (CI with Depot Cache); fall back to plain cargo locally
if command -v depot &>/dev/null; then
  depot cargo build -p alien-cli --bin alien
else
  cargo build -p alien-cli --bin alien
fi

build_filters=()
if [[ "${ALIEN_EXAMPLES_REUSE_BUILT_PACKAGES:-}" != "true" ]]; then
  build_filters+=(--filter @alienplatform/platform-api)
  build_filters+=(--filter @alienplatform/core)
  build_filters+=(--filter @alienplatform/bindings)
  build_filters+=(--filter @alienplatform/commands)
  build_filters+=(--filter @alienplatform/sdk)
  build_filters+=(--filter @alienplatform/testing)
fi

if (( ${#build_filters[@]} > 0 )); then
  NODE_OPTIONS="${NODE_OPTIONS:-} --max-old-space-size=8192" \
    pnpm -r "${build_filters[@]}" run build
fi

pnpm -C "$EXAMPLES_DIR" install \
  --force \
  --no-frozen-lockfile \
  --config.link-workspace-packages=false \
  --config.prefer-workspace-packages=false

# Ensure the @alienplatform/bindings native addon is available to deployed
# workers. A Worker is a `bun build --compile` binary whose bindings loader
# cannot walk the filesystem to find crates/alien-bindings-node; it resolves the
# addon from ALIEN_BINDINGS_ADDON_PATH (loader step 1). alien-worker-runtime spawns the
# worker without clearing the environment, so exporting the var here propagates
# down to the worker child. Build the dev addon if it is missing (mirrors the
# source-build fallback in packages/package-layout/run.ts).
host_triple() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os/$arch" in
    Darwin/arm64) echo "darwin-arm64" ;;
    Darwin/x86_64) echo "darwin-x64" ;;
    Linux/x86_64) echo "linux-x64-gnu" ;;
    Linux/aarch64) echo "linux-arm64-gnu" ;;
    *) echo "unsupported" ;;
  esac
}

TRIPLE="$(host_triple)"
if [[ "$TRIPLE" == "unsupported" ]]; then
  echo "[addon] WARNING: no native addon mapping for $(uname -s)/$(uname -m); binding ops in workers will fail." >&2
else
  BINDINGS_NODE_DIR="$ROOT_DIR/crates/alien-bindings-node"
  DEV_ADDON="$BINDINGS_NODE_DIR/alien-bindings-node.$TRIPLE.node"
  if [[ ! -e "$DEV_ADDON" ]]; then
    echo "[addon] no dev addon for '$TRIPLE' — building with \`napi build --platform --release\`..."
    pnpm --filter @alienplatform/bindings run build:addon
  else
    echo "[addon] using existing dev addon at $DEV_ADDON"
  fi
  if [[ -e "$DEV_ADDON" ]]; then
    export ALIEN_BINDINGS_ADDON_PATH="$DEV_ADDON"
    echo "[addon] ALIEN_BINDINGS_ADDON_PATH=$ALIEN_BINDINGS_ADDON_PATH"
  else
    echo "[addon] WARNING: addon build did not produce $DEV_ADDON; binding ops in workers will fail." >&2
  fi
fi

pnpm -C "$EXAMPLES_DIR" test:projects
