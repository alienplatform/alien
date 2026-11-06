# PasswordUnion

Connection password or Kubernetes Secret reference. Kubernetes workloads resolve
the reference through secretKeyRef before the application loads its binding.


## Supported Types

### `models.Password1`

```typescript
const value: models.Password1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

### `models.Password2`

```typescript
const value: models.Password2 = {
  secretKeyRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

### `string`

```typescript
const value: string = "<value>";
```
