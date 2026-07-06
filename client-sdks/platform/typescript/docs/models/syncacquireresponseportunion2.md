# SyncAcquireResponsePortUnion2

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `number`

```typescript
const value: number = 128403;
```

### `any`

```typescript
const value: any = "<value>";
```

### `models.SyncAcquireResponsePort2`

```typescript
const value: models.SyncAcquireResponsePort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

