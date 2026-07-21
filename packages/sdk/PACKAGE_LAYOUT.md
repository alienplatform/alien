# `@alienplatform/sdk` — package layout contract

> Current contract. The names, subpaths, and dependency rules below describe the
> package as shipped and are enforced by package-layout tests.

## Purpose

`@alienplatform/sdk` is the ergonomic facade for Worker apps. It
provides the Worker handler APIs and re-exports the app-facing binding factories
from [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md), so a Worker author
installs one package. Worker protocol dependencies (nice-grpc, generated Worker
protocol clients) are confined to the `./worker-runtime` subpath.

The direct command surface is separate: `CommandsClient` lives in
[`@alienplatform/commands`](../commands/PACKAGE_LAYOUT.md), and direct bindings live
in [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md).

### Current layout

- Public exports are `.`, `./worker-runtime`, and `./native`.
- Worker/gRPC protocol code lives under `src/worker-runtime/`.
- The root facade re-exports app-facing binding factories; their implementation
  lives in `@alienplatform/bindings`.
- `CommandsClient` and the Container/Daemon pull receiver live in
  `@alienplatform/commands`, not in this package.

## Public surface — from `"."` (facade)

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `command` | function | `command(name, handler)` or `command(name, schema, handler)` | Register a Worker command handler. Schema-less input is `unknown`; Standard Schema validation infers the handler input. |
| `onStorageEvent` | function | Worker storage-event handler registrar | Signature pinned by `src/worker-runtime`. |
| `onCronEvent` | function | Worker cron-event handler registrar | Signature pinned by `src/worker-runtime`. |
| `onQueueMessage` | function | Worker queue-message handler registrar | Signature pinned by `src/worker-runtime`. |
| `waitUntil` | function | `waitUntil(promise): void` | Extend Worker task lifetime past the response. |
| handler types | type | `StorageEvent`, `StorageEventType`, `CronEvent`, `QueueMessage`, `QueueMessageEvent`, `ScheduledEvent` | The event/handler types for the APIs above. |
| `storage`, `kv`, `queue`, `vault`, `container` | function | re-export from `@alienplatform/bindings` | Facade re-export of the binding factories. |
| `Storage`, `Kv`, `Queue`, `Vault` | type | re-export from `@alienplatform/bindings` | Facade re-export of the instance types. |
| error re-exports | error | from `@alienplatform/bindings` and `@alienplatform/core` | Includes `BindingNotConfiguredError`; `AlienError`. |

### Removed from the root surface

These names must remain absent from `"."`:

- `worker()`, `build()`, `artifactRegistry()`, `serviceAccount()` — non-app binding
  factories.
- `AlienContext` and its `AlienContext.fromEnv()` binding-gRPC entry.
- Binding classes for non-app kinds: `Build`, `ArtifactRegistry`, `WorkerBinding`,
  `ServiceAccount`.
- gRPC-era binding errors that the direct bindings package replaces:
  `GrpcConnectionError`, `GrpcCallError`, `BindingNotFoundError` (superseded by
  `BINDING_NOT_CONFIGURED` in `@alienplatform/bindings`).
- `getPostgresConnection` / `PostgresConnection`; applications resolve their own
  Postgres connections.

## Subpaths

### `./worker-runtime`

The **only** location for `nice-grpc` and the generated Worker protocol clients. It
exports:

- `runWorker` — the Worker bootstrap contract. The name `runWorker` is pinned here;
  its signature is pinned by `src/worker-runtime`.
- Worker protocol client internals consumed by generated Worker bootstraps.

Generated Worker bootstraps import this exact subpath.

### `./native`

The embedded-addon bridge for compiled Workers. It re-exports
`installEmbeddedAddon` from `@alienplatform/bindings/native` so generated Worker
bootstraps can register the staged native addon without deep-importing a
transitive dependency.

### `./commands` — DELETED

The old `./commands` subpath is removed. Its `CommandsClient` functionality lives in
[`@alienplatform/commands`](../commands/PACKAGE_LAYOUT.md). The package
must not continue to export `./commands`.

## Exports map

`"."`, `./worker-runtime`, and `./native` only. Every condition carries `types`.
No deep imports.

```jsonc
{
  ".": {
    "types": "./dist/index.d.ts",
    "import": "./dist/index.js"
  },
  "./worker-runtime": {
    "types": "./dist/worker-runtime/index.d.ts",
    "import": "./dist/worker-runtime/index.js"
  },
  "./native": {
    "types": "./dist/native.d.ts",
    "import": "./dist/native.js"
  }
}
```

## Manifest requirements

- `"type": "module"` (ESM-first).
- `"sideEffects": false`.
- `"exports"` and per-condition `"types"` exactly as above; declarations shipped.
- `description` and `keywords` describe the Worker facade.
- Runtime dependencies include `@alienplatform/bindings`, `@alienplatform/core`,
  and the Worker protocol libraries used only by `./worker-runtime`.

## Dependency boundaries

- gRPC and Worker-protocol imports are forbidden anywhere outside the
  `./worker-runtime` source directory.
- No generated binding-service proto clients anywhere in the package. Only Worker
  protocol proto is permitted, and only under `./worker-runtime`.
- MUST NOT export `./commands`.
- MUST NOT ship generated binding-service proto clients.
- Depends on [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md) and
  `@alienplatform/core`.

## Behavior contract

- The facade re-exports binding factories from `@alienplatform/bindings`; importing
  and constructing them requires no deployment and no credentials (see the
  [bindings contract](../bindings/PACKAGE_LAYOUT.md#behavior-contract)).
- Worker handler registration (`command`, `onStorageEvent`, `onCronEvent`,
  `onQueueMessage`, `waitUntil`) is protocol-only at the facade; the Worker runtime
  wiring lives behind `./worker-runtime`.

## Handler and runtime details

- `command(name, handler): void`, `command(name, schema, handler): void`, `onStorageEvent(bucket, handler,
  options?): () => void`, `onCronEvent(schedule, handler): () => void`,
  `onQueueMessage(queue, handler): () => void`, `waitUntil(promise): void`. These
  are protocol-only registrars in `src/worker-runtime/registry.ts` (no gRPC); the
  facade root re-exports them. State is held on `globalThis` under a
  `Symbol.for` key so the facade bundle and the `./worker-runtime` bundle share
  one registry.
- `runWorker(app?: unknown): Promise<void>` lives in `./worker-runtime`.
  `app` is the user module's default export (an object with a `fetch` method for
  HTTP apps) or `undefined`. `runWorker` connects over
  `ALIEN_WORKER_GRPC_ADDRESS`, serves the HTTP handler and registers its port,
  registers the app's handlers, then runs the task-dispatch loop. `waitUntil`
  tasks are reported as they are registered; graceful drain on process shutdown
  is not currently part of this contract.

## Status

- The package-layout consumer test checks packed manifests and contents directly.
- This file is the contract; it defines no runtime code.
