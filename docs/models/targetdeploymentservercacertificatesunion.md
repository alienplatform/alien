# TargetDeploymentServerCaCertificatesUnion

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

### `any`

```typescript
const value: any = "<value>";
```

### `models.TargetDeploymentServerCaCertificates`

```typescript
const value: models.TargetDeploymentServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

