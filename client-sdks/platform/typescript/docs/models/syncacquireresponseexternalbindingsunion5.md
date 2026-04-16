# SyncAcquireResponseExternalBindingsUnion5

Represents a vault binding for secure secret management


## Supported Types

### `models.SyncAcquireResponseExternalBindingsParameterStore`

```typescript
const value: models.SyncAcquireResponseExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

### `models.SyncAcquireResponseExternalBindingsSecretManager`

```typescript
const value: models.SyncAcquireResponseExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

### `models.SyncAcquireResponseExternalBindingsKeyVault`

```typescript
const value: models.SyncAcquireResponseExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

### `models.SyncAcquireResponseExternalBindingsKubernetesSecret`

```typescript
const value: models.SyncAcquireResponseExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

### `models.SyncAcquireResponseExternalBindingsLocalVault`

```typescript
const value: models.SyncAcquireResponseExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

