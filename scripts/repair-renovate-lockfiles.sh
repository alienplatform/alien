#!/usr/bin/env bash
# Refresh pnpm lockfiles after Renovate edits package manifests.
# Renovate runs this before it commits.

set -euo pipefail

npm_files_changed() {
  local changed=false
  local path
  while IFS= read -r path; do
    if printf '%s\n' "$path" | grep -Eq '(^|/)(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml)$'; then
      changed=true
    fi
  done
  printf '%s\n' "$changed"
}

base_ref() {
  if ! git rev-parse --verify --quiet origin/main >/dev/null; then
    git fetch --no-tags --depth=1 origin main
  fi
  if git rev-parse --verify --quiet origin/main >/dev/null; then
    printf '%s\n' origin/main
    return
  fi
  if git rev-parse --verify --quiet refs/remotes/origin/HEAD >/dev/null; then
    printf '%s\n' refs/remotes/origin/HEAD
    return
  fi
  echo "Cannot find origin/main to classify dependency changes" >&2
  return 1
}

refresh_lockfiles() {
  pnpm install --lockfile-only --no-frozen-lockfile --ignore-scripts
  pnpm --dir examples install --lockfile-only --no-frozen-lockfile --ignore-scripts
  pnpm --dir examples/github-agent/packages/dashboard install \
    --lockfile-only --no-frozen-lockfile --ignore-scripts
}

main() {
  local root base changed need
  root="$(git rev-parse --show-toplevel)"
  cd "$root"

  base="$(base_ref)"
  changed="$(
    git diff --name-only "$base"
    git ls-files --others --exclude-standard
  )"
  need="$(printf '%s\n' "$changed" | npm_files_changed)"
  if [[ "$need" != true ]]; then
    echo "No npm files changed."
    return 0
  fi
  refresh_lockfiles
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
