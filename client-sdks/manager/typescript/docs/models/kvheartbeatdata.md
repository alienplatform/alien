# KvHeartbeatData


## Supported Types

### `models.KvHeartbeatDataAwsDynamoDb`

```typescript
const value: models.KvHeartbeatDataAwsDynamoDb = {
  events: [],
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
    health: "unknown",
    lifecycle: "stopped",
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
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
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
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "gcpFirestore",
};
```

### `models.KvHeartbeatDataAzureTable`

```typescript
const value: models.KvHeartbeatDataAzureTable = {
  events: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  storageAccountName: "<value>",
  tableExists: false,
  tableName: "<value>",
  backend: "azureTable",
};
```

### `models.KvHeartbeatDataLocal`

```typescript
const value: models.KvHeartbeatDataLocal = {
  cloudMetadataSupported: true,
  events: [],
  name: "<value>",
  path: "/var/tmp",
  pathExists: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

