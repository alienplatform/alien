#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
sdk_dir="$repo_root/client-sdks/platform/typescript"
schema="$repo_root/client-sdks/platform/openapi.json"
overlay="$repo_root/client-sdks/platform/speakeasy-overlay.yaml"
temp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$temp_dir"
}
trap cleanup EXIT

speakeasy overlay apply \
  --strict \
  --schema "$schema" \
  --overlay "$overlay" \
  --out "$temp_dir/openapi.json"

speakeasy generate sdk \
  --lang typescript \
  --schema "$temp_dir/openapi.json" \
  --out "$sdk_dir"

NODE_OPTIONS=--max-old-space-size=12288 pnpm -C "$sdk_dir" build
