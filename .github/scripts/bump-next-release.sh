#!/usr/bin/env bash
# Release prepare step: after releasing version X, the next release is X with
# the patch plus one. Sets NEXT_RELEASE in the pull-target compatibility table
# to that, so the version PR carries it. Usage: bump-next-release.sh X.Y.Z
# (run from the repository root). Features always carry a literal version, so
# nothing else in the file changes.
set -euo pipefail

version="${1:?usage: bump-next-release.sh X.Y.Z}"
if ! grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' <<< "$version"; then
  echo "::error::not a release version: $version"
  exit 1
fi
file="crates/alien-manager/src/routes/pull_compat.rs"
IFS=. read -r major minor patch <<< "$version"
next="$major.$minor.$((patch + 1))"

sed -i -E "s/^(pub\(crate\) const NEXT_RELEASE: &str = \")[0-9.]+(\";)$/\1${next}\2/" "$file"
grep -qx "pub(crate) const NEXT_RELEASE: &str = \"${next}\";" "$file" || {
  echo "::error file=$file::NEXT_RELEASE line not found"
  exit 1
}
echo "NEXT_RELEASE is now $next"
