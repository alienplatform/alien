# RedisHeartbeatData

Heartbeat data for a Redis resource. Everything comes from the cloud's
management and metrics APIs; the manager never opens a Redis connection.


## Supported Types

### `models.RedisHeartbeatDataElasticache`

```typescript
const value: models.RedisHeartbeatDataElasticache = {
  config: {},
  metrics: {},
  replicationGroupId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "elasticache",
};
```

### `models.RedisHeartbeatDataMemorystore`

```typescript
const value: models.RedisHeartbeatDataMemorystore = {
  config: {},
  instanceName: "<value>",
  metrics: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "memorystore",
};
```

### `models.RedisHeartbeatDataAzureManagedRedis`

```typescript
const value: models.RedisHeartbeatDataAzureManagedRedis = {
  clusterName: "<value>",
  config: {},
  metrics: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "azureManagedRedis",
};
```

### `models.RedisHeartbeatDataLocal`

```typescript
const value: models.RedisHeartbeatDataLocal = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```
