# DataUnion3


## Supported Types

### `operations.DataHorizonPlatform`

```typescript
const value: operations.DataHorizonPlatform = {
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
        severity: "warning",
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

### `operations.DataKubernetes2`

```typescript
const value: operations.DataKubernetes2 = {
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

### `operations.DataLocal3`

```typescript
const value: operations.DataLocal3 = {
  bindMountCount: 241047,
  events: [],
  portCount: 395842,
  runtimeReachable: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

