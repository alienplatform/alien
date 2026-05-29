# DataUnion1


## Supported Types

### `operations.DataAwsS3`

```typescript
const value: operations.DataAwsS3 = {
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

### `operations.DataGcpCloudStorage`

```typescript
const value: operations.DataGcpCloudStorage = {
  encryptionConfigPresent: false,
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
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "gcpCloudStorage",
};
```

### `operations.DataAzureBlob`

```typescript
const value: operations.DataAzureBlob = {
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "scaling",
    partial: false,
    stale: false,
  },
  backend: "azureBlob",
};
```

### `operations.DataLocal1`

```typescript
const value: operations.DataLocal1 = {
  path: "/usr/lib",
  pathExists: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

