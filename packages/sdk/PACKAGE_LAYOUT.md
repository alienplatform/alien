# `@alienplatform/sdk` — package layout contract

> Contract document. The names, subpaths, error codes, and dependency rules below
> are binding for the tasks that implement and enforce them. Implementers may not
> rename anything pinned here. Items whose owner is a later task are marked
> **OPEN (task NN)** and must not be decided in this file.

## Purpose

`@alienplatform/sdk` stays published as the ergonomic facade for Worker apps. It
provides the Worker handler APIs and re-exports the app-facing binding factories
from [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md), so a Worker author
installs one package. Worker protocol dependencies (nice-grpc, generated Worker
protocol clients) are confined to the `./worker-runtime` subpath.

The direct command surface moves out of this package: `CommandsClient` lives in
[`@alienplatform/commands`](../commands/PACKAGE_LAYOUT.md), and direct bindings live
in [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md).

### Current state (truthful baseline)

- Current `exports` are `.` and `./commands`.
- Worker/gRPC protocol code lives in `src/{channel,events,commands,wait-until,grpc-utils}.ts`,
  `src/bindings/*`, and `src/generated/*`.
- `src/commands/` (the `CommandsClient`) is already pure `fetch` + HTTP.

## Public surface — from `"."` (facade)

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `command` | function | `command(name, handler): void` | Register a Worker command handler. |
| `onStorageEvent` | function | Worker storage-event handler registrar | Signature owned by task 03. |
| `onCronEvent` | function | Worker cron-event handler registrar | Signature owned by task 03. |
| `onQueueMessage` | function | Worker queue-message handler registrar | Signature owned by task 03. |
| `waitUntil` | function | `waitUntil(promise): void` | Extend Worker task lifetime past the response. |
| handler types | type | `StorageEvent`, `StorageEventType`, `CronEvent`, `QueueMessage`, `QueueMessageEvent`, `ScheduledEvent` | The event/handler types for the APIs above. |
| `storage`, `kv`, `queue`, `vault` | function | re-export from `@alienplatform/bindings` | Facade re-export of the binding factories. |
| `Storage`, `Kv`, `Queue`, `Vault` | type | re-export from `@alienplatform/bindings` | Facade re-export of the instance types. |
| error re-exports | error | from `@alienplatform/bindings` and `@alienplatform/core` | Includes `BindingNotConfiguredError`; `AlienError`. |

### Deleted from the current root surface

Recorded here as contract; execution is tasks 03/17. These names must be gone from
`"."` after the split:

- `worker()`, `build()`, `artifactRegistry()`, `serviceAccount()` — non-app binding
  factories.
- `AlienContext` and its `AlienContext.fromEnv()` binding-gRPC entry.
- Binding classes for non-app kinds: `Build`, `ArtifactRegistry`, `WorkerBinding`,
  `ServiceAccount`.
- gRPC-era binding errors that the direct bindings package replaces:
  `GrpcConnectionError`, `GrpcCallError`, `BindingNotFoundError` (superseded by
  `BINDING_NOT_CONFIGURED` in `@alienplatform/bindings`).
- `getPostgresConnection` / `PostgresConnection` — destination is **OPEN (task 03)**;
  this file does not decide where Postgres connection resolution moves.

## Subpaths

### `./worker-runtime`

The **only** location for `nice-grpc` and the generated Worker protocol clients. It
exports:

- `runWorker` — the Worker bootstrap contract. The name `runWorker` is pinned here;
  its signature is owned by task 03.
- Worker protocol client internals consumed by generated Worker bootstraps.

Generated Worker bootstraps import this exact subpath (tasks 03/13 depend on it).

### `./commands` — DELETED

The old `./commands` subpath is removed. Its `CommandsClient` functionality moves to
[`@alienplatform/commands`](../commands/PACKAGE_LAYOUT.md) (task 08). The package
must not continue to export `./commands`.

## Exports map

`"."` and `./worker-runtime` only. Every condition carries `types`. No deep imports.

```jsonc
{
  ".": {
    "types": "./dist/index.d.ts",
    "import": "./dist/index.js"
  },
  "./worker-runtime": {
    "types": "./dist/worker-runtime/index.d.ts",
    "import": "./dist/worker-runtime/index.js"
  }
}
```

## Manifest requirements

- `"type": "module"` (ESM-first).
- `"sideEffects": false`.
- `"exports"` and per-condition `"types"` exactly as above; `"types"` top-level for
  legacy resolvers; declarations shipped.
- `description` and `keywords` (updated to drop the standalone-commands framing).
- Support note: Bun and Node ≥ 18.
- `dependencies`: `@alienplatform/bindings` and `@alienplatform/core`.

## Dependency boundaries

- gRPC and Worker-protocol imports are forbidden anywhere outside the
  `./worker-runtime` source directory.
- No generated binding-service proto clients anywhere in the package. Only Worker
  protocol proto is permitted, and only under `./worker-runtime`.
- MUST NOT still export `./commands`.
- MUST NOT still ship generated binding-service proto clients.
- Depends on [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md) and
  `@alienplatform/core`.

## Behavior contract

- The facade re-exports binding factories from `@alienplatform/bindings`; importing
  and constructing them requires no deployment and no credentials (see the
  [bindings contract](../bindings/PACKAGE_LAYOUT.md#behavior-contract)).
- Worker handler registration (`command`, `onStorageEvent`, `onCronEvent`,
  `onQueueMessage`, `waitUntil`) is protocol-only at the facade; the Worker runtime
  wiring lives behind `./worker-runtime`.

## Status

- The split is executed in task 03 (with cleanup of deleted names in task 17).
- This file is the contract; it defines no runtime code.
