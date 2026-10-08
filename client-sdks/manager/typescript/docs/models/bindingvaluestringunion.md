# BindingValueStringUnion

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `string`

```typescript
const value: string = "<value>";
```

### `models.BindingValueString`

```typescript
const value: models.BindingValueString = {
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
