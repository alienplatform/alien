# Client SDKs

Auto-generated API clients for alien-manager and the platform.

## Layout

```
client-sdks/
├── manager/           # alien-manager API clients
│   ├── rust/          # crate: alien-manager-api (progenitor)
│   ├── typescript/    # @aliendotdev/manager-api (Speakeasy)
│   ├── openapi.json   # OpenAPI 3.1 spec (source of truth)
│   └── openapi-3.0.json
└── platform/          # platform API clients
    ├── rust/          # crate: alien-platform-api (progenitor)
    ├── typescript/    # @aliendotdev/platform-api (Speakeasy)
    └── openapi.json
```

## Generation

Rust SDKs use [progenitor](https://github.com/oxidecomputer/progenitor) — types generated at build time from `openapi.json`.
TypeScript SDKs use [Speakeasy](https://www.speakeasyapi.dev/) — generated from the same specs.

```bash
pnpm run generate:api            # Regenerate all SDKs from OpenAPI specs
pnpm run generate:api-rust-sdk   # Rust SDKs only
```

## Don't

- Don't edit generated code — regenerate from the OpenAPI spec
- Don't use "agent" in new fields — use "deployment"
- Don't reference platform/, deepstore/, or horizon/ — this is OSS code
