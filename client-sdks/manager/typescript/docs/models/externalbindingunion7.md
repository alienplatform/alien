# ExternalBindingUnion7

External Redis binding (operator-provided Redis-compatible server)


## Supported Types

### `models.ExternalBindingElasticache`

```typescript
const value: models.ExternalBindingElasticache = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  passwordSecretArn: "<value>",
  port: 842567,
  username: "Ashley.Barrows64",
  service: "elasticache",
  type: "redis",
};
```

### `models.ExternalBindingMemorystore`

```typescript
const value: models.ExternalBindingMemorystore = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  instanceName: "<value>",
  port: "<value>",
  serverCaCertificates: "<value>",
  username: "Edwardo42",
  service: "memorystore",
  type: "redis",
};
```

### `models.ExternalBindingAzureManagedRedis`

```typescript
const value: models.ExternalBindingAzureManagedRedis = {
  databaseResourceId: "<id>",
  host: "questionable-bug.net",
  port: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  username: "Elwyn.Wisozk",
  service: "azure-managed-redis",
  type: "redis",
};
```

### `models.ExternalBindingExternalRedis`

```typescript
const value: models.ExternalBindingExternalRedis = {
  host: "evil-other.info",
  password: {
    secretKeyRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  port: 189392,
  username: "Wilton71",
  service: "external",
  type: "redis",
};
```

### `models.ExternalBindingLocalRedis`

```typescript
const value: models.ExternalBindingLocalRedis = {
  host: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  password: "EJO01fp6xcdr6q7",
  port: "<value>",
  username: "Kole_Stokes91",
  service: "local-redis",
  type: "redis",
};
```
