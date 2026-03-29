# alien-platform-api (Rust SDK)

Auto-generated Rust client for the platform API. Uses [progenitor](https://github.com/oxidecomputer/progenitor) to generate types from `openapi.json` at build time. Generated code is in `$OUT_DIR/codegen.rs`.

## Inspecting Generated Types

```bash
cargo build -p alien-platform-api
CODEGEN=$(find target/debug/build -path "*alien-platform-api*/out/codegen.rs" -type f | xargs ls -t 2>/dev/null | head -1)
grep -A 20 "pub struct DeploymentStatus" $CODEGEN
```

## Regenerating

```bash
pnpm run generate:api-rust-sdk   # openapi → SDK
pnpm run generate:api            # regenerate everything
```

## Don't

- Don't edit generated code — regenerate from the OpenAPI spec
- Don't run generation commands inside a sandbox
- Don't use "agent" in field names — use "deployment"
