# SyncAcquireResponseExternalBindingsUnion6

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.SyncAcquireResponseExternalBindingsUnion1`

```typescript
const value: models.SyncAcquireResponseExternalBindingsUnion1 = {
  service: "local-storage",
  type: "storage",
};
```

### `models.SyncAcquireResponseExternalBindingsUnion2`

```typescript
const value: models.SyncAcquireResponseExternalBindingsUnion2 = {
  service: "pubsub",
  type: "queue",
};
```

### `models.SyncAcquireResponseExternalBindingsUnion3`

```typescript
const value: models.SyncAcquireResponseExternalBindingsUnion3 = {
  service: "dynamodb",
  type: "kv",
};
```

### `models.SyncAcquireResponseExternalBindingsUnion4`

```typescript
const value: models.SyncAcquireResponseExternalBindingsUnion4 = {
  service: "local",
  type: "artifact_registry",
};
```

### `models.SyncAcquireResponseExternalBindingsUnion5`

```typescript
const value: models.SyncAcquireResponseExternalBindingsUnion5 = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

