# DeploymentConfigServerCaCertificatesUnion

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

### `models.DeploymentConfigServerCaCertificates`

```typescript
const value: models.DeploymentConfigServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

