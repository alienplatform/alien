# BindingValueVecStringUnion

Represents a value that can be either a concrete value, a template expression,
or a reference to a Kubernetes Secret


## Supported Types

### `string[]`

```typescript
const value: string[] = [
  "<value 1>",
  "<value 2>",
];
```

### `models.BindingValueVecString`

```typescript
const value: models.BindingValueVecString = {
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
