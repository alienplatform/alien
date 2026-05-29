# DataUnion1


## Supported Types

### `operations.DataAwsS3`

```typescript
const value: operations.DataAwsS3 = {
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

### `operations.DataGcpCloudStorage`

```typescript
const value: operations.DataGcpCloudStorage = {
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
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudStorage",
};
```

### `operations.DataAzureBlob`

```typescript
const value: operations.DataAzureBlob = {
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
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "azureBlob",
};
```

### `operations.DataLocal1`

```typescript
const value: operations.DataLocal1 = {
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
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

