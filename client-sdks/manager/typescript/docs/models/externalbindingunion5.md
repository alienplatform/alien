# ExternalBindingUnion5

External vault binding (HashiCorp Vault, etc.)


## Supported Types

### `models.ExternalBindingParameterStore`

```typescript
const value: models.ExternalBindingParameterStore = {
  vaultPrefix: "<value>",
  service: "parameter-store",
  type: "vault",
};
```

### `models.ExternalBindingSecretManager`

```typescript
const value: models.ExternalBindingSecretManager = {
  vaultPrefix: "<value>",
  service: "secret-manager",
  type: "vault",
};
```

### `models.ExternalBindingKeyVault`

```typescript
const value: models.ExternalBindingKeyVault = {
  vaultName: "<value>",
  service: "key-vault",
  type: "vault",
};
```

### `models.ExternalBindingKubernetesSecret`

```typescript
const value: models.ExternalBindingKubernetesSecret = {
  namespace: "<value>",
  vaultPrefix: "<value>",
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.ExternalBindingLocalVault`

```typescript
const value: models.ExternalBindingLocalVault = {
  dataDir: "<value>",
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```
