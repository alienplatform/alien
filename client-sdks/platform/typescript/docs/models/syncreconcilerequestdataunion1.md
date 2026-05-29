# SyncReconcileRequestDataUnion1


## Supported Types

### `models.DataAwsS3`

```typescript
const value: models.DataAwsS3 = {
  encryptionConfigPresent: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-11-04T18:46:41.433Z"),
      severity: "warning",
    },
  ],
  lifecyclePresent: true,
  name: "<value>",
  publicAccessBlockPresent: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "awsS3",
};
```

### `models.DataGcpCloudStorage`

```typescript
const value: models.DataGcpCloudStorage = {
  encryptionConfigPresent: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-04-25T23:42:18.878Z"),
      severity: "error",
    },
  ],
  lifecyclePresent: false,
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
    health: "unknown",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudStorage",
};
```

### `models.DataAzureBlob`

```typescript
const value: models.DataAzureBlob = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-01-01T04:44:50.232Z"),
      severity: "info",
    },
  ],
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
    health: "healthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "azureBlob",
};
```

### `models.DataLocal1`

```typescript
const value: models.DataLocal1 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-12T08:12:10.995Z"),
      severity: "error",
    },
  ],
  path: "/usr/share",
  pathExists: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

