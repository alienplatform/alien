# SyncAcquireResponsePortUnion3

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

### `models.SyncAcquireResponsePort3`

```typescript
const value: models.SyncAcquireResponsePort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

