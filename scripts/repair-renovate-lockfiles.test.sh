#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR
# shellcheck source=repair-renovate-lockfiles.sh
source "${script_dir}/repair-renovate-lockfiles.sh"

expect_npm() {
  local expected="$1"
  shift
  local actual
  actual="$(printf '%s\n' "$@" | npm_files_changed)"
  if [[ "$actual" != "$expected" ]]; then
    printf 'classification mismatch for: %s\nexpected: %s\nactual: %s\n' "$*" "$expected" "$actual" >&2
    exit 1
  fi
}

expect_npm true "package.json"
expect_npm true "examples/pnpm-lock.yaml" "README.md"
expect_npm true "examples/github-agent/packages/dashboard/pnpm-workspace.yaml"
expect_npm false "Cargo.toml"
expect_npm false "Cargo.lock"
expect_npm false "apps/api/package.json.bak"
expect_npm true "package.json" "crates/foo/Cargo.toml"

echo "ok"
