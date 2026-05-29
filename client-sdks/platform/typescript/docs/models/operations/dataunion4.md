# DataUnion4


## Supported Types

### `operations.DataAws1`

```typescript
const value: operations.DataAws1 = {
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

### `operations.DataGcp1`

```typescript
const value: operations.DataGcp1 = {
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
        reason: "collection-failed",
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

### `operations.DataAzure1`

```typescript
const value: operations.DataAzure1 = {
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

### `operations.DataKubernetes3`

```typescript
const value: operations.DataKubernetes3 = {
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
          controller: false,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 402283,
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
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "kubernetes",
};
```

### `operations.DataLocal4`

```typescript
const value: operations.DataLocal4 = {
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

