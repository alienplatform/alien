# StorageHeartbeatData


## Supported Types

### `models.StorageHeartbeatDataAwsS3`

```typescript
const value: models.StorageHeartbeatDataAwsS3 = {
  encryptionConfigPresent: true,
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "awsS3",
};
```

### `models.StorageHeartbeatDataGcpCloudStorage`

```typescript
const value: models.StorageHeartbeatDataGcpCloudStorage = {
  encryptionConfigPresent: false,
  lifecyclePresent: false,
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudStorage",
};
```

### `models.StorageHeartbeatDataAzureBlob`

```typescript
const value: models.StorageHeartbeatDataAzureBlob = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "azureBlob",
};
```

### `models.StorageHeartbeatDataLocal`

```typescript
const value: models.StorageHeartbeatDataLocal = {
  path: "/usr/lib",
  pathExists: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

