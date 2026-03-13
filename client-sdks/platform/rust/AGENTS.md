# Inspecting Generated Types

This SDK uses [progenitor](https://github.com/oxidecomputer/progenitor) to auto-generate types from `openapi.json` at build time. Generated code is in `$OUT_DIR/codegen.rs` (~18MB).

## Finding the generated file

```bash
# Build to trigger codegen
cargo build -p alien-client-sdk

# Locate codegen.rs (picks the most recently modified)
CODEGEN=$(find target/debug/build -path "*alien-client-sdk*/out/codegen.rs" -type f | xargs ls -t 2>/dev/null | head -1)

# Browse all types
cat $CODEGEN | less

# Find a specific type
grep -A 20 "pub struct DeploymentStatus" $CODEGEN

# Search for fields containing "agent"
grep -i "agent.*:" $CODEGEN | head -20
```

## Regenerating the spec

```bash
# From repo root - generates API openapi.json, copies to SDK, and builds
pnpm run generate:api-rust-sdk

# Or regenerate everything for the API (openapi + all SDKs)
pnpm run generate:api
```

Don't run in sandbox!
