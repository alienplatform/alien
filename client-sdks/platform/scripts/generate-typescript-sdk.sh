#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
sdk_dir="$repo_root/client-sdks/platform/typescript"
speakeasy_bin="${SPEAKEASY_BIN:-speakeasy}"
expected_cli_version="1.799.0"

actual_cli_version="$("$speakeasy_bin" --version | sed -n '1s/^speakeasy version \([^ ]*\).*/\1/p')"
if [[ "$actual_cli_version" != "$expected_cli_version" ]]; then
  echo "Speakeasy CLI $expected_cli_version is required; found ${actual_cli_version:-unknown}." >&2
  exit 1
fi

if [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=all -- "$sdk_dir")" ]]; then
  echo "The Platform TypeScript SDK must be clean before generation." >&2
  exit 1
fi

# Model ownership can change without removing the published deep-import API.
# Retain whole previously published files that generation removes. The export
# producer and complete SDK build below must still validate their dependencies.
published_models="$(mktemp -d)"
trap 'rm -rf "$published_models"' EXIT
while IFS= read -r -d '' published_file; do
  mkdir -p "$published_models/$(dirname "$published_file")"
  cp "$repo_root/$published_file" "$published_models/$published_file"
done < <(git -C "$repo_root" ls-files -z -- "$sdk_dir/src/models" "$sdk_dir/docs/models")

"$speakeasy_bin" overlay apply --strict \
  --schema "$repo_root/client-sdks/platform/openapi.json" \
  --overlay "$repo_root/client-sdks/speakeasy-overlay.yaml" \
  --out /dev/null

(
  cd "$sdk_dir"
  "$speakeasy_bin" run \
    --target platform-typescript \
    --skip-upload-spec \
    --skip-compile \
    --skip-versioning
)

while IFS= read -r -d '' published_file; do
  if [[ ! -f "$repo_root/$published_file" ]]; then
    mkdir -p "$repo_root/$(dirname "$published_file")"
    cp "$published_models/$published_file" "$repo_root/$published_file"
    echo "Preserved published model: $published_file"
  fi
done < <(git -C "$repo_root" ls-files -z -- "$sdk_dir/src/models" "$sdk_dir/docs/models")

# Keep unrelated inline-model regrouping out of this contract update. These
# complete published modules need a separate SDK synchronization review.
while IFS= read -r preserved_model; do
  relative_model="client-sdks/platform/typescript/src/models/$preserved_model"
  cp "$published_models/$relative_model" "$repo_root/$relative_model"
  echo "Retained scoped model: $relative_model"
done < "$repo_root/client-sdks/platform/scripts/preserved-model-files.txt"

while IFS= read -r -d '' generated_file; do
  if [[ "$generated_file" == *.md && -f "$repo_root/$generated_file" ]]; then
    perl -0pi -e 's/[ \t]+(?=\n)//g; s/\n+\z/\n/' "$repo_root/$generated_file"
  fi
done < <(git -C "$repo_root" ls-files -mo --exclude-standard -z -- "$sdk_dir")

# Compile the complete SDK after restoring the published compatibility exports.
node "$repo_root/client-sdks/platform/scripts/generate-compatibility-exports.mjs"

NODE_OPTIONS=--max-old-space-size=16384 pnpm -C "$sdk_dir" build
pnpm -C "$repo_root" install --lockfile-only
node --test "$repo_root/client-sdks/platform/scripts/typescript-sdk.test.mjs"
node --test "$repo_root/client-sdks/platform/scripts/dynamic-containers-sdk.test.mjs"
node --test "$repo_root/client-sdks/platform/scripts/signing-sdk.test.mjs"
