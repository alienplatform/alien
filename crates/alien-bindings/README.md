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

With the `platform-sdk` feature, trusted backend code can open the hosted
remote Storage surface through the deployment's assigned manager:

```rust,no_run
use alien_bindings::RemoteBindings;

let bindings = RemoteBindings::for_deployment(
    "dep_...",
    &std::env::var("ALIEN_API_TOKEN")?,
    None,
)
.await?;
let storage = bindings.storage("uploads").await?;
```

Remote v0 supports only Running, Frozen S3, GCS, and Azure Blob Storage
resources with remote access enabled. The API token and returned provider
credentials are backend secrets. Remote access grants the deployment management
identity exact object read, write, list, delete, and multipart permissions on
the selected bucket or container; it does not create a separate identity per
resource.

This replaces the former unscoped `/v1/resolve-credentials` flow. The dedicated
`RemoteBindings` type exposes only Storage; its handles refresh credentials and
rediscover manager assignment without exposing a non-refreshing provider.

## Adding New Providers

1. Create a new module under `src/providers/` implementing `BindingsProvider`
2. Add feature flag in `Cargo.toml`
3. Update `get_platform_provider()` in `lib.rs`

## Adding New Binding Types

1. Define the trait in `src/traits.rs`
2. Add method to `BindingsProvider`
3. Implement for each provider
