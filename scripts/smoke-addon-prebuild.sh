#!/usr/bin/env bash
#
# Prebuild smoke: prove a published-shape install of the bindings addon works
# with NO build toolchain. Packs the `@alienplatform/core` runtime dep, the
# `@alienplatform/bindings` wrapper, and the per-platform
# `@alienplatform/bindings-<triple>` prebuild into a throwaway consumer, installs
# them from tarballs (npm, no Rust/napi), and runs a real local-KV put/get
# through the prebuilt `.node` (loader step 2, no ALIEN_BINDINGS_ADDON_PATH).
#
# Usage: scripts/smoke-addon-prebuild.sh <triple>
#   e.g. scripts/smoke-addon-prebuild.sh darwin-arm64
#
# Precondition: the built `.node` is already staged into
# packages/bindings/npm/<triple>/ (the release job stages it from the artifact;
# locally, run `napi build --platform --release --target <target>` and copy it).
set -euo pipefail

TRIPLE="${1:?usage: scripts/smoke-addon-prebuild.sh <triple>}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ADDON="packages/bindings/npm/${TRIPLE}/alien-bindings-node.${TRIPLE}.node"
if [ ! -f "$ADDON" ]; then
  echo "[smoke] no staged addon at ${ADDON}; build and stage it first" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The wrapper's dist must exist to be packed.
pnpm --filter @alienplatform/bindings build

# Pack the three packages a published wrapper install needs.
pnpm --filter @alienplatform/core pack --pack-destination "$WORK"
pnpm --filter @alienplatform/bindings pack --pack-destination "$WORK"
( cd "packages/bindings/npm/${TRIPLE}" && npm pack --pack-destination "$WORK" )

# Normalize the version-stamped filenames (order matters: the platform tarball
# also matches the wrapper glob, so move it first).
mv "$WORK"/alienplatform-core-*.tgz "$WORK/core.tgz"
mv "$WORK"/alienplatform-bindings-"${TRIPLE}"-*.tgz "$WORK/platform.tgz"
mv "$WORK"/alienplatform-bindings-*.tgz "$WORK/wrapper.tgz"

cp packages/bindings/scripts/smoke-prebuild.mjs "$WORK/smoke.mjs"

cat > "$WORK/package.json" <<'JSON'
{
  "name": "alien-prebuild-smoke",
  "private": true,
  "type": "module"
}
JSON
# Inject file: deps (the triple is interpolated, so it can't live in the quoted
# heredoc above).
node -e '
  const fs = require("fs");
  const p = process.argv[1];
  const triple = process.argv[2];
  const pkg = JSON.parse(fs.readFileSync(p, "utf8"));
  pkg.dependencies = {
    "@alienplatform/core": "file:./core.tgz",
    "@alienplatform/bindings": "file:./wrapper.tgz",
    [`@alienplatform/bindings-${triple}`]: "file:./platform.tgz",
  };
  fs.writeFileSync(p, JSON.stringify(pkg, null, 2) + "\n");
' "$WORK/package.json" "$TRIPLE"

# Install from tarballs only. No Rust, no napi — the prebuilt .node must carry
# the compiled addon.
( cd "$WORK" && npm install --no-audit --no-fund )

echo "[smoke] node ..."
( cd "$WORK" && node smoke.mjs )

if command -v bun >/dev/null 2>&1; then
  echo "[smoke] bun ..."
  ( cd "$WORK" && bun smoke.mjs )
fi

echo "[smoke] ${TRIPLE}: PASS"
