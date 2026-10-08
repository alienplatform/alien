# PortUnion2

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

### `models.Port2`

```typescript
const value: models.Port2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

### `string`

```typescript
const value: string = "<value>";
```

