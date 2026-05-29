# ContainerHeartbeatData


## Supported Types

### `models.ContainerHeartbeatDataHorizonPlatform`

```typescript
const value: models.ContainerHeartbeatDataHorizonPlatform = {
  attentionCount: 828757,
  containerId: "<id>",
  events: [],
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
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  instances: [],
  name: "<value>",
  namespace: "<value>",
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

