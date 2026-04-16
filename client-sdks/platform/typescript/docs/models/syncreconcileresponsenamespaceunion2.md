# SyncReconcileResponseNamespaceUnion2

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

### `models.SyncReconcileResponseNamespace2`

```typescript
const value: models.SyncReconcileResponseNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

