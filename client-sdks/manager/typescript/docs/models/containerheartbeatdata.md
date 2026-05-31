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
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
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
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
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
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

