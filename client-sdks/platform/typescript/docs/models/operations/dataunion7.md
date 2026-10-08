# DataUnion7


## Supported Types

### `operations.DataAwsDynamoDb`

```typescript
const value: operations.DataAwsDynamoDb = {
  keySchema: [
    {
      attributeName: "<value>",
      keyType: "<value>",
    },
  ],
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsDynamoDb",
};
```

### `operations.DataGcpFirestore`

```typescript
const value: operations.DataGcpFirestore = {
  cmekEnabled: false,
  databaseName: "<value>",
  sourceInfoPresent: false,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "gcpFirestore",
};
```

### `operations.DataAzureTable`

```typescript
const value: operations.DataAzureTable = {
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  storageAccountName: "<value>",
  tableExists: false,
  tableName: "<value>",
  backend: "azureTable",
};
```

### `operations.DataLocal7`

```typescript
const value: operations.DataLocal7 = {
  cloudMetadataSupported: false,
  name: "<value>",
  path: "/dev",
  pathExists: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

