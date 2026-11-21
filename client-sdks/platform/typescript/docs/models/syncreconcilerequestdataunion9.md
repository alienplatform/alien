# SyncReconcileRequestDataUnion9

Heartbeat data for a Redis resource. Everything comes from the cloud's
management and metrics APIs; the manager never opens a Redis connection.


## Supported Types

### `models.DataElasticache`

```typescript
const value: models.DataElasticache = {
  config: {},
  metrics: {},
  replicationGroupId: "<id>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "elasticache",
};
```

### `models.DataMemorystore`

```typescript
const value: models.DataMemorystore = {
  config: {},
  instanceName: "<value>",
  metrics: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "memorystore",
};
```

### `models.DataAzureManagedRedis`

```typescript
const value: models.DataAzureManagedRedis = {
  clusterName: "<value>",
  config: {},
  metrics: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azureManagedRedis",
};
```

### `models.DataLocal9`

```typescript
const value: models.DataLocal9 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  version: "<value>",
  backend: "local",
};
```
