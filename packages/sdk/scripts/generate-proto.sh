#!/bin/bash

# Generate TypeScript types from proto files using ts-proto + nice-grpc

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
PROTO_DIR="$PACKAGE_DIR/../../crates/alien-bindings/proto"
OUT_DIR="$PACKAGE_DIR/src/generated"

# Clean and create output directory
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

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
