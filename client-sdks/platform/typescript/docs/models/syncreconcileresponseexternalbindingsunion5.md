# SyncReconcileResponseExternalBindingsUnion5

Represents a vault binding for secure secret management


## Supported Types

### `models.SyncReconcileResponseExternalBindingsParameterStore`

```typescript
const value: models.SyncReconcileResponseExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

### `models.SyncReconcileResponseExternalBindingsSecretManager`

```typescript
const value: models.SyncReconcileResponseExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

### `models.SyncReconcileResponseExternalBindingsKeyVault`

```typescript
const value: models.SyncReconcileResponseExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

### `models.SyncReconcileResponseExternalBindingsKubernetesSecret`

```typescript
const value: models.SyncReconcileResponseExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.SyncReconcileResponseExternalBindingsLocalVault`

```typescript
const value: models.SyncReconcileResponseExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

