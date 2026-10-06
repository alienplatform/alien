# ExternalBindingUnion

Represents a binding to pre-existing infrastructure.

The binding type must match the resource type it's applied to.
Validated at runtime by the executor.


## Supported Types

### `models.ExternalBindingUnion1`

```typescript
const value: models.ExternalBindingUnion1 = {
  accountName: "<value>",
  containerName: "<value>",
  service: "blob",
  type: "storage",
};
```

### `models.ExternalBindingUnion2`

```typescript
const value: models.ExternalBindingUnion2 = {
  subscription: "<value>",
  topic: "<value>",
  service: "pubsub",
  type: "queue",
};
```

### `models.ExternalBindingUnion3`

```typescript
const value: models.ExternalBindingUnion3 = {
  connectionUrl: "https://specific-following.biz/",
  service: "redis",
  type: "kv",
};
```

### `models.ExternalBindingUnion4`

```typescript
const value: models.ExternalBindingUnion4 = {
  registryName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  resourceGroupName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "acr",
  type: "artifact_registry",
};
```

### `models.ExternalBindingUnion5`

```typescript
const value: models.ExternalBindingUnion5 = {
  dataDir: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

### `models.ExternalBindingContainerAppsEnvironment`

```typescript
const value: models.ExternalBindingContainerAppsEnvironment = {
  defaultDomain: "<value>",
  environmentName: "<value>",
  resourceGroupName: null,
  resourceId: "<id>",
  type: "container_apps_environment",
};
```

### `models.ExternalBindingUnion6`

```typescript
const value: models.ExternalBindingUnion6 = {
  clusterEndpoint: "<value>",
  database: null,
  passwordSecretArn: "<value>",
  port: null,
  username: "Kyle_King",
  service: "aurora",
  type: "postgres",
};
```

### `models.ExternalBindingAi`

```typescript
const value: models.ExternalBindingAi = {
  apiKey: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  provider: "<value>",
  type: "ai",
};
```

