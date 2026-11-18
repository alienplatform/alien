#!/usr/bin/env bash
# Fails when NEXT_RELEASE in the pull-target compatibility table is not newer
# than the latest release tag. A feature gated at NEXT_RELEASE would otherwise
# claim a version that Operators already run without it.
set -euo pipefail

file="crates/alien-manager/src/routes/pull_compat.rs"
next=$(sed -n 's/^pub(crate) const NEXT_RELEASE: &str = "\([0-9.]*\)";/\1/p' "$file")
if [ -z "$next" ]; then
  echo "::error file=$file::NEXT_RELEASE not found"
  exit 1
fi

latest=$(git tag --list "v[0-9]*" --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n1 || true)
if [ -z "$latest" ]; then
  echo "::error::no release tag found; fetch tags before running this check"
  exit 1
fi
latest=${latest#v}

newest=$(printf '%s\n%s\n' "$next" "$latest" | sort -V | tail -n1)
if [ "$next" = "$latest" ] || [ "$newest" != "$next" ]; then
  echo "::error file=$file::NEXT_RELEASE is $next but the latest release is $latest. Set NEXT_RELEASE to the next version and give features that shipped their literal version."
  exit 1
fi
echo "NEXT_RELEASE $next is newer than the latest release $latest"
