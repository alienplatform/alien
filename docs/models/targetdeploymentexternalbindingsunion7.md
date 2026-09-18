# TargetDeploymentExternalBindingsUnion7

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.TargetDeploymentExternalBindingsUnion1`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion1 = {
  service: "s3",
  type: "storage",
};
```

### `models.TargetDeploymentExternalBindingsUnion2`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion2 = {
  service: "servicebus",
  type: "queue",
};
```

### `models.TargetDeploymentExternalBindingsUnion3`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion3 = {
  service: "redis",
  type: "kv",
};
```

### `models.TargetDeploymentExternalBindingsUnion4`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion4 = {
  service: "acr",
  type: "artifact_registry",
};
```

### `models.TargetDeploymentExternalBindingsUnion5`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion5 = {
  service: "parameter-store",
  type: "vault",
};
```

### `models.TargetDeploymentExternalBindingsContainerAppsEnvironment`

```typescript
const value: models.TargetDeploymentExternalBindingsContainerAppsEnvironment = {
  type: "container_apps_environment",
};
```

### `models.TargetDeploymentExternalBindingsUnion6`

```typescript
const value: models.TargetDeploymentExternalBindingsUnion6 = {
  service: "flexible-server",
  type: "postgres",
};
```

### `models.TargetDeploymentExternalBindingsAi`

```typescript
const value: models.TargetDeploymentExternalBindingsAi = {
  provider: "<value>",
  type: "ai",
};
```

