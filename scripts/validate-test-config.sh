#!/usr/bin/env bash
# Validate that the generated alien-manager.toml works end-to-end.
#
# Chain: Terraform (infra/test/) -> gen-env-test.sh -> alien-manager.toml -> manager boots.
#
# Usage: ./scripts/validate-test-config.sh
# Requires: alien-manager binary on PATH (or cargo-built), .env.test, alien-manager.test.toml
set -euo pipefail

TOML_PATH="alien-manager.test.toml"
ENV_PATH=".env.test"
HEALTH_URL="http://localhost:9090/health"
TIMEOUT_SECS=10

# ── Pre-flight checks ───────────────────────────────────────────────────────

if [ ! -f "$TOML_PATH" ]; then
  echo "ERROR: $TOML_PATH not found. Run ./scripts/gen-env-test.sh first." >&2
  exit 1
fi

if [ ! -f "$ENV_PATH" ]; then
  echo "ERROR: $ENV_PATH not found. Run ./scripts/gen-env-test.sh first." >&2
  exit 1
fi

# Source .env.test so AWS credentials are available for the manager process.
set -a
# shellcheck disable=SC1090
source "$ENV_PATH"
set +a

# Export AWS credentials the manager expects (management account).
export AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID"
export AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY"
export AWS_REGION="$AWS_MANAGEMENT_REGION"

# ── Start alien-manager ─────────────────────────────────────────────────────

echo "Starting alien-manager with $TOML_PATH ..."

# Prefer a pre-built binary; fall back to cargo run.
if command -v alien-manager &>/dev/null; then
  alien-manager --config "$TOML_PATH" &
else
  cargo run --bin alien-manager -- --config "$TOML_PATH" &
fi

MANAGER_PID=$!

# Ensure we clean up on exit regardless of success/failure.
cleanup() {
  if kill -0 "$MANAGER_PID" 2>/dev/null; then
    echo "Stopping alien-manager (PID $MANAGER_PID) ..."
    kill "$MANAGER_PID" 2>/dev/null || true
    wait "$MANAGER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# ── Wait for /health ────────────────────────────────────────────────────────

echo "Waiting for $HEALTH_URL (timeout: ${TIMEOUT_SECS}s) ..."

elapsed=0
while [ "$elapsed" -lt "$TIMEOUT_SECS" ]; do
  if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
    echo "Health check passed."
    exit 0
  fi

  # Check the process hasn't crashed.
  if ! kill -0 "$MANAGER_PID" 2>/dev/null; then
    echo "ERROR: alien-manager exited before becoming healthy." >&2
    wait "$MANAGER_PID" 2>/dev/null
    exit 1
  fi

  sleep 1
  elapsed=$((elapsed + 1))
done

echo "ERROR: alien-manager did not become healthy within ${TIMEOUT_SECS}s." >&2
exit 1
