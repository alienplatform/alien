#!/usr/bin/env bash
set -euo pipefail

# Runs example-project tests against local source code, without making examples
# part of the root workspace and without relying on relative package paths in
# committed example manifests.
#
# Strategy:
# 1. Build local CLI + local TS packages that examples consume.
# 2. Temporarily inject `pnpm.overrides` into examples/package.json pointing to
#    local file paths in this checkout.
# 3. Install + run example tests from examples/ as if examples were standalone.
# 4. Always restore examples/package.json (trap cleanup), even on failures.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"
EXAMPLES_DIR="$ROOT_DIR/examples"
EXAMPLES_PACKAGE_JSON="$EXAMPLES_DIR/package.json"
EXAMPLES_PACKAGE_JSON_BACKUP="$(mktemp)"

cp "$EXAMPLES_PACKAGE_JSON" "$EXAMPLES_PACKAGE_JSON_BACKUP"
cleanup() {
  cp "$EXAMPLES_PACKAGE_JSON_BACKUP" "$EXAMPLES_PACKAGE_JSON"
  rm -f "$EXAMPLES_PACKAGE_JSON_BACKUP"
}
trap cleanup EXIT

cargo build -p alien-cli --bin alien

pnpm -r \
  --filter @alienplatform/platform-api \
  --filter @alienplatform/core \
  --filter @alienplatform/commands-client \
  --filter @alienplatform/bindings \
  --filter @alienplatform/testing \
  run build

node - "$EXAMPLES_PACKAGE_JSON" "$ROOT_DIR" <<'NODE'
const fs = require("fs");
const path = require("path");

const packageJsonPath = process.argv[2];
const rootDir = process.argv[3];

const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
packageJson.pnpm = packageJson.pnpm || {};
// Local-only override wiring for this test run.
// This avoids requiring workspace links or committed ../ paths in examples.
packageJson.pnpm.overrides = {
  ...(packageJson.pnpm.overrides || {}),
  "@alienplatform/platform-api": `file:${path.join(rootDir, "client-sdks/platform/typescript")}`,
  "@alienplatform/core": `file:${path.join(rootDir, "packages/core")}`,
  "@alienplatform/commands-client": `file:${path.join(rootDir, "packages/commands-client")}`,
  "@alienplatform/bindings": `file:${path.join(rootDir, "packages/bindings")}`,
  "@alienplatform/testing": `file:${path.join(rootDir, "packages/testing")}`
};

fs.writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);
NODE

pnpm -C "$EXAMPLES_DIR" install \
  --force \
  --no-frozen-lockfile \
  --lockfile=false \
  --config.link-workspace-packages=false \
  --config.prefer-workspace-packages=false

pnpm -C "$EXAMPLES_DIR" test:projects
