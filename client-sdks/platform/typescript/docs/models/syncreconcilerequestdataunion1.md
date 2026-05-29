# SyncReconcileRequestDataUnion1


## Supported Types

### `models.DataAwsS3`

```typescript
const value: models.DataAwsS3 = {
  encryptionConfigPresent: true,
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: true,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsS3",
};
```

### `models.DataGcpCloudStorage`

```typescript
const value: models.DataGcpCloudStorage = {
  encryptionConfigPresent: false,
  lifecyclePresent: false,
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
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
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "azureBlob",
};
```

### `models.DataLocal1`

```typescript
const value: models.DataLocal1 = {
  path: "/usr/lib",
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
    health: "unhealthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

