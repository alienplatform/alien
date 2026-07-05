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
| `CommandReceiverConfigInvalidError` | error | `defineError({ code: "COMMAND_RECEIVER_CONFIG_INVALID", context: { … } })` | Thrown when receiver env config is empty/invalid. Context names `ALIEN_COMMANDS_URL`. |
| sender error types | error | migrated from the current `@alienplatform/sdk/commands` error set | The final exported sender-error set is OPEN (task 08); migration source is the existing `@alienplatform/sdk/commands` errors. |
| shared error primitives | re-export | `AlienError`, `defineError` (from `@alienplatform/core`) | Re-exported for consumer error handling. |

### Receiver environment contract

- `ALIEN_COMMANDS_URL` — location of the command server. **Pinned now.** An empty
  or invalid receiver environment fails with `COMMAND_RECEIVER_CONFIG_INVALID`,
  which names `ALIEN_COMMANDS_URL`.
- The receiver also reads a token variable and a target-resource variable. Their
  final env-var names are OPEN (task 08) and will be pinned in this file when
  task 08 lands; their roles are: the outbound-HTTPS auth token, and the target
  command-capable resource the receiver leases for.

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
- `createCommandReceiver()` reads `ALIEN_COMMANDS_URL` from the environment. An
  empty/invalid receiver environment throws `CommandReceiverConfigInvalidError`
  (code `COMMAND_RECEIVER_CONFIG_INVALID`) naming `ALIEN_COMMANDS_URL`.
- The receiver leases only commands addressed to its own target resource, over
  outbound HTTPS; it never sees another target's commands.

## Status

- Package implemented in task 08 (TypeScript — pure protocol sender + pull
  receiver).
- Rust twin `alien-commands` in task 09.
- This file is the contract; it defines no runtime code.
