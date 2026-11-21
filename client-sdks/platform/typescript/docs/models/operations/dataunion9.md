# DataUnion9

Heartbeat data for a Redis resource. Everything comes from the cloud's
management and metrics APIs; the manager never opens a Redis connection.


## Supported Types

### `operations.DataElasticache`

```typescript
const value: operations.DataElasticache = {
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

### `operations.DataMemorystore`

```typescript
const value: operations.DataMemorystore = {
  config: {},
  instanceName: "<value>",
  metrics: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
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

### `operations.DataAzureManagedRedis`

```typescript
const value: operations.DataAzureManagedRedis = {
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

### `operations.DataLocal9`

```typescript
const value: operations.DataLocal9 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  version: "<value>",
  backend: "local",
};
```
