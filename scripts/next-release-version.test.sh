#!/usr/bin/env bash
# Tests for next-release-version.sh. Run: scripts/next-release-version.test.sh
set -uo pipefail
script="$(cd "$(dirname "$0")" && pwd)/next-release-version.sh"
failures=0

# check <description> <workspace version> <tags> <abandon> <expected: version or "refused">
check() {
  local description=$1 workspace=$2 tags=$3 abandon=$4 expected=$5 repo output status actual
  repo=$(mktemp -d)
  (
    cd "$repo" &&
      git init -q &&
      printf '[workspace.package]\nversion = "%s"\n\n[workspace.dependencies]\n' "$workspace" > Cargo.toml &&
      git add Cargo.toml &&
      git -c user.name=t -c user.email=t@example.com commit -qm init &&
      for tag in $tags; do git tag "$tag"; done
  )
  output=$(cd "$repo" && ABANDON_VERSION=$abandon "$script" 2>&1)
  status=$?
  rm -rf "$repo"
  if [ "$status" -eq 0 ]; then
    actual=$(sed -n 's/^version=//p' <<< "$output")
  else
    actual=refused
  fi
  if [ "$actual" = "$expected" ]; then
    echo "ok: $description"
  else
    echo "FAIL: $description: expected $expected, got $actual"
    echo "$output" | sed 's/^/    /'
    failures=$((failures + 1))
  fi
}

check "released main bumps one patch" 3.3.65 "v3.3.64 v3.3.65" "" 3.3.66
check "no tags starts from 0.0.0" 0.0.0 "" "" 0.0.1
check "an unreleased version on main is refused" 3.3.66 "v3.3.65" "" refused
check "the exact untagged version is abandoned" 3.3.66 "v3.3.65" 3.3.66 3.3.67
check "a different version is refused" 3.3.66 "v3.3.65" 3.3.65 refused
check "a tagged version is not abandoned" 3.3.66 "v3.3.65 v3.3.66" 3.3.66 refused
check "abandoning when nothing is unreleased is refused" 3.3.65 "v3.3.65" 3.3.65 refused
check "a version older than the latest tag is refused" 3.3.64 "v3.3.65" 3.3.64 refused

if [ "$failures" -ne 0 ]; then
  echo "$failures test(s) failed"
  exit 1
fi
