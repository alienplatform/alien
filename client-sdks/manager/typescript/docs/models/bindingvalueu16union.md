# BindingValueU16Union

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `number`

```typescript
const value: number = 128403;
```

### `models.BindingValueU16`

```typescript
const value: models.BindingValueU16 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```
