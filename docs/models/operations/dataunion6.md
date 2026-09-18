# DataUnion6


## Supported Types

### `operations.DataAwsSqs`

```typescript
const value: operations.DataAwsSqs = {
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

### `operations.DataGcpPubSub`

```typescript
const value: operations.DataGcpPubSub = {
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

### `operations.DataAzureServiceBus`

```typescript
const value: operations.DataAzureServiceBus = {
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

### `operations.DataLocal6`

```typescript
const value: operations.DataLocal6 = {
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

