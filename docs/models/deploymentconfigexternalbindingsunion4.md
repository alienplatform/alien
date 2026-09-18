# DeploymentConfigExternalBindingsUnion4

Service-type based artifact registry binding that supports multiple registry providers


## Supported Types

### `models.DeploymentConfigExternalBindingsEcr`

```typescript
const value: models.DeploymentConfigExternalBindingsEcr = {
  service: "ecr",
  type: "artifact_registry",
};
```

### `models.DeploymentConfigExternalBindingsAcr`

```typescript
const value: models.DeploymentConfigExternalBindingsAcr = {
  service: "acr",
  type: "artifact_registry",
};
```

### `models.DeploymentConfigExternalBindingsGar`

```typescript
const value: models.DeploymentConfigExternalBindingsGar = {
  service: "gar",
  type: "artifact_registry",
};
```

### `models.DeploymentConfigExternalBindingsLocal`

```typescript
const value: models.DeploymentConfigExternalBindingsLocal = {
  service: "local",
  type: "artifact_registry",
};
```

