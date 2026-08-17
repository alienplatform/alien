# DeploymentConfigExternalBindingsUnion1

Service-type based storage binding that supports multiple storage providers


## Supported Types

### `models.DeploymentConfigExternalBindingsS3`

```typescript
const value: models.DeploymentConfigExternalBindingsS3 = {
  service: "s3",
  type: "storage",
};
```

### `models.DeploymentConfigExternalBindingsBlob`

```typescript
const value: models.DeploymentConfigExternalBindingsBlob = {
  service: "blob",
  type: "storage",
};
```

### `models.DeploymentConfigExternalBindingsGcs`

```typescript
const value: models.DeploymentConfigExternalBindingsGcs = {
  service: "gcs",
  type: "storage",
};
```

### `models.DeploymentConfigExternalBindingsLocalStorage`

```typescript
const value: models.DeploymentConfigExternalBindingsLocalStorage = {
  service: "local-storage",
  type: "storage",
};
```

