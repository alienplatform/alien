#!/usr/bin/env bash
# Guards the pull-target compatibility table. The release prepare step bumps
# NEXT_RELEASE (bump-next-release.sh); this catches hand edits. Fails when
#  - NEXT_RELEASE is not newer than the latest release tag (a feature gated at
#    it would claim a version Operators already run without it), or
#  - a feature gate names NEXT_RELEASE instead of a literal version (the bump
#    at release would move the gate to the following release), or
#  - a feature gate is newer than NEXT_RELEASE.
# Holds both at the release commit (latest tag X-1, NEXT_RELEASE X+1) and after
# the tag (latest X, NEXT_RELEASE X+1).
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
if grep -nE '^    NEXT_RELEASE,$' "$file"; then
  echo "::error file=$file::A feature gate uses NEXT_RELEASE; write the literal version ($next) so the release bump does not move it."
  exit 1
fi
for since in $(grep -E '^    "[0-9]+\.[0-9]+\.[0-9]+",$' "$file" | tr -d ' ",'); do
  if [ "$(printf '%s\n%s\n' "$since" "$next" | sort -V | tail -n1)" != "$next" ]; then
    echo "::error file=$file::A feature gate names $since, newer than NEXT_RELEASE $next."
    exit 1
  fi
done
echo "NEXT_RELEASE $next is newer than the latest release $latest"
