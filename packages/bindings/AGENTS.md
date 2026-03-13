# @aliendotdev/bindings

TypeScript SDK for Alien bindings. Provides gRPC clients for Storage, KV, Queue, Vault, Build, ArtifactRegistry, Function, and ServiceAccount.

## Commands

```bash
pnpm generate  # Regenerate TypeScript from Proto
pnpm build     # Build the package
pnpm test:ts   # Type check
```

## Type Architecture

```
┌─────────────────┐
│   Rust Types    │  ← Source of truth (alien-core)
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌───────┐  ┌───────────┐
│ Proto │  │ OpenAPI   │
│ (wire)│  │ (schema)  │
└───┬───┘  └─────┬─────┘
    │            │
    ▼            ▼
┌─────────┐  ┌────────────┐
│ts-proto │  │ Kubb/Zod   │
│(internal)│  │(@alien/core)│ ← Public API types
└────┬────┘  └──────┬─────┘
     │              │
     └──────┬───────┘
            ▼
   Transform proto → core types
   in binding implementations
```

**Key principle:** Domain types (`BuildStatus`, `StorageEvent`, etc.) come from `@aliendotdev/core`. Proto types are internal - used only for gRPC wire format. Bindings transform proto ↔ public types.

## Package Structure

```
src/
├── generated/        # Auto-generated from Proto (DO NOT EDIT)
├── bindings/         # Individual binding clients (see bindings/AGENTS.md)
├── context.ts        # AlienContext - main entry point
├── events.ts         # Event handlers (storage, cron, queue)
├── types.ts          # SDK-specific types (options, results)
└── index.ts          # Public exports
```

## Proto Definitions

Protos live in `crates/alien-bindings/proto/`. When modified:

1. Run `pnpm generate` in this package
2. Update binding implementations to match new proto types
3. Verify TypeScript matches Rust behavior (see `src/bindings/AGENTS.md`)

## Adding New Bindings

1. Add proto in `crates/alien-bindings/proto/`
2. Run `pnpm generate`
3. Create binding in `src/bindings/<name>.ts`
4. Add types to `src/types.ts` (public API) 
5. Export from `src/bindings/index.ts` and `src/index.ts`
6. Add accessor method to `AlienContext`

See `src/bindings/AGENTS.md` for implementation guidelines.
