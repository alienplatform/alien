# SyncAcquireResponseDeploymentExternalBindingsUnion5

Represents a vault binding for secure secret management


## Supported Types

### `models.SyncAcquireResponseDeploymentExternalBindingsParameterStore`

```typescript
const value:
  models.SyncAcquireResponseDeploymentExternalBindingsParameterStore = {
    service: "parameter-store",
    type: "vault",
  };
```

### `models.SyncAcquireResponseDeploymentExternalBindingsSecretManager`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsSecretManager =
  {
    service: "secret-manager",
    type: "vault",
  };
```

### `models.SyncAcquireResponseDeploymentExternalBindingsKeyVault`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

### `models.SyncAcquireResponseDeploymentExternalBindingsKubernetesSecret`

```typescript
const value:
  models.SyncAcquireResponseDeploymentExternalBindingsKubernetesSecret = {
    service: "kubernetes-secret",
    type: "vault",
  };
```

### `models.SyncAcquireResponseDeploymentExternalBindingsLocalVault`

```typescript
const value: models.SyncAcquireResponseDeploymentExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

