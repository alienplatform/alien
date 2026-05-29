# ComputeClusterHeartbeatData


## Supported Types

### `models.ComputeClusterHeartbeatDataAws`

```typescript
const value: models.ComputeClusterHeartbeatDataAws = {
  capacityGroups: [
    {
      currentMachines: 902187,
      desiredMachines: 772214,
      groupId: "<id>",
    },
  ],
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "aws",
};
```

### `models.ComputeClusterHeartbeatDataGcp`

```typescript
const value: models.ComputeClusterHeartbeatDataGcp = {
  capacityGroups: [],
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 875017,
      desiredMachines: 786839,
      groupId: "<id>",
      providerId: "<id>",
    },
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "gcp",
};
```

### `models.ComputeClusterHeartbeatDataAzure`

```typescript
const value: models.ComputeClusterHeartbeatDataAzure = {
  capacityGroups: [],
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "azure",
};
```

### `models.ComputeClusterHeartbeatDataLocal`

```typescript
const value: models.ComputeClusterHeartbeatDataLocal = {
  dockerAvailable: true,
  events: [],
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

