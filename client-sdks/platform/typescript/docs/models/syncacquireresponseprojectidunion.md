# SyncAcquireResponseProjectIdUnion

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

### `models.SyncAcquireResponseProjectId`

```typescript
const value: models.SyncAcquireResponseProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

