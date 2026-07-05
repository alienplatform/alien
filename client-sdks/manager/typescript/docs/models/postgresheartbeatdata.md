# PostgresHeartbeatData


## Supported Types

### `models.PostgresHeartbeatDataAurora`

```typescript
const value: models.PostgresHeartbeatDataAurora = {
  clusterIdentifier: "<value>",
  neverPauses: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "aurora",
};
```

### `models.PostgresHeartbeatDataCloudSQL`

```typescript
const value: models.PostgresHeartbeatDataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "cloudSql",
};
```

### `models.PostgresHeartbeatDataFlexibleServer`

```typescript
const value: models.PostgresHeartbeatDataFlexibleServer = {
  serverName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "flexibleServer",
};
```

### `models.PostgresHeartbeatDataLocal`

```typescript
const value: models.PostgresHeartbeatDataLocal = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```

