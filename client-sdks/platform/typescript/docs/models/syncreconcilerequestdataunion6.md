# SyncReconcileRequestDataUnion6


## Supported Types

### `models.DataAwsSqs`

```typescript
const value: models.DataAwsSqs = {
  approximateCounts: false,
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "awsSqs",
};
```

### `models.DataGcpPubSub`

```typescript
const value: models.DataGcpPubSub = {
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  subscriptionLabels: {
    "key": "<value>",
  },
  subscriptionPushAttributes: {},
  topicLabels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  topicName: "<value>",
  backend: "gcpPubSub",
};
```

### `models.DataAzureServiceBus`

```typescript
const value: models.DataAzureServiceBus = {
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "scaling",
    partial: false,
    stale: false,
  },
  backend: "azureServiceBus",
};
```

### `models.DataLocal6`

```typescript
const value: models.DataLocal6 = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

