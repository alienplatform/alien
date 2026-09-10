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
pnpm run generate:manager-rust-sdk # Regenerate manager Rust SDK inputs
pnpm run generate:manager-api      # Regenerate manager TypeScript SDK
pnpm run generate:platform-api     # Regenerate platform TypeScript SDK from checked-in spec
```

### Platform TypeScript SDK drift

The supported Platform API generation pipeline copies the same contract to
`platform/openapi.json`, `platform/rust/openapi.json`, and
`platform/rust/openapi-3.0.json` before running Speakeasy.

Before including a Platform TypeScript SDK regeneration in a feature PR,
inspect the generated diff from a clean worktree. Keep generated models and
documentation caused by the feature. If Speakeasy also rewrites unrelated
endpoints or models, restore those unrelated files to their pre-generation
state and refresh them in a dedicated SDK-sync PR. Do not manually edit or
selectively patch generated SDK code to force a smaller diff.

## Don't

- Don't edit generated code — regenerate from the OpenAPI spec
- Don't use "agent" in new fields — use "deployment"
- Don't reference private workspace repo paths — this is OSS code
