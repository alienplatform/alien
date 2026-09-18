# DataUnion8


## Supported Types

### `operations.DataAurora`

```typescript
const value: operations.DataAurora = {
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

### `operations.DataCloudSQL`

```typescript
const value: operations.DataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
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

### `operations.DataFlexibleServer`

```typescript
const value: operations.DataFlexibleServer = {
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

### `operations.DataLocal8`

```typescript
const value: operations.DataLocal8 = {
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

