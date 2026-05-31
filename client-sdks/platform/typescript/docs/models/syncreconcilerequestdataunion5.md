# SyncReconcileRequestDataUnion5


## Supported Types

### `models.DataAws2`

```typescript
const value: models.DataAws2 = {
  capacityGroups: [],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: true,
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
  name: "<value>",
  nodes: {},
  providerFleets: [],
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
    lifecycle: "failed",
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
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
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
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

