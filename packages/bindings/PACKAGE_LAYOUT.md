# `@alienplatform/bindings` — package layout contract

> Contract document. The names, subpaths, error codes, and dependency rules below
> define the package's public compatibility contract. Changes to them require a
> corresponding package API and release review.

## Purpose

`@alienplatform/bindings` is the public, direct bindings package for TypeScript: a
thin wrapper over a napi-rs native addon. The Rust crate `alien-bindings` is the
single provider implementation (S3/GCS/Blob, DynamoDB/Firestore/Table Storage,
SQS/Pub-Sub/Service Bus, the vaults, the local providers, and their credential
chains). The TypeScript layer carries only types, the binding factories, and error
mapping; every storage/kv/queue/vault/container operation runs the Rust provider in-process.

The package exposes five app-facing binding kinds — **storage**, **kv**,
**queue**, **vault**, and linked **container** discovery. It has no JS cloud SDK dependencies and no provider logic of
its own.

## Public surface — all exports from `"."`

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `storage` | function | `storage(name: string): Storage` | Factory. Resolves the `name` binding from the environment. |
| `kv` | function | `kv(name: string): Kv` | Factory. |
| `queue` | function | `queue(name: string): Queue` | Factory. |
| `vault` | function | `vault(name: string): Vault` | Factory. |
| `Bindings` | class | `Bindings.forRemoteDeployment(options): Promise<Bindings>` | Trusted-backend entry point for remote Storage access to an existing deployment. |
| `container` | function | `container(name: string): Container` | Lazy, read-only linked-service discovery. |
| `Storage` | type | resource handle | Instance type returned by `storage()`. Operation method signatures mirror the Rust `alien-bindings` storage handle. |
| `RemoteStorage` | type | `Pick<Storage, "get" \| "put" \| "delete" \| "list" \| "head">` | Narrow handle returned by `Bindings.storage()`. |
| `Kv` | type | resource handle | Instance type returned by `kv()`. Method signatures mirror the Rust handle. |
| `Queue` | type | resource handle | Instance type returned by `queue()`. Method signatures mirror the Rust handle. |
| `Container` | type | resource handle | `getInternalUrl()` and nullable `getPublicUrl()`. |
| `Vault` | type | resource handle | Instance type returned by `vault()`. Method signatures mirror the Rust handle. |
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

Remote `Bindings` deliberately exposes no `kv`, `queue`, or `vault` methods.
Its Storage handle deliberately excludes copy and signed URLs.

Remote access is a trusted-backend API. Its Alien API token and short-lived
provider credentials must never be shipped to a browser or other untrusted
client. v0 accepts only Running, Frozen, remote-enabled S3, GCS, and Azure Blob
resources. The customer setup grants the deployment management identity the
five public object operations on each opted-in bucket or container.

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
  `alien build`'s compile staging and backed by the napi addon in
  `crates/alien-bindings-node`.

## Manifest requirements

- `"type": "module"` (ESM-first).
- `"sideEffects": false`.
- `"exports"` and per-condition `"types"` exactly as above; `"types"` top-level for
  legacy resolvers; declarations shipped.
- `optionalDependencies` — the per-platform prebuild packages
  `@alienplatform/bindings-<platform>`. Shipped set: `@alienplatform/bindings-darwin-arm64`,
  `@alienplatform/bindings-darwin-x64`, `@alienplatform/bindings-linux-x64-gnu`,
  `@alienplatform/bindings-linux-arm64-gnu`. This entry describes the **published**
  manifest only: `scripts/inject-optional-deps.mjs` injects it at publish time,
  pinning each entry to the wrapper's own release version, from its own `TRIPLES`
  const — chosen over `napi prepublish`, which rewrites/regenerates more than
  needed. `TRIPLES` must mirror the `napi.targets` config in
  `crates/alien-bindings-node/package.json`, which is the source of truth for the
  release build matrix (the per-triple legs in the release workflow). The
  workspace source manifest (`packages/bindings/package.json`) carries no
  `optionalDependencies` — adding the per-platform packages there would pin
  unpublished versions and break `pnpm install --frozen-lockfile` before release.
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

- Importing the package and constructing any environment factory (`storage("x")`, `kv("y")`, …)
  requires no deployment and no cloud credentials. Construction never performs I/O.
- The first operation against a binding that has no `ALIEN_<NAME>_BINDING` in the
  environment throws `BindingNotConfiguredError` (code `BINDING_NOT_CONFIGURED`),
  and the error names the missing env var `ALIEN_<NAME>_BINDING` in its context.
- `Bindings.forRemoteDeployment` forwards only the deployment ID, token, and
  optional Alien API base URL. This async constructor loads the native addon and
  discovers the assigned manager. It retains one native bindings handle,
  resolves and caches each named Storage handle lazily, refreshes provider
  credentials without replacing that handle, periodically rediscovers manager
  assignment, and translates native errors to `AlienError`. Rotating the Alien
  API token requires constructing a new `Bindings` value.

## Status

- Package implemented: TypeScript wrapper + napi-rs addon over
  `alien-bindings` (`crates/alien-bindings-node`).
- Per-platform prebuilds and the final `optionalDependencies` list are
  produced by the release pipeline (`.github/workflows/release.yml`).
- This file is the contract; it defines no runtime code.
