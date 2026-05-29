# DaemonHeartbeatData


## Supported Types

### `models.DaemonHeartbeatDataAws`

```typescript
const value: models.DaemonHeartbeatDataAws = {
  assignedMachines: 818243,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonName: "<value>",
  desiredMachines: 655722,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  healthyInstances: 188895,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
  daemonName: "<value>",
  desiredMachines: 976582,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  healthyInstances: 396125,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
  daemonName: "<value>",
  desiredMachines: 587230,
  events: [],
  healthyInstances: 564532,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
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

