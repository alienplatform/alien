#!/usr/bin/env bash
# Compile every crate we publish to crates.io on its own, without the workspace-hack crate.
#
# The workspace-hack crate (publish = false) turns on extra features for shared dependencies,
# so a crate can compile in the workspace while missing a feature in its own manifest. The
# release removes workspace-hack before `cargo publish --no-verify`, so nothing else would
# catch it before users do. This runs the same `cargo hakari remove-deps` and then checks each
# published crate alone (`-p`, so no other crate's features are unified in).
#
# The crate list is read from the publish step of .github/workflows/release.yml.
# This rewrites Cargo.toml files and Cargo.lock: run it only in a throwaway checkout (CI).
set -euo pipefail

cd "$(dirname "$0")/.."

# From the line with `crate-publish-order.py` to the line that ends the crate list with `)`.
roots=$(sed -n '/scripts\/crate-publish-order\.py/,/)$/p' .github/workflows/release.yml |
  sed '1d' | tr -d ' \\)')
if [ -z "$roots" ]; then
  echo "::error::could not read the published crate list from .github/workflows/release.yml"
  exit 1
fi

publish_order=$(cargo metadata --no-deps --format-version 1 |
  python3 scripts/crate-publish-order.py $roots)

cargo hakari remove-deps --yes

# Each crate is checked with its default features and with none: a user can depend on either,
# and the workspace build only ever sees the union. (A check per feature would cost much more.)
metadata=$(cargo metadata --no-deps --format-version 1)
failed=()
check() {
  local crate=$1
  shift
  echo "::group::cargo check -p $crate $*"
  if ! cargo check -p "$crate" "$@"; then
    failed+=("$crate $*")
  fi
  echo "::endgroup::"
}
for crate in $publish_order; do
  check "$crate"
  if [ "$(jq -r --arg crate "$crate" \
    '.packages[] | select(.name == $crate) | .features | has("default")' <<<"$metadata")" = true ]; then
    check "$crate" --no-default-features
  fi
done

if [ "${#failed[@]}" -gt 0 ]; then
  echo "::error::these published crates do not compile without workspace-hack:"
  printf '::error::  %s\n' "${failed[@]}"
  exit 1
fi
