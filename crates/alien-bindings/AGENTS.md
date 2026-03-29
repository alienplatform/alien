# alien-bindings

Binding abstractions for storage, KV, vault, queue, build, container, function, artifact registry, and service accounts. Provides traits, providers, and gRPC transport.

## Architecture

```
traits.rs              — Binding trait definitions (Storage, Kv, Vault, Queue, Build, etc.)
provider.rs            — BindingsProvider: holds all binding instances for a deployment
alien_context.rs       — AlienContext: entry point for application code
providers/             — Platform-specific implementations (AWS, GCP, Azure, Local)
grpc/                  — gRPC server + client for cross-process binding access
```

## Key Types

- `Binding` — Marker trait for all binding types
- `Storage` — Object store with presigned URL support (extends `ObjectStore`)
- `Kv`, `Vault`, `Queue`, `Build`, `Function`, `Container` — Resource binding traits
- `ServiceAccount` — Cloud credential impersonation
- `BindingsProvider` — Holds all bindings for a deployment, keyed by binding name
- `AlienContext` — Application-facing entry point (wraps `BindingsProvider`)

## Feature Flags

- **`grpc`** — gRPC server/client for cross-process binding access (used by alien-runtime)
- **`openapi`** — OpenAPI schema generation support

## Subdirectory AGENTS.md Files

- `src/grpc/AGENTS.md` — Adding new gRPC resource services
- `tests/AGENTS.md` — Writing binding integration tests

## Don't

- Don't expose gRPC/proto types to users — transform to public types from `alien-core`
- Don't add platform-specific logic in traits — put it in `providers/`
- Don't use "agent" — use "deployment"
