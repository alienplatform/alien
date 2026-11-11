#!/usr/bin/env bash
# Verifies that a linux-gnu native addon loads on common older-glibc base images.
#
#   verify-addon-portability.sh <addon.node> [max-glibc]
#
# 1. The addon must not need a glibc symbol version above max-glibc (default 2.28).
# 2. If docker is available, the addon must load and answer version() inside
#    node:22-bookworm-slim (glibc 2.36) and amazonlinux:2023 (glibc 2.34),
#    both of which are older than the CI runner's own glibc.
set -euo pipefail

addon="${1:?usage: $0 <addon.node> [max-glibc]}"
max="${2:-2.28}"
addon="$(cd "$(dirname "$addon")" && pwd)/$(basename "$addon")"

needed="$(objdump -T "$addon" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -1)"
echo "$addon needs glibc ${needed:-none} (max allowed $max)"
if [[ -n "$needed" ]] && [[ "$(printf '%s\n%s\n' "$needed" "$max" | sort -V | tail -1)" != "$max" ]]; then
  echo "::error::$(basename "$addon") needs GLIBC_$needed, above the allowed GLIBC_$max" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker not available; skipping container load test"
  exit 0
fi

dir="$(dirname "$addon")"
check='const a = require(process.argv[1]); const v = a.version(); if (!v) throw new Error("version() returned empty"); console.log("loaded, version " + v)'
echo "== node:22-bookworm-slim"
docker run --rm -v "$dir:/addon:ro" node:22-bookworm-slim \
  node -e "$check" "/addon/$(basename "$addon")"
echo "== amazonlinux:2023"
docker run --rm -e DNF_OPTS="${DNF_OPTS:-}" -v "$dir:/addon:ro" amazonlinux:2023 \
  bash -c 'dnf $DNF_OPTS install -y -q nodejs22 >/dev/null && node -e "$1" "$2"' _ "$check" "/addon/$(basename "$addon")"
