# SyncReconcileResponseVaultNameUnion

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `any`

```typescript
const value: any = "<value>";
```

### `string`

```typescript
const value: string = "<value>";
```

### `models.SyncReconcileResponseVaultName`

```typescript
const value: models.SyncReconcileResponseVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

