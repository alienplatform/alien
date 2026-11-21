# DataUnion11


## Supported Types

### `operations.DataAwsVpc`

```typescript
const value: operations.DataAwsVpc = {
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

### `operations.DataGcpVpc`

```typescript
const value: operations.DataGcpVpc = {
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

### `operations.DataAzureVnet`

```typescript
const value: operations.DataAzureVnet = {
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

