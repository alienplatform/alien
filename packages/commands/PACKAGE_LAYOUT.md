# `@alienplatform/commands` — package layout contract

> Contract document. The names, subpaths, error codes, and dependency rules below
> are binding for the tasks that implement and enforce them. Implementers may not
> rename anything pinned here. Items whose owner is a later task are marked
> **OPEN (task NN)** and must not be decided in this file.

## Purpose

`@alienplatform/commands` is the public command package for TypeScript: the command
**sender** and the non-Worker (pull) **receiver**. It is pure TypeScript over
`fetch` — protocol types and HTTP only. It carries no native code and no provider
logic. Large payloads are exchanged as storage-backed presigned HTTP transfers, so
the package never links `@alienplatform/bindings`.

The Rust crate `alien-commands` is the protocol twin (base protocol types plus
server and receiver roles); this package is the TypeScript sender + receiver over
the same wire protocol.

## Public surface — all exports from `"."`

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `CommandsClient` | class | `new CommandsClient({ managerUrl, deploymentId, token })` | Sender. Constructor options `{ managerUrl: string; deploymentId: string; token: string }`. |
| `CommandsClient#target` | method | `.target(name: string)` | Scopes the client to a target command-capable resource. Return type OPEN (task 08). |
| `CommandsClient#invoke` | method | `.invoke(name: string, input, options?)` | Invokes a command and resolves to its response. `input`/`options`/response types OPEN (task 08). |
| `createCommandReceiver` | function | `createCommandReceiver(): CommandReceiver` | Constructs the pull receiver from environment configuration. |
| `CommandReceiver` | type | receiver handle | `.handle(name: string, handler)` registers a handler; `.run(): Promise<void>` leases and dispatches. Handler context `{ input, signal, deadline, commandId, attempt }`. Concrete field types OPEN (task 08). |
| `CommandReceiverConfigInvalidError` | error | `defineError({ code: "COMMAND_RECEIVER_CONFIG_INVALID", context: { … } })` | Thrown when receiver env config is empty/invalid. Context names the offending variable — any of the five in the receiver environment contract below. **DECIDED(09).** |
| sender error types | error | migrated from the current `@alienplatform/sdk/commands` error set | The final exported sender-error set is OPEN (task 08); migration source is the existing `@alienplatform/sdk/commands` errors. |
| shared error primitives | re-export | `AlienError`, `defineError` (from `@alienplatform/core`) | Re-exported for consumer error handling. |

### Receiver environment contract

All five variables below are required; an empty, missing, or invalid value
fails fast with `COMMAND_RECEIVER_CONFIG_INVALID`, naming the specific
variable. **DECIDED(09)** — the Rust receiver (`alien_commands::Receiver`)
reads the identical names; task 08's TypeScript receiver must match them
exactly so the two are behavior-identical twins.

| Env var | Requirement | DECIDED(09) |
|---|---|---|
| `ALIEN_COMMANDS_URL` | Base URL of the command server. | Pinned since this file's creation. |
| `ALIEN_COMMANDS_TOKEN` | Bearer token for outbound requests. | Reused from the existing worker command-polling token variable. |
| `ALIEN_DEPLOYMENT_ID` | Deployment the leased commands belong to. | Reused; lease requests require it. |
| `ALIEN_COMMANDS_TARGET_RESOURCE_ID` | This resource's id within the deployment's stack. | Reused from the existing target-resource variable. |
| `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` | `container` \| `daemon`, lowercase; any other value (e.g. `worker`) is rejected. | New. Lease requests need a typed target and a receiver must not guess it — the worker runtime hardcodes `worker`; a Container/Daemon receiver gets its type injected. |

## Exports map

Single entry point. Every condition carries `types`. No deep imports.

```jsonc
{
  ".": {
    "types": "./dist/index.d.ts",
    "import": "./dist/index.js"
  }
}
```

## Manifest requirements

- `"type": "module"` (ESM-first).
- `"sideEffects": false`.
- `"exports"` and per-condition `"types"` exactly as above; `"types"` top-level for
  legacy resolvers; declarations shipped.
- `description` and `keywords`.
- Zero runtime dependencies beyond `@alienplatform/core` (the transport is the
  platform `fetch`).
- Support note: Bun and Node ≥ 18 (global `fetch`).

## Dependency boundaries

MUST NOT depend on, import, or reference:

- Worker app protocol files.
- gRPC packages — `@grpc/grpc-js`, `nice-grpc`.
- [`@alienplatform/bindings`](../bindings/PACKAGE_LAYOUT.md) (large payloads use
  presigned HTTP, not the bindings addon).

MAY depend on:

- `@alienplatform/core` (error definitions).

## Behavior contract

- Importing the package and constructing `CommandsClient` requires no deployment and
  no cloud credentials.
- `createCommandReceiver()` reads the receiver environment contract above. An
  empty/invalid value on any of the five variables throws
  `CommandReceiverConfigInvalidError` (code `COMMAND_RECEIVER_CONFIG_INVALID`)
  naming that variable.
- The receiver leases only commands addressed to its own target resource, over
  outbound HTTPS; it never sees another target's commands.
- **DECIDED(09).** Execution budget: each command runs under
  `min(envelope.deadline, lease_expires_at)` — there is no lease-renewal call,
  so the lease expiry always bounds the budget. On expiry the handler is
  aborted, its cancellation signal (`ctx.signal`) fires, and the receiver
  submits a `HANDLER_TIMEOUT` error response.
- **DECIDED(09).** Error codes the receiver submits: `UNKNOWN_COMMAND` (no
  handler registered for the leased command name), `HANDLER_ERROR` (the
  handler threw/rejected, including a response-serialization failure), and
  `HANDLER_TIMEOUT` (budget expiry, above). A params-decode failure is
  submitted under the decode error's own code, not a receiver-specific one.
- **DECIDED(09).** Delivery is at-least-once: a lease that expires without a
  submitted response is redelivered. The handler context's `attempt` field
  carries the delivery attempt starting at 1 (greater than 1 means
  redelivery); handlers must tolerate running more than once for the same
  command.
- **DECIDED(09).** Shutdown/drain, worded precisely: once a shutdown signal
  is raised, the receiver stops *starting* new lease polls (checked at the
  top of each poll loop iteration) — a poll already in flight when shutdown
  is raised still completes, and any leases it returns are dispatched and
  drained like the rest of the batch. Every in-flight command finishes
  within its own budget before the receiver's run loop returns; no command
  created after shutdown is ever leased.
- **DECIDED(09).** Lease parameters: poll every 5 seconds, `maxLeases` 10,
  `leaseSeconds` 60 per poll — identical to the existing worker-runtime
  command-polling defaults.
- **DECIDED(09).** `ctx.input` is the decoded command param bytes: the same
  bytes the params envelope carries after decode, prior to any
  handler-side parsing. The concrete TypeScript context field types remain
  OPEN (task 08); only this byte-for-byte encoding identity between the
  Rust and TypeScript receivers is pinned here.
- **DECIDED(09).** A successful handler response body is the JSON encoding
  of the handler's return value (`JSON.stringify`-equivalent), submitted as
  the command's success response payload.

## Status

- Package implemented in task 08 (TypeScript — pure protocol sender + pull
  receiver).
- Rust twin `alien-commands` in task 09.
- This file is the contract; it defines no runtime code.
