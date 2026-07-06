#!/bin/bash

# Generate TypeScript types from proto files using ts-proto + nice-grpc

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
# The worker app protocol (Control + WaitUntil) is the only proto surface that
# still has source .proto files; it lives in alien-worker-protocol. The remaining
# generated clients (storage, kv, queue, ...) have no source protos in-tree, so we
# regenerate over the worker-protocol outputs in place rather than wiping OUT_DIR.
PROTO_DIR="$PACKAGE_DIR/../../crates/alien-worker-protocol/proto"
OUT_DIR="$PACKAGE_DIR/src/generated"

mkdir -p "$OUT_DIR"

# Remove only the outputs we're about to regenerate (control + wait_until, plus
# the google/protobuf well-known types they pull in). We deliberately don't
# `rm -rf "$OUT_DIR"`: the other binding clients (storage, kv, queue, ...) have
# no source .proto files in-tree, so wiping the whole directory would delete
# generated code this script can't reproduce.
rm -f "$OUT_DIR/control.ts" "$OUT_DIR/wait_until.ts"
rm -f "$OUT_DIR/google/protobuf/timestamp.ts" "$OUT_DIR/google/protobuf/duration.ts"

# Find protoc - prefer system installation
if command -v protoc &> /dev/null; then
  PROTOC="protoc"
else
  echo "Error: protoc not found. Install via 'brew install protobuf' on macOS."
  exit 1
fi

# Find ts-proto plugin
TS_PROTO_PLUGIN="$PACKAGE_DIR/node_modules/.bin/protoc-gen-ts_proto"

if [ ! -f "$TS_PROTO_PLUGIN" ]; then
  echo "Error: protoc-gen-ts_proto not found. Run 'pnpm install' first."
  exit 1
fi

echo "Using protoc: $PROTOC"
echo "Using ts-proto plugin: $TS_PROTO_PLUGIN"
echo "Generating TypeScript types from proto files..."

# Generate types for all proto files
# Using nice-grpc options as per the playbook
"$PROTOC" \
  --plugin="protoc-gen-ts_proto=$TS_PROTO_PLUGIN" \
  --ts_proto_out="$OUT_DIR" \
  --ts_proto_opt=outputServices=nice-grpc,outputServices=generic-definitions,useExactTypes=false,esModuleInterop=true,importSuffix=.js \
  --proto_path="$PROTO_DIR" \
  "$PROTO_DIR"/*.proto

echo "Generated TypeScript types in $OUT_DIR"
echo "Done!"
