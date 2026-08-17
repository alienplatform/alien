# TargetDeploymentExternalBindingsUnion5

Represents a vault binding for secure secret management


## Supported Types

### `models.TargetDeploymentExternalBindingsParameterStore`

```typescript
const value: models.TargetDeploymentExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

### `models.TargetDeploymentExternalBindingsSecretManager`

```typescript
const value: models.TargetDeploymentExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

### `models.TargetDeploymentExternalBindingsKeyVault`

```typescript
const value: models.TargetDeploymentExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

### `models.TargetDeploymentExternalBindingsKubernetesSecret`

```typescript
const value: models.TargetDeploymentExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.TargetDeploymentExternalBindingsLocalVault`

```typescript
const value: models.TargetDeploymentExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

