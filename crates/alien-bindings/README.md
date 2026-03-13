# Alien Bindings

A Rust library that provides platform-agnostic bindings for Alien applications to access cloud resources like storage, KV, and more.

## Purpose

The `alien-bindings` crate allows Alien applications to interact with various cloud resources without being tightly coupled to specific cloud providers. Applications can use the same code regardless of whether they're running on AWS, Google Cloud, or other supported platforms.

## Features

- 🔌 Provider-agnostic bindings for cloud resources
- 🧩 Modular design with feature flags to reduce binary size
- 🔄 Automatic provider detection based on environment
- 🚀 Built on industry-standard libraries like `object_store`

## Usage

Add to your Cargo.toml:

```toml
[dependencies]
alien-bindings = { version = "0.1.0", features = ["aws", "gcp"] }
```

Example using storage binding:

```rust
use alien_bindings::{get_platform_provider, Storage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the appropriate provider based on the environment
    let provider = get_platform_provider()?;
    
    // Load a storage binding called "my-storage"
    // Environment variables like ALIEN_MY_STORAGE_BUCKET_NAME must be set
    let storage = provider.load_storage("my-storage").await?;
    
    // Use the storage binding
    let data = b"Hello, Alien!";
    storage.put(
        &object_store::path::Path::from("hello.txt"),
        data.to_vec().into(),
    ).await?;
    
    Ok(())
}
```

## Running Tests

Tests require cloud provider credentials to be set as environment variables. Create a `.env.test` file in the **workspace root directory** (the parent directory of `alien-bindings/`):

```bash
# Copy the example file
cp .env.test.example .env.test

# Edit the file with your test credentials
nano .env.test
```

Run tests with specific features, capturing output:

```bash
# Run AWS tests
cargo test --features aws -- --nocapture

# Run GCP tests  
cargo test --features gcp -- --nocapture

# Run all tests
cargo test -- --nocapture
```

## Adding New Providers

To add a new provider (e.g., Azure):

1. Create a new module under `src/providers/`:
   ```
   src/providers/
   ├── azure/
   │   ├── mod.rs     # Contains AzureBindingsProvider implementation
   │   └── storage.rs # Contains AzureStorage implementation
   ```

2. Implement the `BindingsProvider` trait:
   ```rust
   // src/providers/azure/mod.rs
   pub struct AzureBindingsProvider;
   
   #[async_trait]
   impl BindingsProvider for AzureBindingsProvider {
       async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>, Error> {
           // Azure-specific implementation
       }
   }
   ```

3. Add feature flag in `Cargo.toml`:
   ```toml
   [features]
   azure = ["azure_sdk_dependency"]
   ```

4. Update `get_platform_provider()` in `lib.rs` to include the new provider.

## Adding New Binding Types

To add a new binding type (e.g., Queue):

1. Define the trait in `src/traits.rs`:
   ```rust
   #[async_trait]
   pub trait Queue: Binding {
       async fn send(&self, message: &[u8]) -> Result<(), Error>;
       async fn receive(&self) -> Result<Option<Vec<u8>>, Error>;
   }
   ```

2. Add the corresponding method to `BindingsProvider`:
   ```rust
   #[async_trait]
   pub trait BindingsProvider: Send + Sync {
       // Existing methods...
       async fn load_queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>, Error>;
   }
   ```

3. Implement the new binding for each provider.
