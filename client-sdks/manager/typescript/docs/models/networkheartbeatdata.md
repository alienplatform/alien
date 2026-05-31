# NetworkHeartbeatData


## Supported Types

### `models.NetworkHeartbeatDataAwsVpc`

```typescript
const value: models.NetworkHeartbeatDataAwsVpc = {
  availabilityZones: [],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  routeTableCount: 704849,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

### `models.NetworkHeartbeatDataGcpVpc`

```typescript
const value: models.NetworkHeartbeatDataGcpVpc = {
  isByoVpc: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "gcpVpc",
};
```

### `models.NetworkHeartbeatDataAzureVnet`

```typescript
const value: models.NetworkHeartbeatDataAzureVnet = {
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

