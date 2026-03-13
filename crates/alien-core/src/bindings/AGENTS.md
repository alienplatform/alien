# Bindings

Binding types define how applications connect to resources. Each binding enum uses `#[serde(tag = "service")]` to differentiate variants.

## Critical Rule: Unique Service Tags

**Each variant must have a unique `service` tag, even across different resource types.**

### Why This Matters

With `#[serde(tag = "service")]`, serde uses only the `service` field to determine which variant to deserialize. If multiple binding types share the same tag (e.g., all local variants using `"local"`), serde will successfully deserialize the wrong type by ignoring extra fields.

### Example Problem

```rust
// ❌ BAD: Both use "local"
pub enum VaultBinding {
    Local(LocalVaultBinding),  // → {"service": "local", "vaultName": "...", "dataDir": "..."}
}

pub enum KvBinding {
    Local(LocalKvBinding),     // → {"service": "local", "dataDir": "..."}
}

// Result: VaultBinding JSON can deserialize as KvBinding (drops vaultName field)
```

### Solution

```rust
// ✅ GOOD: Each has unique tag
pub enum VaultBinding {
    #[serde(rename = "local-vault")]
    Local(LocalVaultBinding),  // → {"service": "local-vault", ...}
}

pub enum KvBinding {
    #[serde(rename = "local-kv")]
    Local(LocalKvBinding),     // → {"service": "local-kv", ...}
}

pub enum StorageBinding {
    #[serde(rename = "local-storage")]
    Local(LocalStorageBinding), // → {"service": "local-storage", ...}
}
```

## Binding Structure

Each resource type has its own binding enum:

- `StorageBinding` - S3, GCS, Blob, Local Storage
- `KvBinding` - DynamoDB, Firestore, Table Storage, Redis, Local KV
- `VaultBinding` - Parameter Store, Secret Manager, Key Vault, Local Vault
- `QueueBinding` - SQS, Pub/Sub, Service Bus, Local Queue
- `FunctionBinding` - Lambda, Cloud Run, Container Apps, Local Function
- `ContainerBinding` - ECS, GKE, AKS, Local Container

All variants are defined in `alien-core/src/bindings/`.

## Implementation

1. **Define binding struct** with `#[serde(rename_all = "camelCase")]`
2. **Add to enum** with unique service tag
3. **Test serialization** roundtrip to verify tag uniqueness

