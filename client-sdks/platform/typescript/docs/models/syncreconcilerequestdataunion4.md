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
  daemonName: "<value>",
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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: false,
  },
  unavailableInstances: 702316,
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
  daemonName: "<value>",
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
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  unavailableInstances: 673342,
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
  daemonName: "<value>",
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
        reason: "not-installed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  unavailableInstances: 614597,
  backend: "azure",
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
  daemonName: "<value>",
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

