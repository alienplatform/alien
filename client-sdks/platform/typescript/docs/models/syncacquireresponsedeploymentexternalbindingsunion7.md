# SyncAcquireResponseDeploymentExternalBindingsUnion7

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion1`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion1 = {
  service: "blob",
  type: "storage",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion2`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion2 = {
  service: "local-queue",
  type: "queue",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion3`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion3 = {
  service: "firestore",
  type: "kv",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion4`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion4 = {
  service: "ecr",
  type: "artifact_registry",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion5`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion5 = {
  service: "key-vault",
  type: "vault",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsContainerAppsEnvironment`

```typescript
const value:
  models.SyncAcquireResponseDeploymentExternalBindingsContainerAppsEnvironment =
    {
      type: "container_apps_environment",
    };
```

### `models.SyncAcquireResponseDeploymentExternalBindingsUnion6`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsUnion6 = {
  service: "aurora",
  type: "postgres",
};
```

