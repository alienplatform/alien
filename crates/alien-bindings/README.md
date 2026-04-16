# alien-bindings

Platform-agnostic binding abstractions for Alien applications. Defines traits for storage, KV, vault, queue, build, container, function, artifact registry, and service accounts, with per-platform provider implementations.

## Architecture

- `traits.rs` — Binding trait definitions (`Storage`, `Kv`, `Vault`, `Queue`, `Build`, etc.)
- `provider.rs` — `BindingsProvider`: holds all binding instances for a deployment, keyed by name
- `alien_context.rs` — `AlienContext`: application-facing entry point (wraps `BindingsProvider`)
- `providers/` — Platform-specific implementations (AWS, GCP, Azure, Local, etc.)
- `grpc/` — gRPC server + client for cross-process binding access (used by alien-runtime)

## Feature Flags

- `grpc` — gRPC server/client for cross-process binding access
- `openapi` — OpenAPI schema generation
- Platform features: `aws`, `gcp`, `azure`, `kubernetes`, `local`, `test`

## Usage

```rust
use alien_bindings::{get_platform_provider, Storage};

let provider = get_platform_provider()?;
let storage = provider.load_storage("my-storage").await?;
storage.put(&Path::from("hello.txt"), data.into()).await?;
```

## Adding New Providers

1. Create a new module under `src/providers/` implementing `BindingsProvider`
2. Add feature flag in `Cargo.toml`
3. Update `get_platform_provider()` in `lib.rs`

## Adding New Binding Types

1. Define the trait in `src/traits.rs`
2. Add method to `BindingsProvider`
3. Implement for each provider
