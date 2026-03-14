#!/usr/bin/env bash
set -euo pipefail

# Preferred examples test mode.
#
# Goal:
# - Validate that examples can install and test like an external clone that only
#   depends on published @aliendotdev packages.
#
# Behavior:
# 1. Build local CLI binary (used by the testing framework).
# 2. Try examples install in "published package" mode.
# 3. If published packages are unavailable (common before release), fall back to
#    scripts/test-examples-local.sh so CI/local dev can still validate examples.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# Ensure local dev CLI exists (testing framework will auto-discover target/debug/alien)
# Use depot cargo when available (CI with Depot Cache); fall back to plain cargo locally
if command -v depot &>/dev/null; then
  depot cargo build -p alien-cli --bin alien
else
  cargo build -p alien-cli --bin alien
fi

if ! pnpm -C examples install \
  --force \
  --no-frozen-lockfile \
  --config.link-workspace-packages=false \
  --config.prefer-workspace-packages=false; then
  echo "Published @aliendotdev/* packages are not available yet; falling back to local override mode."
  exec "$ROOT_DIR/scripts/test-examples-local.sh"
fi

pnpm -C examples test:projects
