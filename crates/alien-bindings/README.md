# alien-bindings

Platform-agnostic binding abstractions for Alien applications. Defines traits for storage, KV, vault, queue, build, container, function, artifact registry, and service accounts, with per-platform provider implementations.

## Architecture

- `traits.rs` — Binding trait definitions (`Storage`, `Kv`, `Vault`, `Queue`, `Build`, etc.)
- `provider.rs` — `BindingsProvider`: holds all binding instances for a deployment, keyed by name
- `bindings.rs` — `Bindings`: app-facing entry point over `BindingsProvider` (`storage`, `kv`, `queue`, `vault`)
- `providers/` — Platform-specific implementations (AWS, GCP, Azure, Local, etc.)

## Feature Flags

- `openapi` — OpenAPI schema generation
- Platform features: `aws`, `gcp`, `azure`, `kubernetes`, `local`, `test`

## Usage

```rust
use alien_bindings::Bindings;
use object_store::path::Path;

let bindings = Bindings::from_env()?;
let storage = bindings.storage("files").await?;
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
