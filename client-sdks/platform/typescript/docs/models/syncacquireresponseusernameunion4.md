# SyncAcquireResponseUsernameUnion4

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

### `models.SyncAcquireResponseUsername4`

```typescript
const value: models.SyncAcquireResponseUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```
