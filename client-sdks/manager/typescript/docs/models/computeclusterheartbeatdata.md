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
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
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
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
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
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "azure",
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
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

