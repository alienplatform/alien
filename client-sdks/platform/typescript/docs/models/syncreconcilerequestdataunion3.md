# SyncReconcileRequestDataUnion3


## Supported Types

### `models.DataHorizonPlatform`

```typescript
const value: models.DataHorizonPlatform = {
  attentionCount: 261747,
  containerId: "<id>",
  events: [],
  replicaUnits: [],
  replicas: {},
  schedulingMode: "daemon",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "horizonPlatform",
};
```

### `models.DataKubernetes2`

```typescript
const value: models.DataKubernetes2 = {
  events: [],
  name: "<value>",
  namespace: "<value>",
  pods: [],
  replicas: {},
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  workloadKind: "replicaSet",
  backend: "kubernetes",
};
```

### `models.DataLocal3`

```typescript
const value: models.DataLocal3 = {
  bindMountCount: 241047,
  events: [],
  portCount: 395842,
  runtimeReachable: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

