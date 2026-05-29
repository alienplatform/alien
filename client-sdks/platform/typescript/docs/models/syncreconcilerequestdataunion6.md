# SyncReconcileRequestDataUnion6


## Supported Types

### `models.DataAwsSqs`

```typescript
const value: models.DataAwsSqs = {
  approximateCounts: false,
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
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
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-08-12T10:51:43.799Z"),
      severity: "warning",
    },
  ],
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
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
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-12-11T21:43:07.631Z"),
      severity: "error",
    },
  ],
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "azureServiceBus",
};
```

### `models.DataLocal6`

```typescript
const value: models.DataLocal6 = {
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

