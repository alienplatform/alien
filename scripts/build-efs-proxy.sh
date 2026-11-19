#!/usr/bin/env bash
# Builds the TLS proxy the sandbox agent starts to attach S3 Files, from AWS's efs-utils at a
# pinned commit, as a static binary for the machine this runs on, and copies it with the
# certificate authority bundle it checks the mount target against.
#
#   scripts/build-efs-proxy.sh <output-directory>
#
# The only change to upstream is docker/efs-proxy-no-fips.patch: the proxy's `s2n-tls` dependency
# loses its `fips` feature, which pulls a FIPS build of the crypto library that does not build for
# every target we ship. Nothing else is altered, and the proxy is built without its AWS SDK
# features (no direct S3 reads, no CloudWatch), which a mount through the agent does not use.
set -euo pipefail

EFS_UTILS_REPOSITORY="https://github.com/aws/efs-utils.git"
EFS_UTILS_COMMIT="2bca638e6ea0929f59d5f73940c575d548197b72"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
patch_file="$here/../docker/efs-proxy-no-fips.patch"
output="${1:?usage: build-efs-proxy.sh <output-directory>}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git clone --quiet "$EFS_UTILS_REPOSITORY" "$work/efs-utils"
git -C "$work/efs-utils" checkout --quiet "$EFS_UTILS_COMMIT"
git -C "$work/efs-utils" apply --check "$patch_file"
git -C "$work/efs-utils" apply "$patch_file"

host="$(rustc -vV | sed -n 's/^host: //p')"
# `--config` replaces any rustflags a caller's cargo configuration sets; the proxy must be static
# so one binary runs on every base image.
(cd "$work/efs-utils/src" && CARGO_TARGET_DIR="$work/target" cargo build --release --locked \
  -p efs-proxy --no-default-features --target "$host" \
  --config 'build.rustflags=["-C","target-feature=+crt-static"]')

install -m 0755 "$work/target/$host/release/efs-proxy" "$output/efs-proxy"
install -m 0444 "$work/efs-utils/dist/efs-utils.crt" "$output/efs-utils.crt"
# A dynamically linked proxy would fail on a base image without the matching libc.
if ldd "$output/efs-proxy" 2>&1 | grep -qv "not a dynamic executable\|statically linked"; then
  if ldd "$output/efs-proxy" 2>&1 | grep -q "=>"; then
    echo "efs-proxy is dynamically linked" >&2
    exit 1
  fi
fi
echo "efs-proxy built from efs-utils $EFS_UTILS_COMMIT for $host"
