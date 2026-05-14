# SyncReconcileResponseDefaultDomainUnion

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

### `models.SyncReconcileResponseDefaultDomain`

```typescript
const value: models.SyncReconcileResponseDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

