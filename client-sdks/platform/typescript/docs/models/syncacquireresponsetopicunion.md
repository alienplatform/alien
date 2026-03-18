# SyncAcquireResponseTopicUnion

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

### `models.SyncAcquireResponseTopic`

```typescript
const value: models.SyncAcquireResponseTopic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

