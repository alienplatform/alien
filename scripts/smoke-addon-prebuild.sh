#!/usr/bin/env bash
#
# Prove a publish-equivalent @alienplatform/bindings install resolves the
# correct native optionalDependency without invoking a compiler toolchain.
#
# Usage: scripts/smoke-addon-prebuild.sh <triple> [--execute]
#
# Every target gets npm install-resolution verification. --execute additionally
# runs both npm and Bun installs plus a real local-KV put/get, and must therefore
# only be used when <triple> is native to the runner.
set -euo pipefail

TRIPLE="${1:?usage: scripts/smoke-addon-prebuild.sh <triple> [--execute]}"
MODE="${2:-}"
if [ -n "$MODE" ] && [ "$MODE" != "--execute" ]; then
  echo "usage: scripts/smoke-addon-prebuild.sh <triple> [--execute]" >&2
  exit 2
fi

case "$TRIPLE" in
  darwin-arm64) TARGET_OS=darwin; TARGET_CPU=arm64; TARGET_LIBC= ;;
  darwin-x64) TARGET_OS=darwin; TARGET_CPU=x64; TARGET_LIBC= ;;
  linux-x64-gnu) TARGET_OS=linux; TARGET_CPU=x64; TARGET_LIBC=glibc ;;
  linux-arm64-gnu) TARGET_OS=linux; TARGET_CPU=arm64; TARGET_LIBC=glibc ;;
  *) echo "unsupported addon triple: $TRIPLE" >&2; exit 2 ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ADDON="packages/bindings/npm/${TRIPLE}/alien-bindings-node.${TRIPLE}.node"
if [ ! -f "$ADDON" ]; then
  echo "[smoke] no staged addon at ${ADDON}; build and stage it first" >&2
  exit 1
fi

