# ExternalBindingUnion7

Connection details for a Redis-compatible server, one variant per backend.

Service tags are unique across all binding enums (serde dispatches on
`service` alone), so the key-value store's `redis` tag is not reused.


## Supported Types

### `models.ExternalBindingElasticache`

```typescript
const value: models.ExternalBindingElasticache = {
  host: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  passwordSecretArn: "<value>",
  port: 420901,
  username: null,
  service: "elasticache",
  type: "redis",
};
```

### `models.ExternalBindingMemorystore`

```typescript
const value: models.ExternalBindingMemorystore = {
  host: "scared-vein.com",
  instanceName: "<value>",
  port: "<value>",
  serverCaCertificates: "<value>",
  username: "Saul16",
  service: "memorystore",
  type: "redis",
};
```

### `models.ExternalBindingAzureManagedRedis`

```typescript
const value: models.ExternalBindingAzureManagedRedis = {
  databaseResourceId: "<id>",
  host: "spirited-pension.net",
  port: "<value>",
  username: "Enos71",
  service: "azure-managed-redis",
  type: "redis",
};
```

### `models.ExternalBindingExternalRedis`

```typescript
const value: models.ExternalBindingExternalRedis = {
  host: "precious-honesty.name",
  password: {
    secretKeyRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  port: 984309,
  username: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "external",
  type: "redis",
};
```

### `models.ExternalBindingLocalRedis`

```typescript
const value: models.ExternalBindingLocalRedis = {
  host: "ignorant-husband.info",
  password: "01fp6xcdr6q7kbW",
  port: "<value>",
  username: "Estel.Reichel",
  service: "local-redis",
  type: "redis",
};
```
