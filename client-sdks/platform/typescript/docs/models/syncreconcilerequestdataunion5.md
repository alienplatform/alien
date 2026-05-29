# SyncReconcileRequestDataUnion5


## Supported Types

### `models.DataAws2`

```typescript
const value: models.DataAws2 = {
  capacityGroups: [],
  events: [],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "aws",
};
```

### `models.DataGcp2`

```typescript
const value: models.DataGcp2 = {
  capacityGroups: [
    {
      currentMachines: 496999,
      desiredMachines: 708581,
      groupId: "<id>",
    },
  ],
  events: [],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 22022,
      desiredMachines: 972056,
      groupId: "<id>",
      providerId: "<id>",
    },
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "gcp",
};
```

### `models.DataAzure2`

```typescript
const value: models.DataAzure2 = {
  capacityGroups: [
    {
      currentMachines: 986352,
      desiredMachines: 1134,
      groupId: "<id>",
    },
  ],
  events: [],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 567819,
      desiredMachines: 375470,
      groupId: "<id>",
      providerId: "<id>",
    },
  ],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azure",
};
```

### `models.DataLocal5`

```typescript
const value: models.DataLocal5 = {
  dockerAvailable: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-09-24T00:42:05.626Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

