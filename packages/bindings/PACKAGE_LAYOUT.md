# `@alienplatform/bindings` — package layout contract

> Contract document. The names, subpaths, error codes, and dependency rules below
> are binding for the tasks that implement and enforce them. Implementers may not
> rename anything pinned here. Items whose owner is a later task are marked
> **OPEN (task NN)** and must not be decided in this file.

## Purpose

`@alienplatform/bindings` is the public, direct bindings package for TypeScript: a
thin wrapper over a napi-rs native addon. The Rust crate `alien-bindings` is the
single provider implementation (S3/GCS/Blob, DynamoDB/Firestore/Table Storage,
SQS/Pub-Sub/Service Bus, the vaults, the local providers, and their credential
chains). The TypeScript layer carries only types, the binding factories, and error
mapping; every storage/kv/queue/vault operation runs the Rust provider in-process.

The package exposes exactly the four app-facing binding kinds — **storage**, **kv**,
**queue**, **vault**. It has no JS cloud SDK dependencies and no provider logic of
its own.

## Public surface — all exports from `"."`

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `storage` | function | `storage(name: string): Storage` | Factory. Resolves the `name` binding from the environment. |
| `kv` | function | `kv(name: string): Kv` | Factory. |
| `queue` | function | `queue(name: string): Queue` | Factory. |
| `vault` | function | `vault(name: string): Vault` | Factory. |
| `Storage` | type | resource handle | Instance type returned by `storage()`. Operation method signatures OPEN (task 04) — mirror the Rust `alien-bindings` storage handle. |
| `Kv` | type | resource handle | Instance type returned by `kv()`. Method signatures OPEN (task 04). |
| `Queue` | type | resource handle | Instance type returned by `queue()`. Method signatures OPEN (task 04). |
| `Vault` | type | resource handle | Instance type returned by `vault()`. Method signatures OPEN (task 04). |
| `BindingNotConfiguredError` | error | `defineError({ code: "BINDING_NOT_CONFIGURED", context: { binding, envVar } })` | Thrown on the first operation against an unconfigured binding. `binding` is the binding name; `envVar` is `ALIEN_<NAME>_BINDING`. |
| shared error primitives | re-export | `AlienError`, `defineError` (from `@alienplatform/core`) | Re-exported so consumers handle bindings errors without a direct `@alienplatform/core` import. |

### Intentionally not exposed

The non-app binding kinds are deliberately absent from this package and must not be
added:

- worker invoke
- container
- build
- artifact-registry
- service-account

These live only on the Rust `BindingsProvider` (manager, controllers, tooling,
remote bindings) and are never part of an app-facing surface.

## Exports map

Only two entry points. Every condition carries `types`. No deep imports.

```jsonc
{
  ".": {
    "types": "./dist/index.d.ts",
    "import": "./dist/index.js"
  },
  "./native": {
    "types": "./dist/native.d.ts",
    "import": "./dist/native.js"
  }
}
```

- `"."` — the factories, instance types, and errors above.
- `./native` — static-embed entry for `bun build --compile`. It imports the
  platform `.node` addon through a statically analyzable specifier so the compiler
  can stage it. The subpath name `./native` is pinned here; it is consumed by
  task 13's compile staging and produced by task 04's addon.

## Manifest requirements

- `"type": "module"` (ESM-first).
- `"sideEffects": false`.
- `"exports"` and per-condition `"types"` exactly as above; `"types"` top-level for
  legacy resolvers; declarations shipped.
- `optionalDependencies` — the per-platform prebuild packages
  `@alienplatform/bindings-<platform>`. Initial set: `@alienplatform/bindings-darwin-arm64`,
  `@alienplatform/bindings-darwin-x64`, `@alienplatform/bindings-linux-x64-gnu`,
  `@alienplatform/bindings-linux-arm64-gnu`. This entry describes the **published**
  manifest only: `napi prepublish` injects it at publish time (task 04a) from the
  `napi.targets` config in `crates/alien-bindings-node/package.json`,
  which is the source of truth for the platform list. The workspace source manifest
  (`packages/bindings/package.json`) carries no `optionalDependencies` — adding the
  per-platform packages there would pin unpublished versions and break `pnpm install
  --frozen-lockfile` before task 04a publishes them. The final platform list is OPEN
  (task 04a).
- `description` and `keywords`.
- Support note: Bun ≥ 1.0.23 and Node ≥ 18 (Node-API / napi-rs addon).
- `dependencies`: `@alienplatform/core` (errors) only.

## Dependency boundaries

MUST NOT depend on, import, or reference:

- Cloud SDKs — `@aws-sdk/*`, `@google-cloud/*`, `@azure/*`.
- gRPC packages — `@grpc/grpc-js`, `nice-grpc`.
- Generated binding proto clients.
- The env vars `ALIEN_BINDINGS_GRPC_ADDRESS` or `ALIEN_BINDINGS_MODE`.
- Worker protocol files.
- `@alienplatform/sdk` or [`@alienplatform/commands`](../commands/PACKAGE_LAYOUT.md).

MAY depend on:

- `@alienplatform/core` (error definitions).

## Behavior contract

- Importing the package and constructing any factory (`storage("x")`, `kv("y")`, …)
  requires no deployment and no cloud credentials. Construction never performs I/O.
- The first operation against a binding that has no `ALIEN_<NAME>_BINDING` in the
  environment throws `BindingNotConfiguredError` (code `BINDING_NOT_CONFIGURED`),
  and the error names the missing env var `ALIEN_<NAME>_BINDING` in its context.

## Status

- Package implemented in task 04 (TypeScript wrapper + napi-rs addon over
  `alien-bindings`).
- Per-platform prebuilds and the final `optionalDependencies` list in task 04a.
- This file is the contract; it defines no runtime code.
