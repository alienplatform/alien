# SyncReconcileRequestDataUnion7


## Supported Types

### `models.DataAwsDynamoDb`

```typescript
const value: models.DataAwsDynamoDb = {
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
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "scaling",
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

### `models.DataAzureTable`

```typescript
const value: models.DataAzureTable = {
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "scaling",
    partial: false,
    stale: true,
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

