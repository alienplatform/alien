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
        reason: "timed-out",
        severity: "info",
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
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: false,
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
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: false,
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

