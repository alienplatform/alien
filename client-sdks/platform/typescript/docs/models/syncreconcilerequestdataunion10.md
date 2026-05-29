# SyncReconcileRequestDataUnion10


## Supported Types

### `models.DataAwsVpc`

```typescript
const value: models.DataAwsVpc = {
  availabilityZones: [
    "<value 1>",
    "<value 2>",
  ],
  events: [],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  routeTableCount: 642691,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

### `models.DataGcpVpc`

```typescript
const value: models.DataGcpVpc = {
  events: [],
  isByoVpc: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
  backend: "gcpVpc",
};
```

### `models.DataAzureVnet`

```typescript
const value: models.DataAzureVnet = {
  events: [],
  isByoVnet: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "azureVnet",
};
```

