# SyncReconcileRequestDataUnion8


## Supported Types

### `models.DataAurora`

```typescript
const value: models.DataAurora = {
  clusterIdentifier: "<value>",
  neverPauses: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
  backend: "aurora",
};
```

### `models.DataCloudSQL`

```typescript
const value: models.DataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "cloudSql",
};
```

### `models.DataFlexibleServer`

```typescript
const value: models.DataFlexibleServer = {
  serverName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "flexibleServer",
};
```

### `models.DataLocal8`

```typescript
const value: models.DataLocal8 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```

