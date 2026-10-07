# BindingValueOptionStringUnion

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `string`

```typescript
const value: string = "<value>";
```

### `models.BindingValueOptionString`

```typescript
const value: models.BindingValueOptionString = {
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
