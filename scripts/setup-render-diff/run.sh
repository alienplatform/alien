#!/usr/bin/env bash
# Differential render of unprotected setup templates.
#
# Renders every fixture stack in fixtures/ as CloudFormation, Terraform (AWS, GCP, Azure) and
# Helm, with the renderer in renderer/, once on a base revision (default origin/main) and once
# on the working tree, then diffs the two. The diff must equal expected.diff exactly: an
# unprotected stack renders byte-identical to the base except for the changes listed there.
#
# Intended today (base = the merge base with main): every AWS management role denies writes
# under /signed/ (`OnlySetupChangesSigningSettings`); Terraform AWS repacks its management
# statements among its policies to fit it.
#
# Usage: scripts/setup-render-diff/run.sh [base-ref]
#   SETUP_RENDER_DIFF_BASE_TARGET  cargo target dir for the base build (default: a temp dir)
#   SETUP_RENDER_DIFF_OUT          keep the rendered trees and the diff here
#   SETUP_RENDER_DIFF_BASE_FEATURES  renderer features for the base (`pins` if it has SetupPins)
#   UPDATE=1                       write the actual diff to expected.diff
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
base=${1:-origin/main}
work=$(mktemp -d)
out=${SETUP_RENDER_DIFF_OUT:-$work/out}
mkdir -p "$out/base" "$out/head"

cleanup() {
  if [ -f "$work/Cargo.toml" ]; then
    cp "$work/Cargo.toml" "$repo/Cargo.toml"
    cp "$work/Cargo.lock" "$repo/Cargo.lock"
  fi
  rm -rf "$repo/crates/setup-render-diff"
  git -C "$repo" worktree remove --force "$work/base" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# Add the renderer to the checkout's workspace and run it there.
render() {
  local checkout=$1 dest=$2 features=$3
  rm -rf "$checkout/crates/setup-render-diff"
  cp -r "$here/renderer" "$checkout/crates/setup-render-diff"
  sed -i 's|^members = \[|members = [\n  "crates/setup-render-diff",|' "$checkout/Cargo.toml"
  (cd "$checkout" && cargo run --quiet -p setup-render-diff --features "$features" -- "$here/fixtures" "$dest")
}

git -C "$repo" worktree add --quiet --detach "$work/base" "$base"
CARGO_TARGET_DIR=${SETUP_RENDER_DIFF_BASE_TARGET:-$work/target} render "$work/base" "$out/base" "${SETUP_RENDER_DIFF_BASE_FEATURES:-}"

cp "$repo/Cargo.toml" "$work/Cargo.toml"
cp "$repo/Cargo.lock" "$work/Cargo.lock"
render "$repo" "$out/head" pins

# Without the timestamps diff puts on file headers, so equal renders give equal diffs.
(cd "$out" && diff -ru base head || true) |
  sed -E 's/^(---|\+\+\+) ([^\t]+)\t.*/\1 \2/' > "$out/actual.diff"
if [ "${UPDATE:-}" = 1 ]; then
  cp "$out/actual.diff" "$here/expected.diff"
  echo "wrote $here/expected.diff"
elif diff -u "$here/expected.diff" "$out/actual.diff"; then
  echo "unprotected setup renders differ from $base only as expected.diff lists"
else
  echo "unprotected setup renders differ from $base beyond expected.diff (diff of diffs above)" >&2
  exit 1
fi
