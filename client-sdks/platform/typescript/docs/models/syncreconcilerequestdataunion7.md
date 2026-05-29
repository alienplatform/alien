# SyncReconcileRequestDataUnion7


## Supported Types

### `models.DataAwsDynamoDb`

```typescript
const value: models.DataAwsDynamoDb = {
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

### `models.DataGcpFirestore`

```typescript
const value: models.DataGcpFirestore = {
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
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "gcpFirestore",
};
```

### `models.DataAzureTable`

```typescript
const value: models.DataAzureTable = {
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
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  storageAccountName: "<value>",
  tableExists: true,
  tableName: "<value>",
  backend: "azureTable",
};
```

### `models.DataLocal7`

```typescript
const value: models.DataLocal7 = {
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

