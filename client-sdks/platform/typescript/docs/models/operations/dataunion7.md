# DataUnion7


## Supported Types

### `operations.DataAwsDynamoDb`

```typescript
const value: operations.DataAwsDynamoDb = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-11-03T14:53:55.376Z"),
      severity: "warning",
    },
  ],
  keySchema: [],
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: false,
    stale: true,
  },
  backend: "awsDynamoDb",
};
```

### `operations.DataGcpFirestore`

```typescript
const value: operations.DataGcpFirestore = {
  cmekEnabled: false,
  databaseName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-13T08:16:25.783Z"),
      severity: "info",
    },
  ],
  sourceInfoPresent: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "gcpFirestore",
};
```

### `operations.DataAzureTable`

```typescript
const value: operations.DataAzureTable = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-08T10:49:40.534Z"),
      severity: "warning",
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
    health: "degraded",
    lifecycle: "failed",
    partial: true,
    stale: true,
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
  events: [],
  name: "<value>",
  path: "/usr/src",
  pathExists: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

