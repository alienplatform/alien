# DataUnion5


## Supported Types

### `operations.DataAws2`

```typescript
const value: operations.DataAws2 = {
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

### `operations.DataGcp2`

```typescript
const value: operations.DataGcp2 = {
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
        reason: "collection-failed",
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

### `operations.DataAzure2`

```typescript
const value: operations.DataAzure2 = {
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

### `operations.DataMachines2`

```typescript
const value: operations.DataMachines2 = {
  capacityGroups: [
    {
      currentMachines: 818927,
      desiredMachines: 925900,
      groupId: "<id>",
    },
  ],
  machines: [],
  name: "<value>",
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "machines",
};
```

### `operations.DataLocal5`

```typescript
const value: operations.DataLocal5 = {
  dockerAvailable: false,
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```
