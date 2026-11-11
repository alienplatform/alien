#!/usr/bin/env bash
# Builds the linux-gnu Node addon against glibc 2.28 so it loads on Debian 12,
# Amazon Linux 2023 and RHEL 9 (a plain build links the CI runner's newer glibc).
#
#   build-linux-addon.sh <rust-target> [cargo args...]
#
# Needs zig and cargo-zigbuild (the setup-zigbuild action). Writes
# crates/alien-bindings-node/alien-bindings-node.<npm-triple>.node, the file
# `napi build --platform` would produce; the crate has no generated index.js.
set -euo pipefail

target="${1:?usage: $0 <x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu>}"
case "$target" in
  x86_64-unknown-linux-gnu) triple=linux-x64-gnu ;;
  aarch64-unknown-linux-gnu) triple=linux-arm64-gnu ;;
  *) echo "unsupported target $target" >&2; exit 1 ;;
esac
shift

root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$root"
cargo zigbuild --release --locked -p alien-bindings-node --lib --target "$target.2.28" "$@"
cp "${CARGO_TARGET_DIR:-target}/$target/release/libalien_bindings_node.so" \
  "crates/alien-bindings-node/alien-bindings-node.$triple.node"
