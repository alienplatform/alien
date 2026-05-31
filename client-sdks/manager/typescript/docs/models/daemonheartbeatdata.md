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
      ready: true,
      replicaId: "<id>",
    },
  ],
  daemonName: "<value>",
  desiredMachines: 915980,
  events: [],
  healthyInstances: 401388,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
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
      ready: true,
      replicaId: "<id>",
    },
  ],
  daemonName: "<value>",
  desiredMachines: 550789,
  events: [],
  healthyInstances: 289674,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
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
      ready: true,
      replicaId: "<id>",
    },
  ],
  daemonName: "<value>",
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
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  unavailableInstances: 821001,
  backend: "azure",
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
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "kubernetes",
};
```

### `models.DaemonHeartbeatDataLocal`

```typescript
const value: models.DaemonHeartbeatDataLocal = {
  commandSupported: true,
  daemonName: "<value>",
  events: [],
  imagePathPresent: true,
  runtimeId: "<id>",
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

