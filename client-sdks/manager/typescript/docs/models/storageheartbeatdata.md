# StorageHeartbeatData


## Supported Types

### `models.StorageHeartbeatDataAwsS3`

```typescript
const value: models.StorageHeartbeatDataAwsS3 = {
  encryptionConfigPresent: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: false,
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
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "awsS3",
};
```

### `models.StorageHeartbeatDataGcpCloudStorage`

```typescript
const value: models.StorageHeartbeatDataGcpCloudStorage = {
  encryptionConfigPresent: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  lifecyclePresent: true,
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
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudStorage",
};
```

### `models.StorageHeartbeatDataAzureBlob`

```typescript
const value: models.StorageHeartbeatDataAzureBlob = {
  events: [],
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
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "azureBlob",
};
```

### `models.StorageHeartbeatDataLocal`

```typescript
const value: models.StorageHeartbeatDataLocal = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  path: "/private/tmp",
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
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

