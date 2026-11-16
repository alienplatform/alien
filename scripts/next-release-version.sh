#!/usr/bin/env bash
# Prints the next stable version as `version=X.Y.Z` on its last line.
#
# The next version is the latest `v*` tag plus one patch. If main's workspace
# version is newer than that tag, a stable publish is still pending, and the
# script refuses: finish that publish instead. A publish that stopped part way
# can leave a version that can never be published cleanly (some registries
# already hold it, no tag). Setting ABANDON_VERSION to exactly that untagged
# version bumps past it instead.
#
# Run from the repository root. Env: ABANDON_VERSION (optional).
set -euo pipefail
ABANDON_VERSION=${ABANDON_VERSION:-}

LATEST_TAG=$(git tag --list "v[0-9]*" --sort=-v:refname | head -n1)
if [ -z "$LATEST_TAG" ]; then
  LATEST_TAG="v0.0.0"
fi
CURRENT_VERSION=${LATEST_TAG#v}
echo "Latest tag: $LATEST_TAG"

WORKSPACE_VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
if [ "$WORKSPACE_VERSION" != "$CURRENT_VERSION" ]; then
  # A stable publish that stopped part way leaves its version on main
  # without a tag, and some of its packages already on registries, so
  # that version can never be published cleanly. Naming it here bumps
  # past it. Only the exact untagged version on main is accepted.
  if [ -z "$ABANDON_VERSION" ]; then
    echo "::error::main already contains unreleased version v${WORKSPACE_VERSION}; run the stable publish action instead"
    exit 1
  fi
  if [ "$ABANDON_VERSION" != "$WORKSPACE_VERSION" ]; then
    echo "::error::abandon_unreleased_version is ${ABANDON_VERSION}, but main's unreleased version is ${WORKSPACE_VERSION}"
    exit 1
  fi
  if git rev-parse --verify --quiet "refs/tags/v${WORKSPACE_VERSION}" >/dev/null; then
    echo "::error::v${WORKSPACE_VERSION} is tagged, so it is released and cannot be abandoned"
    exit 1
  fi
  if [ "$(printf '%s\n%s\n' "$CURRENT_VERSION" "$WORKSPACE_VERSION" | sort -V | tail -n1)" != "$WORKSPACE_VERSION" ]; then
    echo "::error::v${WORKSPACE_VERSION} is not newer than the latest tag ${LATEST_TAG}"
    exit 1
  fi
  echo "Abandoning unreleased v${WORKSPACE_VERSION}; bumping from it"
  CURRENT_VERSION=$WORKSPACE_VERSION
elif [ -n "$ABANDON_VERSION" ]; then
  echo "::error::abandon_unreleased_version is set, but main has no unreleased version"
  exit 1
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"
VERSION="${MAJOR}.${MINOR}.$((PATCH + 1))"

echo "Bump type: patch"
echo "New version: v${VERSION}"
echo "version=$VERSION"
