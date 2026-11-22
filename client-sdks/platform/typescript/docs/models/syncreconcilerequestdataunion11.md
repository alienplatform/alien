# SyncReconcileRequestDataUnion11


## Supported Types

### `models.DataAwsVpc`

```typescript
const value: models.DataAwsVpc = {
  availabilityZones: [
    "<value 1>",
    "<value 2>",
  ],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [],
  routeTableCount: 759318,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsVpc",
};
```

### `models.DataGcpVpc`

```typescript
const value: models.DataGcpVpc = {
  isByoVpc: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "gcpVpc",
};
```

### `models.DataAzureVnet`

```typescript
const value: models.DataAzureVnet = {
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

