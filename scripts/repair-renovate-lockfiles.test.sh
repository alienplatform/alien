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

calls="$(mktemp)"
(
  pnpm() {
    printf '%s\n' "$*" >>"$calls"
  }
  refresh_lockfiles
)
expected="$(
  cat <<'EOF'
install --lockfile-only --no-frozen-lockfile --ignore-scripts
--dir examples install --lockfile-only --no-frozen-lockfile --ignore-scripts
--dir examples/github-agent/packages/dashboard install --lockfile-only --no-frozen-lockfile --ignore-scripts
EOF
)"
actual="$(cat "$calls")"
rm -f "$calls"
if [[ "$actual" != "$expected" ]]; then
  printf 'lockfile refresh commands mismatch\nexpected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
  exit 1
fi

noop="$(mktemp -d)"
git init -q -b main "$noop"
git -C "$noop" -c user.email=test@example.com -c user.name=test commit -q --allow-empty -m init
git -C "$noop" remote add origin "$noop"
git -C "$noop" update-ref refs/remotes/origin/main HEAD
noop_msg="$(cd "$noop" && main)"
rm -rf "$noop"
if [[ "$noop_msg" != "No npm files changed." ]]; then
  echo "unchanged tree should skip lockfile repair" >&2
  exit 1
fi

echo "ok"