WORK="$(mktemp -d)"
REGISTRY_PID=""
export npm_config_cache="$WORK/npm-cache"
cleanup() {
  if [ -n "$REGISTRY_PID" ]; then kill "$REGISTRY_PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

WRAPPER_VERSION="$(node -p 'require("./packages/bindings/package.json").version')"
CORE_VERSION="$(node -p 'require("./packages/core/package.json").version')"
PLATFORM_VERSION="$(node -p "require('./packages/bindings/npm/${TRIPLE}/package.json').version")"
if [ "$CORE_VERSION" != "$WRAPPER_VERSION" ] || [ "$PLATFORM_VERSION" != "$WRAPPER_VERSION" ]; then
  echo "[smoke] release versions are not locked: core=${CORE_VERSION}, wrapper=${WRAPPER_VERSION}, ${TRIPLE}=${PLATFORM_VERSION}" >&2
  exit 1
fi

mkdir -p "$WORK/packs" "$WORK/wrapper-unpacked"

# Pack the release artifacts. pnpm rewrites workspace protocol dependencies to
# their publishable semver form. Repack the wrapper after injecting the same
# optionalDependencies used immediately before the real publish.
pnpm --filter @alienplatform/core pack --pack-destination "$WORK/packs"
pnpm --filter @alienplatform/bindings pack --pack-destination "$WORK/packs"
( cd "packages/bindings/npm/${TRIPLE}" && npm pack --pack-destination "$WORK/packs" )

mv "$WORK"/packs/alienplatform-core-*.tgz "$WORK/core.tgz"
mv "$WORK"/packs/alienplatform-bindings-"${TRIPLE}"-*.tgz "$WORK/platform.tgz"
mv "$WORK"/packs/alienplatform-bindings-*.tgz "$WORK/wrapper-source.tgz"

tar -xzf "$WORK/wrapper-source.tgz" -C "$WORK/wrapper-unpacked"
node packages/bindings/scripts/inject-optional-deps.mjs \
  "$WORK/wrapper-unpacked/package/package.json"
npm pack --ignore-scripts --pack-destination "$WORK/packs" \
  "$WORK/wrapper-unpacked/package"
mv "$WORK"/packs/alienplatform-bindings-*.tgz "$WORK/wrapper.tgz"

for artifact in core platform wrapper; do
  tar -xOf "$WORK/${artifact}.tgz" package/package.json > "$WORK/${artifact}-package.json"
done

# The JavaScript template literals are intentionally protected from the shell.
# shellcheck disable=SC2016
node -e '
  const fs = require("fs");
  const root = process.argv[1];
  const entries = ["core", "platform", "wrapper"].map(name => ({
    manifest: `${root}/${name}-package.json`,
    tarball: `${root}/${name}.tgz`,
  }));
  fs.writeFileSync(`${root}/registry.json`, JSON.stringify(entries));
' "$WORK"

node packages/bindings/scripts/serve-smoke-registry.mjs \
  "$WORK/registry.json" "$WORK/registry-ready" > "$WORK/registry.log" 2>&1 &
REGISTRY_PID=$!
for _ in $(seq 1 50); do
  if [ -s "$WORK/registry-ready" ]; then break; fi
  if ! kill -0 "$REGISTRY_PID" 2>/dev/null; then
    cat "$WORK/registry.log" >&2
    exit 1
  fi
  sleep 0.1
done
if [ ! -s "$WORK/registry-ready" ]; then
  echo "[smoke] local package registry did not start" >&2
  cat "$WORK/registry.log" >&2
  exit 1
fi
REGISTRY="$(cat "$WORK/registry-ready")"

prepare_consumer() {
  local dir="$1"
  mkdir -p "$dir"
  # The JavaScript template literals are intentionally protected from the shell.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("fs");
    const [path, version] = process.argv.slice(1);
    fs.writeFileSync(path, `${JSON.stringify({
      name: "alien-prebuild-smoke",
      private: true,
      type: "module",
      dependencies: { "@alienplatform/bindings": version },
    }, null, 2)}\n`);
  ' "$dir/package.json" "$WRAPPER_VERSION"
  printf '@alienplatform:registry=%s\n' "$REGISTRY" > "$dir/.npmrc"
  cp packages/bindings/scripts/smoke-prebuild.mjs "$dir/smoke.mjs"
}

run_native_smoke() {
  local runtime="$1"
  local data_dir="$WORK/${runtime}-kv"
  local binding_json
  mkdir -p "$data_dir"
  binding_json="$(node -e '
    process.stdout.write(JSON.stringify({
      service: "local-kv",
      dataDir: process.argv[1],
    }));
  ' "$data_dir")"
  ALIEN_DEPLOYMENT_TYPE=local ALIEN_CACHE_BINDING="$binding_json" \
    "$runtime" smoke.mjs
}

prepare_consumer "$WORK/npm-consumer"
(
  cd "$WORK/npm-consumer"
  npm_config_os="$TARGET_OS" npm_config_cpu="$TARGET_CPU" \
    npm_config_libc="$TARGET_LIBC" \
    npm install --ignore-scripts --no-audit --no-fund
  node "$REPO_ROOT/packages/bindings/scripts/verify-prebuild-install.mjs" \
    "$TRIPLE" "$WRAPPER_VERSION"
  if [ "$MODE" = "--execute" ]; then
    echo "[smoke] npm + node real KV behavior ..."
    run_native_smoke node
  fi
)

if [ "$MODE" = "--execute" ]; then
  if ! command -v bun >/dev/null 2>&1; then
    echo "[smoke] Bun is required for native prebuild execution" >&2
    exit 1
  fi
  prepare_consumer "$WORK/bun-consumer"
  (
    cd "$WORK/bun-consumer"
    bun install --ignore-scripts
    bun "$REPO_ROOT/packages/bindings/scripts/verify-prebuild-install.mjs" \
      "$TRIPLE" "$WRAPPER_VERSION"
    echo "[smoke] Bun real KV behavior ..."
    run_native_smoke bun
  )
fi

echo "[smoke] ${TRIPLE}: PASS"
