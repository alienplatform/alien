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
cp -R "$sdk_dir/src/models" "$published_models/models"
cp -R "$sdk_dir/docs/models" "$published_models/model-docs"

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

# Copy only missing files; current generated models always win over old output.
node --input-type=module - "$published_models" "$sdk_dir" <<'JS'
import { cp } from "node:fs/promises";
const [, , snapshot, sdk] = process.argv;
const options = { recursive: true, force: false, errorOnExist: false };
await cp(`${snapshot}/models`, `${sdk}/src/models`, options);
await cp(`${snapshot}/model-docs`, `${sdk}/docs/models`, options);
JS

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
