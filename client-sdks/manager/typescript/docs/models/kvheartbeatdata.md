# KvHeartbeatData


## Supported Types

### `models.KvHeartbeatDataAwsDynamoDb`

```typescript
const value: models.KvHeartbeatDataAwsDynamoDb = {
  keySchema: [],
  name: "<value>",
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
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "awsDynamoDb",
};
```

### `models.KvHeartbeatDataGcpFirestore`

```typescript
const value: models.KvHeartbeatDataGcpFirestore = {
  cmekEnabled: true,
  databaseName: "<value>",
  sourceInfoPresent: false,
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
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "gcpFirestore",
};
```

### `models.KvHeartbeatDataAzureTable`

```typescript
const value: models.KvHeartbeatDataAzureTable = {
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
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  storageAccountName: "<value>",
  tableExists: true,
  tableName: "<value>",
  backend: "azureTable",
};
```

### `models.KvHeartbeatDataLocal`

```typescript
const value: models.KvHeartbeatDataLocal = {
  cloudMetadataSupported: true,
  name: "<value>",
  path: "/etc",
  pathExists: false,
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
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

