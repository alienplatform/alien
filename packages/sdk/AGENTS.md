# @alienplatform/sdk

TypeScript facade for Alien Worker apps. The root export provides Worker
handler registration (`command`, storage/queue/cron handlers, and `waitUntil`)
and re-exports the app-facing `storage`, `kv`, `queue`, and `vault` factories
from `@alienplatform/bindings`.

The command sender and the app-owned Container/Daemon pull receiver live in
`@alienplatform/commands`. Do not restore the deleted
`@alienplatform/sdk/commands` subpath or add `CommandsClient` to this package.

## Commands

```bash
pnpm generate  # Regenerate TypeScript from the Worker proto
pnpm build     # Build the package
pnpm test:ts   # Type check
```

## Package Boundaries

```text
Worker app
   |
   +-- @alienplatform/sdk          handler registration + binding facade
   |    +-- @alienplatform/bindings  in-process Storage/KV/Queue/Vault
   |    `-- ./worker-runtime         Worker protocol bootstrap and clients
   |
   `-- @alienplatform/commands     command sender + Container/Daemon receiver
```

Worker protocol code and gRPC imports stay behind `./worker-runtime`. The root
facade is safe to import for handler registration and bindings. Binding
implementation code belongs in `@alienplatform/bindings`.

## Package Structure

```text
src/
+-- index.ts          # Public Worker facade
+-- native.ts         # Embedded-addon bridge for compiled Workers
`-- worker-runtime/   # Worker protocol bootstrap and generated clients
```

## Proto Definitions

Worker protos live in `crates/alien-worker-protocol/proto/`. When modified:

1. Run `pnpm generate` in this package.
2. Update Worker runtime implementations to match the new proto types.
3. Verify TypeScript behavior matches the Rust Worker protocol.

## Adding App-Facing Bindings

Implement the binding in `packages/bindings`, then re-export only the
app-facing factory and instance type from `src/index.ts`. Do not add gRPC
binding clients, `AlienContext`, infrastructure-management bindings, or command
sender APIs to this facade.
