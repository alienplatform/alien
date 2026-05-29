# NetworkHeartbeatData


## Supported Types

### `models.NetworkHeartbeatDataAwsVpc`

```typescript
const value: models.NetworkHeartbeatDataAwsVpc = {
  availabilityZones: [],
  events: [],
  isByoVpc: true,
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  routeTableCount: 501979,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

### `models.NetworkHeartbeatDataGcpVpc`

```typescript
const value: models.NetworkHeartbeatDataGcpVpc = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  isByoVpc: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "gcpVpc",
};
```

### `models.NetworkHeartbeatDataAzureVnet`

```typescript
const value: models.NetworkHeartbeatDataAzureVnet = {
  events: [],
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

