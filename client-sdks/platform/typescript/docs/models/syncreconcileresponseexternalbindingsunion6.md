# SyncReconcileResponseExternalBindingsUnion6

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.SyncReconcileResponseExternalBindingsUnion1`

```typescript
const value: models.SyncReconcileResponseExternalBindingsUnion1 = {
  service: "blob",
  type: "storage",
};
```

### `models.SyncReconcileResponseExternalBindingsUnion2`

```typescript
const value: models.SyncReconcileResponseExternalBindingsUnion2 = {
  service: "sqs",
  type: "queue",
};
```

### `models.SyncReconcileResponseExternalBindingsUnion3`

```typescript
const value: models.SyncReconcileResponseExternalBindingsUnion3 = {
  service: "dynamodb",
  type: "kv",
};
```

### `models.SyncReconcileResponseExternalBindingsUnion4`

```typescript
const value: models.SyncReconcileResponseExternalBindingsUnion4 = {
  service: "local",
  type: "artifact_registry",
};
```

### `models.SyncReconcileResponseExternalBindingsUnion5`

```typescript
const value: models.SyncReconcileResponseExternalBindingsUnion5 = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

