# DataUnion10


## Supported Types

### `operations.DataAwsVpc`

```typescript
const value: operations.DataAwsVpc = {
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
        reason: "api-unavailable",
        severity: "info",
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

### `operations.DataGcpVpc`

```typescript
const value: operations.DataGcpVpc = {
  events: [],
  isByoVpc: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "scaling",
    partial: false,
    stale: true,
  },
  backend: "gcpVpc",
};
```

### `operations.DataAzureVnet`

```typescript
const value: operations.DataAzureVnet = {
  events: [],
  isByoVnet: true,
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
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "azureVnet",
};
```

