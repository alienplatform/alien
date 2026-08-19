# DeploymentConfigExternalBindingsUnion7

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.DeploymentConfigExternalBindingsUnion1`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion1 = {
  service: "gcs",
  type: "storage",
};
```

### `models.DeploymentConfigExternalBindingsUnion2`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion2 = {
  service: "servicebus",
  type: "queue",
};
```

### `models.DeploymentConfigExternalBindingsUnion3`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion3 = {
  service: "redis",
  type: "kv",
};
```

### `models.DeploymentConfigExternalBindingsUnion4`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion4 = {
  service: "ecr",
  type: "artifact_registry",
};
```

### `models.DeploymentConfigExternalBindingsUnion5`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion5 = {
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.DeploymentConfigExternalBindingsContainerAppsEnvironment`

```typescript
const value: models.DeploymentConfigExternalBindingsContainerAppsEnvironment = {
  type: "container_apps_environment",
};
```

### `models.DeploymentConfigExternalBindingsUnion6`

```typescript
const value: models.DeploymentConfigExternalBindingsUnion6 = {
  service: "flexible-server",
  type: "postgres",
};
```

### `models.DeploymentConfigExternalBindingsAi`

```typescript
const value: models.DeploymentConfigExternalBindingsAi = {
  provider: "<value>",
  type: "ai",
};
```

