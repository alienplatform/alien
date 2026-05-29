# SyncReconcileRequestDataUnion4


## Supported Types

### `models.DataAws1`

```typescript
const value: models.DataAws1 = {
  assignedMachines: 644340,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonName: "<value>",
  desiredMachines: 753830,
  events: [],
  healthyInstances: 896332,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
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
  daemonName: "<value>",
  desiredMachines: 139194,
  events: [],
  healthyInstances: 504997,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
  daemonName: "<value>",
  desiredMachines: 516924,
  events: [],
  healthyInstances: 583805,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  unavailableInstances: 602836,
  backend: "azure",
};
```

### `models.DataKubernetes3`

```typescript
const value: models.DataKubernetes3 = {
  commandSupported: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-29T04:54:57.889Z"),
      severity: "warning",
    },
  ],
  instances: [
    {
      name: "<value>",
      ownerReferences: [
        {
          controller: true,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 905674,
    },
  ],
  name: "<value>",
  namespace: "<value>",
  replicas: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "running",
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
      observedAt: new Date("2025-03-14T01:07:50.346Z"),
      severity: "error",
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

