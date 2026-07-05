#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MODE=${1:-scan}

FORBIDDEN_REGEX='ALIEN_BASE_PLATFORM|ALIENCFG|alien\.dev/build|managed-by[[:space:]]*[:=][[:space:]]*["'\'']?alien|fluent-bit|fluentbit|fluent/fluent-bit|/var/lib/alien-operator|HeaderValue::from_static\("alien-operator"\)|name = "alien-operator"|about = "Alien Operator'

scan_path() {
  local path=$1
  if rg -n --pcre2 "$FORBIDDEN_REGEX" "$path"; then
    echo "white-label leak check failed for $path" >&2
    return 1
  fi
}

if [[ "$MODE" == "--expect-fail-fixture" ]]; then
  tmp=$(mktemp)
  printf 'env:\n- name: ALIEN_BASE_PLATFORM\n' >"$tmp"
  if ! scan_path "$tmp" >/dev/null 2>&1; then
    rm -f "$tmp"
    exit 0
  fi
  rm -f "$tmp"
  echo "expected deliberate leak fixture to fail the scanner" >&2
  exit 1
fi

scan_path "$ROOT/crates/alien-helm/src/generator.rs"
scan_path "$ROOT/crates/alien-operator/src"
scan_path "$ROOT/crates/alien-core/src/embedded_config.rs"
scan_path "$ROOT/crates/alien-core/src/runtime_environment.rs"
scan_path "$ROOT/crates/alien-bindings/src/providers/build/kubernetes.rs"
scan_path "$ROOT/crates/alien-bindings/src/providers/vault/kubernetes_secret.rs"
scan_path "$ROOT/docker/Dockerfile.alien-operator"

echo "white-label leak check passed"
