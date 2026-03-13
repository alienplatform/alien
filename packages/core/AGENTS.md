# @aliendotdev/core

TypeScript SDK for defining Alien stacks and resources. Types are generated from the Rust schemas.

## Package Structure

```
packages/core/
├── src/
│   ├── generated/          # Auto-generated from Rust (DO NOT EDIT)
│   │   ├── zod/           # Zod schemas for all types
│   │   └── schemas/       # JSON schemas
│   ├── stack.ts           # Stack builder class
│   ├── resource.ts        # Base Resource class
│   ├── function.ts        # Function resource builder
│   ├── storage.ts         # Storage resource builder
│   ├── kv.ts              # KV resource builder
│   ├── queue.ts           # Queue resource builder
│   ├── vault.ts           # Vault resource builder
│   └── ...
├── kubb.config.ts         # Code generation config
└── openapi.json           # Generated OpenAPI spec (from Rust)
```

## Commands

```bash
# Regenerate types from Rust schemas
pnpm generate

# Build the package
pnpm build

# Run tests
pnpm test

# Type check
pnpm test:ts
```

## Code Generation

Types are generated from Rust using a two-step process:

1. **Rust → OpenAPI**: `cargo run --bin schema_exporter` exports `openapi.json`
2. **OpenAPI → TypeScript**: Kubb generates Zod schemas from `openapi.json`

When you modify types in `alien-core` (Rust), run `pnpm generate` to update TypeScript types.

**Important**: Never edit files in `src/generated/` directly - they will be overwritten.

## Adding New Resource Types

1. Define the resource in Rust (`alien-core/src/resources/`)
2. Run `pnpm generate` to get the TypeScript types
3. Create a builder class in `src/<resource>.ts`
4. Export from `src/index.ts`

## Stack Builder

The `Stack` class is a builder for defining resources:

```typescript
import { Stack, Storage, Function } from "@aliendotdev/core"

const storage = new Storage("data").build()
const fn = new Function("processor")
  .code({ type: "image", image: "my-image:latest" })
  .link(storage)
  .build()

export default new Stack("my-stack")
  .add(storage, "frozen")
  .add(fn, "live")
  .add(remoteStorage, "frozen", { remoteAccess: true })  // Enable remote bindings
  .build()
```

## Resource Options

### remoteAccess

When `remoteAccess: true`, binding params are synced to StackState for external access (BYOB use case):

```typescript
.add(resource, "frozen", { remoteAccess: true })
```

Default is `false` to prevent sensitive data in synced state.

