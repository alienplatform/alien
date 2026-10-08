# SyncReconcileRequestDataUnion4


## Supported Types

### `models.DataAws1`

```typescript
const value: models.DataAws1 = {
  assignedMachines: 644340,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 896332,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 510224,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: false,
    stale: false,
  },
  unavailableInstances: 873077,
  backend: "aws",
};
```

### `models.DataGcp1`

```typescript
const value: models.DataGcp1 = {
  assignedMachines: 159021,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonInstances: [],
  desiredMachines: 144012,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 314896,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  unavailableInstances: 530018,
  backend: "gcp",
};
```

### `models.DataAzure1`

```typescript
const value: models.DataAzure1 = {
  assignedMachines: 3703,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonInstances: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 583805,
  events: [],
  healthyInstances: 986297,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  unavailableInstances: 624401,
  backend: "azure",
};
```

### `models.DataMachines1`

```typescript
const value: models.DataMachines1 = {
  assignedMachines: 323362,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 395294,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 133817,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  unavailableInstances: 26745,
  backend: "machines",
};
```

### `models.DataKubernetes3`

```typescript
const value: models.DataKubernetes3 = {
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
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "kubernetes",
};
```

### `models.DataLocal4`

```typescript
const value: models.DataLocal4 = {
  commandSupported: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      severity: "warning",
      timestamp: new Date("2026-01-17T09:27:27.938Z"),
    },
  ],
  imagePathPresent: true,
  runtimeId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

