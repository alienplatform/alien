# DaemonHeartbeatData


## Supported Types

### `models.DaemonHeartbeatDataAws`

```typescript
const value: models.DaemonHeartbeatDataAws = {
  assignedMachines: 818243,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 915980,
  events: [],
  healthyInstances: 401388,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
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
  unavailableInstances: 429918,
  backend: "aws",
};
```

### `models.DaemonHeartbeatDataGcp`

```typescript
const value: models.DaemonHeartbeatDataGcp = {
  assignedMachines: 629305,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 550789,
  events: [],
  healthyInstances: 289674,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
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
  unavailableInstances: 685429,
  backend: "gcp",
};
```

### `models.DaemonHeartbeatDataAzure`

```typescript
const value: models.DaemonHeartbeatDataAzure = {
  assignedMachines: 950576,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 469652,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 926835,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
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
  unavailableInstances: 821001,
  backend: "azure",
};
```

### `models.DaemonHeartbeatDataMachines`

```typescript
const value: models.DaemonHeartbeatDataMachines = {
  assignedMachines: 335329,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [],
  desiredMachines: 299843,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 565744,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
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
  unavailableInstances: 588890,
  backend: "machines",
};
```

### `models.DaemonHeartbeatDataKubernetes`

```typescript
const value: models.DaemonHeartbeatDataKubernetes = {
  commandSupported: false,
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
  backend: "kubernetes",
};
```

### `models.DaemonHeartbeatDataLocal`

```typescript
const value: models.DaemonHeartbeatDataLocal = {
  commandSupported: true,
  events: [],
  imagePathPresent: true,
  runtimeId: "<id>",
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
