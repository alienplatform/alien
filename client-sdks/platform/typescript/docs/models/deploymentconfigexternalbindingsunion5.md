# DeploymentConfigExternalBindingsUnion5

Represents a vault binding for secure secret management


## Supported Types

### `models.DeploymentConfigExternalBindingsParameterStore`

```typescript
const value: models.DeploymentConfigExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

### `models.DeploymentConfigExternalBindingsSecretManager`

```typescript
const value: models.DeploymentConfigExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

### `models.DeploymentConfigExternalBindingsKeyVault`

```typescript
const value: models.DeploymentConfigExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

### `models.DeploymentConfigExternalBindingsKubernetesSecret`

```typescript
const value: models.DeploymentConfigExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.DeploymentConfigExternalBindingsLocalVault`

```typescript
const value: models.DeploymentConfigExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

