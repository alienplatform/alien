# ComputeClusterHeartbeatData


## Supported Types

### `models.ComputeClusterHeartbeatDataAws`

```typescript
const value: models.ComputeClusterHeartbeatDataAws = {
  capacityGroups: [
    {
      currentMachines: 799180,
      desiredMachines: 987548,
      groupId: "<id>",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 240908,
      desiredMachines: 853061,
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
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "aws",
};
```

### `models.ComputeClusterHeartbeatDataGcp`

```typescript
const value: models.ComputeClusterHeartbeatDataGcp = {
  capacityGroups: [],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 240908,
      desiredMachines: 853061,
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
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcp",
};
```

### `models.ComputeClusterHeartbeatDataAzure`

```typescript
const value: models.ComputeClusterHeartbeatDataAzure = {
  capacityGroups: [],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 240908,
      desiredMachines: 853061,
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
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azure",
};
```

### `models.ComputeClusterHeartbeatDataMachines`

```typescript
const value: models.ComputeClusterHeartbeatDataMachines = {
  capacityGroups: [
    {
      currentMachines: 799180,
      desiredMachines: 987548,
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
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "machines",
};
```

### `models.ComputeClusterHeartbeatDataLocal`

```typescript
const value: models.ComputeClusterHeartbeatDataLocal = {
  dockerAvailable: true,
  name: "<value>",
  networkAvailable: true,
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
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

