# SyncReconcileResponseServerCaCertificatesUnion

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `string[]`

```typescript
const value: string[] = [
  "<value 1>",
  "<value 2>",
];
```

### `any`

```typescript
const value: any = "<value>";
```

### `models.SyncReconcileResponseServerCaCertificates`

```typescript
const value: models.SyncReconcileResponseServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```
