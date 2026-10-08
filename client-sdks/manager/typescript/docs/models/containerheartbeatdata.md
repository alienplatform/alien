# ContainerHeartbeatData


## Supported Types

### `models.ContainerHeartbeatDataHorizonPlatform`

```typescript
const value: models.ContainerHeartbeatDataHorizonPlatform = {
  attentionCount: 828757,
  containerId: "<id>",
  events: [],
  replicaUnits: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  replicas: {},
  schedulingMode: "daemon",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "horizonPlatform",
};
```

### `models.ContainerHeartbeatDataKubernetes`

```typescript
const value: models.ContainerHeartbeatDataKubernetes = {
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  name: "<value>",
  namespace: "<value>",
  pods: [],
  replicas: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  workloadKind: "deployment",
  backend: "kubernetes",
};
```

### `models.ContainerHeartbeatDataLocal`

```typescript
const value: models.ContainerHeartbeatDataLocal = {
  bindMountCount: 916242,
  events: [],
  portCount: 776553,
  runtimeReachable: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

