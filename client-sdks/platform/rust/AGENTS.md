# alien-platform-api (Rust SDK)

Auto-generated Rust client for the platform API. Uses [progenitor](https://github.com/oxidecomputer/progenitor) to generate types from `openapi.json` at build time. Generated code is in `$OUT_DIR/codegen.rs`.

## Inspecting Generated Types

```bash
cargo build -p alien-platform-api
CODEGEN=$(find target/debug/build -path "*alien-platform-api*/out/codegen.rs" -type f | xargs ls -t 2>/dev/null | head -1)
grep -A 20 "pub struct DeploymentStatus" $CODEGEN
```

## Regenerating

`alien-platform-api` generates Rust code at Cargo build time from
`openapi.json`. Do not edit generated Rust code manually. Update
`openapi.json` through the platform API generation pipeline, then run
`cargo build -p alien-platform-api` to inspect generated types.

## Don't

- Don't edit generated code — regenerate from the OpenAPI spec
- Don't run generation commands inside a sandbox
- Don't use "agent" in field names — use "deployment"
