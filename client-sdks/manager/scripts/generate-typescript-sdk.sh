#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
sdk_dir="$repo_root/client-sdks/manager/typescript"
speakeasy_bin="${SPEAKEASY_BIN:-speakeasy}"
expected_cli_version="1.680.11"

actual_cli_version="$("$speakeasy_bin" --version | sed -n '1s/^speakeasy version \([^ ]*\).*/\1/p')"
if [[ "$actual_cli_version" != "$expected_cli_version" ]]; then
  echo "Speakeasy CLI $expected_cli_version is required; found ${actual_cli_version:-unknown}." >&2
  exit 1
fi

if [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=all -- "$sdk_dir")" ]]; then
  echo "The Manager TypeScript SDK must be clean before generation." >&2
  exit 1
fi

"$speakeasy_bin" generate sdk \
  --lang typescript \
  --schema "$repo_root/client-sdks/manager/openapi.json" \
  --out "$sdk_dir" \
  --skip-versioning

while IFS= read -r -d '' generated_file; do
  if [[ "$generated_file" == *.md && -f "$repo_root/$generated_file" ]]; then
    perl -0pi -e 's/[ \t]+(?=\n)//g; s/\n+\z/\n/' "$repo_root/$generated_file"
  fi
done < <(git -C "$repo_root" ls-files -mo --exclude-standard -z -- "$sdk_dir")

NODE_OPTIONS=--max-old-space-size=12288 pnpm -C "$sdk_dir" build
node --test "$repo_root/client-sdks/manager/scripts/typescript-sdk.test.mjs"
